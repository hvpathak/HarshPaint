//! An editing session: the document, the current tool and its settings, the
//! selection, and undo history. The app forwards mouse events and command ids here;
//! the CLI and tests drive exactly the same code.

use std::path::PathBuf;

use crate::canvas::{Raster, Rect, add_dirty};
use crate::color::{Rgba, mul_255};
use crate::commands::Command;
use crate::document::{Document, Layer};
use crate::error::{Error, Result};
use crate::history::{Change, History, Patch};
use crate::image_ops::{self, Rotation};
use crate::mask::Mask;
use crate::shapes::{self, ShapeKind, ShapeStyle};
use crate::stabilizer::Stabilizer;
use crate::symmetry::Symmetry;
use crate::{fill, text};

pub const MAX_BRUSH: u32 = 200;
pub const DEFAULT_WIDTH: u32 = 1280;
pub const DEFAULT_HEIGHT: u32 = 720;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    Pencil,
    #[default]
    Brush,
    Eraser,
    Spray,
    Fill,
    Picker,
    Text,
    Select,
    Shape,
}

impl Tool {
    pub fn label(self) -> &'static str {
        match self {
            Tool::Pencil => "Pencil",
            Tool::Brush => "Brush",
            Tool::Eraser => "Eraser",
            Tool::Spray => "Spray",
            Tool::Fill => "Fill",
            Tool::Picker => "Colour picker",
            Tool::Text => "Text",
            Tool::Select => "Select",
            Tool::Shape => "Shape",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    /// Left mouse button: colour 1.
    Primary,
    /// Right mouse button: colour 2.
    Secondary,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub color1: Rgba,
    pub color2: Rgba,
    /// Brush / pencil / eraser / outline size in pixels.
    pub size: u32,
    /// 0.0 (soft) ..= 1.0 (hard).
    pub hardness: f32,
    pub shape: ShapeKind,
    pub shape_style: ShapeStyle,
    /// Fill tolerance, 0..=255.
    pub tolerance: u8,
    pub symmetry: Symmetry,
    /// 0..=100.
    pub stabilizer: u8,
    /// Crisp, aliased drawing for pixel art.
    pub pixel_art: bool,
    /// Pasted or moved pixels of colour 2 become see-through.
    pub transparent_selection: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            color1: Rgba::BLACK,
            color2: Rgba::WHITE,
            size: 5,
            hardness: 0.85,
            shape: ShapeKind::Rectangle,
            shape_style: ShapeStyle::Outline,
            tolerance: 0,
            symmetry: Symmetry::Off,
            stabilizer: 0,
            pixel_art: false,
            transparent_selection: false,
        }
    }
}

/// What the app should do after a mouse press.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Feedback {
    None,
    /// Show a text box at this picture position.
    PlaceText((f32, f32)),
    /// The colour picker changed a colour.
    Picked(Rgba),
}

/// A stroke in progress: the layer as it was, plus coverage masks drawn so far.
struct Stroke {
    tool: Tool,
    label: &'static str,
    layer: usize,
    background: bool,
    snapshot: Raster,
    mask: Mask,
    fill_mask: Option<Mask>,
    color: Rgba,
    fill_color: Rgba,
    /// Eraser with the right button swaps `from` for `to` (Paint's colour replace).
    replace: Option<(Rgba, Rgba, u8)>,
    erase_to: Rgba,
    /// Area touched by each symmetry copy, kept separate so a stroke on opposite
    /// sides of the picture never redraws (or stores) everything in between.
    copy_rects: Vec<Option<Rect>>,
    stabilizer: Stabilizer,
    last: Option<Vec<(f32, f32)>>,
    start: (f32, f32),
    constrain: bool,
    rng: u64,
}

impl Stroke {
    fn shade(&self, base: Rgba, m: u8, m2: u8) -> Rgba {
        match (self.tool, self.replace) {
            (Tool::Eraser, Some((from, to, tolerance))) => {
                if m > 0 && base.distance(from) <= tolerance {
                    to.scaled(m).over(base)
                } else {
                    base
                }
            }
            (Tool::Eraser, None) if self.background => self.erase_to.scaled(m).over(base),
            (Tool::Eraser, None) => base.with_alpha(mul_255(base.a, 255 - m)),
            _ => {
                let mut out = base;
                if m2 > 0 {
                    out = self.fill_color.scaled(m2).over(out);
                }
                if m > 0 {
                    out = self.color.scaled(m).over(out);
                }
                out
            }
        }
    }

    /// Recomputes `rect` of the layer from the snapshot and the masks.
    fn apply(&self, layer: &mut Raster, rect: Rect) {
        let r = rect.clip(layer.width(), layer.height());
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                let base = self.snapshot.get(x, y).unwrap_or_default();
                let m = self.mask.get(x, y);
                let m2 = self.fill_mask.as_ref().map_or(0, |f| f.get(x, y));
                layer.set(x, y, self.shade(base, m, m2));
            }
        }
    }

    fn next_random(&mut self) -> f32 {
        // xorshift64: deterministic, so the same stroke always sprays the same dots.
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }
}

#[derive(Debug, Clone)]
struct Floating {
    raster: Raster,
    x: i64,
    y: i64,
    layer: usize,
    /// The layer before the selection was lifted (for undo).
    origin: Raster,
    /// The layer with the hole left behind (redrawn under the piece as it moves).
    base: Raster,
    source: Rect,
    drawn: Rect,
}

#[derive(Debug, Clone)]
struct Selection {
    rect: Rect,
    floating: Option<Floating>,
}

#[derive(Debug, Clone, Copy)]
enum Drag {
    NewSelection { anchor: (u32, u32) },
    Move { grab: (f32, f32), start: (i64, i64) },
}

pub struct Session {
    pub doc: Document,
    pub settings: Settings,
    pub path: Option<PathBuf>,
    tool: Tool,
    previous_tool: Tool,
    history: History,
    stroke: Option<Stroke>,
    selection: Option<Selection>,
    drag: Option<Drag>,
    dirty: Vec<Rect>,
    modified: bool,
    strokes_started: u64,
}

impl Default for Session {
    fn default() -> Self {
        let doc = Document::new(DEFAULT_WIDTH, DEFAULT_HEIGHT, Rgba::WHITE)
            .unwrap_or_else(|_| Document::from_raster(fallback_raster()));
        Self::new(doc)
    }
}

/// A 1×1 picture, used only if even the default canvas can't be allocated.
fn fallback_raster() -> Raster {
    Raster::from_rgba8(1, 1, &[255, 255, 255, 255]).unwrap_or_else(|_| fallback_raster())
}

