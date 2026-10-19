//! The command catalog: every action, with its label, menu and shortcut. Paint's own
//! shortcuts come from the accelerator table of the Paint installed on this PC
//! (REA evidence in docs/parity.md); the rest are HarshPaint's additions.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    // File
    New,
    Open,
    Save,
    SaveAs,
    ExportTimelapse,
    Exit,
    // Edit
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    DeleteSelection,
    Deselect,
    // Image
    Crop,
    Resize,
    CanvasSize,
    RotateRight,
    RotateLeft,
    Rotate180,
    FlipVertical,
    FlipHorizontal,
    InvertColors,
    // Layers
    LayerNew,
    LayerDuplicate,
    LayerDelete,
    LayerMergeDown,
    LayerUp,
    LayerDown,
    // Tools
    ToolPencil,
    ToolBrush,
    ToolEraser,
    ToolSpray,
    ToolFill,
    ToolPicker,
    ToolText,
    ToolSelect,
    ToolLine,
    ToolRectangle,
    ToolEllipse,
    SwapColors,
    SizeUp,
    SizeDown,
    // Symmetry and stabilizer
    SymmetryOff,
    SymmetryMirrorX,
    SymmetryMirrorY,
    SymmetryQuad,
    SymmetryRadial6,
    SymmetryRadial8,
    SymmetryKaleido6,
    SymmetryKaleido8,
    SymmetryKaleido12,
    StabilizerOff,
    StabilizerLow,
    StabilizerMedium,
    StabilizerHigh,
    // View
    ZoomIn,
    ZoomOut,
    Zoom100,
    ZoomFit,
    Gridlines,
    PixelArt,
    StatusBar,
    FullScreen,
    CommandPalette,
    // Help
    About,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    File,
    Edit,
    Image,
    Layers,
    Tools,
    Symmetry,
    View,
    Help,
}

impl Menu {
    pub const ALL: [Menu; 8] = [
        Menu::File,
        Menu::Edit,
        Menu::Image,
        Menu::Layers,
        Menu::Tools,
        Menu::Symmetry,
        Menu::View,
        Menu::Help,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Menu::File => "File",
            Menu::Edit => "Edit",
            Menu::Image => "Image",
            Menu::Layers => "Layers",
            Menu::Tools => "Tools",
            Menu::Symmetry => "Symmetry",
            Menu::View => "View",
            Menu::Help => "Help",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    A,
    B,
    C,
    E,
    F,
    G,
    I,
    J,
    K,
    L,
    N,
    O,
    P,
    R,
    S,
    T,
    V,
    W,
    X,
    Y,
    Z,
    Num0,
    Plus,
    Minus,
    PageUp,
    PageDown,
    Delete,
    Escape,
    F11,
    F12,
}

impl Key {
    pub fn label(self) -> &'static str {
        match self {
            Key::A => "A",
            Key::B => "B",
            Key::C => "C",
            Key::E => "E",
            Key::F => "F",
            Key::G => "G",
            Key::I => "I",
            Key::J => "J",
            Key::K => "K",
            Key::L => "L",
            Key::N => "N",
            Key::O => "O",
            Key::P => "P",
            Key::R => "R",
            Key::S => "S",
            Key::T => "T",
            Key::V => "V",
            Key::W => "W",
            Key::X => "X",
            Key::Y => "Y",
            Key::Z => "Z",
            Key::Num0 => "0",
            Key::Plus => "Plus",
            Key::Minus => "Minus",
            Key::PageUp => "PgUp",
            Key::PageDown => "PgDn",
            Key::Delete => "Del",
            Key::Escape => "Esc",
            Key::F11 => "F11",
            Key::F12 => "F12",
        }
    }

    /// A single letter typed without Ctrl; such shortcuts pause while typing text.
    pub fn is_letter(self) -> bool {
        self.label().len() == 1 && self.label().chars().all(|c| c.is_ascii_alphabetic())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub ctrl: bool,
    pub shift: bool,
    pub key: Key,
}

impl Shortcut {
    const fn key(key: Key) -> Self {
        Self {
            ctrl: false,
            shift: false,
            key,
        }
    }
    const fn ctrl(key: Key) -> Self {
        Self {
            ctrl: true,
            shift: false,
            key,
        }
    }
    const fn ctrl_shift(key: Key) -> Self {
        Self {
            ctrl: true,
            shift: true,
            key,
        }
    }

