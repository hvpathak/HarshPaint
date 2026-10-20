//! The egui shell: draws the session and turns clicks, drags and key presses into
//! catalog commands and pointer events. Painting logic lives in `harshpaint-core`.

use std::borrow::Cow;
use std::cmp::Reverse;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use eframe::egui::{
    self, Align, Align2, Color32, ColorImage, CursorIcon, KeyboardShortcut, Layout, Modifiers,
    Pos2, Rect as ERect, Sense, Stroke, TextureHandle, TextureOptions, Vec2, pos2, vec2,
};
use harshpaint_core::canvas::Raster;
use harshpaint_core::color::{PAINT_PALETTE, Rgba};
use harshpaint_core::commands::{self, CATALOG, Command, Key, Menu, Shortcut};
use harshpaint_core::document::Document;
use harshpaint_core::io::{self, FileKind};
use harshpaint_core::session::{Button, Feedback, MAX_BRUSH, Mods, Session, Tool};
use harshpaint_core::shapes::{ShapeKind, ShapeStyle};
use harshpaint_core::symmetry::Symmetry;
use harshpaint_core::timelapse::{self, TimelapseOptions};
use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};

use crate::fonts::{self, TEXT_FONTS};

pub const APP_NAME: &str = "HarshPaint";
const ZOOMS: [f32; 13] = [
    0.125, 0.25, 0.5, 1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 16.0, 24.0, 32.0,
];
/// A deferred change to the session, applied after a dialog closes.
type SessionEdit = Box<dyn FnOnce(&mut Session) -> harshpaint_core::Result<()>>;

const ACCENT: Color32 = Color32::from_rgb(30, 136, 229);

struct View {
    zoom: f32,
    /// Canvas top-left relative to the canvas panel's top-left, in screen points.
    offset: Vec2,
    fit_pending: bool,
    panel: ERect,
}

struct TextBox {
    at: (f32, f32),
    content: String,
    family: usize,
    size: f32,
    focus: bool,
}

enum Dialog {
    New { w: u32, h: u32 },
    Resize { w: u32, h: u32, keep: bool },
    CanvasSize { w: u32, h: u32 },
    Palette { query: String, focus: bool },
}

pub struct PaintApp {
    session: Session,
    view: View,
    texture: Option<TextureHandle>,
    texture_key: (u32, u32, bool),
    checker: Option<TextureHandle>,
    show_grid: bool,
    show_status: bool,
    full_screen: bool,
    drawing: Option<Button>,
    last_pointer: Option<(f32, f32)>,
    hover: Option<(f32, f32)>,
    text_box: Option<TextBox>,
    dialog: Option<Dialog>,
    status: Option<String>,
    allow_close: bool,
    window_title: String,
    clipboard: Option<arboard::Clipboard>,
}

