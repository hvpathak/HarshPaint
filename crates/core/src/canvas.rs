//! Pixel storage. Every access is bounds-checked: coordinates come from the mouse,
//! files and command parameters, so they are treated as untrusted.

use crate::color::Rgba;
use crate::error::{Error, Result};

/// Largest width or height HarshPaint will create or open.
pub const MAX_SIDE: u32 = 16_384;
/// Largest total pixel count (100 MP = 400 MB per layer).
pub const MAX_PIXELS: u64 = 100_000_000;

/// Checks a size before anything is allocated for it.
pub fn check_size(width: u64, height: u64) -> Result<(u32, u32)> {
    if width == 0 || height == 0 {
        return Err(Error::EmptySize);
    }
    if width > u64::from(MAX_SIDE)
        || height > u64::from(MAX_SIDE)
        || width.saturating_mul(height) > MAX_PIXELS
    {
        return Err(Error::TooLarge { width, height });
    }
    Ok((width as u32, height as u32))
}

/// A rectangle of pixels, always non-negative. `w`/`h` may be zero (empty).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub const fn new(x: u32, y: u32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }

    pub fn right(&self) -> u32 {
        self.x.saturating_add(self.w)
    }

    pub fn bottom(&self) -> u32 {
        self.y.saturating_add(self.h)
    }

    pub fn area(&self) -> u64 {
        u64::from(self.w) * u64::from(self.h)
    }

    pub fn contains(&self, x: u32, y: u32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }

    pub fn union(self, other: Rect) -> Rect {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Rect::new(
            x,
            y,
            self.right().max(other.right()) - x,
            self.bottom().max(other.bottom()) - y,
        )
    }

    /// The part of this rect inside a `width` × `height` canvas.
    pub fn clip(self, width: u32, height: u32) -> Rect {
        let x = self.x.min(width);
        let y = self.y.min(height);
        Rect::new(
            x,
            y,
            self.right().min(width) - x,
            self.bottom().min(height) - y,
        )
    }

    /// Bounding box of a floating-point area, padded by one pixel and clipped.
    pub fn from_bounds(x0: f32, y0: f32, x1: f32, y1: f32, width: u32, height: u32) -> Rect {
        let lo = |v: f32| -> u32 {
            if v.is_nan() {
                0
            } else {
                (v.floor() - 1.0).clamp(0.0, u32::MAX as f32) as u32
            }
        };
        let hi = |v: f32| -> u32 {
            if v.is_nan() {
                0
            } else {
                (v.ceil() + 1.0).clamp(0.0, u32::MAX as f32) as u32
            }
        };
        let (ax, bx) = (lo(x0.min(x1)), hi(x0.max(x1)));
        let (ay, by) = (lo(y0.min(y1)), hi(y0.max(y1)));
        Rect::new(ax, ay, bx.saturating_sub(ax), by.saturating_sub(ay)).clip(width, height)
    }
}

/// Grows `acc` to include `r`.
pub fn add_dirty(acc: &mut Option<Rect>, r: Rect) {
    if r.is_empty() {
        return;
    }
    *acc = Some(acc.map_or(r, |a| a.union(r)));
}

/// An RGBA image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raster {
    width: u32,
    height: u32,
    pixels: Vec<Rgba>,
}

impl Raster {
    pub fn new(width: u32, height: u32, fill: Rgba) -> Result<Self> {
        let (w, h) = check_size(u64::from(width), u64::from(height))?;
        Ok(Self {
            width: w,
            height: h,
            pixels: vec![fill; w as usize * h as usize],
        })
    }

