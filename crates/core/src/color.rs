//! Colours (straight, non-premultiplied RGBA8) and blending.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const TRANSPARENT: Rgba = Rgba::new(0, 0, 0, 0);
    pub const BLACK: Rgba = Rgba::rgb(0, 0, 0);
    pub const WHITE: Rgba = Rgba::rgb(255, 255, 255);

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::new(r, g, b, 255)
    }

    pub fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }

    /// Scales alpha by `coverage` (0..=255), as brushes do at soft edges.
    pub fn scaled(self, coverage: u8) -> Self {
        self.with_alpha(mul_255(self.a, coverage))
    }

    /// This colour painted over `dst` ("source over").
    pub fn over(self, dst: Rgba) -> Rgba {
        if self.a == 255 || dst.a == 0 {
            return self;
        }
        if self.a == 0 {
            return dst;
        }
        let sa = f32::from(self.a) / 255.0;
        let da = f32::from(dst.a) / 255.0;
        let out_a = sa + da * (1.0 - sa);
        let channel = |s: u8, d: u8| {
            let v = (f32::from(s) * sa + f32::from(d) * da * (1.0 - sa)) / out_a;
            to_u8(v)
        };
        Rgba::new(
            channel(self.r, dst.r),
            channel(self.g, dst.g),
            channel(self.b, dst.b),
            to_u8(out_a * 255.0),
        )
    }

    /// Largest per-channel difference, used as the fill tolerance distance.
    pub fn distance(self, other: Rgba) -> u8 {
        let d = |a: u8, b: u8| a.abs_diff(b);
        d(self.r, other.r)
            .max(d(self.g, other.g))
            .max(d(self.b, other.b))
            .max(d(self.a, other.a))
    }

    pub fn inverted(self) -> Rgba {
        Rgba::new(255 - self.r, 255 - self.g, 255 - self.b, self.a)
    }

    /// Hue in degrees, saturation and value in 0..=1.
    pub fn to_hsv(self) -> (f32, f32, f32) {
        let r = f32::from(self.r) / 255.0;
        let g = f32::from(self.g) / 255.0;
        let b = f32::from(self.b) / 255.0;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;
        let hue = if delta <= f32::EPSILON {
            0.0
        } else if (max - r).abs() <= f32::EPSILON {
            60.0 * ((g - b) / delta).rem_euclid(6.0)
        } else if (max - g).abs() <= f32::EPSILON {
            60.0 * ((b - r) / delta + 2.0)
        } else {
            60.0 * ((r - g) / delta + 4.0)
        };
        let sat = if max <= f32::EPSILON {
            0.0
        } else {
            delta / max
        };
        (hue, sat, max)
    }

    pub fn from_hsv(hue: f32, sat: f32, val: f32) -> Rgba {
        let h = hue.rem_euclid(360.0) / 60.0;
        let s = sat.clamp(0.0, 1.0);
        let v = val.clamp(0.0, 1.0);
        let c = v * s;
        let x = c * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
        let (r, g, b) = match h as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let m = v - c;
        Rgba::rgb(
            to_u8((r + m) * 255.0),
            to_u8((g + m) * 255.0),
            to_u8((b + m) * 255.0),
        )
    }
}

/// `a * b / 255`, rounded.
pub fn mul_255(a: u8, b: u8) -> u8 {
    let p = u32::from(a) * u32::from(b) + 128;
    ((p + (p >> 8)) >> 8) as u8
}

pub fn to_u8(v: f32) -> u8 {
    if v.is_nan() {
        0
    } else {
        v.round().clamp(0.0, 255.0) as u8
    }
}

/// Paint's default palette, as shown in its colour panel (two rows of ten).
pub const PAINT_PALETTE: [Rgba; 20] = [
    Rgba::rgb(0, 0, 0),
    Rgba::rgb(127, 127, 127),
    Rgba::rgb(136, 0, 21),
    Rgba::rgb(237, 28, 36),
    Rgba::rgb(255, 127, 39),
    Rgba::rgb(255, 242, 0),
    Rgba::rgb(34, 177, 76),
    Rgba::rgb(0, 162, 232),
    Rgba::rgb(63, 72, 204),
    Rgba::rgb(163, 73, 164),
    Rgba::rgb(255, 255, 255),
    Rgba::rgb(195, 195, 195),
    Rgba::rgb(185, 122, 87),
    Rgba::rgb(255, 174, 201),
    Rgba::rgb(255, 201, 14),
    Rgba::rgb(239, 228, 176),
    Rgba::rgb(181, 230, 29),
    Rgba::rgb(153, 217, 234),
    Rgba::rgb(112, 146, 190),
    Rgba::rgb(200, 191, 231),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn over_handles_opaque_transparent_and_half() {
        let red = Rgba::rgb(255, 0, 0);
        assert_eq!(red.over(Rgba::WHITE), red);
        assert_eq!(Rgba::TRANSPARENT.over(red), red);
        assert_eq!(red.over(Rgba::TRANSPARENT), red);
        let half = red.with_alpha(128).over(Rgba::WHITE);
        assert_eq!(half.a, 255);
        assert!((126..=129).contains(&half.g), "{half:?}");
        let on_clear = red.with_alpha(100).over(Rgba::TRANSPARENT);
        assert_eq!(on_clear, red.with_alpha(100));
    }

    #[test]
    fn mul_255_is_exact_at_the_ends() {
        assert_eq!(mul_255(255, 255), 255);
        assert_eq!(mul_255(255, 0), 0);
        assert_eq!(mul_255(200, 255), 200);
        assert_eq!(mul_255(255, 128), 128);
    }

    #[test]
    fn hsv_round_trips_palette() {
        for c in PAINT_PALETTE {
            let (h, s, v) = c.to_hsv();
            let back = Rgba::from_hsv(h, s, v);
            assert!(c.distance(back) <= 1, "{c:?} -> {back:?}");
        }
    }

    #[test]
    fn distance_and_invert() {
        assert_eq!(Rgba::BLACK.distance(Rgba::WHITE), 255);
        assert_eq!(Rgba::WHITE.inverted(), Rgba::BLACK);
        assert_eq!(to_u8(f32::NAN), 0);
        assert_eq!(to_u8(1e9), 255);
    }
}