    /// More modifiers first, so Ctrl+X never swallows Ctrl+Shift+X.
    pub fn modifier_count(self) -> u8 {
        u8::from(self.ctrl) + u8::from(self.shift)
    }

    pub fn label(self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.shift {
            parts.push("Shift");
        }
        parts.push(self.key.label());
        parts.join("+")
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CommandSpec {
    pub command: Command,
    /// Stable id for the CLI, the command palette and agents (`area.action`).
    pub id: &'static str,
    pub label: &'static str,
    pub menu: Option<Menu>,
    pub shortcut: Option<Shortcut>,
    pub separator_before: bool,
}

const fn spec(
    command: Command,
    id: &'static str,
    label: &'static str,
    menu: Menu,
    shortcut: Option<Shortcut>,
) -> CommandSpec {
    CommandSpec {
        command,
        id,
        label,
        menu: Some(menu),
        shortcut,
        separator_before: false,
    }
}

const fn sep(mut s: CommandSpec) -> CommandSpec {
    s.separator_before = true;
    s
}

use Command as C;
use Key as K;
use Menu as M;
use Shortcut as S;

pub const CATALOG: &[CommandSpec] = &[
    spec(C::New, "file.new", "New", M::File, Some(S::ctrl(K::N))),
    spec(C::Open, "file.open", "Open…", M::File, Some(S::ctrl(K::O))),
    spec(C::Save, "file.save", "Save", M::File, Some(S::ctrl(K::S))),
    spec(
        C::SaveAs,
        "file.saveAs",
        "Save as…",
        M::File,
        Some(S::key(K::F12)),
    ),
    sep(spec(
        C::ExportTimelapse,
        "file.exportTimelapse",
        "Export time-lapse GIF…",
        M::File,
        Some(S::ctrl_shift(K::T)),
    )),
    sep(spec(C::Exit, "file.exit", "Exit", M::File, None)),
    spec(C::Undo, "edit.undo", "Undo", M::Edit, Some(S::ctrl(K::Z))),
    spec(C::Redo, "edit.redo", "Redo", M::Edit, Some(S::ctrl(K::Y))),
    sep(spec(
        C::Cut,
        "edit.cut",
        "Cut",
        M::Edit,
        Some(S::ctrl(K::X)),
    )),
    spec(C::Copy, "edit.copy", "Copy", M::Edit, Some(S::ctrl(K::C))),
    spec(
        C::Paste,
        "edit.paste",
        "Paste",
        M::Edit,
        Some(S::ctrl(K::V)),
    ),
    sep(spec(
        C::SelectAll,
        "edit.selectAll",
        "Select all",
        M::Edit,
        Some(S::ctrl(K::A)),
    )),
    spec(
        C::DeleteSelection,
        "edit.delete",
        "Delete selection",
        M::Edit,
        Some(S::key(K::Delete)),
    ),
    spec(
        C::Deselect,
        "edit.deselect",
        "Deselect",
        M::Edit,
        Some(S::key(K::Escape)),
    ),
    spec(
        C::Crop,
        "image.crop",
        "Crop to selection",
        M::Image,
        Some(S::ctrl_shift(K::X)),
    ),
    spec(
        C::Resize,
        "image.resize",
        "Resize…",
        M::Image,
        Some(S::ctrl(K::W)),
    ),
    spec(
        C::CanvasSize,
        "image.properties",
        "Canvas size…",
        M::Image,
        Some(S::ctrl(K::E)),
    ),
    sep(spec(
        C::RotateRight,
        "image.rotateRight",
        "Rotate right 90°",
        M::Image,
        None,
    )),
    spec(
        C::RotateLeft,
        "image.rotateLeft",
        "Rotate left 90°",
        M::Image,
        None,
    ),
    spec(
        C::Rotate180,
        "image.rotate180",
        "Rotate 180°",
        M::Image,
        None,
    ),
    spec(
        C::FlipVertical,
        "image.flipVertical",
        "Flip vertical",
        M::Image,
        None,
    ),
    spec(
        C::FlipHorizontal,
        "image.flipHorizontal",
        "Flip horizontal",
        M::Image,
        None,
    ),
    sep(spec(
        C::InvertColors,
        "image.invert",
        "Invert colours",
        M::Image,
        Some(S::ctrl_shift(K::I)),
    )),
    spec(
        C::LayerNew,
        "layer.new",
        "New layer",
        M::Layers,
        Some(S::ctrl_shift(K::N)),
    ),
    spec(
        C::LayerDuplicate,
        "layer.duplicate",
        "Duplicate layer",
        M::Layers,
        Some(S::ctrl(K::J)),
    ),
    spec(
        C::LayerDelete,
        "layer.delete",
        "Delete layer",
        M::Layers,
        None,
    ),
    spec(
        C::LayerMergeDown,
        "layer.mergeDown",
        "Merge down",
        M::Layers,
        Some(S::ctrl_shift(K::E)),
    ),
    sep(spec(
        C::LayerUp,
        "layer.up",
        "Move layer up",
        M::Layers,
        None,
    )),
    spec(
        C::LayerDown,
        "layer.down",
        "Move layer down",
        M::Layers,
        None,
    ),
    spec(
        C::ToolPencil,
        "tool.pencil",
        "Pencil",
        M::Tools,
        Some(S::key(K::P)),
    ),
    spec(
        C::ToolBrush,
        "tool.brush",
        "Brush",
        M::Tools,
        Some(S::key(K::B)),
    ),
    spec(
        C::ToolEraser,
        "tool.eraser",
        "Eraser",
        M::Tools,
        Some(S::key(K::E)),
    ),
    spec(
        C::ToolSpray,
        "tool.spray",
        "Spray",
        M::Tools,
        Some(S::key(K::A)),
    ),
    spec(
        C::ToolFill,
        "tool.fill",
        "Fill with colour",
        M::Tools,
        Some(S::key(K::F)),
    ),
    spec(
        C::ToolPicker,
        "tool.picker",
        "Colour picker",
        M::Tools,
        Some(S::key(K::I)),
    ),
    spec(
        C::ToolText,
        "tool.text",
        "Text",
        M::Tools,
        Some(S::key(K::T)),
    ),
    spec(
        C::ToolSelect,
        "tool.select",
        "Select",
        M::Tools,
        Some(S::key(K::S)),
    ),
    sep(spec(
        C::ToolLine,
        "tool.line",
        "Line",
        M::Tools,
        Some(S::key(K::L)),
    )),
    spec(
        C::ToolRectangle,
        "tool.rectangle",
        "Rectangle",
        M::Tools,
        Some(S::key(K::R)),
    ),
    spec(
        C::ToolEllipse,
        "tool.ellipse",
        "Oval",
        M::Tools,
        Some(S::key(K::O)),
    ),
    sep(spec(
        C::SwapColors,
        "tool.swapColors",
        "Swap colours",
        M::Tools,
        Some(S::key(K::X)),
    )),
    spec(
        C::SizeUp,
        "tool.sizeUp",
        "Bigger brush",
        M::Tools,
        Some(S::ctrl(K::Plus)),
    ),
    spec(
        C::SizeDown,
        "tool.sizeDown",
        "Smaller brush",
        M::Tools,
        Some(S::ctrl(K::Minus)),
    ),
    sep(spec(
        C::StabilizerOff,
        "stabilizer.off",
        "Stabilizer: off",
        M::Tools,
        None,
    )),
    spec(
        C::StabilizerLow,
        "stabilizer.low",
        "Stabilizer: low",
        M::Tools,
        None,
    ),
    spec(
        C::StabilizerMedium,
        "stabilizer.medium",
        "Stabilizer: medium",
        M::Tools,
        None,
    ),
    spec(
        C::StabilizerHigh,
        "stabilizer.high",
        "Stabilizer: high",
        M::Tools,
        None,
    ),
    spec(C::SymmetryOff, "symmetry.off", "Off", M::Symmetry, None),
    spec(
        C::SymmetryMirrorX,
        "symmetry.mirrorX",
        "Mirror left ↔ right",
        M::Symmetry,
        None,
    ),
    spec(
        C::SymmetryMirrorY,
        "symmetry.mirrorY",
        "Mirror top ↕ bottom",
        M::Symmetry,
        None,
    ),
    spec(
        C::SymmetryQuad,
        "symmetry.quad",
        "Four-way",
        M::Symmetry,
        None,
    ),
    sep(spec(
        C::SymmetryRadial6,
        "symmetry.radial6",
        "Radial ×6",
        M::Symmetry,
        None,
    )),
    spec(
        C::SymmetryRadial8,
        "symmetry.radial8",
        "Radial ×8",
        M::Symmetry,
        None,
    ),
    sep(spec(
        C::SymmetryKaleido6,
        "symmetry.kaleido6",
        "Kaleidoscope ×6",
        M::Symmetry,
        None,
    )),
    spec(
        C::SymmetryKaleido8,
        "symmetry.kaleido8",
        "Kaleidoscope ×8",
        M::Symmetry,
        None,
    ),
    spec(
        C::SymmetryKaleido12,
        "symmetry.kaleido12",
        "Kaleidoscope ×12",
        M::Symmetry,
        None,
    ),
    spec(
        C::ZoomIn,
        "view.zoomIn",
        "Zoom in",
        M::View,
        Some(S::ctrl(K::PageUp)),
    ),
    spec(
        C::ZoomOut,
        "view.zoomOut",
        "Zoom out",
        M::View,
        Some(S::ctrl(K::PageDown)),
    ),
    spec(
        C::Zoom100,
        "view.zoom100",
        "Zoom to 100%",
        M::View,
        Some(S::ctrl(K::Num0)),
    ),
    spec(C::ZoomFit, "view.zoomFit", "Fit to window", M::View, None),
    sep(spec(
        C::Gridlines,
        "view.gridlines",
        "Gridlines",
        M::View,
        Some(S::ctrl(K::G)),
    )),
    spec(
        C::PixelArt,
        "view.pixelArt",
        "Pixel-art mode",
        M::View,
        Some(S::ctrl_shift(K::P)),
    ),
    spec(C::StatusBar, "view.statusBar", "Status bar", M::View, None),
    spec(
        C::FullScreen,
        "view.fullScreen",
        "Full screen",
        M::View,
        Some(S::key(K::F11)),
    ),
    sep(spec(
        C::CommandPalette,
        "view.commandPalette",
        "Command palette…",
        M::View,
        Some(S::ctrl(K::K)),
    )),
    spec(C::About, "help.about", "About HarshPaint", M::Help, None),
];

pub fn spec_of(command: Command) -> Option<&'static CommandSpec> {
    CATALOG.iter().find(|s| s.command == command)
}

pub fn by_id(id: &str) -> Option<&'static CommandSpec> {
    CATALOG.iter().find(|s| s.id == id)
}

pub fn in_menu(menu: Menu) -> impl Iterator<Item = &'static CommandSpec> {
    CATALOG.iter().filter(move |s| s.menu == Some(menu))
}

