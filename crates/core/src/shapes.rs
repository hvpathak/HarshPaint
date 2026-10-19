//! Shapes as outlines (point lists). Everything, even ellipses and hearts, becomes
//! a polygon, so symmetry can rotate any shape and one rasterizer draws them all.

use std::f32::consts::{PI, TAU};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShapeKind {
    #[default]
    Line,
    Rectangle,
    RoundedRectangle,
    Ellipse,
    Triangle,
    RightTriangle,
    Diamond,
    Pentagon,
    Hexagon,
    Arrow,
    Star,
    Heart,
}

impl ShapeKind {
    pub const ALL: [ShapeKind; 12] = [
        ShapeKind::Line,
        ShapeKind::Rectangle,
        ShapeKind::RoundedRectangle,
        ShapeKind::Ellipse,
        ShapeKind::Triangle,
        ShapeKind::RightTriangle,
        ShapeKind::Diamond,
        ShapeKind::Pentagon,
        ShapeKind::Hexagon,
        ShapeKind::Arrow,
        ShapeKind::Star,
        ShapeKind::Heart,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ShapeKind::Line => "Line",
            ShapeKind::Rectangle => "Rectangle",
            ShapeKind::RoundedRectangle => "Rounded rectangle",
            ShapeKind::Ellipse => "Oval",
            ShapeKind::Triangle => "Triangle",
            ShapeKind::RightTriangle => "Right triangle",
            ShapeKind::Diamond => "Diamond",
            ShapeKind::Pentagon => "Pentagon",
            ShapeKind::Hexagon => "Hexagon",
            ShapeKind::Arrow => "Arrow",
            ShapeKind::Star => "Star",
            ShapeKind::Heart => "Heart",
        }
    }

    /// Lines are open paths; everything else is closed.
    pub fn is_closed(self) -> bool {
        self != ShapeKind::Line
    }
}

/// How a closed shape is painted: outline with colour 1, fill with colour 2, or both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShapeStyle {
    #[default]
    Outline,
    Fill,
    Both,
}

impl ShapeStyle {
    pub fn label(self) -> &'static str {
        match self {
            ShapeStyle::Outline => "Outline",
            ShapeStyle::Fill => "Fill",
            ShapeStyle::Both => "Outline + fill",
        }
    }
}

/// The outline of `kind` dragged from `a` to `b`. With `constrain` (Shift held),
/// the box is square and lines snap to 45°.
pub fn outline(kind: ShapeKind, a: (f32, f32), b: (f32, f32), constrain: bool) -> Vec<(f32, f32)> {
    let b = if constrain {
        constrained(kind, a, b)
    } else {
        b
    };
    if kind == ShapeKind::Line {
        return vec![a, b];
    }
    let (x0, y0) = (a.0.min(b.0), a.1.min(b.1));
    let (w, h) = ((b.0 - a.0).abs(), (b.1 - a.1).abs());
    let (cx, cy) = (x0 + w / 2.0, y0 + h / 2.0);
    // Unit-box points scaled into the drag box.
    let fit = |pts: &[(f32, f32)]| -> Vec<(f32, f32)> {
        pts.iter().map(|&(u, v)| (x0 + u * w, y0 + v * h)).collect()
    };
    let regular = |n: u32, start: f32| -> Vec<(f32, f32)> {
        (0..n)
            .map(|i| {
                let t = start + TAU * i as f32 / n as f32;
                (cx + w / 2.0 * t.cos(), cy + h / 2.0 * t.sin())
            })
            .collect()
    };
    match kind {
        ShapeKind::Line => vec![a, b],
        ShapeKind::Rectangle => fit(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]),
        ShapeKind::RoundedRectangle => rounded_rect(x0, y0, w, h),
        ShapeKind::Ellipse => {
            let n = (((w + h) * 0.75) as u32).clamp(24, 360);
            regular(n, 0.0)
        }
        ShapeKind::Triangle => fit(&[(0.5, 0.0), (1.0, 1.0), (0.0, 1.0)]),
        ShapeKind::RightTriangle => fit(&[(0.0, 0.0), (1.0, 1.0), (0.0, 1.0)]),
        ShapeKind::Diamond => fit(&[(0.5, 0.0), (1.0, 0.5), (0.5, 1.0), (0.0, 0.5)]),
        ShapeKind::Pentagon => regular(5, -PI / 2.0),
        ShapeKind::Hexagon => regular(6, 0.0),
        ShapeKind::Arrow => fit(&[
            (0.0, 0.3),
            (0.6, 0.3),
            (0.6, 0.0),
            (1.0, 0.5),
            (0.6, 1.0),
            (0.6, 0.7),
            (0.0, 0.7),
        ]),
        ShapeKind::Star => (0..10)
            .map(|i| {
                let t = -PI / 2.0 + PI * i as f32 / 5.0;
                let r = if i % 2 == 0 { 1.0 } else { 0.4 };
                (cx + w / 2.0 * r * t.cos(), cy + h / 2.0 * r * t.sin())
            })
            .collect(),
        ShapeKind::Heart => heart(x0, y0, w, h),
    }
}