impl PaintApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial_file: Option<PathBuf>) -> Self {
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        fonts::install_ui_fonts(&cc.egui_ctx);
        let mut app = Self {
            session: Session::default(),
            view: View {
                zoom: 1.0,
                offset: Vec2::ZERO,
                fit_pending: true,
                panel: ERect::NOTHING,
            },
            texture: None,
            texture_key: (0, 0, false),
            checker: None,
            show_grid: false,
            show_status: true,
            full_screen: false,
            drawing: None,
            last_pointer: None,
            hover: None,
            text_box: None,
            dialog: None,
            status: None,
            allow_close: false,
            window_title: String::new(),
            clipboard: None,
        };
        if let Some(path) = initial_file {
            app.open_path(&path);
        }
        app
    }

    // ---- Commands ----------------------------------------------------------------

    /// Last-resort guard: a bug in a command shows a message and keeps the picture.
    fn dispatch(&mut self, command: Command, ctx: &egui::Context) {
        let outcome = panic::catch_unwind(AssertUnwindSafe(|| self.run(command, ctx)));
        if outcome.is_err() {
            let id = commands::spec_of(command).map_or("unknown", |s| s.id);
            show_error(&format!(
                "Something went wrong while running \"{id}\". Your picture is still here; please save it."
            ));
        }
    }

    fn run(&mut self, command: Command, ctx: &egui::Context) {
        self.status = None;
        match command {
            Command::New => {
                if self.confirm_discard() {
                    let (w, h) = (self.session.doc.width(), self.session.doc.height());
                    self.dialog = Some(Dialog::New { w, h });
                }
            }
            Command::Open => self.open_file(),
            Command::Save => {
                self.save();
            }
            Command::SaveAs => {
                self.save_as();
            }
            Command::ExportTimelapse => self.export_timelapse(),
            Command::Exit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Command::Cut => {
                if self.copy() {
                    let result = self.session.run(Command::DeleteSelection).map(|_| ());
                    self.report(result);
                }
            }
            Command::Copy => {
                self.copy();
            }
            Command::Paste => self.paste(),
            Command::Resize => {
                let (w, h) = (self.session.doc.width(), self.session.doc.height());
                self.dialog = Some(Dialog::Resize { w, h, keep: true });
            }
            Command::CanvasSize => {
                let (w, h) = (self.session.doc.width(), self.session.doc.height());
                self.dialog = Some(Dialog::CanvasSize { w, h });
            }
            Command::ZoomIn => self.step_zoom(true),
            Command::ZoomOut => self.step_zoom(false),
            Command::Zoom100 => {
                let c = self.view.panel.center();
                self.zoom_at(c, 1.0);
            }
            Command::ZoomFit => self.view.fit_pending = true,
            Command::Gridlines => self.show_grid = !self.show_grid,
            Command::StatusBar => self.show_status = !self.show_status,
            Command::FullScreen => {
                self.full_screen = !self.full_screen;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.full_screen));
            }
            Command::CommandPalette => {
                self.dialog = Some(Dialog::Palette {
                    query: String::new(),
                    focus: true,
                });
            }
            Command::About => show_about(),
            other => {
                let result = self.session.run(other).map(|_| ());
                self.report(result);
                if other == Command::PixelArt
                    && self.session.settings.pixel_art
                    && self.view.zoom < 4.0
                {
                    let c = self.view.panel.center();
                    self.zoom_at(c, 8.0);
                }
            }
        }
    }

    fn report(&mut self, result: harshpaint_core::Result<()>) {
        if let Err(e) = result {
            self.status = Some(capitalise(&e.to_string()));
        }
    }

    fn checked(&self, command: Command) -> Option<bool> {
        match command {
            Command::Gridlines => Some(self.show_grid),
            Command::StatusBar => Some(self.show_status),
            Command::FullScreen => Some(self.full_screen),
            _ => self.session.is_checked(command),
        }
    }

    // ---- Files ----------------------------------------------------------------------

    fn open_file(&mut self) {
        if !self.confirm_discard() {
            return;
        }
        let picked = FileDialog::new()
            .add_filter("Pictures", &["png", "jpg", "jpeg", "bmp", "gif", "hpaint"])
            .add_filter("HarshPaint project", &["hpaint"])
            .add_filter("All files", &["*"])
            .pick_file();
        if let Some(path) = picked {
            self.open_path(&path);
        }
    }

    fn open_path(&mut self, path: &Path) {
        match io::open(path) {
            Ok(doc) => {
                self.session.load(doc, Some(path.to_path_buf()));
                self.view.fit_pending = true;
                self.text_box = None;
            }
            Err(e) => show_error(&format!("Could not open {}:\n{e}", path.display())),
        }
    }

    /// Returns true if the picture was saved.
    fn save(&mut self) -> bool {
        match self.session.path.clone() {
            Some(path) if FileKind::from_path(&path).is_some() => self.save_to(path),
            _ => self.save_as(),
        }
    }

    fn save_as(&mut self) -> bool {
        let layered = self.session.doc.layers().len() > 1;
        let stem = self
            .session
            .path
            .as_deref()
            .and_then(Path::file_stem)
            .map_or_else(
                || "Untitled".to_owned(),
                |s| s.to_string_lossy().into_owned(),
            );
        let ext = if layered { "hpaint" } else { "png" };
        let picked = FileDialog::new()
            .add_filter("PNG", &["png"])
            .add_filter("HarshPaint project (keeps layers)", &["hpaint"])
            .add_filter("JPEG", &["jpg", "jpeg"])
            .add_filter("Bitmap", &["bmp"])
            .add_filter("GIF", &["gif"])
            .set_file_name(format!("{stem}.{ext}"))
            .save_file();
        let Some(mut path) = picked else { return false };
        if FileKind::from_path(&path).is_none() {
            path.set_extension(ext);
        }
        self.save_to(path)
    }

    fn save_to(&mut self, path: PathBuf) -> bool {
        match io::save(&self.session.doc, &path) {
            Ok(()) => {
                let flattened = self.session.doc.layers().len() > 1
                    && !FileKind::from_path(&path).is_some_and(FileKind::keeps_layers);
                self.status = Some(if flattened {
                    "Saved (layers were merged; save as .hpaint to keep them)".to_owned()
                } else {
                    "Saved".to_owned()
                });
                self.session.mark_saved(path);
                true
            }
            Err(e) => {
                show_error(&format!("Could not save {}:\n{e}", path.display()));
                false
            }
        }
    }

    fn export_timelapse(&mut self) {
        let gif = match timelapse::render(
            &self.session.doc,
            self.session.history(),
            &TimelapseOptions::default(),
        ) {
            Ok(gif) => gif,
            Err(e) => {
                self.status = Some(capitalise(&e.to_string()));
                return;
            }
        };
        let Some(mut path) = FileDialog::new()
            .add_filter("Animated GIF", &["gif"])
            .set_file_name("timelapse.gif")
            .save_file()
        else {
            return;
        };
        path.set_extension("gif");
        match io::write_atomically(&path, &gif) {
            Ok(()) => self.status = Some(format!("Time-lapse saved ({} KB)", gif.len() / 1024)),
            Err(e) => show_error(&format!("Could not save the time-lapse:\n{e}")),
        }
    }

    /// Asks to save unsaved changes. Returns false if the user cancelled.
    fn confirm_discard(&mut self) -> bool {
        if !self.session.is_modified() {
            return true;
        }
        let answer = MessageDialog::new()
            .set_level(MessageLevel::Warning)
            .set_title(APP_NAME)
            .set_description(format!(
                "Do you want to save changes to {}?",
                self.display_name()
            ))
            .set_buttons(MessageButtons::YesNoCancel)
            .show();
        match answer {
            MessageDialogResult::Yes => self.save(),
            MessageDialogResult::No => true,
            _ => false,
        }
    }

    fn display_name(&self) -> String {
        self.session
            .path
            .as_deref()
            .and_then(Path::file_name)
            .map_or_else(
                || "Untitled".to_owned(),
                |n| n.to_string_lossy().into_owned(),
            )
    }

    // ---- Clipboard --------------------------------------------------------------------

    fn clipboard(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        self.clipboard.as_mut()
    }

    fn copy(&mut self) -> bool {
        let piece = match self.session.copy_selection() {
            Ok(p) => p,
            Err(e) => {
                self.status = Some(capitalise(&e.to_string()));
                return false;
            }
        };
        let image = arboard::ImageData {
            width: piece.width() as usize,
            height: piece.height() as usize,
            bytes: Cow::Owned(piece.to_rgba8()),
        };
        let ok = self.clipboard().is_some_and(|c| c.set_image(image).is_ok());
        if !ok {
            self.status = Some("Couldn't reach the clipboard".to_owned());
        }
        ok
    }

    fn paste(&mut self) {
        let image = self.clipboard().and_then(|c| c.get_image().ok());
        let Some(image) = image else {
            self.status = Some("There's no picture on the clipboard".to_owned());
            return;
        };
        match Raster::from_rgba8(image.width as u32, image.height as u32, &image.bytes) {
            Ok(piece) => self.session.paste(piece),
            Err(e) => self.status = Some(capitalise(&e.to_string())),
        }
    }

    // ---- View -------------------------------------------------------------------------

    fn zoom_at(&mut self, anchor: Pos2, zoom: f32) {
        let zoom = zoom.clamp(0.05, 64.0);
        let rel = anchor - self.view.panel.min;
        let k = zoom / self.view.zoom;
        self.view.offset = rel - (rel - self.view.offset) * k;
        self.view.zoom = zoom;
    }

    fn step_zoom(&mut self, up: bool) {
        let z = self.view.zoom;
        let next = if up {
            ZOOMS.iter().copied().find(|&s| s > z * 1.01).unwrap_or(z)
        } else {
            ZOOMS
                .iter()
                .rev()
                .copied()
                .find(|&s| s < z * 0.99)
                .unwrap_or(z)
        };
        let c = self.view.panel.center();
        self.zoom_at(c, next);
    }

    fn fit(&mut self, panel: ERect) {
        let (w, h) = (
            self.session.doc.width() as f32,
            self.session.doc.height() as f32,
        );
        let zoom = ((panel.width() - 48.0) / w)
            .min((panel.height() - 48.0) / h)
            .clamp(0.05, 8.0);
        let zoom = if zoom > 1.0 { zoom.floor() } else { zoom };
        self.view.zoom = zoom;
        self.view.offset = (panel.size() - vec2(w * zoom, h * zoom)) / 2.0;
        self.view.fit_pending = false;
    }

    fn sync_texture(&mut self, ctx: &egui::Context) {
        let (w, h) = (self.session.doc.width(), self.session.doc.height());
        let nearest = self.view.zoom >= 2.0 || self.session.settings.pixel_art;
        let options = if nearest {
            TextureOptions::NEAREST
        } else {
            TextureOptions::LINEAR
        };
        if self.texture.is_none() || self.texture_key != (w, h, nearest) {
            let pixels = self.session.doc.composite_rect(self.session.doc.bounds());
            let image = color_image(w, h, &pixels);
            self.texture = Some(ctx.load_texture("canvas", image, options));
            self.texture_key = (w, h, nearest);
            self.session.take_dirty();
        } else {
            // Upload each changed area on its own: a symmetric stroke touches small
            // patches all over the picture, not one big rectangle.
            for r in self.session.take_dirty() {
                let r = r.clip(w, h);
                if !r.is_empty()
                    && let Some(t) = self.texture.as_mut()
                {
                    let pixels = self.session.doc.composite_rect(r);
                    t.set_partial(
                        [r.x as usize, r.y as usize],
                        color_image(r.w, r.h, &pixels),
                        options,
                    );
                }
            }
        }
        if self.checker.is_none() {
            let light = Color32::from_gray(238);
            let dark = Color32::from_gray(204);
            let image = ColorImage {
                size: [2, 2],
                pixels: vec![light, dark, dark, light],
            };
            self.checker = Some(ctx.load_texture("checker", image, TextureOptions::NEAREST_REPEAT));
        }
    }

    // ---- Panels ---------------------------------------------------------------------------

    fn menu_bar(&mut self, ui: &mut egui::Ui) -> Option<Command> {
        let mut clicked = None;
        egui::menu::bar(ui, |ui| {
            for menu in Menu::ALL {
                ui.menu_button(menu.label(), |ui| {
                    for spec in commands::in_menu(menu) {
                        if spec.separator_before {
                            ui.separator();
                        }
                        let shortcut = spec.shortcut.map(Shortcut::label).unwrap_or_default();
                        let mut button = egui::Button::new(spec.label).shortcut_text(shortcut);
                        if let Some(on) = self.checked(spec.command) {
                            button = button.selected(on);
                        }
                        if ui.add(button).clicked() {
                            clicked = Some(spec.command);
                            ui.close_menu();
                        }
                    }
                });
            }
        });
        clicked
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) -> Option<Command> {
        let mut clicked = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            // Tools
            for (command, label) in [
                (Command::ToolSelect, "⬚ Select"),
                (Command::ToolPencil, "✏ Pencil"),
                (Command::ToolBrush, "🖌 Brush"),
                (Command::ToolEraser, "⌫ Eraser"),
                (Command::ToolSpray, "∴ Spray"),
                (Command::ToolFill, "◍ Fill"),
                (Command::ToolPicker, "⊙ Pick"),
                (Command::ToolText, "A Text"),
            ] {
                let on = self.checked(command).unwrap_or(false);
                let tip = commands::spec_of(command).map_or(String::new(), |s| {
                    let key = s
                        .shortcut
                        .map(|k| format!(" ({})", k.label()))
                        .unwrap_or_default();
                    format!("{}{key}", s.label)
                });
                if ui.selectable_label(on, label).on_hover_text(tip).clicked() {
                    clicked = Some(command);
                }
            }
            ui.separator();

            // Shapes
            let shape_on = self.session.tool() == Tool::Shape;
            let s = &mut self.session.settings;
            let mut shape = s.shape;
            egui::ComboBox::from_id_salt("shape")
                .selected_text(if shape_on {
                    format!("◆ {}", shape.label())
                } else {
                    "◆ Shapes".to_owned()
                })
                .show_ui(ui, |ui| {
                    for kind in ShapeKind::ALL {
                        ui.selectable_value(&mut shape, kind, kind.label());
                    }
                });
            if shape != s.shape {
                self.session.set_shape(shape);
            }
            let s = &mut self.session.settings;
            egui::ComboBox::from_id_salt("style")
                .selected_text(s.shape_style.label())
                .show_ui(ui, |ui| {
                    for style in [ShapeStyle::Outline, ShapeStyle::Fill, ShapeStyle::Both] {
                        ui.selectable_value(&mut s.shape_style, style, style.label());
                    }
                });
            ui.separator();

            // Size and tool options
            ui.label("Size");
            ui.add(
                egui::Slider::new(&mut s.size, 1..=MAX_BRUSH)
                    .logarithmic(true)
                    .suffix(" px"),
            );
            match self.session.tool() {
                Tool::Brush => {
                    let s = &mut self.session.settings;
                    ui.label("Hardness");
                    ui.add(egui::Slider::new(&mut s.hardness, 0.0..=1.0).fixed_decimals(2));
                }
                Tool::Fill | Tool::Eraser => {
                    let s = &mut self.session.settings;
                    ui.label("Tolerance");
                    ui.add(egui::Slider::new(&mut s.tolerance, 0..=255));
                }
                Tool::Select => {
                    let s = &mut self.session.settings;
                    ui.checkbox(&mut s.transparent_selection, "Transparent selection");
                }
                _ => {}
            }
        });

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let s = &mut self.session.settings;
            // Signature features, front and centre.
            ui.label(egui::RichText::new("✨").color(ACCENT));
            egui::ComboBox::from_id_salt("symmetry")
                .selected_text(format!("Symmetry: {}", s.symmetry.label()))
                .show_ui(ui, |ui| {
                    for mode in Symmetry::ALL {
                        ui.selectable_value(&mut s.symmetry, mode, mode.label());
                    }
                });
            ui.label("Smoothing");
            ui.add(egui::Slider::new(&mut s.stabilizer, 0..=100).suffix("%"));
            let pixel_art = s.pixel_art;
            if ui
                .selectable_label(pixel_art, "▦ Pixel art")
                .on_hover_text("Crisp pixels and a grid when zoomed in (Ctrl+Shift+P)")
                .clicked()
            {
                clicked = Some(Command::PixelArt);
            }
            ui.separator();

            // Colours: colour 1 / colour 2 like Paint, then the palette.
            let s = &mut self.session.settings;
            let mut c1 = [s.color1.r, s.color1.g, s.color1.b];
            let mut c2 = [s.color2.r, s.color2.g, s.color2.b];
            ui.label("Colour 1");
            if egui::color_picker::color_edit_button_srgb(ui, &mut c1).changed() {
                s.color1 = Rgba::rgb(c1[0], c1[1], c1[2]);
            }
            if ui
                .small_button("⇄")
                .on_hover_text("Swap colours (X)")
                .clicked()
            {
                clicked = Some(Command::SwapColors);
            }
            ui.label("Colour 2");
            if egui::color_picker::color_edit_button_srgb(ui, &mut c2).changed() {
                s.color2 = Rgba::rgb(c2[0], c2[1], c2[2]);
            }
            ui.add_space(6.0);
            for c in PAINT_PALETTE {
                let swatch = egui::Button::new("")
                    .fill(Color32::from_rgb(c.r, c.g, c.b))
                    .min_size(vec2(18.0, 18.0))
                    .corner_radius(3.0);
                let r = ui
                    .add(swatch)
                    .on_hover_text("Left click: colour 1 · Right click: colour 2");
                if r.clicked() {
                    s.color1 = c;
                }
                if r.secondary_clicked() {
                    s.color2 = c;
                }
            }
        });
        clicked
    }

    fn layers_panel(&mut self, ui: &mut egui::Ui) -> Option<Command> {
        let mut clicked = None;
        ui.add_space(4.0);
        ui.heading("Layers");
        ui.horizontal_wrapped(|ui| {
            for (command, label, tip) in [
                (Command::LayerNew, "+ New", "New layer (Ctrl+Shift+N)"),
                (Command::LayerDuplicate, "⧉", "Duplicate (Ctrl+J)"),
                (Command::LayerMergeDown, "⤓", "Merge down (Ctrl+Shift+E)"),
                (Command::LayerUp, "⬆", "Move up"),
                (Command::LayerDown, "⬇", "Move down"),
                (Command::LayerDelete, "🗑", "Delete layer"),
            ] {
                if ui.button(label).on_hover_text(tip).clicked() {
                    clicked = Some(command);
                }
            }
        });
        ui.separator();
        let active = self.session.doc.active_index();
        let count = self.session.doc.layers().len();
        egui::ScrollArea::vertical()
            .max_height(260.0)
            .show(ui, |ui| {
                for i in (0..count).rev() {
                    let Some(layer) = self.session.doc.layer(i) else {
                        continue;
                    };
                    let (name, mut visible) = (layer.name.clone(), layer.visible);
                    ui.horizontal(|ui| {
                        if ui
                            .checkbox(&mut visible, "")
                            .on_hover_text("Show / hide")
                            .changed()
                        {
                            self.session.set_layer_visible(i, visible);
                        }
                        let label = egui::RichText::new(name).strong();
                        if ui.selectable_label(i == active, label).clicked() {
                            self.session.select_layer(i);
                        }
                    });
                }
            });
        if let Some(layer) = self.session.doc.active() {
            let mut opacity = layer.opacity * 100.0;
            ui.horizontal(|ui| {
                ui.label("Opacity");
                if ui
                    .add(
                        egui::Slider::new(&mut opacity, 0.0..=100.0)
                            .suffix("%")
                            .fixed_decimals(0),
                    )
                    .changed()
                {
                    self.session.set_layer_opacity(active, opacity / 100.0);
                }
            });
        }
        ui.add_space(12.0);
        ui.separator();
        ui.heading("Share");
        ui.label(format!("{} steps recorded", self.session.history().len()));
        if ui
            .add(egui::Button::new("🎞 Export time-lapse GIF").fill(ACCENT.gamma_multiply(0.35)))
            .on_hover_text("Watch your drawing paint itself (Ctrl+Shift+T)")
            .clicked()
        {
            clicked = Some(Command::ExportTimelapse);
        }
        clicked
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some((x, y)) = self.hover {
                ui.label(format!("📍 {}, {} px", x.floor() as i64, y.floor() as i64));
                ui.separator();
            }
            if let Some((_, _, w, h)) = self.session.selection_outline() {
                ui.label(format!("⬚ {} × {}", w as u32, h as u32));
                ui.separator();
            }
            ui.label(format!(
                "🖼 {} × {} px",
                self.session.doc.width(),
                self.session.doc.height()
            ));
            ui.separator();
            ui.label(format!(
                "{} · {}",
                self.session.tool().label(),
                self.session.settings.symmetry.label()
            ));
            if let Some(msg) = &self.status {
                ui.separator();
                ui.label(egui::RichText::new(msg).color(ACCENT));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("+").clicked() {
                    self.step_zoom(true);
                }
                ui.label(format!("{:.0}%", self.view.zoom * 100.0));
                if ui.small_button("-").clicked() {
                    self.step_zoom(false);
                }
                if ui.small_button("Fit").clicked() {
                    self.view.fit_pending = true;
                }
            });
        });
    }

    // ---- Canvas ---------------------------------------------------------------------------

    fn canvas(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let (panel, response) =
            ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.view.panel = panel;
        if self.view.fit_pending && panel.width() > 64.0 {
            self.fit(panel);
        }
        self.sync_texture(ctx);

        let zoom = self.view.zoom;
        let (w, h) = (
            self.session.doc.width() as f32,
            self.session.doc.height() as f32,
        );
        let origin = panel.min + self.view.offset;
        let canvas = ERect::from_min_size(origin, vec2(w * zoom, h * zoom));
        let to_image = |p: Pos2| ((p.x - origin.x) / zoom, (p.y - origin.y) / zoom);
        let to_screen = |x: f32, y: f32| pos2(origin.x + x * zoom, origin.y + y * zoom);
        let painter = ui.painter_at(panel);

        painter.rect_filled(
            canvas.translate(vec2(4.0, 5.0)),
            2.0,
            Color32::from_black_alpha(70),
        );
        if let Some(ch) = &self.checker {
            let uv = ERect::from_min_max(
                Pos2::ZERO,
                pos2(canvas.width() / 16.0, canvas.height() / 16.0),
            );
            painter.image(ch.id(), canvas, uv, Color32::WHITE);
        }
        if let Some(t) = &self.texture {
            painter.image(
                t.id(),
                canvas,
                ERect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }

        // Pixel grid
        if (self.show_grid || self.session.settings.pixel_art) && zoom >= 4.0 {
            let vis = canvas.intersect(panel);
            let grid = Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(120, 120, 120, 70));
            let x0 = ((vis.min.x - origin.x) / zoom).floor().max(0.0) as u32;
            let x1 = ((vis.max.x - origin.x) / zoom).ceil().min(w) as u32;
            let y0 = ((vis.min.y - origin.y) / zoom).floor().max(0.0) as u32;
            let y1 = ((vis.max.y - origin.y) / zoom).ceil().min(h) as u32;
            for x in x0..=x1 {
                let sx = origin.x + x as f32 * zoom;
                painter.line_segment([pos2(sx, vis.min.y), pos2(sx, vis.max.y)], grid);
            }
            for y in y0..=y1 {
                let sy = origin.y + y as f32 * zoom;
                painter.line_segment([pos2(vis.min.x, sy), pos2(vis.max.x, sy)], grid);
            }
        }

        // Symmetry guides
        let symmetry = self.session.settings.symmetry;
        if symmetry != Symmetry::Off {
            let (cx, cy) = self.session.centre();
            let c = to_screen(cx, cy);
            let guide = Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(30, 136, 229, 110));
            let reach = (w.max(h)) * zoom;
            let spokes: Vec<f32> = match symmetry {
                Symmetry::MirrorX => vec![90.0],
                Symmetry::MirrorY => vec![0.0],
                Symmetry::Quad => vec![0.0, 90.0],
                Symmetry::Radial { arms, .. } => (0..arms.clamp(2, 64))
                    .map(|k| 360.0 * f32::from(k) / f32::from(arms.clamp(2, 64)) - 90.0)
                    .collect(),
                Symmetry::Off => vec![],
            };
            let radial = matches!(symmetry, Symmetry::Radial { .. });
            for deg in spokes {
                let d = Vec2::angled(deg.to_radians()) * reach;
                let start = if radial { c } else { c - d };
                painter.line_segment([start, c + d], guide);
            }
            painter.circle_filled(c, 3.0, ACCENT);
        }

        // Selection (marching ants)
        if let Some((x, y, sw, sh)) = self.session.selection_outline() {
            let r = ERect::from_min_max(to_screen(x, y), to_screen(x + sw, y + sh));
            let pts = [
                r.left_top(),
                r.right_top(),
                r.right_bottom(),
                r.left_bottom(),
                r.left_top(),
            ];
            painter.add(egui::Shape::line(
                pts.to_vec(),
                Stroke::new(1.0_f32, Color32::WHITE),
            ));
            painter.extend(egui::Shape::dashed_line(
                &pts,
                Stroke::new(1.0_f32, Color32::BLACK),
                4.0,
                4.0,
            ));
        }

        self.handle_canvas_input(ctx, &response, panel, &to_image);

        // Brush outline plus ghost cursors where symmetry will paint.
        if let (Some((ix, iy)), true) = (self.hover, response.hovered()) {
            let tool = self.session.tool();
            if matches!(
                tool,
                Tool::Brush | Tool::Pencil | Tool::Eraser | Tool::Spray
            ) {
                let radius = (self.session.settings.size as f32 / 2.0 * zoom).max(2.0);
                let points = symmetry.map((ix, iy), self.session.centre());
                for (i, (px, py)) in points.into_iter().enumerate() {
                    let p = to_screen(px, py);
                    let alpha = if i == 0 { 220 } else { 90 };
                    let ring =
                        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(0, 0, 0, alpha));
                    if tool == Tool::Eraser {
                        painter.rect_stroke(
                            ERect::from_center_size(p, Vec2::splat(radius * 2.0)),
                            0.0,
                            ring,
                            egui::StrokeKind::Middle,
                        );
                    } else {
                        painter.circle_stroke(p, radius, ring);
                        painter.circle_stroke(
                            p,
                            radius + 1.0,
                            Stroke::new(
                                1.0_f32,
                                Color32::from_rgba_unmultiplied(255, 255, 255, alpha),
                            ),
                        );
                    }
                }
            }
            ctx.set_cursor_icon(match tool {
                Tool::Text => CursorIcon::Text,
                Tool::Select if self.session.has_selection() => CursorIcon::Move,
                _ => CursorIcon::Crosshair,
            });
        }

        self.text_box_ui(ctx, &to_screen);
    }

    fn handle_canvas_input(
        &mut self,
        ctx: &egui::Context,
        response: &egui::Response,
        panel: ERect,
        to_image: &dyn Fn(Pos2) -> (f32, f32),
    ) {
        let (
            pos,
            primary_pressed,
            secondary_pressed,
            primary_down,
            secondary_down,
            middle_down,
            delta,
            scroll,
            zoom_delta,
            mods,
            space,
        ) = ctx.input(|i| {
            (
                i.pointer.latest_pos(),
                i.pointer.button_pressed(egui::PointerButton::Primary),
                i.pointer.button_pressed(egui::PointerButton::Secondary),
                i.pointer.primary_down(),
                i.pointer.secondary_down(),
                i.pointer.middle_down(),
                i.pointer.delta(),
                i.smooth_scroll_delta,
                i.zoom_delta(),
                i.modifiers,
                i.key_down(egui::Key::Space),
            )
        });
        let mods = Mods {
            shift: mods.shift,
            ctrl: mods.command,
        };
        self.hover = pos.filter(|p| panel.contains(*p)).map(to_image);

        if response.hovered() {
            if (zoom_delta - 1.0).abs() > f32::EPSILON {
                if let Some(p) = pos {
                    let z = self.view.zoom * zoom_delta;
                    self.zoom_at(p, z);
                }
            } else if scroll != Vec2::ZERO {
                self.view.offset += scroll;
            }
        }
        // Pan with the middle button, or Space + drag.
        if self.drawing.is_none() && (middle_down || (space && primary_down)) && response.hovered()
        {
            self.view.offset += delta;
            ctx.set_cursor_icon(CursorIcon::Grabbing);
            return;
        }

        if self.drawing.is_none() && response.hovered() && self.dialog.is_none() {
            let button = if primary_pressed {
                Some(Button::Primary)
            } else if secondary_pressed {
                Some(Button::Secondary)
            } else {
                None
            };
            if let (Some(button), Some(p)) = (button, pos) {
                if self.text_box.is_some() {
                    self.commit_text();
                }
                let at = to_image(p);
                match self.session.pointer_down(at, button, mods) {
                    Feedback::PlaceText(at) => {
                        self.text_box = Some(TextBox {
                            at,
                            content: String::new(),
                            family: 0,
                            size: 32.0,
                            focus: true,
                        });
                    }
                    Feedback::Picked(_) => {}
                    Feedback::None => {
                        self.drawing = Some(button);
                        self.last_pointer = Some(at);
                    }
                }
            }
        }
        if let Some(button) = self.drawing {
            if let Some(p) = pos {
                let at = to_image(p);
                if self.last_pointer != Some(at) {
                    self.session.pointer_move(at, mods);
                    self.last_pointer = Some(at);
                }
            }
            let released = match button {
                Button::Primary => !primary_down,
                Button::Secondary => !secondary_down,
            };
            if released {
                let at = self.last_pointer.unwrap_or((0.0, 0.0));
                self.session.pointer_up(at, mods);
                self.drawing = None;
                self.last_pointer = None;
            }
        }
    }

    fn text_box_ui(&mut self, ctx: &egui::Context, to_screen: &dyn Fn(f32, f32) -> Pos2) {
        let Some(tb) = self.text_box.as_mut() else {
            return;
        };
        let mut place = false;
        let mut cancel = false;
        egui::Area::new(egui::Id::new("text-box"))
            .fixed_pos(to_screen(tb.at.0, tb.at.1))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    let edit = ui.add(
                        egui::TextEdit::multiline(&mut tb.content)
                            .desired_rows(2)
                            .desired_width(280.0)
                            .hint_text("Type, then press Ctrl+Enter"),
                    );
                    if std::mem::take(&mut tb.focus) {
                        edit.request_focus();
                    }
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("font")
                            .selected_text(TEXT_FONTS.get(tb.family).map_or("Font", |f| f.name))
                            .show_ui(ui, |ui| {
                                for (i, f) in TEXT_FONTS.iter().enumerate() {
                                    ui.selectable_value(&mut tb.family, i, f.name);
                                }
                            });
                        ui.add(
                            egui::DragValue::new(&mut tb.size)
                                .range(4.0..=500.0)
                                .suffix(" px"),
                        );
                        place |= ui.button("Place").clicked();
                        cancel |= ui.button("Cancel").clicked();
                    });
                    ui.input(|i| {
                        place |= i.modifiers.command && i.key_pressed(egui::Key::Enter);
                        cancel |= i.key_pressed(egui::Key::Escape);
                    });
                });
            });
        if place {
            self.commit_text();
        } else if cancel {
            self.text_box = None;
        }
    }

    fn commit_text(&mut self) {
        let Some(tb) = self.text_box.take() else {
            return;
        };
        if tb.content.trim().is_empty() {
            return;
        }
        let Some(font) = TEXT_FONTS.get(tb.family).and_then(fonts::read) else {
            self.status =
                Some("That font isn't installed on this computer; pick another".to_owned());
            return;
        };
        let result = self.session.place_text(&tb.content, &font, tb.size, tb.at);
        self.report(result);
    }

    // ---- Dialogs --------------------------------------------------------------------------

    fn dialogs(&mut self, ctx: &egui::Context) -> Option<Command> {
        let mut run = None;
        let mut close = false;
        let mut apply: Option<SessionEdit> = None;
        let mut new_size = None;
        match self.dialog.as_mut() {
            None => return None,
            Some(Dialog::Palette { query, focus }) => {
                egui::Window::new("Command palette")
                    .anchor(Align2::CENTER_TOP, vec2(0.0, 80.0))
                    .collapsible(false)
                    .resizable(false)
                    .show(ctx, |ui| {
                        let edit = ui.add(
                            egui::TextEdit::singleline(query)
                                .hint_text("Type a command…")
                                .desired_width(360.0),
                        );
                        if std::mem::take(focus) {
                            edit.request_focus();
                        }
                        let found = commands::search(query);
                        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if enter && let Some(first) = found.first() {
                            run = Some(first.command);
                        }
                        egui::ScrollArea::vertical()
                            .max_height(320.0)
                            .show(ui, |ui| {
                                for spec in found.iter().take(30) {
                                    let key =
                                        spec.shortcut.map(Shortcut::label).unwrap_or_default();
                                    if ui
                                        .add(
                                            egui::Button::new(spec.label)
                                                .shortcut_text(key)
                                                .min_size(vec2(360.0, 0.0)),
                                        )
                                        .clicked()
                                    {
                                        run = Some(spec.command);
                                    }
                                }
                            });
                        close |= ui.input(|i| i.key_pressed(egui::Key::Escape));
                    });
                close |= run.is_some();
            }
            Some(Dialog::New { w, h }) => {
                if size_window(ctx, "New picture", w, h, None, &mut close) {
                    new_size = Some((*w, *h));
                }
            }
            Some(Dialog::Resize { w, h, keep }) => {
                let (ow, oh) = (self.session.doc.width(), self.session.doc.height());
                if size_window(ctx, "Resize", w, h, Some((keep, ow, oh)), &mut close) {
                    let (nw, nh) = (*w, *h);
                    apply = Some(Box::new(move |s: &mut Session| s.resize(nw, nh)));
                }
            }
            Some(Dialog::CanvasSize { w, h }) => {
                if size_window(ctx, "Canvas size", w, h, None, &mut close) {
                    let (nw, nh) = (*w, *h);
                    apply = Some(Box::new(move |s: &mut Session| s.canvas_size(nw, nh)));
                }
            }
        }
        if let Some((w, h)) = new_size {
            match Document::new(w, h, Rgba::WHITE) {
                Ok(doc) => {
                    self.session.load(doc, None);
                    self.view.fit_pending = true;
                }
                Err(e) => show_error(&capitalise(&e.to_string())),
            }
            close = true;
        }
        if let Some(f) = apply {
            let result = f(&mut self.session);
            self.report(result);
            self.view.fit_pending = true;
            close = true;
        }
        if close {
            self.dialog = None;
        }
        run
    }

    // ---- Per-frame input --------------------------------------------------------------------

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let typing = ctx.wants_keyboard_input() || self.text_box.is_some() || self.dialog.is_some();
        let mut bound: Vec<_> = CATALOG
            .iter()
            .filter_map(|s| s.shortcut.map(|k| (s.command, k)))
            .collect();
        bound.sort_by_key(|(_, k)| Reverse(k.modifier_count()));
        for (command, shortcut) in bound {
            // While typing, leave letters, Delete, Esc and text-editing keys to the text box.
            let text_key = !shortcut.ctrl
                || matches!(
                    command,
                    Command::Cut
                        | Command::Copy
                        | Command::Paste
                        | Command::SelectAll
                        | Command::Undo
                        | Command::Redo
                );
            if typing && text_key {
                continue;
            }
            if consume(ctx, shortcut) {
                self.dispatch(command, ctx);
            }
        }
    }

    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            if self.confirm_discard() {
                self.allow_close = true;
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone()));
        if let Some(path) = dropped
            && self.confirm_discard()
        {
            self.open_path(&path);
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let dirty = if self.session.is_modified() { "*" } else { "" };
        let title = format!("{dirty}{} - {APP_NAME}", self.display_name());
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }
}