/// Commands whose label or id contains every word of `query` (command palette).
pub fn search(query: &str) -> Vec<&'static CommandSpec> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    CATALOG
        .iter()
        .filter(|s| {
            let hay = format!("{} {}", s.label, s.id).to_lowercase();
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_commands_and_shortcuts_are_unique() {
        let ids: HashSet<_> = CATALOG.iter().map(|s| s.id).collect();
        assert_eq!(ids.len(), CATALOG.len());
        let commands: HashSet<_> = CATALOG.iter().map(|s| s.command).collect();
        assert_eq!(commands.len(), CATALOG.len());
        let shortcuts: Vec<_> = CATALOG.iter().filter_map(|s| s.shortcut).collect();
        let unique: HashSet<_> = shortcuts.iter().collect();
        assert_eq!(unique.len(), shortcuts.len());
    }

    #[test]
    fn paint_shortcuts_match_the_installed_paint() {
        // From Paint's accelerator table (see docs/parity.md).
        let expect = [
            ("file.saveAs", "F12"),
            ("image.crop", "Ctrl+Shift+X"),
            ("image.resize", "Ctrl+W"),
            ("image.properties", "Ctrl+E"),
            ("image.invert", "Ctrl+Shift+I"),
            ("view.gridlines", "Ctrl+G"),
            ("view.zoomIn", "Ctrl+PgUp"),
            ("view.zoomOut", "Ctrl+PgDn"),
            ("view.fullScreen", "F11"),
            ("tool.sizeUp", "Ctrl+Plus"),
        ];
        for (id, label) in expect {
            assert_eq!(
                by_id(id)
                    .and_then(|s| s.shortcut)
                    .map(Shortcut::label)
                    .as_deref(),
                Some(label),
                "{id}"
            );
        }
    }

    #[test]
    fn search_and_lookup() {
        assert!(
            search("kaleido 12")
                .iter()
                .any(|s| s.command == C::SymmetryKaleido12)
        );
        assert!(search("zzz").is_empty());
        assert_eq!(spec_of(C::Undo).map(|s| s.id), Some("edit.undo"));
        assert!(Key::P.is_letter() && !Key::F11.is_letter() && !Key::Num0.is_letter());
    }
}
