//! Whole-image transforms: rotate, flip, invert, resize, canvas size.

use crate::canvas::{Raster, check_size};
use crate::color::{Rgba, to_u8};
use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    Right90,
    Left90,
    Half,
}

pub fn rotate(src: &Raster, rotation: Rotation) -> Result<Raster> {
    let (w, h) = (src.width(), src.height());
    let (nw, nh) = match rotation {
        Rotation::Half => (w, h),
        _ => (h, w),
    };
    let mut out = Raster::new(nw, nh, Rgba::TRANSPARENT)?;
    for y in 0..h {
        for (x, &c) in (0..w).zip(src.row(y, 0, w)) {
            let (nx, ny) = match rotation {
                Rotation::Right90 => (h - 1 - y, x),
                Rotation::Left90 => (y, w - 1 - x),
                Rotation::Half => (w - 1 - x, h - 1 - y),
            };
            out.set(nx, ny, c);
        }
    }
    Ok(out)
}

pub fn flip(src: &Raster, horizontal: bool) -> Result<Raster> {
    let (w, h) = (src.width(), src.height());
    let mut out = Raster::new(w, h, Rgba::TRANSPARENT)?;
    for y in 0..h {
        let ty = if horizontal { y } else { h - 1 - y };
        let row = out.row_mut(ty, 0, w);
        row.copy_from_slice(src.row(y, 0, w));
        if horizontal {
            row.reverse();
        }
    }
    Ok(out)
}

pub fn invert(raster: &mut Raster) {
    for p in raster.pixels_mut() {
        *p = p.inverted();
    }
}

/// Scales to `width` × `height`: bilinear when `smooth`, nearest-neighbour otherwise
/// (pixel art stays crisp).
pub fn resize(src: &Raster, width: u32, height: u32, smooth: bool) -> Result<Raster> {
    let (nw, nh) = check_size(u64::from(width), u64::from(height))?;
    let (sw, sh) = (src.width() as f32, src.height() as f32);
    let mut out = Raster::new(nw, nh, Rgba::TRANSPARENT)?;
    let fx = sw / nw as f32;
    let fy = sh / nh as f32;
    for y in 0..nh {
        for x in 0..nw {
            let sx = (x as f32 + 0.5) * fx - 0.5;
            let sy = (y as f32 + 0.5) * fy - 0.5;
            let c = if smooth {
                bilinear(src, sx, sy)
            } else {
                src.get(
                    (sx + 0.5).clamp(0.0, sw - 1.0) as u32,
                    (sy + 0.5).clamp(0.0, sh - 1.0) as u32,
                )
                .unwrap_or_default()
            };
            out.set(x, y, c);
        }
    }
    Ok(out)
}

fn bilinear(src: &Raster, x: f32, y: f32) -> Rgba {
    let max_x = src.width().saturating_sub(1) as f32;
    let max_y = src.height().saturating_sub(1) as f32;
    let x = x.clamp(0.0, max_x);
    let y = y.clamp(0.0, max_y);
    let (x0, y0) = (x.floor(), y.floor());
    let (x1, y1) = ((x0 + 1.0).min(max_x), (y0 + 1.0).min(max_y));
    let (tx, ty) = (x - x0, y - y0);
    let px = |a: f32, b: f32| src.get(a as u32, b as u32).unwrap_or_default();
    let (c00, c10, c01, c11) = (px(x0, y0), px(x1, y0), px(x0, y1), px(x1, y1));
    // Weight colour by alpha so transparent pixels don't darken edges.
    let w = [
        (1.0 - tx) * (1.0 - ty),
        tx * (1.0 - ty),
        (1.0 - tx) * ty,
        tx * ty,
    ];
    let cs = [c00, c10, c01, c11];
    let alpha: f32 = cs.iter().zip(w).map(|(c, w)| f32::from(c.a) * w).sum();
    if alpha <= f32::EPSILON {
        return Rgba::TRANSPARENT;
    }
    let ch = |f: fn(&Rgba) -> u8| -> u8 {
        let v: f32 = cs
            .iter()
            .zip(w)
            .map(|(c, w)| f32::from(f(c)) * f32::from(c.a) * w)
            .sum();
        to_u8(v / alpha)
    };
    Rgba::new(ch(|c| c.r), ch(|c| c.g), ch(|c| c.b), to_u8(alpha))
}

/// Changes the canvas size, keeping the picture at the top-left. New area is `fill`.
pub fn canvas_size(src: &Raster, width: u32, height: u32, fill: Rgba) -> Result<Raster> {
    let (nw, nh) = check_size(u64::from(width), u64::from(height))?;
    let mut out = Raster::new(nw, nh, fill)?;
    out.draw(src, 0, 0, false, None);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Raster {
        // 3×2:  A B C
        //       D E F
        let mut r = Raster::new(3, 2, Rgba::WHITE).unwrap();
        for (i, (x, y)) in [(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1)]
            .into_iter()
            .enumerate()
        {
            r.set(x, y, Rgba::rgb(i as u8, 0, 0));
        }
        r
    }

    #[test]
    fn rotations_move_corners_correctly() {
        let r = sample();
        let right = rotate(&r, Rotation::Right90).unwrap();
        assert_eq!((right.width(), right.height()), (2, 3));
        assert_eq!(right.get(1, 0), Some(Rgba::rgb(0, 0, 0))); // A goes top-right
        assert_eq!(right.get(0, 0), Some(Rgba::rgb(3, 0, 0))); // D goes top-left
        let left = rotate(&r, Rotation::Left90).unwrap();
        assert_eq!(left.get(0, 0), Some(Rgba::rgb(2, 0, 0))); // C goes top-left
        let half = rotate(&r, Rotation::Half).unwrap();
        assert_eq!(half.get(0, 0), Some(Rgba::rgb(5, 0, 0)));
        // Four right turns are the identity.
        let mut back = r.clone();
        for _ in 0..4 {
            back = rotate(&back, Rotation::Right90).unwrap();
        }
        assert_eq!(back, r);
    }

    #[test]
    fn flips_and_invert() {
        let r = sample();
        assert_eq!(flip(&r, true).unwrap().get(0, 0), Some(Rgba::rgb(2, 0, 0)));
        assert_eq!(flip(&r, false).unwrap().get(0, 0), Some(Rgba::rgb(3, 0, 0)));
        let mut i = r.clone();
        invert(&mut i);
        invert(&mut i);
        assert_eq!(i, r);
    }

    #[test]
    fn resize_keeps_solid_colours_and_checks_size() {
        let r = Raster::new(4, 4, Rgba::rgb(10, 20, 30)).unwrap();
        for smooth in [true, false] {
            let big = resize(&r, 9, 7, smooth).unwrap();
            assert_eq!(big.get(8, 6), Some(Rgba::rgb(10, 20, 30)));
        }
        assert!(resize(&r, 0, 5, true).is_err());
        assert!(resize(&r, 99_999, 5, true).is_err());
    }

    #[test]
    fn canvas_size_pads_with_fill() {
        let r = sample();
        let big = canvas_size(&r, 5, 5, Rgba::WHITE).unwrap();
        assert_eq!(big.get(0, 0), Some(Rgba::rgb(0, 0, 0)));
        assert_eq!(big.get(4, 4), Some(Rgba::WHITE));
        let small = canvas_size(&r, 1, 1, Rgba::WHITE).unwrap();
        assert_eq!(small.get(0, 0), Some(Rgba::rgb(0, 0, 0)));
    }
}
