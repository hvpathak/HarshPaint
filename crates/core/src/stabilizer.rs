//! Smooth strokes ("lazy rope"): the pen trails the mouse on an invisible string.
//! Small wobbles never pull the string tight, so they never reach the canvas.

#[derive(Debug, Clone)]
pub struct Stabilizer {
    /// String length in pixels; 0 disables smoothing.
    length: f32,
    pen: Option<(f32, f32)>,
}

impl Stabilizer {
    /// `strength` 0..=100 maps to a string of 0..=40 pixels.
    pub fn new(strength: u8) -> Self {
        Self {
            length: f32::from(strength.min(100)) * 0.4,
            pen: None,
        }
    }

    /// Feeds a mouse position; returns where the pen is now.
    pub fn update(&mut self, target: (f32, f32)) -> (f32, f32) {
        let Some(pen) = self.pen else {
            self.pen = Some(target);
            return target;
        };
        let (dx, dy) = (target.0 - pen.0, target.1 - pen.1);
        let dist = (dx * dx + dy * dy).sqrt();
        let next = if dist > self.length && dist.is_finite() {
            let pull = (dist - self.length) / dist;
            (pen.0 + dx * pull, pen.1 + dy * pull)
        } else {
            pen
        };
        self.pen = Some(next);
        next
    }

    pub fn pen(&self) -> Option<(f32, f32)> {
        self.pen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Total distance travelled along a path.
    fn path_length(points: &[(f32, f32)]) -> f32 {
        points
            .windows(2)
            .map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt())
            .sum()
    }

    #[test]
    fn zero_strength_follows_exactly() {
        let mut s = Stabilizer::new(0);
        assert_eq!(s.update((1.0, 1.0)), (1.0, 1.0));
        assert_eq!(s.update((5.0, 9.0)), (5.0, 9.0));
    }

    #[test]
    fn jitter_is_smoothed_away() {
        // A horizontal line with ±3 px of hand shake.
        let input: Vec<(f32, f32)> = (0..200)
            .map(|i| (i as f32 * 2.0, if i % 2 == 0 { 3.0 } else { -3.0 }))
            .collect();
        let mut s = Stabilizer::new(50);
        let output: Vec<_> = input.iter().map(|&p| s.update(p)).collect();
        // Skip the warm-up while the string first pulls tight.
        let max_wobble = output
            .iter()
            .skip(100)
            .map(|p| p.1.abs())
            .fold(0.0, f32::max);
        assert!(max_wobble < 0.5, "wobble {max_wobble}");
        assert!(path_length(&output) < path_length(&input));
    }

    #[test]
    fn non_finite_input_does_not_poison_the_pen() {
        let mut s = Stabilizer::new(10);
        s.update((0.0, 0.0));
        let p = s.update((f32::INFINITY, 0.0));
        assert!(p.0.is_finite());
    }
}
