// Hide the console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod fonts;

use std::path::PathBuf;

/// Title bar, taskbar and dock icon. On Windows the .exe icon is embedded by build.rs.
const ICON_PNG: &[u8] = include_bytes!("../../../assets/icon-256.png");

fn main() -> eframe::Result {
    // `harshpaint picture.png` opens that picture.
    let initial_file = std::env::args_os().nth(1).map(PathBuf::from);

    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title(app::APP_NAME)
        .with_app_id("harshpaint")
        .with_inner_size([1360.0, 860.0])
        .with_min_inner_size([640.0, 420.0])
        .with_drag_and_drop(true);
    // A bad icon must never stop the app from starting.
    if let Ok(icon) = eframe::icon_data::from_png_bytes(ICON_PNG) {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        app::APP_NAME,
        options,
        Box::new(move |cc| Ok(Box::new(app::PaintApp::new(cc, initial_file)))),
    )
}
