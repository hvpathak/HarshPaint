//! Fonts the computer already has, on Windows, macOS and Linux. HarshPaint ships
//! no font files: the interface and the Text tool use these, and fall back to
//! egui's built-in font when none are found.

use std::path::{Path, PathBuf};

/// A font family offered by the Text tool, with file names to look for on each OS.
pub struct FontFamily {
    pub name: &'static str,
    candidates: &'static [&'static str],
}

pub const TEXT_FONTS: &[FontFamily] = &[
    FontFamily {
        name: "Sans",
        candidates: &[
            "segoeui.ttf",
            "arial.ttf",
            "Arial.ttf",
            "SFNS.ttf",
            "Helvetica.ttc",
            "DejaVuSans.ttf",
            "LiberationSans-Regular.ttf",
            "NotoSans-Regular.ttf",
        ],
    },
    FontFamily {
        name: "Sans Bold",
        candidates: &[
            "segoeuib.ttf",
            "arialbd.ttf",
            "Arial Bold.ttf",
            "DejaVuSans-Bold.ttf",
            "LiberationSans-Bold.ttf",
            "NotoSans-Bold.ttf",
        ],
    },
    FontFamily {
        name: "Serif",
        candidates: &[
            "times.ttf",
            "Times New Roman.ttf",
            "Times.ttc",
            "DejaVuSerif.ttf",
            "LiberationSerif-Regular.ttf",
            "NotoSerif-Regular.ttf",
        ],
    },
    FontFamily {
        name: "Mono",
        candidates: &[
            "consola.ttf",
            "Menlo.ttc",
            "DejaVuSansMono.ttf",
            "LiberationMono-Regular.ttf",
            "NotoSansMono-Regular.ttf",
        ],
    },
    FontFamily {
        name: "Comic",
        candidates: &[
            "comic.ttf",
            "Comic Sans MS.ttf",
            "Chalkboard.ttc",
            "ComicNeue-Regular.ttf",
        ],
    },
    FontFamily {
        name: "Impact",
        candidates: &["impact.ttf", "Impact.ttf", "Anton-Regular.ttf"],
    },
];

/// Interface fonts: (egui name, candidates, used for monospace too?).
const UI_TEXT: &[&str] = &[
    "segoeui.ttf",
    "SFNS.ttf",
    "Helvetica.ttc",
    "DejaVuSans.ttf",
    "NotoSans-Regular.ttf",
    "LiberationSans-Regular.ttf",
];
const UI_SYMBOLS: &[&str] = &[
    "seguisym.ttf",
    "Apple Symbols.ttf",
    "DejaVuSans.ttf",
    "NotoSansSymbols2-Regular.ttf",
];
const UI_EMOJI: &[&str] = &["seguiemj.ttf", "NotoEmoji-Regular.ttf"];

fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(windir) = std::env::var_os("WINDIR") {
        dirs.push(Path::new(&windir).join("Fonts"));
    }
    for d in [
        "/System/Library/Fonts",
        "/System/Library/Fonts/Supplemental",
        "/Library/Fonts",
        "/usr/share/fonts",
        "/usr/local/share/fonts",
    ] {
        dirs.push(PathBuf::from(d));
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        dirs.push(home.join("Library").join("Fonts"));
        dirs.push(home.join(".local").join("share").join("fonts"));
        dirs.push(home.join(".fonts"));
    }
    dirs
}

/// Finds the first candidate file name anywhere under the font folders
/// (Linux keeps fonts in sub-folders, so the search goes a few levels deep).
pub fn find(candidates: &[&str]) -> Option<PathBuf> {
    let dirs = font_dirs();
    for name in candidates {
        for dir in &dirs {
            if let Some(found) = search(dir, name, 4, &mut 0) {
                return Some(found);
            }
        }
    }
    None
}

fn search(dir: &Path, name: &str, depth: u32, budget: &mut u32) -> Option<PathBuf> {
    let direct = dir.join(name);
    if direct.is_file() {
        return Some(direct);
    }
    if depth == 0 {
        return None;
    }
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        *budget += 1;
        if *budget > 20_000 {
            return None;
        }
        let path = entry.path();
        if path.is_dir()
            && let Some(found) = search(&path, name, depth - 1, budget)
        {
            return Some(found);
        }
    }
    None
}

pub fn read(family: &FontFamily) -> Option<Vec<u8>> {
    std::fs::read(find(family.candidates)?).ok()
}

/// Sets the interface fonts. Anything missing is skipped.
pub fn install_ui_fonts(ctx: &eframe::egui::Context) {
    use eframe::egui::{
        FontData, FontDefinitions, FontFamily::Monospace, FontFamily::Proportional,
    };
    let mut fonts = FontDefinitions::default();
    let mut add = |name: &str, candidates: &[&str], first: bool| {
        let Some(bytes) = find(candidates).and_then(|p| std::fs::read(p).ok()) else {
            return;
        };
        fonts.font_data.insert(
            name.to_owned(),
            std::sync::Arc::new(FontData::from_owned(bytes)),
        );
        for family in [Proportional, Monospace] {
            let list = fonts.families.entry(family.clone()).or_default();
            if first && family == Proportional {
                list.insert(0, name.to_owned());
            } else {
                list.push(name.to_owned());
            }
        }
    };
    add("ui-text", UI_TEXT, true);
    add("ui-symbols", UI_SYMBOLS, false);
    add("ui-emoji", UI_EMOJI, false);
    ctx.set_fonts(fonts);
}
