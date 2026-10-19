//! Engine for HarshPaint. Everything here works without a window, so the desktop
//! app, the CLI and the tests share it. Nothing in this crate may depend on egui,
//! eframe, winit or rfd.

pub mod canvas;
pub mod color;
pub mod commands;
pub mod document;
pub mod error;
pub mod fill;
pub mod history;
pub mod image_ops;
pub mod io;
pub mod mask;
pub mod session;
pub mod shapes;
pub mod stabilizer;
pub mod symmetry;
pub mod text;
pub mod timelapse;

pub use error::{Error, Result};
