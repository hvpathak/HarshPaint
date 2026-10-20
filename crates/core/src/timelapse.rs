//! Time-lapse export: replays the undo history from the first kept step to now and
//! encodes it as a looping animated GIF.

use std::io::Cursor;

use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, Frame, RgbaImage};

use crate::color::Rgba;
use crate::document::Document;
use crate::error::{Error, Result};
use crate::history::History;
use crate::image_ops;

pub struct TimelapseOptions {
    /// Most frames in the GIF (long histories skip steps evenly).
    pub max_frames: usize,
    /// Longest side of each frame in pixels.
    pub max_side: u32,
    pub frame_ms: u32,
    /// How long the finished picture stays on screen before looping.
    pub hold_last_ms: u32,
}

impl Default for TimelapseOptions {
    fn default() -> Self {
        Self {
            max_frames: 90,
            max_side: 480,
            frame_ms: 70,
            hold_last_ms: 1800,
        }
    }
}

pub fn render(doc: &Document, history: &History, opts: &TimelapseOptions) -> Result<Vec<u8>> {
    let steps: Vec<_> = history.steps().collect();
    if steps.is_empty() {
        return Err(Error::Unavailable(
            "draw something first: the time-lapse replays your strokes",
        ));
    }
    // Rewind a copy to the oldest kept state.
    let mut replay = doc.clone();
    for step in steps.iter().rev() {
        step.change.apply(&mut replay, false);
    }
    let stride = steps.len().div_ceil(opts.max_frames.max(2) - 1).max(1);
    let mut frames = vec![frame(&replay, opts, opts.frame_ms)?];
    for (i, step) in steps.iter().enumerate() {
        step.change.apply(&mut replay, true);
        let last = i + 1 == steps.len();
        if last || (i + 1) % stride == 0 {
            let delay = if last {
                opts.hold_last_ms
            } else {
                opts.frame_ms
            };
            frames.push(frame(&replay, opts, delay)?);
        }
    }
    let mut out = Cursor::new(Vec::new());
    {
        // Speed 10 of 30: good colours, fast enough for ~90 frames.
        let mut enc = GifEncoder::new_with_speed(&mut out, 10);
        enc.set_repeat(Repeat::Infinite)?;
        enc.encode_frames(frames)?;
    }
    Ok(out.into_inner())
}

fn frame(doc: &Document, opts: &TimelapseOptions, delay_ms: u32) -> Result<Frame> {
    let mut flat = doc.flatten()?;
    for p in flat.pixels_mut() {
        *p = p.over(Rgba::WHITE);
    }
    let longest = flat.width().max(flat.height());
    if longest > opts.max_side {
        let scale = opts.max_side as f32 / longest as f32;
        let w = ((flat.width() as f32 * scale).round() as u32).max(1);
        let h = ((flat.height() as f32 * scale).round() as u32).max(1);
        flat = image_ops::resize(&flat, w, h, true)?;
    }
    let img = RgbaImage::from_raw(flat.width(), flat.height(), flat.to_rgba8())
        .ok_or_else(|| Error::Format("internal pixel buffer has the wrong size".into()))?;
    Ok(Frame::from_parts(
        img,
        0,
        0,
        Delay::from_numer_denom_ms(delay_ms, 1),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::Rect;
    use crate::history::Change;

    #[test]
    fn renders_a_looping_gif_and_leaves_the_document_alone() {
        let mut doc = Document::new(600, 300, Rgba::WHITE).unwrap();
        let mut hist = History::default();
        for i in 0..5u32 {
            let rect = Rect::new(i * 10, 0, 10, 10);
            let before = doc.active().unwrap().raster.read_rect(rect);
            doc.active_mut()
                .unwrap()
                .raster
                .fill_rect(rect, Rgba::BLACK);
            let after = doc.active().unwrap().raster.read_rect(rect);
            hist.push(
                "Paint",
                Change::Pixels {
                    layer: 0,
                    rect,
                    before,
                    after,
                },
            );
        }
        let snapshot = doc.clone();
        let gif = render(&doc, &hist, &TimelapseOptions::default()).unwrap();
        assert_eq!(&gif[..6], b"GIF89a");
        assert_eq!(doc, snapshot);
        let decoded = crate::io::decode_image(&gif).unwrap();
        assert_eq!(decoded.width(), 480); // scaled down to max_side
    }

    #[test]
    fn empty_history_is_a_friendly_error() {
        let doc = Document::new(10, 10, Rgba::WHITE).unwrap();
        assert!(render(&doc, &History::default(), &TimelapseOptions::default()).is_err());
    }
}