    /// Builds a raster from tightly packed RGBA8 bytes.
    pub fn from_rgba8(width: u32, height: u32, bytes: &[u8]) -> Result<Self> {
        let (w, h) = check_size(u64::from(width), u64::from(height))?;
        let expected = w as usize * h as usize * 4;
        if bytes.len() != expected {
            return Err(Error::Format(format!(
                "expected {expected} bytes of pixel data, got {}",
                bytes.len()
            )));
        }
        let pixels = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&[r, g, b, a]| Rgba::new(r, g, b, a))
            .collect();
        Ok(Self {
            width: w,
            height: h,
            pixels,
        })
    }

    pub fn to_rgba8(&self) -> Vec<u8> {
        self.pixels
            .iter()
            .flat_map(|p| [p.r, p.g, p.b, p.a])
            .collect()
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.width, self.height)
    }

    pub fn pixels(&self) -> &[Rgba] {
        &self.pixels
    }

    pub fn pixels_mut(&mut self) -> &mut [Rgba] {
        &mut self.pixels
    }

    pub fn index(&self, x: u32, y: u32) -> Option<usize> {
        (x < self.width && y < self.height).then(|| y as usize * self.width as usize + x as usize)
    }

    pub fn get(&self, x: u32, y: u32) -> Option<Rgba> {
        self.index(x, y).and_then(|i| self.pixels.get(i).copied())
    }

    pub fn set(&mut self, x: u32, y: u32, c: Rgba) {
        if let Some(p) = self.index(x, y).and_then(|i| self.pixels.get_mut(i)) {
            *p = c;
        }
    }

    /// One row of pixels from `x` for `w` pixels, clipped to the image.
    pub fn row(&self, y: u32, x: u32, w: u32) -> &[Rgba] {
        let Some(start) = self.index(x, y) else {
            return &[];
        };
        let len = w.min(self.width - x) as usize;
        self.pixels.get(start..start + len).unwrap_or(&[])
    }

    pub fn row_mut(&mut self, y: u32, x: u32, w: u32) -> &mut [Rgba] {
        let Some(start) = self.index(x, y) else {
            return &mut [];
        };
        let len = w.min(self.width - x) as usize;
        self.pixels.get_mut(start..start + len).unwrap_or(&mut [])
    }

    /// Copies the pixels of `rect` (clipped) into a new raster.
    pub fn crop(&self, rect: Rect) -> Result<Raster> {
        let r = rect.clip(self.width, self.height);
        let mut out = Raster::new(r.w, r.h, Rgba::TRANSPARENT)?;
        for dy in 0..r.h {
            let src = self.row(r.y + dy, r.x, r.w);
            out.row_mut(dy, 0, r.w).copy_from_slice(src);
        }
        Ok(out)
    }

    /// Pixels of `rect` as a flat list (used by undo history).
    pub fn read_rect(&self, rect: Rect) -> Vec<Rgba> {
        let r = rect.clip(self.width, self.height);
        let mut out = Vec::with_capacity(r.area() as usize);
        for dy in 0..r.h {
            out.extend_from_slice(self.row(r.y + dy, r.x, r.w));
        }
        out
    }

    /// Writes pixels previously taken with [`Raster::read_rect`].
    pub fn write_rect(&mut self, rect: Rect, data: &[Rgba]) {
        let r = rect.clip(self.width, self.height);
        for (dy, src) in (0..r.h).zip(data.chunks(r.w.max(1) as usize)) {
            let dst = self.row_mut(r.y + dy, r.x, r.w);
            let n = dst.len().min(src.len());
            if let (Some(d), Some(s)) = (dst.get_mut(..n), src.get(..n)) {
                d.copy_from_slice(s);
            }
        }
    }

    /// Draws `src` with its top-left at (`x`, `y`), blending ("over") or replacing.
    /// Pixels equal to `skip` are left out, which is how transparent selection works.
    pub fn draw(&mut self, src: &Raster, x: i64, y: i64, blend: bool, skip: Option<Rgba>) -> Rect {
        let mut dirty = None;
        for sy in 0..src.height {
            let ty = y + i64::from(sy);
            if ty < 0 || ty >= i64::from(self.height) {
                continue;
            }
            for sx in 0..src.width {
                let tx = x + i64::from(sx);
                if tx < 0 || tx >= i64::from(self.width) {
                    continue;
                }
                let Some(c) = src.get(sx, sy) else { continue };
                if skip == Some(c) {
                    continue;
                }
                let (tx, ty) = (tx as u32, ty as u32);
                let out = if blend {
                    self.get(tx, ty).map_or(c, |d| c.over(d))
                } else {
                    c
                };
                self.set(tx, ty, out);
                add_dirty(&mut dirty, Rect::new(tx, ty, 1, 1));
            }
        }
        dirty.unwrap_or_default()
    }

    pub fn fill_rect(&mut self, rect: Rect, c: Rgba) {
        let r = rect.clip(self.width, self.height);
        for dy in 0..r.h {
            self.row_mut(r.y + dy, r.x, r.w).fill(c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bad_sizes_before_allocating() {
        assert!(matches!(check_size(0, 10), Err(Error::EmptySize)));
        assert!(matches!(
            check_size(20_000, 10),
            Err(Error::TooLarge { .. })
        ));
        assert!(matches!(
            check_size(16_384, 16_384),
            Err(Error::TooLarge { .. })
        ));
        assert!(check_size(10_000, 10_000).is_ok());
        assert!(Raster::new(u32::MAX, u32::MAX, Rgba::WHITE).is_err());
    }

    #[test]
    fn out_of_bounds_access_is_ignored() {
        let mut r = Raster::new(4, 3, Rgba::WHITE).unwrap();
        r.set(99, 99, Rgba::BLACK);
        assert_eq!(r.get(4, 0), None);
        assert_eq!(r.get(3, 2), Some(Rgba::WHITE));
        assert!(r.row(5, 0, 4).is_empty());
        assert_eq!(r.row(1, 2, 100).len(), 2);
    }

    #[test]
    fn read_and_write_rect_round_trip() {
        let mut r = Raster::new(5, 5, Rgba::WHITE).unwrap();
        r.fill_rect(Rect::new(1, 1, 2, 2), Rgba::BLACK);
        let rect = Rect::new(0, 0, 3, 3);
        let saved = r.read_rect(rect);
        r.fill_rect(rect, Rgba::rgb(9, 9, 9));
        r.write_rect(rect, &saved);
        assert_eq!(r.get(1, 1), Some(Rgba::BLACK));
        assert_eq!(r.get(0, 0), Some(Rgba::WHITE));
    }

    #[test]
    fn rgba8_round_trip_and_length_check() {
        let mut r = Raster::new(3, 2, Rgba::TRANSPARENT).unwrap();
        r.set(2, 1, Rgba::new(1, 2, 3, 4));
        let bytes = r.to_rgba8();
        assert_eq!(Raster::from_rgba8(3, 2, &bytes).unwrap(), r);
        assert!(Raster::from_rgba8(3, 2, &bytes[1..]).is_err());
    }

    #[test]
    fn rect_math() {
        let a = Rect::new(1, 1, 2, 2);
        assert_eq!(a.union(Rect::new(5, 0, 1, 1)), Rect::new(1, 0, 5, 3));
        assert_eq!(Rect::new(8, 8, 10, 10).clip(10, 10), Rect::new(8, 8, 2, 2));
        assert!(Rect::new(20, 20, 5, 5).clip(10, 10).is_empty());
        let b = Rect::from_bounds(-5.0, -5.0, 2.0, 2.0, 10, 10);
        assert_eq!((b.x, b.y), (0, 0));
        assert!(Rect::from_bounds(f32::NAN, 0.0, 1.0, 1.0, 10, 10).w <= 3);
    }

    #[test]
    fn draw_clips_and_skips_transparent_colour() {
        let mut dst = Raster::new(4, 4, Rgba::WHITE).unwrap();
        let mut src = Raster::new(2, 2, Rgba::BLACK).unwrap();
        src.set(0, 0, Rgba::WHITE);
        let dirty = dst.draw(&src, 3, 3, false, Some(Rgba::WHITE));
        assert_eq!(dst.get(3, 3), Some(Rgba::WHITE)); // skipped
        assert_eq!(dirty, Rect::default());
        dst.draw(&src, -1, -1, false, None);
        assert_eq!(dst.get(0, 0), Some(Rgba::BLACK));
    }
}