impl Session {
    pub fn new(doc: Document) -> Self {
        let bounds = doc.bounds();
        Self {
            doc,
            settings: Settings::default(),
            path: None,
            tool: Tool::default(),
            previous_tool: Tool::default(),
            history: History::default(),
            stroke: None,
            selection: None,
            drag: None,
            dirty: vec![bounds],
            modified: false,
            strokes_started: 0,
        }
    }

    /// Replaces the picture (New / Open), keeping tool settings.
    pub fn load(&mut self, doc: Document, path: Option<PathBuf>) {
        let settings = self.settings.clone();
        let tool = self.tool;
        *self = Session::new(doc);
        self.settings = settings;
        self.tool = tool;
        self.path = path;
    }

    // ---- State the app reads ---------------------------------------------------

    pub fn tool(&self) -> Tool {
        self.tool
    }

    pub fn set_tool(&mut self, tool: Tool) {
        self.finish_stroke();
        if tool != Tool::Select {
            self.commit_floating();
            self.selection = None;
        }
        if tool == Tool::Picker && self.tool != Tool::Picker {
            self.previous_tool = self.tool;
        }
        self.tool = tool;
    }

    pub fn is_modified(&self) -> bool {
        self.modified
    }

    pub fn mark_saved(&mut self, path: PathBuf) {
        self.path = Some(path);
        self.modified = false;
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    /// Areas the app must redraw since the last call.
    pub fn take_dirty(&mut self) -> Vec<Rect> {
        std::mem::take(&mut self.dirty)
    }

    fn touch(&mut self, rect: Rect) {
        push_rect(&mut self.dirty, rect);
    }

    fn touch_all(&mut self) {
        let b = self.doc.bounds();
        self.touch(b);
    }

    /// The selection or floating piece as (x, y, w, h) in picture pixels.
    pub fn selection_outline(&self) -> Option<(f32, f32, f32, f32)> {
        let sel = self.selection.as_ref()?;
        Some(match &sel.floating {
            Some(f) => (
                f.x as f32,
                f.y as f32,
                f.raster.width() as f32,
                f.raster.height() as f32,
            ),
            None => (
                sel.rect.x as f32,
                sel.rect.y as f32,
                sel.rect.w as f32,
                sel.rect.h as f32,
            ),
        })
    }

    pub fn has_selection(&self) -> bool {
        self.selection.is_some()
    }

    pub fn centre(&self) -> (f32, f32) {
        (
            self.doc.width() as f32 / 2.0,
            self.doc.height() as f32 / 2.0,
        )
    }

    /// Whether a toggle-style command is currently on (menu check marks).
    pub fn is_checked(&self, command: Command) -> Option<bool> {
        let s = &self.settings;
        let shape = |k: ShapeKind| self.tool == Tool::Shape && s.shape == k;
        Some(match command {
            Command::ToolPencil => self.tool == Tool::Pencil,
            Command::ToolBrush => self.tool == Tool::Brush,
            Command::ToolEraser => self.tool == Tool::Eraser,
            Command::ToolSpray => self.tool == Tool::Spray,
            Command::ToolFill => self.tool == Tool::Fill,
            Command::ToolPicker => self.tool == Tool::Picker,
            Command::ToolText => self.tool == Tool::Text,
            Command::ToolSelect => self.tool == Tool::Select,
            Command::ToolLine => shape(ShapeKind::Line),
            Command::ToolRectangle => shape(ShapeKind::Rectangle),
            Command::ToolEllipse => shape(ShapeKind::Ellipse),
            Command::PixelArt => s.pixel_art,
            Command::StabilizerOff => s.stabilizer == 0,
            Command::StabilizerLow => s.stabilizer == 25,
            Command::StabilizerMedium => s.stabilizer == 50,
            Command::StabilizerHigh => s.stabilizer == 80,
            _ => s.symmetry == symmetry_of(command)?,
        })
    }

    // ---- Commands ----------------------------------------------------------------

    /// Runs a command that needs no dialog. Returns `Ok(false)` for commands the app
    /// handles itself (files, clipboard, zoom, dialogs).
    pub fn run(&mut self, command: Command) -> Result<bool> {
        if let Some(mode) = symmetry_of(command) {
            self.settings.symmetry = mode;
            return Ok(true);
        }
        match command {
            Command::Undo => self.undo(),
            Command::Redo => self.redo(),
            Command::SelectAll => self.select_all(),
            Command::DeleteSelection => self.delete_selection()?,
            Command::Deselect => {
                self.commit_floating();
                self.selection = None;
            }
            Command::Crop => self.crop()?,
            Command::RotateRight => self.rotate(Rotation::Right90)?,
            Command::RotateLeft => self.rotate(Rotation::Left90)?,
            Command::Rotate180 => self.rotate(Rotation::Half)?,
            Command::FlipVertical => self.flip(false)?,
            Command::FlipHorizontal => self.flip(true)?,
            Command::InvertColors => self.invert()?,
            Command::LayerNew => self.layer_op("New layer", Document::add_layer)?,
            Command::LayerDuplicate => {
                self.layer_op("Duplicate layer", Document::duplicate_layer)?
            }
            Command::LayerDelete => self.layer_op("Delete layer", Document::delete_layer)?,
            Command::LayerMergeDown => self.layer_op("Merge down", Document::merge_down)?,
            Command::LayerUp => self.layer_op("Move layer", |d| d.move_layer(true))?,
            Command::LayerDown => self.layer_op("Move layer", |d| d.move_layer(false))?,
            Command::ToolPencil => self.set_tool(Tool::Pencil),
            Command::ToolBrush => self.set_tool(Tool::Brush),
            Command::ToolEraser => self.set_tool(Tool::Eraser),
            Command::ToolSpray => self.set_tool(Tool::Spray),
            Command::ToolFill => self.set_tool(Tool::Fill),
            Command::ToolPicker => self.set_tool(Tool::Picker),
            Command::ToolText => self.set_tool(Tool::Text),
            Command::ToolSelect => self.set_tool(Tool::Select),
            Command::ToolLine => self.set_shape(ShapeKind::Line),
            Command::ToolRectangle => self.set_shape(ShapeKind::Rectangle),
            Command::ToolEllipse => self.set_shape(ShapeKind::Ellipse),
            Command::SwapColors => {
                std::mem::swap(&mut self.settings.color1, &mut self.settings.color2);
            }
            Command::SizeUp => {
                let s = self.settings.size;
                self.settings.size = (s + (s / 4).max(1)).min(MAX_BRUSH);
            }
            Command::SizeDown => {
                let s = self.settings.size;
                self.settings.size = s.saturating_sub((s / 5).max(1)).max(1);
            }
            Command::StabilizerOff => self.settings.stabilizer = 0,
            Command::StabilizerLow => self.settings.stabilizer = 25,
            Command::StabilizerMedium => self.settings.stabilizer = 50,
            Command::StabilizerHigh => self.settings.stabilizer = 80,
            Command::PixelArt => {
                self.settings.pixel_art = !self.settings.pixel_art;
                if self.settings.pixel_art {
                    self.settings.size = 1;
                    self.set_tool(Tool::Pencil);
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    pub fn set_shape(&mut self, kind: ShapeKind) {
        self.settings.shape = kind;
        self.set_tool(Tool::Shape);
    }

    // ---- Mouse --------------------------------------------------------------------

    pub fn pointer_down(&mut self, p: (f32, f32), button: Button, mods: Mods) -> Feedback {
        self.finish_stroke();
        match self.tool {
            Tool::Picker => {
                let (Some(x), Some(y)) = (pixel(p.0), pixel(p.1)) else {
                    return Feedback::None;
                };
                let Some(c) = self.doc.sample(x, y) else {
                    return Feedback::None;
                };
                match button {
                    Button::Primary => self.settings.color1 = c,
                    Button::Secondary => self.settings.color2 = c,
                }
                self.tool = self.previous_tool;
                Feedback::Picked(c)
            }
            Tool::Fill => {
                self.fill_at(p, button, mods.shift);
                Feedback::None
            }
            Tool::Text => {
                self.commit_floating();
                Feedback::PlaceText(p)
            }
            Tool::Select => {
                self.select_down(p);
                Feedback::None
            }
            _ => {
                self.begin_stroke(p, button, mods);
                Feedback::None
            }
        }
    }

    pub fn pointer_move(&mut self, p: (f32, f32), mods: Mods) {
        if let Some(st) = self.stroke.as_mut() {
            st.constrain = mods.shift;
            self.extend_stroke(p);
        } else if self.drag.is_some() {
            self.select_move(p);
        }
    }

    pub fn pointer_up(&mut self, p: (f32, f32), mods: Mods) {
        self.pointer_move(p, mods);
        self.finish_stroke();
        if let Some(Drag::NewSelection { .. }) = self.drag
            && self.selection.as_ref().is_some_and(|s| s.rect.is_empty())
        {
            self.selection = None;
        }
        self.drag = None;
    }

    // ---- Strokes -------------------------------------------------------------------

    fn begin_stroke(&mut self, p: (f32, f32), button: Button, mods: Mods) {
        let Some(layer) = self.doc.active() else {
            return;
        };
        let (w, h) = (self.doc.width(), self.doc.height());
        let s = &self.settings;
        let (color, other) = match button {
            Button::Primary => (s.color1, s.color2),
            Button::Secondary => (s.color2, s.color1),
        };
        let label = match self.tool {
            Tool::Pencil => "Pencil",
            Tool::Eraser => "Eraser",
            Tool::Spray => "Spray",
            Tool::Shape => s.shape.label(),
            _ => "Brush",
        };
        self.strokes_started += 1;
        self.stroke = Some(Stroke {
            tool: self.tool,
            label,
            layer: self.doc.active_index(),
            background: self.doc.active_index() == 0,
            snapshot: layer.raster.clone(),
            mask: Mask::new(w, h),
            fill_mask: (self.tool == Tool::Shape).then(|| Mask::new(w, h)),
            color,
            fill_color: other,
            replace: (self.tool == Tool::Eraser && button == Button::Secondary).then_some((
                s.color1,
                s.color2,
                s.tolerance.max(8),
            )),
            erase_to: s.color2,
            copy_rects: vec![None; s.symmetry.copies()],
            stabilizer: Stabilizer::new(if self.tool == Tool::Shape {
                0
            } else {
                s.stabilizer
            }),
            last: None,
            start: p,
            constrain: mods.shift,
            rng: 0x9E37_79B9_7F4A_7C15 ^ self.strokes_started,
        });
        self.extend_stroke(p);
    }

    fn extend_stroke(&mut self, p: (f32, f32)) {
        let centre = self.centre();
        let settings = self.settings.clone();
        let Some(st) = self.stroke.as_mut() else {
            return;
        };
        let Some(layer) = self.doc.layer_mut(st.layer) else {
            return;
        };
        let size = settings.size.clamp(1, MAX_BRUSH);
        let radius = size as f32 / 2.0;
        let aliased = settings.pixel_art || st.tool == Tool::Pencil;

        // Every small area changed by this move; each is recomputed on its own.
        let mut touched: Vec<Rect> = Vec::new();
        if st.tool == Tool::Shape {
            // Shapes are redrawn from scratch on every move.
            for old in st.copy_rects.iter().flatten().copied() {
                st.mask.clear_rect(old);
                if let Some(f) = st.fill_mask.as_mut() {
                    f.clear_rect(old);
                }
                touched.push(old);
            }
            let outline = shapes::outline(settings.shape, st.start, p, st.constrain);
            let mut copies = vec![Vec::with_capacity(outline.len()); settings.symmetry.copies()];
            for pt in outline {
                for (copy, q) in copies.iter_mut().zip(settings.symmetry.map(pt, centre)) {
                    copy.push(q);
                }
            }
            st.copy_rects = vec![None; copies.len()];
            let closed = settings.shape.is_closed();
            for (poly, slot) in copies.iter().zip(st.copy_rects.iter_mut()) {
                if closed
                    && settings.shape_style != ShapeStyle::Outline
                    && let Some(f) = st.fill_mask.as_mut()
                {
                    add_dirty(slot, f.fill_polygon(poly, aliased));
                }
                if !closed || settings.shape_style != ShapeStyle::Fill {
                    let edges = poly.len() - usize::from(!closed);
                    for i in 0..edges {
                        let (Some(&a), Some(&b)) = (poly.get(i), poly.get((i + 1) % poly.len()))
                        else {
                            continue;
                        };
                        add_dirty(slot, st.mask.capsule(a, b, radius, 1.0, aliased));
                    }
                }
                if let Some(r) = *slot {
                    touched.push(r);
                }
            }
        } else {
            let pen = st.stabilizer.update(p);
            let points = settings.symmetry.map(pen, centre);
            let previous = st
                .last
                .replace(points.clone())
                .unwrap_or_else(|| points.clone());
            for (i, (&a, &b)) in previous.iter().zip(&points).enumerate() {
                let r = match st.tool {
                    Tool::Pencil if size <= 1 => st.mask.pixel_line(
                        (a.0.floor() as i64, a.1.floor() as i64),
                        (b.0.floor() as i64, b.1.floor() as i64),
                    ),
                    Tool::Eraser => {
                        let steps =
                            (((b.0 - a.0).abs().max((b.1 - a.1).abs())).ceil() as u32).min(20_000);
                        let mut r = None;
                        for i in 0..=steps {
                            let t = if steps == 0 {
                                0.0
                            } else {
                                i as f32 / steps as f32
                            };
                            let q = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                            add_dirty(&mut r, st.mask.square(q.0, q.1, size.max(2)));
                        }
                        r.unwrap_or_default()
                    }
                    Tool::Spray => {
                        let spread = radius * 3.0 + 2.0;
                        let dots = (size / 2).clamp(3, 40);
                        let mut r = None;
                        for _ in 0..dots {
                            let dist = spread * st.next_random().sqrt();
                            let angle = std::f32::consts::TAU * st.next_random();
                            let (dx, dy) = (b.0 + dist * angle.cos(), b.1 + dist * angle.sin());
                            if let (Some(x), Some(y)) = (pixel(dx), pixel(dy)) {
                                st.mask.raise(x, y, 255);
                                add_dirty(
                                    &mut r,
                                    Rect::new(x, y, 1, 1).clip(st.mask.width(), st.mask.height()),
                                );
                            }
                        }
                        r.unwrap_or_default()
                    }
                    _ => st.mask.capsule(
                        a,
                        b,
                        radius,
                        if aliased { 1.0 } else { settings.hardness },
                        aliased,
                    ),
                };
                if let Some(slot) = st.copy_rects.get_mut(i) {
                    add_dirty(slot, r);
                }
                touched.push(r);
            }
        }
        for &r in &touched {
            st.apply(&mut layer.raster, r);
        }
        for r in touched {
            push_rect(&mut self.dirty, r);
        }
    }

    fn finish_stroke(&mut self) {
        let Some(st) = self.stroke.take() else { return };
        let rects: Vec<Rect> = st.copy_rects.iter().flatten().copied().collect();
        let Some(union) = rects.iter().copied().reduce(Rect::union) else {
            return;
        };
        let Some(layer) = self.doc.layer(st.layer) else {
            return;
        };
        // Separate patches only pay off when the copies are spread apart.
        let separate: u64 = rects.iter().map(Rect::area).sum();
        let change = if rects.len() == 1 || separate >= union.area() {
            Change::Pixels {
                layer: st.layer,
                rect: union,
                before: st.snapshot.read_rect(union),
                after: layer.raster.read_rect(union),
            }
        } else {
            Change::Patches {
                layer: st.layer,
                patches: rects
                    .iter()
                    .map(|&rect| Patch {
                        rect,
                        before: st.snapshot.read_rect(rect),
                        after: layer.raster.read_rect(rect),
                    })
                    .collect(),
            }
        };
        self.history.push(st.label, change);
        self.modified = true;
    }

    fn fill_at(&mut self, p: (f32, f32), button: Button, everywhere: bool) {
        let (Some(x), Some(y)) = (pixel(p.0), pixel(p.1)) else {
            return;
        };
        let color = match button {
            Button::Primary => self.settings.color1,
            Button::Secondary => self.settings.color2,
        };
        let tolerance = self.settings.tolerance;
        let index = self.doc.active_index();
        let Some(layer) = self.doc.active_mut() else {
            return;
        };
        let before_all = layer.raster.clone();
        let rect = fill::fill(&mut layer.raster, x, y, color, tolerance, everywhere);
        if rect.is_empty() {
            return;
        }
        let after = layer.raster.read_rect(rect);
        let before = before_all.read_rect(rect);
        self.history.push(
            "Fill",
            Change::Pixels {
                layer: index,
                rect,
                before,
                after,
            },
        );
        self.modified = true;
        self.touch(rect);
    }

    /// Draws text with colour 1 at `at` (top-left). The app supplies the font bytes.
    pub fn place_text(
        &mut self,
        content: &str,
        font: &[u8],
        size_px: f32,
        at: (f32, f32),
    ) -> Result<()> {
        if content.trim().is_empty() {
            return Ok(());
        }
        let index = self.doc.active_index();
        let Some(layer) = self.doc.active_mut() else {
            return Err(Error::Unavailable("there is no layer to draw on"));
        };
        let mut mask = Mask::new(layer.raster.width(), layer.raster.height());
        let rect = text::render(
            &mut mask,
            font,
            content,
            at,
            size_px,
            self.settings.pixel_art,
        )?;
        if rect.is_empty() {
            return Ok(());
        }
        let stroke = Stroke {
            tool: Tool::Text,
            label: "Text",
            layer: index,
            background: index == 0,
            snapshot: layer.raster.clone(),
            mask,
            fill_mask: None,
            color: self.settings.color1,
            fill_color: self.settings.color2,
            replace: None,
            erase_to: self.settings.color2,
            copy_rects: vec![Some(rect)],
            stabilizer: Stabilizer::new(0),
            last: None,
            start: at,
            constrain: false,
            rng: 1,
        };
        stroke.apply(&mut layer.raster, rect);
        self.touch(rect);
        self.stroke = Some(stroke);
        self.finish_stroke();
        Ok(())
    }

    // ---- Selection ------------------------------------------------------------------

    fn select_down(&mut self, p: (f32, f32)) {
        let inside = self
            .selection_outline()
            .is_some_and(|(x, y, w, h)| p.0 >= x && p.1 >= y && p.0 < x + w && p.1 < y + h);
        if inside {
            if self
                .selection
                .as_ref()
                .is_some_and(|s| s.floating.is_none())
            {
                self.lift();
            }
            if let Some(f) = self.selection.as_ref().and_then(|s| s.floating.as_ref()) {
                self.drag = Some(Drag::Move {
                    grab: p,
                    start: (f.x, f.y),
                });
            }
            return;
        }
        self.commit_floating();
        self.selection = None;
        let anchor = self.clamp_pixel(p);
        self.drag = Some(Drag::NewSelection { anchor });
    }

    fn select_move(&mut self, p: (f32, f32)) {
        match self.drag {
            Some(Drag::NewSelection { anchor }) => {
                let (x1, y1) = self.clamp_pixel(p);
                let (x0, y0) = anchor;
                let rect = Rect::new(x0.min(x1), y0.min(y1), x0.abs_diff(x1), y0.abs_diff(y1));
                self.selection = Some(Selection {
                    rect,
                    floating: None,
                });
            }
            Some(Drag::Move { grab, start }) => {
                let x = start.0 + (p.0 - grab.0).round() as i64;
                let y = start.1 + (p.1 - grab.1).round() as i64;
                self.move_floating(x, y);
            }
            None => {}
        }
    }

    fn clamp_pixel(&self, p: (f32, f32)) -> (u32, u32) {
        let clamp = |v: f32, max: u32| -> u32 {
            if v.is_nan() {
                0
            } else {
                v.round().clamp(0.0, max as f32) as u32
            }
        };
        (clamp(p.0, self.doc.width()), clamp(p.1, self.doc.height()))
    }

    /// The colour a cleared area gets: colour 2 on the background, see-through elsewhere.
    fn hole_colour(&self, layer: usize) -> Rgba {
        if layer == 0 {
            self.settings.color2
        } else {
            Rgba::TRANSPARENT
        }
    }

    /// Cuts the selected pixels out of the layer into a movable piece.
    fn lift(&mut self) {
        let index = self.doc.active_index();
        let hole = self.hole_colour(index);
        let skip = self
            .settings
            .transparent_selection
            .then_some(self.settings.color2);
        let Some(sel) = self.selection.as_mut() else {
            return;
        };
        if sel.floating.is_some() || sel.rect.is_empty() {
            return;
        }
        let Some(layer) = self.doc.layer_mut(index) else {
            return;
        };
        let Ok(piece) = layer.raster.crop(sel.rect) else {
            return;
        };
        let origin = layer.raster.clone();
        layer.raster.fill_rect(sel.rect, hole);
        let base = layer.raster.clone();
        layer.raster.draw(
            &piece,
            i64::from(sel.rect.x),
            i64::from(sel.rect.y),
            true,
            skip,
        );
        sel.floating = Some(Floating {
            raster: piece,
            x: i64::from(sel.rect.x),
            y: i64::from(sel.rect.y),
            layer: index,
            origin,
            base,
            source: sel.rect,
            drawn: sel.rect,
        });
    }

    fn move_floating(&mut self, x: i64, y: i64) {
        let skip = self
            .settings
            .transparent_selection
            .then_some(self.settings.color2);
        let Some(f) = self.selection.as_mut().and_then(|s| s.floating.as_mut()) else {
            return;
        };
        let Some(layer) = self.doc.layer_mut(f.layer) else {
            return;
        };
        let old = f.drawn;
        layer.raster.write_rect(old, &f.base.read_rect(old));
        layer.raster.draw(&f.raster, x, y, true, skip);
        f.x = x;
        f.y = y;
        f.drawn = placed_rect(
            x,
            y,
            f.raster.width(),
            f.raster.height(),
            layer.raster.width(),
            layer.raster.height(),
        );
        let changed = old.union(f.drawn);
        push_rect(&mut self.dirty, changed);
    }

    /// Stamps a moved or pasted piece into its layer as one undo step.
    fn commit_floating(&mut self) {
        let Some(f) = self.selection.as_mut().and_then(|s| s.floating.take()) else {
            return;
        };
        self.selection = None;
        let Some(layer) = self.doc.layer(f.layer) else {
            return;
        };
        let rect = f.source.union(f.drawn);
        if rect.is_empty() {
            return;
        }
        let before = f.origin.read_rect(rect);
        let after = layer.raster.read_rect(rect);
        if before != after {
            self.history.push(
                "Move selection",
                Change::Pixels {
                    layer: f.layer,
                    rect,
                    before,
                    after,
                },
            );
            self.modified = true;
        }
    }

    pub fn select_all(&mut self) {
        self.finish_stroke();
        self.commit_floating();
        self.tool = Tool::Select;
        self.selection = Some(Selection {
            rect: self.doc.bounds(),
            floating: None,
        });
    }

    /// The selected pixels (for Copy). Errors when nothing is selected.
    pub fn copy_selection(&self) -> Result<Raster> {
        let sel = self
            .selection
            .as_ref()
            .ok_or(Error::Unavailable("select something first"))?;
        if let Some(f) = &sel.floating {
            return Ok(f.raster.clone());
        }
        let layer = self
            .doc
            .active()
            .ok_or(Error::Unavailable("there is no layer"))?;
        if sel.rect.is_empty() {
            return Err(Error::Unavailable("select something first"));
        }
        layer.raster.crop(sel.rect)
    }

    pub fn delete_selection(&mut self) -> Result<()> {
        let sel = self
            .selection
            .take()
            .ok_or(Error::Unavailable("select something first"))?;
        if let Some(f) = sel.floating {
            // Drop the piece: the layer goes back to the lifted base.
            if let Some(layer) = self.doc.layer_mut(f.layer) {
                layer.raster = f.base;
                let rect = f.source.union(f.drawn);
                let before = f.origin.read_rect(rect);
                let after = layer.raster.read_rect(rect);
                self.history.push(
                    "Delete",
                    Change::Pixels {
                        layer: f.layer,
                        rect,
                        before,
                        after,
                    },
                );
                self.touch(rect);
                self.modified = true;
            }
            return Ok(());
        }
        let index = self.doc.active_index();
        let hole = self.hole_colour(index);
        let layer = self
            .doc
            .active_mut()
            .ok_or(Error::Unavailable("there is no layer"))?;
        let before = layer.raster.read_rect(sel.rect);
        layer.raster.fill_rect(sel.rect, hole);
        let after = layer.raster.read_rect(sel.rect);
        self.history.push(
            "Delete",
            Change::Pixels {
                layer: index,
                rect: sel.rect,
                before,
                after,
            },
        );
        self.touch(sel.rect);
        self.modified = true;
        Ok(())
    }

    /// Pastes `piece` as a movable selection at the top-left of the picture.
    pub fn paste(&mut self, piece: Raster) {
        self.finish_stroke();
        self.commit_floating();
        let index = self.doc.active_index();
        let skip = self
            .settings
            .transparent_selection
            .then_some(self.settings.color2);
        let Some(layer) = self.doc.active_mut() else {
            return;
        };
        let origin = layer.raster.clone();
        layer.raster.draw(&piece, 0, 0, true, skip);
        let drawn = placed_rect(
            0,
            0,
            piece.width(),
            piece.height(),
            layer.raster.width(),
            layer.raster.height(),
        );
        self.selection = Some(Selection {
            rect: drawn,
            floating: Some(Floating {
                raster: piece,
                x: 0,
                y: 0,
                layer: index,
                base: origin.clone(),
                origin,
                source: Rect::default(),
                drawn,
            }),
        });
        self.tool = Tool::Select;
        self.touch(drawn);
    }

    fn crop(&mut self) -> Result<()> {
        self.commit_floating_keep_rect();
        let rect = self
            .selection
            .take()
            .map(|s| s.rect)
            .filter(|r| !r.is_empty())
            .ok_or(Error::Unavailable("select the area to keep first"))?;
        self.transform_all("Crop", |r| r.crop(rect))
    }

    /// Commits a floating piece but keeps a selection around where it landed.
    fn commit_floating_keep_rect(&mut self) {
        let landed = self
            .selection
            .as_ref()
            .and_then(|s| s.floating.as_ref())
            .map(|f| f.drawn);
        self.commit_floating();
        if let Some(rect) = landed {
            self.selection = Some(Selection {
                rect,
                floating: None,
            });
        }
    }

    // ---- Image and layer operations --------------------------------------------------

    /// Transforms the floating piece if there is a selection, otherwise the whole picture.
    fn transform_selection_or_all(
        &mut self,
        label: &'static str,
        f: impl Fn(&Raster) -> Result<Raster>,
    ) -> Result<()> {
        self.finish_stroke();
        if self.selection.is_some() {
            self.lift();
            let skip = self
                .settings
                .transparent_selection
                .then_some(self.settings.color2);
            let Some(fl) = self.selection.as_mut().and_then(|s| s.floating.as_mut()) else {
                return Ok(());
            };
            fl.raster = f(&fl.raster)?;
            if let Some(layer) = self.doc.layer_mut(fl.layer) {
                let old = fl.drawn;
                layer.raster.write_rect(old, &fl.base.read_rect(old));
                layer.raster.draw(&fl.raster, fl.x, fl.y, true, skip);
                fl.drawn = placed_rect(
                    fl.x,
                    fl.y,
                    fl.raster.width(),
                    fl.raster.height(),
                    layer.raster.width(),
                    layer.raster.height(),
                );
                let changed = old.union(fl.drawn);
                push_rect(&mut self.dirty, changed);
            }
            return Ok(());
        }
        self.transform_all(label, f)
    }

    fn rotate(&mut self, rotation: Rotation) -> Result<()> {
        self.transform_selection_or_all("Rotate", |r| image_ops::rotate(r, rotation))
    }

    fn flip(&mut self, horizontal: bool) -> Result<()> {
        self.transform_selection_or_all("Flip", |r| image_ops::flip(r, horizontal))
    }

    fn invert(&mut self) -> Result<()> {
        if self.selection.is_some() {
            return self.transform_selection_or_all("Invert colours", |r| {
                let mut out = r.clone();
                image_ops::invert(&mut out);
                Ok(out)
            });
        }
        self.finish_stroke();
        let index = self.doc.active_index();
        let layer = self
            .doc
            .active_mut()
            .ok_or(Error::Unavailable("there is no layer"))?;
        let rect = layer.raster.bounds();
        let before = layer.raster.read_rect(rect);
        image_ops::invert(&mut layer.raster);
        let after = layer.raster.read_rect(rect);
        self.history.push(
            "Invert colours",
            Change::Pixels {
                layer: index,
                rect,
                before,
                after,
            },
        );
        self.modified = true;
        self.touch_all();
        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        let smooth = !self.settings.pixel_art;
        self.transform_all("Resize", |r| image_ops::resize(r, width, height, smooth))
    }

    pub fn canvas_size(&mut self, width: u32, height: u32) -> Result<()> {
        self.finish_stroke();
        self.commit_floating();
        self.selection = None;
        let before = self.doc.layers().to_vec();
        let before_active = self.doc.active_index();
        let mut after = Vec::with_capacity(before.len());
        for (i, l) in before.iter().enumerate() {
            let fill = self.hole_colour(i);
            after.push(Layer {
                raster: image_ops::canvas_size(&l.raster, width, height, fill)?,
                ..l.clone()
            });
        }
        self.replace_layers("Canvas size", before, before_active, after)
    }

    fn transform_all(
        &mut self,
        label: &'static str,
        f: impl Fn(&Raster) -> Result<Raster>,
    ) -> Result<()> {
        self.finish_stroke();
        self.commit_floating();
        self.selection = None;
        let before = self.doc.layers().to_vec();
        let before_active = self.doc.active_index();
        let after = before
            .iter()
            .map(|l| {
                Ok(Layer {
                    raster: f(&l.raster)?,
                    ..l.clone()
                })
            })
            .collect::<Result<Vec<_>>>()?;
        self.replace_layers(label, before, before_active, after)
    }

    fn replace_layers(
        &mut self,
        label: &'static str,
        before: Vec<Layer>,
        before_active: usize,
        after: Vec<Layer>,
    ) -> Result<()> {
        self.doc.replace_layers(after.clone())?;
        let after_active = self.doc.active_index();
        self.history.push(
            label,
            Change::Layers {
                before,
                before_active,
                after,
                after_active,
            },
        );
        self.modified = true;
        self.touch_all();
        Ok(())
    }

    fn layer_op(
        &mut self,
        label: &'static str,
        op: impl FnOnce(&mut Document) -> Result<()>,
    ) -> Result<()> {
        self.finish_stroke();
        self.commit_floating();
        self.selection = None;
        let before = self.doc.layers().to_vec();
        let before_active = self.doc.active_index();
        op(&mut self.doc)?;
        let after = self.doc.layers().to_vec();
        let after_active = self.doc.active_index();
        self.history.push(
            label,
            Change::Layers {
                before,
                before_active,
                after,
                after_active,
            },
        );
        self.modified = true;
        self.touch_all();
        Ok(())
    }

    /// Layer panel tweaks (not undo steps, like moving a slider).
    pub fn set_layer_visible(&mut self, index: usize, visible: bool) {
        if let Some(l) = self.doc.layer_mut(index) {
            l.visible = visible;
            self.modified = true;
            self.touch_all();
        }
    }

    pub fn set_layer_opacity(&mut self, index: usize, opacity: f32) {
        if let Some(l) = self.doc.layer_mut(index) {
            l.opacity = if opacity.is_finite() {
                opacity.clamp(0.0, 1.0)
            } else {
                1.0
            };
            self.modified = true;
            self.touch_all();
        }
    }

    pub fn select_layer(&mut self, index: usize) {
        self.finish_stroke();
        self.commit_floating();
        self.selection = None;
        self.doc.set_active(index);
    }

    pub fn undo(&mut self) {
        self.finish_stroke();
        self.commit_floating();
        self.selection = None;
        if self.history.undo(&mut self.doc).is_some() {
            self.modified = true;
            self.touch_all();
        }
    }

    pub fn redo(&mut self) {
        self.finish_stroke();
        self.commit_floating();
        self.selection = None;
        if self.history.redo(&mut self.doc).is_some() {
            self.modified = true;
            self.touch_all();
        }
    }
}

fn symmetry_of(command: Command) -> Option<Symmetry> {
    Some(match command {
        Command::SymmetryOff => Symmetry::Off,
        Command::SymmetryMirrorX => Symmetry::MirrorX,
        Command::SymmetryMirrorY => Symmetry::MirrorY,
        Command::SymmetryQuad => Symmetry::Quad,
        Command::SymmetryRadial6 => Symmetry::Radial {
            arms: 6,
            mirror: false,
        },
        Command::SymmetryRadial8 => Symmetry::Radial {
            arms: 8,
            mirror: false,
        },
        Command::SymmetryKaleido6 => Symmetry::Radial {
            arms: 6,
            mirror: true,
        },
        Command::SymmetryKaleido8 => Symmetry::Radial {
            arms: 8,
            mirror: true,
        },
        Command::SymmetryKaleido12 => Symmetry::Radial {
            arms: 12,
            mirror: true,
        },
        _ => return None,
    })
}

/// Adds `r` to a list of areas to redraw, merging into one rectangle once the list
/// gets long (so a frame never uploads hundreds of tiny patches).
fn push_rect(list: &mut Vec<Rect>, r: Rect) {
    if r.is_empty() {
        return;
    }
    if list.len() >= 64 {
        let all = list.drain(..).fold(r, Rect::union);
        list.push(all);
    } else {
        list.push(r);
    }
}

/// A picture coordinate as a whole pixel index, if it isn't negative or NaN.
fn pixel(v: f32) -> Option<u32> {
    (v >= 0.0 && v.is_finite()).then(|| v.floor().min(u32::MAX as f32) as u32)
}

/// Where a `w`×`h` piece at (`x`, `y`) lands on a `cw`×`ch` canvas.
fn placed_rect(x: i64, y: i64, w: u32, h: u32, cw: u32, ch: u32) -> Rect {
    let x0 = x.clamp(0, i64::from(cw));
    let y0 = y.clamp(0, i64::from(ch));
    let x1 = (x + i64::from(w)).clamp(0, i64::from(cw));
    let y1 = (y + i64::from(h)).clamp(0, i64::from(ch));
    Rect::new(x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(w: u32, h: u32) -> Session {
        Session::new(Document::new(w, h, Rgba::WHITE).unwrap())
    }

    fn drag(s: &mut Session, from: (f32, f32), to: (f32, f32), button: Button) {
        let m = Mods::default();
        s.pointer_down(from, button, m);
        for i in 1..=10 {
            let t = i as f32 / 10.0;
            s.pointer_move(
                (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t),
                m,
            );
        }
        s.pointer_up(to, m);
    }

    fn px(s: &Session, x: u32, y: u32) -> Rgba {
        s.doc.sample(x, y).unwrap()
    }

    #[test]
    fn brush_stroke_paints_and_undo_restores() {
        let mut s = session(64, 64);
        let original = s.doc.clone();
        drag(&mut s, (10.0, 32.0), (50.0, 32.0), Button::Primary);
        assert_eq!(px(&s, 30, 32), Rgba::BLACK);
        assert!(s.is_modified());
        s.undo();
        assert_eq!(s.doc, original);
        s.redo();
        assert_eq!(px(&s, 30, 32), Rgba::BLACK);
    }

    #[test]
    fn spread_out_symmetric_stroke_stores_small_patches() {
        let mut s = session(400, 400);
        s.settings.symmetry = Symmetry::MirrorX;
        s.settings.size = 4;
        let original = s.doc.clone();
        s.take_dirty(); // the first full-picture draw
        drag(&mut s, (10.0, 200.0), (20.0, 200.0), Button::Primary);
        assert_eq!(px(&s, 385, 200), Rgba::BLACK);
        // Two small areas near the edges, not the 400-px-wide box between them.
        let last = s.history().steps().last().map(|st| &st.change);
        assert!(matches!(last, Some(Change::Patches { patches, .. }) if patches.len() == 2));
        assert!(s.take_dirty().iter().all(|r| r.w < 100));
        s.undo();
        assert_eq!(s.doc, original);
    }

    #[test]
    fn right_button_paints_colour_two() {
        let mut s = session(32, 32);
        s.settings.color2 = Rgba::rgb(255, 0, 0);
        drag(&mut s, (5.0, 16.0), (25.0, 16.0), Button::Secondary);
        assert_eq!(px(&s, 15, 16), Rgba::rgb(255, 0, 0));
    }

    #[test]
    fn symmetry_paints_the_mirror_image() {
        let mut s = session(100, 100);
        s.settings.symmetry = Symmetry::MirrorX;
        drag(&mut s, (10.0, 20.0), (20.0, 20.0), Button::Primary);
        assert_eq!(px(&s, 15, 20), Rgba::BLACK);
        assert_eq!(px(&s, 85, 20), Rgba::BLACK); // mirrored across x = 50
        s.settings.symmetry = Symmetry::Radial {
            arms: 8,
            mirror: true,
        };
        drag(&mut s, (50.0, 10.0), (50.0, 30.0), Button::Primary);
        assert_eq!(px(&s, 50, 80), Rgba::BLACK); // the arm opposite
    }

    #[test]
    fn eraser_clears_layers_and_paints_background() {
        let mut s = session(40, 40);
        s.settings.color2 = Rgba::rgb(0, 255, 0);
        drag(&mut s, (5.0, 20.0), (35.0, 20.0), Button::Primary);
        s.run(Command::ToolEraser).unwrap();
        drag(&mut s, (5.0, 20.0), (35.0, 20.0), Button::Primary);
        assert_eq!(px(&s, 20, 20), Rgba::rgb(0, 255, 0));
        s.run(Command::LayerNew).unwrap();
        s.run(Command::ToolBrush).unwrap();
        drag(&mut s, (5.0, 10.0), (35.0, 10.0), Button::Primary);
        s.run(Command::ToolEraser).unwrap();
        drag(&mut s, (5.0, 10.0), (35.0, 10.0), Button::Primary);
        assert_eq!(s.doc.active().unwrap().raster.get(20, 10).unwrap().a, 0);
    }

    #[test]
    fn right_drag_eraser_replaces_colour_one_only() {
        let mut s = session(40, 40);
        s.settings.color1 = Rgba::rgb(255, 0, 0);
        s.settings.size = 10;
        drag(&mut s, (5.0, 20.0), (20.0, 20.0), Button::Primary);
        s.settings.color2 = Rgba::rgb(0, 0, 255);
        s.run(Command::ToolEraser).unwrap();
        drag(&mut s, (5.0, 20.0), (35.0, 20.0), Button::Secondary);
        assert_eq!(px(&s, 12, 20), Rgba::rgb(0, 0, 255)); // was red
        assert_eq!(px(&s, 30, 20), Rgba::WHITE); // was white: untouched
    }

    #[test]
    fn fill_and_fill_everywhere() {
        let mut s = session(20, 20);
        s.settings.size = 1;
        s.run(Command::ToolPencil).unwrap();
        drag(&mut s, (10.5, 0.5), (10.5, 19.5), Button::Primary); // wall
        s.run(Command::ToolFill).unwrap();
        s.settings.color1 = Rgba::rgb(255, 0, 0);
        s.pointer_down((2.0, 2.0), Button::Primary, Mods::default());
        assert_eq!(px(&s, 2, 2), Rgba::rgb(255, 0, 0));
        assert_eq!(px(&s, 15, 2), Rgba::WHITE);
        s.settings.color1 = Rgba::rgb(0, 0, 255);
        s.pointer_down(
            (2.0, 2.0),
            Button::Primary,
            Mods {
                shift: true,
                ctrl: false,
            },
        );
        assert_eq!(px(&s, 2, 2), Rgba::rgb(0, 0, 255));
        s.undo();
        assert_eq!(px(&s, 2, 2), Rgba::rgb(255, 0, 0));
    }

    #[test]
    fn rectangle_outline_and_filled_shapes() {
        let mut s = session(50, 50);
        s.set_shape(ShapeKind::Rectangle);
        s.settings.size = 2;
        drag(&mut s, (10.0, 10.0), (40.0, 40.0), Button::Primary);
        assert_eq!(px(&s, 10, 25), Rgba::BLACK);
        assert_eq!(px(&s, 25, 25), Rgba::WHITE);
        s.settings.shape_style = ShapeStyle::Fill;
        s.settings.color2 = Rgba::rgb(0, 200, 0);
        drag(&mut s, (12.0, 12.0), (38.0, 38.0), Button::Primary);
        assert_eq!(px(&s, 25, 25), Rgba::rgb(0, 200, 0));
        assert_eq!(s.history().len(), 2);
    }

    #[test]
    fn picker_sets_colour_and_returns_to_previous_tool() {
        let mut s = session(10, 10);
        s.doc
            .active_mut()
            .unwrap()
            .raster
            .set(3, 3, Rgba::rgb(1, 2, 3));
        s.run(Command::ToolPicker).unwrap();
        let fb = s.pointer_down((3.5, 3.5), Button::Primary, Mods::default());
        assert_eq!(fb, Feedback::Picked(Rgba::rgb(1, 2, 3)));
        assert_eq!(s.settings.color1, Rgba::rgb(1, 2, 3));
        assert_eq!(s.tool(), Tool::Brush);
        // Off-canvas clicks do nothing.
        s.run(Command::ToolPicker).unwrap();
        assert_eq!(
            s.pointer_down((-5.0, 99.0), Button::Primary, Mods::default()),
            Feedback::None
        );
    }

    #[test]
    fn select_move_and_undo() {
        let mut s = session(40, 40);
        s.doc
            .active_mut()
            .unwrap()
            .raster
            .fill_rect(Rect::new(0, 0, 10, 10), Rgba::BLACK);
        let original = s.doc.clone();
        s.run(Command::ToolSelect).unwrap();
        drag(&mut s, (0.0, 0.0), (10.0, 10.0), Button::Primary);
        assert!(s.has_selection());
        drag(&mut s, (5.0, 5.0), (25.0, 25.0), Button::Primary); // move it
        assert_eq!(px(&s, 25, 25), Rgba::BLACK);
        assert_eq!(px(&s, 2, 2), Rgba::WHITE); // hole filled with colour 2
        s.run(Command::Deselect).unwrap();
        s.undo();
        assert_eq!(s.doc, original);
    }

    #[test]
    fn copy_paste_crop_and_delete() {
        let mut s = session(30, 20);
        assert!(s.copy_selection().is_err());
        assert!(s.run(Command::Crop).is_err());
        assert!(s.run(Command::DeleteSelection).is_err());
        s.doc.active_mut().unwrap().raster.set(1, 1, Rgba::BLACK);
        s.run(Command::ToolSelect).unwrap();
        drag(&mut s, (0.0, 0.0), (5.0, 4.0), Button::Primary);
        let piece = s.copy_selection().unwrap();
        assert_eq!((piece.width(), piece.height()), (5, 4));
        s.run(Command::Crop).unwrap();
        assert_eq!((s.doc.width(), s.doc.height()), (5, 4));
        s.paste(piece);
        s.run(Command::DeleteSelection).unwrap();
        assert_eq!(px(&s, 1, 1), Rgba::BLACK); // the pasted copy was removed, not the original
        s.undo();
        s.undo();
        assert_eq!((s.doc.width(), s.doc.height()), (30, 20));
    }

    #[test]
    fn whole_image_operations_undo() {
        let mut s = session(30, 20);
        s.doc.active_mut().unwrap().raster.set(0, 0, Rgba::BLACK);
        s.run(Command::RotateRight).unwrap();
        assert_eq!((s.doc.width(), s.doc.height()), (20, 30));
        assert_eq!(px(&s, 19, 0), Rgba::BLACK);
        s.run(Command::FlipHorizontal).unwrap();
        s.run(Command::InvertColors).unwrap();
        s.resize(10, 15).unwrap();
        s.canvas_size(40, 40).unwrap();
        assert!(s.resize(0, 1).is_err());
        for _ in 0..5 {
            s.undo();
        }
        assert_eq!((s.doc.width(), s.doc.height()), (30, 20));
        assert_eq!(px(&s, 0, 0), Rgba::BLACK);
    }

    #[test]
    fn layer_commands_and_merge() {
        let mut s = session(10, 10);
        s.run(Command::LayerNew).unwrap();
        s.run(Command::LayerDuplicate).unwrap();
        assert_eq!(s.doc.layers().len(), 3);
        s.run(Command::LayerMergeDown).unwrap();
        s.run(Command::LayerDelete).unwrap();
        assert!(s.run(Command::LayerDelete).is_err());
        s.undo();
        assert_eq!(s.doc.layers().len(), 2);
        s.set_layer_opacity(0, f32::NAN);
        assert_eq!(s.doc.layer(0).unwrap().opacity, 1.0);
    }

    #[test]
    fn settings_commands() {
        let mut s = session(4, 4);
        s.run(Command::SwapColors).unwrap();
        assert_eq!(s.settings.color1, Rgba::WHITE);
        for _ in 0..100 {
            s.run(Command::SizeUp).unwrap();
        }
        assert_eq!(s.settings.size, MAX_BRUSH);
        for _ in 0..100 {
            s.run(Command::SizeDown).unwrap();
        }
        assert_eq!(s.settings.size, 1);
        s.run(Command::SymmetryKaleido12).unwrap();
        assert_eq!(s.is_checked(Command::SymmetryKaleido12), Some(true));
        s.run(Command::PixelArt).unwrap();
        assert_eq!(s.tool(), Tool::Pencil);
        assert!(!s.run(Command::Save).unwrap());
    }

    #[test]
    fn stabilized_spray_and_pencil_do_not_panic_at_edges() {
        let mut s = session(16, 16);
        s.settings.stabilizer = 80;
        for tool in [
            Command::ToolSpray,
            Command::ToolPencil,
            Command::ToolEraser,
            Command::ToolBrush,
        ] {
            s.run(tool).unwrap();
            drag(&mut s, (-50.0, -50.0), (500.0, 40.0), Button::Primary);
            drag(
                &mut s,
                (f32::NAN, 3.0),
                (3.0, f32::INFINITY),
                Button::Primary,
            );
        }
    }
}
