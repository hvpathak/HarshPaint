//! One error type for the engine. Every message is written for the person using
//! the app, because the UI shows it as-is.

use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    /// The requested picture is bigger than HarshPaint allows.
    TooLarge {
        width: u64,
        height: u64,
    },
    /// A width or height of zero.
    EmptySize,
    Io(std::io::Error),
    /// A file that could not be decoded or encoded.
    Format(String),
    /// A command that needs something that isn't there (no selection, no layer…).
    Unavailable(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::TooLarge { width, height } => write!(
                f,
                "{width} × {height} pixels is too large (HarshPaint allows up to {} pixels per side and {} megapixels)",
                crate::canvas::MAX_SIDE,
                crate::canvas::MAX_PIXELS / 1_000_000
            ),
            Error::EmptySize => write!(f, "the picture must be at least 1 × 1 pixel"),
            Error::Io(e) => write!(f, "{e}"),
            Error::Format(msg) => write!(f, "{msg}"),
            Error::Unavailable(what) => write!(f, "{what}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<image::ImageError> for Error {
    fn from(e: image::ImageError) -> Self {
        Error::Format(format!("the image could not be read or written: {e}"))
    }
}
