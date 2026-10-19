//! Text tool rendering. The caller supplies the font file's bytes (the app reads
//! fonts that Windows already has), so no fonts ship inside HarshPaint.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};

use crate::canvas::{Rect, add_dirty};
use crate::error::{Error, Result};
use crate::mask::Mask;

/// Longest text one placement may contain.
pub const MAX_CHARS: usize = 10_000;

/// Draws `text` with its top-left at `origin` into `mask`. Lines break on `\n`.
pub fn render(
    mask: &mut Mask,
    font_bytes: &[u8],
    text: &str,
    origin: (f32, f32),
    size_px: f32,
    aliased: bool,
) -> Result<Rect> {
    let font = FontRef::try_from_slice(font_bytes)
        .map_err(|_| Error::Format("that font file could not be read".into()))?;
    let size = if size_px.is_finite() {
        size_px.clamp(4.0, 500.0)
    } else {
        24.0
    };
    let scaled = font.as_scaled(PxScale::from(size));
    let line_height = scaled.height() + scaled.line_gap();
    let mut dirty = None;

    for (row, line) in text
        .chars()
        .take(MAX_CHARS)
        .collect::<String>()
        .split('\n')
        .enumerate()
    {
        let baseline = origin.1 + scaled.ascent() + line_height * row as f32;
        let mut x = origin.0;
        let mut prev = None;
        for c in line.chars() {
            let id = font.glyph_id(c);
            if let Some(p) = prev {
                x += scaled.kern(p, id);
            }
            let glyph = id.with_scale_and_position(PxScale::from(size), point(x, baseline));
            x += scaled.h_advance(id);
            prev = Some(id);
            let Some(outlined) = font.outline_glyph(glyph) else {
                continue;
            };
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, cov| {
                let px = bounds.min.x as i64 + i64::from(gx);
                let py = bounds.min.y as i64 + i64::from(gy);
                if px < 0 || py < 0 {
                    return;
                }
                let value = if aliased {
                    if cov >= 0.5 { 255 } else { 0 }
                } else {
                    (cov.clamp(0.0, 1.0) * 255.0).round() as u8
                };
                if value > 0 {
                    mask.raise(px as u32, py as u32, value);
                    if (px as u64) < u64::from(mask.width())
                        && (py as u64) < u64::from(mask.height())
                    {
                        add_dirty(&mut dirty, Rect::new(px as u32, py as u32, 1, 1));
                    }
                }
            });
        }
    }
    Ok(dirty.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system_font() -> Option<Vec<u8>> {
        let dir = std::env::var_os("WINDIR")?;
        std::fs::read(std::path::Path::new(&dir).join("Fonts").join("arial.ttf")).ok()
    }

    #[test]
    fn draws_text_with_a_system_font() {
        let Some(font) = system_font() else {
            return; // not on Windows; the bad-font test still runs
        };
        let mut m = Mask::new(200, 60);
        let dirty = render(&mut m, &font, "Hi\nthere", (5.0, 5.0), 20.0, false).unwrap();
        assert!(!dirty.is_empty());
        assert!(
            dirty.bottom() > 30,
            "second line should be lower: {dirty:?}"
        );
        // Text far off the canvas is clipped, not a crash.
        render(&mut m, &font, "off", (-500.0, 5000.0), 20.0, true).unwrap();
    }

    #[test]
    fn bad_font_is_an_error() {
        let mut m = Mask::new(10, 10);
        assert!(render(&mut m, b"not a font", "x", (0.0, 0.0), 12.0, false).is_err());
    }
}
