//! Fill with colour. Contiguous fill uses an explicit span stack (no recursion, so
//! huge areas can't overflow the stack) and a bit set to remember visited pixels.

use crate::canvas::{Raster, Rect, add_dirty};
use crate::color::Rgba;

/// Fills the area connected to (`x`, `y`) whose colour is within `tolerance` of the
/// starting pixel. With `everywhere`, every matching pixel in the picture is filled.
/// Returns the changed area.
pub fn fill(
    raster: &mut Raster,
    x: u32,
    y: u32,
    color: Rgba,
    tolerance: u8,
    everywhere: bool,
) -> Rect {
    let Some(target) = raster.get(x, y) else {
        return Rect::default();
    };
    if target == color && tolerance == 0 {
        return Rect::default();
    }
    let matches = |c: Rgba| c.distance(target) <= tolerance;
    let (w, h) = (raster.width(), raster.height());
    let mut dirty = None;

    if everywhere {
        for py in 0..h {
            let mut changed = false;
            for p in raster.row_mut(py, 0, w) {
                if matches(*p) {
                    *p = color;
                    changed = true;
                }
            }
            if changed {
                add_dirty(&mut dirty, Rect::new(0, py, w, 1));
            }
        }
        return dirty.unwrap_or_default();
    }

    let mut visited = vec![0u64; (w as usize * h as usize).div_ceil(64)];
    let seen = |v: &[u64], x: u32, y: u32| -> bool {
        let i = y as usize * w as usize + x as usize;
        v.get(i / 64)
            .is_some_and(|word| word & (1 << (i % 64)) != 0)
    };
    let mark = |v: &mut [u64], x: u32, y: u32| {
        let i = y as usize * w as usize + x as usize;
        if let Some(word) = v.get_mut(i / 64) {
            *word |= 1 << (i % 64);
        }
    };

    let mut stack = vec![(x, y)];
    while let Some((sx, sy)) = stack.pop() {
        if seen(&visited, sx, sy) || !raster.get(sx, sy).is_some_and(matches) {
            continue;
        }
        // Grow the span left and right along this row.
        let mut left = sx;
        while left > 0
            && !seen(&visited, left - 1, sy)
            && raster.get(left - 1, sy).is_some_and(matches)
        {
            left -= 1;
        }
        let mut right = sx;
        while right + 1 < w
            && !seen(&visited, right + 1, sy)
            && raster.get(right + 1, sy).is_some_and(matches)
        {
            right += 1;
        }
        for px in left..=right {
            mark(&mut visited, px, sy);
            raster.set(px, sy, color);
        }
        add_dirty(&mut dirty, Rect::new(left, sy, right - left + 1, 1));
        // Queue the neighbouring rows, one seed per run of matching pixels.
        for ny in [sy.checked_sub(1), sy.checked_add(1).filter(|&n| n < h)]
            .into_iter()
            .flatten()
        {
            let mut in_run = false;
            for px in left..=right {
                let ok = !seen(&visited, px, ny) && raster.get(px, ny).is_some_and(matches);
                if ok && !in_run {
                    stack.push((px, ny));
                }
                in_run = ok;
            }
        }
    }
    dirty.unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed() -> Raster {
        // A 10×10 white image with a black square outline from (2,2) to (7,7).
        let mut r = Raster::new(10, 10, Rgba::WHITE).unwrap();
        for i in 2..=7 {
            r.set(i, 2, Rgba::BLACK);
            r.set(i, 7, Rgba::BLACK);
            r.set(2, i, Rgba::BLACK);
            r.set(7, i, Rgba::BLACK);
        }
        r
    }

    #[test]
    fn fills_inside_only() {
        let mut r = boxed();
        let red = Rgba::rgb(255, 0, 0);
        let dirty = fill(&mut r, 4, 4, red, 0, false);
        assert_eq!(r.get(4, 4), Some(red));
        assert_eq!(r.get(6, 6), Some(red));
        assert_eq!(r.get(0, 0), Some(Rgba::WHITE));
        assert_eq!(r.get(2, 2), Some(Rgba::BLACK));
        assert_eq!(dirty, Rect::new(3, 3, 4, 4));
    }

    #[test]
    fn everywhere_replaces_disconnected_matches() {
        let mut r = boxed();
        let red = Rgba::rgb(255, 0, 0);
        fill(&mut r, 0, 0, red, 0, true);
        assert_eq!(r.get(0, 0), Some(red));
        assert_eq!(r.get(4, 4), Some(red)); // inside the box too
    }

    #[test]
    fn tolerance_includes_near_colours() {
        let mut r = Raster::new(3, 1, Rgba::WHITE).unwrap();
        r.set(1, 0, Rgba::rgb(250, 250, 250));
        r.set(2, 0, Rgba::rgb(200, 200, 200));
        fill(&mut r, 0, 0, Rgba::BLACK, 10, false);
        assert_eq!(r.get(1, 0), Some(Rgba::BLACK));
        assert_eq!(r.get(2, 0), Some(Rgba::rgb(200, 200, 200)));
    }

    #[test]
    fn huge_fill_does_not_overflow_and_outside_is_noop() {
        let mut r = Raster::new(2048, 2048, Rgba::WHITE).unwrap();
        let d = fill(&mut r, 1000, 1000, Rgba::BLACK, 0, false);
        assert_eq!(d, r.bounds());
        assert_eq!(
            fill(&mut r, 99_999, 0, Rgba::WHITE, 0, false),
            Rect::default()
        );
    }
}