impl eframe::App for PaintApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_close_request(ctx);
        self.handle_dropped_files(ctx);
        self.handle_shortcuts(ctx);
        self.update_title(ctx);

        let mut clicked = None;
        egui::TopBottomPanel::top("menu").show(ctx, |ui| clicked = self.menu_bar(ui));
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.add_space(3.0);
            clicked = clicked.or(self.toolbar(ui));
            ui.add_space(2.0);
        });
        if self.show_status {
            egui::TopBottomPanel::bottom("status").show(ctx, |ui| self.status_bar(ui));
        }
        egui::SidePanel::right("layers")
            .default_width(210.0)
            .resizable(true)
            .show(ctx, |ui| {
                clicked = clicked.or(self.layers_panel(ui));
            });
        let backdrop = if ctx.style().visuals.dark_mode {
            Color32::from_gray(38)
        } else {
            Color32::from_gray(205)
        };
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(backdrop))
            .show(ctx, |ui| self.canvas(ui, ctx));
        if let Some(c) = self.dialogs(ctx) {
            clicked = clicked.or(Some(c));
        }

        // Run after drawing so dialogs never open from inside a menu.
        if let Some(command) = clicked {
            self.dispatch(command, ctx);
        }
    }
}

/// A width × height dialog (New, Resize, Canvas size). Returns true when OK is pressed.
fn size_window(
    ctx: &egui::Context,
    title: &str,
    w: &mut u32,
    h: &mut u32,
    keep_ratio: Option<(&mut bool, u32, u32)>,
    close: &mut bool,
) -> bool {
    let mut ok = false;
    egui::Window::new(title)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            let max = harshpaint_core::canvas::MAX_SIDE;
            let (old_w, old_h) = (*w, *h);
            egui::Grid::new(title).num_columns(2).show(ui, |ui| {
                ui.label("Width");
                ui.add(egui::DragValue::new(w).range(1..=max).suffix(" px"));
                ui.end_row();
                ui.label("Height");
                ui.add(egui::DragValue::new(h).range(1..=max).suffix(" px"));
                ui.end_row();
            });
            if let Some((keep, ow, oh)) = keep_ratio {
                ui.checkbox(keep, "Keep aspect ratio");
                if *keep && ow > 0 && oh > 0 {
                    if *w != old_w {
                        *h = ((*w as f32 * oh as f32 / ow as f32).round() as u32).clamp(1, max);
                    } else if *h != old_h {
                        *w = ((*h as f32 * ow as f32 / oh as f32).round() as u32).clamp(1, max);
                    }
                }
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    ok = true;
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    *close = true;
                }
            });
        });
    ok
}

