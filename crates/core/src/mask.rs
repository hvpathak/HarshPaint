//! Coverage masks: every stroke first draws "how much of each pixel is covered"
//! (0..=255) into a mask, keeping the maximum. The layer is then recomputed from a
//! snapshot plus the mask. Overlapping dabs and symmetry copies therefore never
//! darken each other, and a soft stroke looks the same however fast you move.

use crate::canvas::{Rect, add_dirty};

#[derive(Debug, Clone)]
pub struct Mask {
    width: u32,
    height: u32,
    cov: Vec<u8>,
}

impl Mask {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            cov: vec![0; width as usize * height as usize],
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn get(&self, x: u32, y: u32) -> u8 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        self.cov
            .get(y as usize * self.width as usize + x as usize)
            .copied()
            .unwrap_or(0)
    }

    /// Keeps the larger of the current and new coverage.
    pub fn raise(&mut self, x: u32, y: u32, value: u8) {
        if x >= self.width || y >= self.height || value == 0 {
            return;
        }
        if let Some(c) = self
            .cov
            .get_mut(y as usize * self.width as usize + x as usize)
        {
            *c = (*c).max(value);
        }
    }

    pub fn clear_rect(&mut self, rect: Rect) {
        let r = rect.clip(self.width, self.height);
        for y in r.y..r.bottom() {
            let start = y as usize * self.width as usize + r.x as usize;
            if let Some(row) = self.cov.get_mut(start..start + r.w as usize) {
                row.fill(0);
            }
        }
    }

    /// A round, optionally soft-edged capsule from `a` to `b` (a single dab when
    /// `a == b`). `hardness` 1.0 is a crisp edge; 0.0 fades from the centre.
    /// With `aliased`, coverage is all-or-nothing (pixel art).
    pub fn capsule(
        &mut self,
        a: (f32, f32),
        b: (f32, f32),
        radius: f32,
        hardness: f32,
        aliased: bool,
    ) -> Rect {
        let radius = if radius.is_finite() {
            radius.max(0.5)
        } else {
            0.5
        };
        let pad = radius + 1.0;
        let rect = Rect::from_bounds(
            a.0.min(b.0) - pad,
            a.1.min(b.1) - pad,
            a.0.max(b.0) + pad,
            a.1.max(b.1) + pad,
            self.width,
            self.height,
        );
        let hardness = hardness.clamp(0.0, 1.0);
        let inner = radius * hardness;
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len2 = dx * dx + dy * dy;
        let mut dirty = None;
        for y in rect.y..rect.bottom() {
            for x in rect.x..rect.right() {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let t = if len2 > f32::EPSILON {
                    (((px - a.0) * dx + (py - a.1) * dy) / len2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let (cx, cy) = (a.0 + dx * t, a.1 + dy * t);
                let d = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
                let cov = if aliased {
                    if d <= radius { 1.0 } else { 0.0 }
                } else if d <= inner {
                    1.0
                } else {
                    // Fade from the hard core to the edge, plus half a pixel of anti-aliasing.
                    let span = (radius - inner).max(0.0) + 0.5;
                    ((radius + 0.5 - d) / span).clamp(0.0, 1.0)
                };
                if cov > 0.0 {
                    self.raise(x, y, (cov * 255.0).round() as u8);
                    add_dirty(&mut dirty, Rect::new(x, y, 1, 1));
                }
            }
        }
        dirty.unwrap_or_default()
    }

    /// A one-pixel-wide aliased line (Bresenham), for the 1 px pencil.
    pub fn pixel_line(&mut self, a: (i64, i64), b: (i64, i64)) -> Rect {
        let (mut x, mut y) = a;
        let dx = (b.0 - x).abs();
        let dy = -(b.1 - y).abs();
        let sx = if x < b.0 { 1 } else { -1 };
        let sy = if y < b.1 { 1 } else { -1 };
        let mut err = dx + dy;
        let mut dirty = None;
        // Bounded: a line can't be longer than this many steps.
        for _ in 0..=(dx - dy) {
            if x >= 0 && y >= 0 && x < i64::from(self.width) && y < i64::from(self.height) {
                self.raise(x as u32, y as u32, 255);
                add_dirty(&mut dirty, Rect::new(x as u32, y as u32, 1, 1));
            }
            if x == b.0 && y == b.1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
        dirty.unwrap_or_default()
    }

    /// A square block of `size` pixels centred on (`cx`, `cy`), like Paint's eraser.
    pub fn square(&mut self, cx: f32, cy: f32, size: u32) -> Rect {
        let half = size as f32 / 2.0;
        let x0 = (cx - half).floor();
        let y0 = (cy - half).floor();
        let rect = Rect::from_bounds(
            x0 + 1.0,
            y0 + 1.0,
            x0 + size as f32 - 1.0,
            y0 + size as f32 - 1.0,
            self.width,
            self.height,
        );
        for y in rect.y..rect.bottom() {
            for x in rect.x..rect.right() {
                self.raise(x, y, 255);
            }
        }
        rect
    }

    /// Fills a polygon (even-odd) with anti-aliasing from 4 sub-scanlines per row
    /// and exact horizontal coverage. `points` are in pixel coordinates.
    pub fn fill_polygon(&mut self, points: &[(f32, f32)], aliased: bool) -> Rect {
        if points.len() < 3 || points.iter().any(|p| !p.0.is_finite() || !p.1.is_finite()) {
            return Rect::default();
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for &(x, y) in points {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        let rect = Rect::from_bounds(x0, y0, x1, y1, self.width, self.height);
        if rect.is_empty() {
            return rect;
        }
        let subs: &[f32] = if aliased {
            &[0.5]
        } else {
            &[0.125, 0.375, 0.625, 0.875]
        };
        let weight = 1.0 / subs.len() as f32;
        let mut row = vec![0.0f32; rect.w as usize];
        let mut xs: Vec<f32> = Vec::with_capacity(points.len());
        for y in rect.y..rect.bottom() {
            row.fill(0.0);
            for &s in subs {
                let sy = y as f32 + s;
                xs.clear();
                for (i, &(ax, ay)) in points.iter().enumerate() {
                    let Some(&(bx, by)) = points.get((i + 1) % points.len()) else {
                        continue;
                    };
                    if (ay <= sy && by > sy) || (by <= sy && ay > sy) {
                        xs.push(ax + (sy - ay) / (by - ay) * (bx - ax));
                    }
                }
                xs.sort_by(|a, b| a.total_cmp(b));
                for &[l, r] in xs.as_chunks::<2>().0 {
                    let (l, r) = if aliased {
                        ((l - 0.5).ceil() + 0.0, (r - 0.5).ceil())
                    } else {
                        (l, r)
                    };
                    add_span(&mut row, rect.x as f32, l, r, weight);
                }
            }
            for (i, c) in row.iter().enumerate() {
                if *c > 0.0 {
                    self.raise(rect.x + i as u32, y, (c.min(1.0) * 255.0).round() as u8);
                }
            }
        }
        rect
    }
}

/// Adds coverage for the horizontal span [l, r) into `row`, whose first pixel is at `origin`.
fn add_span(row: &mut [f32], origin: f32, l: f32, r: f32, weight: f32) {
    if r <= l {
        return;
    }
    let len = row.len() as f32;
    let l = (l - origin).clamp(0.0, len);
    let r = (r - origin).clamp(0.0, len);
    let first = l.floor() as usize;
    let last = (r.ceil() as usize).min(row.len());
    for i in first..last {
        let px0 = i as f32;
        let overlap = (r.min(px0 + 1.0) - l.max(px0)).max(0.0);
        if let Some(c) = row.get_mut(i) {
            *c += overlap * weight;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capsule_covers_its_middle_and_not_far_away() {
        let mut m = Mask::new(20, 20);
        let dirty = m.capsule((5.0, 10.0), (15.0, 10.0), 2.0, 1.0, false);
        assert_eq!(m.get(10, 9), 255);
        assert_eq!(m.get(10, 2), 0);
        assert!(dirty.contains(10, 10));
        assert!(!dirty.contains(10, 1));
    }

    #[test]
    fn overlapping_dabs_do_not_accumulate() {
        let mut m = Mask::new(10, 10);
        m.capsule((5.0, 5.0), (5.0, 5.0), 3.0, 0.0, false);
        let once = m.get(6, 5);
        m.capsule((5.0, 5.0), (5.0, 5.0), 3.0, 0.0, false);
        assert_eq!(m.get(6, 5), once);
    }

    #[test]
    fn pixel_line_is_connected_and_hits_endpoints() {
        let mut m = Mask::new(10, 10);
        m.pixel_line((0, 0), (9, 4));
        assert_eq!(m.get(0, 0), 255);
        assert_eq!(m.get(9, 4), 255);
        for x in 0..10 {
            assert!((0..10).any(|y| m.get(x, y) == 255), "gap at column {x}");
        }
        // Lines off the canvas must not panic.
        m.pixel_line((-100, -100), (100_000, 5));
    }

    #[test]
    fn polygon_fills_inside_with_soft_edges() {
        let mut m = Mask::new(20, 20);
        m.fill_polygon(&[(2.0, 2.0), (18.0, 2.0), (18.0, 18.0), (2.0, 18.0)], false);
        assert_eq!(m.get(10, 10), 255);
        assert_eq!(m.get(1, 10), 0);
        let mut tri = Mask::new(20, 20);
        tri.fill_polygon(&[(0.0, 0.0), (20.0, 0.0), (0.0, 20.0)], false);
        let edge = tri.get(10, 9);
        assert!(
            edge > 0 && edge < 255,
            "diagonal edge should be partial, got {edge}"
        );
        // Degenerate input is ignored.
        assert!(
            m.fill_polygon(&[(1.0, 1.0), (f32::NAN, 2.0), (3.0, 3.0)], false)
                .is_empty()
        );
    }

    #[test]
    fn square_matches_requested_size() {
        let mut m = Mask::new(20, 20);
        m.square(10.0, 10.0, 4);
        let count = (0..20)
            .flat_map(|y| (0..20).map(move |x| (x, y)))
            .filter(|&(x, y)| m.get(x, y) > 0)
            .count();
        assert_eq!(count, 16);
    }
}