fn constrained(kind: ShapeKind, a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    if kind == ShapeKind::Line {
        // Snap to the nearest multiple of 45°.
        let len = (dx * dx + dy * dy).sqrt();
        let angle = (dy.atan2(dx) / (PI / 4.0)).round() * (PI / 4.0);
        return (a.0 + len * angle.cos(), a.1 + len * angle.sin());
    }
    let side = dx.abs().max(dy.abs());
    (a.0 + side.copysign(dx), a.1 + side.copysign(dy))
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32) -> Vec<(f32, f32)> {
    let r = (w.min(h) * 0.18).max(0.0);
    let corners = [
        (x + w - r, y + r, -PI / 2.0),
        (x + w - r, y + h - r, 0.0),
        (x + r, y + h - r, PI / 2.0),
        (x + r, y + r, PI),
    ];
    let mut pts = Vec::with_capacity(4 * 9);
    for (cx, cy, start) in corners {
        for i in 0..=8 {
            let t = start + PI / 2.0 * i as f32 / 8.0;
            pts.push((cx + r * t.cos(), cy + r * t.sin()));
        }
    }
    pts
}

fn heart(x: f32, y: f32, w: f32, h: f32) -> Vec<(f32, f32)> {
    // The classic parametric heart, normalised into the box.
    let raw: Vec<(f32, f32)> = (0..120)
        .map(|i| {
            let t = TAU * i as f32 / 120.0;
            let hx = 16.0 * t.sin().powi(3);
            let hy =
                13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos();
            (hx, -hy)
        })
        .collect();
    let (mut minx, mut miny, mut maxx, mut maxy) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for &(px, py) in &raw {
        minx = minx.min(px);
        miny = miny.min(py);
        maxx = maxx.max(px);
        maxy = maxy.max(py);
    }
    let (sw, sh) = (
        (maxx - minx).max(f32::EPSILON),
        (maxy - miny).max(f32::EPSILON),
    );
    raw.iter()
        .map(|&(px, py)| (x + (px - minx) / sw * w, y + (py - miny) / sh * h))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shape_stays_inside_its_box() {
        for kind in ShapeKind::ALL {
            let pts = outline(kind, (10.0, 20.0), (110.0, 70.0), false);
            assert!(pts.len() >= 2, "{kind:?}");
            for &(x, y) in &pts {
                assert!(
                    (9.9..=110.1).contains(&x) && (19.9..=70.1).contains(&y),
                    "{kind:?} {x},{y}"
                );
            }
        }
    }

    #[test]
    fn shift_makes_squares_and_45_degree_lines() {
        let sq = outline(ShapeKind::Rectangle, (0.0, 0.0), (50.0, 20.0), true);
        assert!(sq.contains(&(50.0, 50.0)));
        let line = outline(ShapeKind::Line, (0.0, 0.0), (10.0, 9.0), true);
        let end = line[1];
        assert!((end.0 - end.1).abs() < 1e-3);
    }

    #[test]
    fn zero_size_drags_do_not_produce_nan() {
        for kind in ShapeKind::ALL {
            for (x, y) in outline(kind, (5.0, 5.0), (5.0, 5.0), true) {
                assert!(x.is_finite() && y.is_finite(), "{kind:?}");
            }
        }
    }
}