fn color_image(w: u32, h: u32, pixels: &[Rgba]) -> ColorImage {
    let bytes: Vec<u8> = pixels.iter().flat_map(|p| [p.r, p.g, p.b, p.a]).collect();
    ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &bytes)
}

/// Consumes the shortcut if it was pressed this frame.
fn consume(ctx: &egui::Context, shortcut: Shortcut) -> bool {
    let mut modifiers = Modifiers::NONE;
    if shortcut.ctrl {
        modifiers |= Modifiers::COMMAND;
    }
    if shortcut.shift {
        modifiers |= Modifiers::SHIFT;
    }
    egui_keys(shortcut.key)
        .iter()
        .any(|key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(modifiers, *key))))
}

fn egui_keys(key: Key) -> &'static [egui::Key] {
    use egui::Key as E;
    match key {
        Key::A => &[E::A],
        Key::B => &[E::B],
        Key::C => &[E::C],
        Key::E => &[E::E],
        Key::F => &[E::F],
        Key::G => &[E::G],
        Key::I => &[E::I],
        Key::J => &[E::J],
        Key::K => &[E::K],
        Key::L => &[E::L],
        Key::N => &[E::N],
        Key::O => &[E::O],
        Key::P => &[E::P],
        Key::R => &[E::R],
        Key::S => &[E::S],
        Key::T => &[E::T],
        Key::V => &[E::V],
        Key::W => &[E::W],
        Key::X => &[E::X],
        Key::Y => &[E::Y],
        Key::Z => &[E::Z],
        Key::Num0 => &[E::Num0],
        // "=" is the unshifted "+" key on most layouts.
        Key::Plus => &[E::Plus, E::Equals],
        Key::Minus => &[E::Minus],
        Key::PageUp => &[E::PageUp],
        Key::PageDown => &[E::PageDown],
        Key::Delete => &[E::Delete],
        Key::Escape => &[E::Escape],
        Key::F11 => &[E::F11],
        Key::F12 => &[E::F12],
    }
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

fn show_error(message: &str) {
    MessageDialog::new()
        .set_level(MessageLevel::Error)
        .set_title(APP_NAME)
        .set_description(message)
        .show();
}

fn show_about() {
    MessageDialog::new()
        .set_title(APP_NAME)
        .set_description(format!(
            "{APP_NAME} {}\n\nA fast, friendly paint app written in Rust.\n\
             Layers · Symmetry · Smooth strokes · Pixel art · Time-lapse GIFs\n\n\
             Left click paints with colour 1, right click with colour 2.\n\
             Ctrl+K opens the command palette.",
            env!("CARGO_PKG_VERSION")
        ))
        .show();
}
