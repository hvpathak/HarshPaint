//! Symmetry painting: every point you draw is copied into mirrored or rotated
//! positions around the centre of the picture, live, as you paint.

use std::f32::consts::TAU;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Symmetry {
    #[default]
    Off,
    /// Mirror across the vertical centre line (left ↔ right).
    MirrorX,
    /// Mirror across the horizontal centre line (top ↔ bottom).
    MirrorY,
    /// Both mirrors: four copies.
    Quad,
    /// `arms` rotated copies; with `mirror`, each arm is also reflected (kaleidoscope).
    Radial { arms: u8, mirror: bool },
}

impl Symmetry {
    pub fn label(self) -> String {
        match self {
            Symmetry::Off => "Off".into(),
            Symmetry::MirrorX => "Mirror ↔".into(),
            Symmetry::MirrorY => "Mirror ↕".into(),
            Symmetry::Quad => "Four-way".into(),
            Symmetry::Radial {
                arms,
                mirror: false,
            } => format!("Radial ×{arms}"),
            Symmetry::Radial { arms, mirror: true } => format!("Kaleidoscope ×{arms}"),
        }
    }

    /// How many copies each point produces (including the original).
    pub fn copies(self) -> usize {
        match self {
            Symmetry::Off => 1,
            Symmetry::MirrorX | Symmetry::MirrorY => 2,
            Symmetry::Quad => 4,
            Symmetry::Radial { arms, mirror } => {
                usize::from(arms.clamp(2, 64)) * if mirror { 2 } else { 1 }
            }
        }
    }

    /// All copies of `p` around `centre`. Copy `i` of consecutive points forms one
    /// continuous stroke, so callers can draw segment i→i between them.
    pub fn map(self, p: (f32, f32), centre: (f32, f32)) -> Vec<(f32, f32)> {
        let (cx, cy) = centre;
        let (dx, dy) = (p.0 - cx, p.1 - cy);
        match self {
            Symmetry::Off => vec![p],
            Symmetry::MirrorX => vec![p, (cx - dx, p.1)],
            Symmetry::MirrorY => vec![p, (p.0, cy - dy)],
            Symmetry::Quad => vec![p, (cx - dx, p.1), (p.0, cy - dy), (cx - dx, cy - dy)],
            Symmetry::Radial { arms, mirror } => {
                let n = arms.clamp(2, 64);
                let mut out = Vec::with_capacity(self.copies());
                for k in 0..n {
                    let a = TAU * f32::from(k) / f32::from(n);
                    let (s, c) = a.sin_cos();
                    out.push((cx + dx * c - dy * s, cy + dx * s + dy * c));
                    if mirror {
                        // Reflect across the vertical axis, then rotate.
                        let (mx, my) = (-dx, dy);
                        out.push((cx + mx * c - my * s, cy + mx * s + my * c));
                    }
                }
                out
            }
        }
    }

    /// Every symmetry mode in menu order.
    pub const ALL: [Symmetry; 9] = [
        Symmetry::Off,
        Symmetry::MirrorX,
        Symmetry::MirrorY,
        Symmetry::Quad,
        Symmetry::Radial {
            arms: 6,
            mirror: false,
        },
        Symmetry::Radial {
            arms: 8,
            mirror: false,
        },
        Symmetry::Radial {
            arms: 6,
            mirror: true,
        },
        Symmetry::Radial {
            arms: 8,
            mirror: true,
        },
        Symmetry::Radial {
            arms: 12,
            mirror: true,
        },
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: (f32, f32), b: (f32, f32)) -> bool {
        (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3
    }

    #[test]
    fn mirrors_reflect_around_centre() {
        let c = (50.0, 50.0);
        assert_eq!(
            Symmetry::MirrorX.map((10.0, 20.0), c),
            vec![(10.0, 20.0), (90.0, 20.0)]
        );
        assert_eq!(
            Symmetry::MirrorY.map((10.0, 20.0), c),
            vec![(10.0, 20.0), (10.0, 80.0)]
        );
        let q = Symmetry::Quad.map((10.0, 20.0), c);
        assert!(q.contains(&(90.0, 80.0)));
    }

    #[test]
    fn radial_copies_are_rotations() {
        let pts = Symmetry::Radial {
            arms: 4,
            mirror: false,
        }
        .map((60.0, 50.0), (50.0, 50.0));
        assert_eq!(pts.len(), 4);
        assert!(close(pts[1], (50.0, 60.0)));
        assert!(close(pts[2], (40.0, 50.0)));
        assert!(close(pts[3], (50.0, 40.0)));
    }

    #[test]
    fn every_mode_produces_its_declared_count() {
        for mode in Symmetry::ALL {
            assert_eq!(
                mode.map((3.0, 4.0), (10.0, 10.0)).len(),
                mode.copies(),
                "{mode:?}"
            );
        }
        // Absurd arm counts are clamped, not allocated.
        assert_eq!(
            Symmetry::Radial {
                arms: 255,
                mirror: true
            }
            .copies(),
            128
        );
        assert_eq!(
            Symmetry::Radial {
                arms: 0,
                mirror: false
            }
            .copies(),
            2
        );
    }

    #[test]
    fn kaleidoscope_includes_reflections() {
        let pts = Symmetry::Radial {
            arms: 2,
            mirror: true,
        }
        .map((60.0, 40.0), (50.0, 50.0));
        assert!(pts.iter().any(|&p| close(p, (40.0, 40.0))));
    }
}
