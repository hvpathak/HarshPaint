//! Undo and redo. Strokes store only the pixels they changed (before and after);
//! whole-picture changes store layer snapshots. Old steps are dropped once the
//! history passes its memory budget, so long sessions can't exhaust RAM.

use std::collections::VecDeque;

use crate::canvas::Rect;
use crate::color::Rgba;
use crate::document::{Document, Layer};

/// Default memory budget for undo steps.
pub const DEFAULT_BUDGET_BYTES: usize = 512 * 1024 * 1024;

/// One rectangle of a layer, before and after.
#[derive(Debug, Clone)]
pub struct Patch {
    pub rect: Rect,
    pub before: Vec<Rgba>,
    pub after: Vec<Rgba>,
}

#[derive(Debug, Clone)]
pub enum Change {
    /// Several separate rectangles of one layer (a symmetric stroke touches many
    /// small areas far apart; storing their bounding box would waste memory).
    /// Overlapping patches hold identical pixels, so their order doesn't matter.
    Patches { layer: usize, patches: Vec<Patch> },
    /// A rectangle of one layer, before and after.
    Pixels {
        layer: usize,
        rect: Rect,
        before: Vec<Rgba>,
        after: Vec<Rgba>,
    },
    /// The whole layer stack, before and after (size changes, layer edits).
    Layers {
        before: Vec<Layer>,
        before_active: usize,
        after: Vec<Layer>,
        after_active: usize,
    },
}

impl Change {
    fn bytes(&self) -> usize {
        let px = std::mem::size_of::<Rgba>();
        match self {
            Change::Patches { patches, .. } => patches
                .iter()
                .map(|p| (p.before.len() + p.after.len()) * px)
                .sum(),
            Change::Pixels { before, after, .. } => (before.len() + after.len()) * px,
            Change::Layers { before, after, .. } => before
                .iter()
                .chain(after)
                .map(|l| std::mem::size_of_val(l.raster.pixels()))
                .sum(),
        }
    }

    pub(crate) fn apply(&self, doc: &mut Document, forward: bool) {
        match self {
            Change::Patches { layer, patches } => {
                if let Some(l) = doc.layer_mut(*layer) {
                    for p in patches {
                        l.raster
                            .write_rect(p.rect, if forward { &p.after } else { &p.before });
                    }
                }
            }
            Change::Pixels {
                layer,
                rect,
                before,
                after,
            } => {
                if let Some(l) = doc.layer_mut(*layer) {
                    l.raster
                        .write_rect(*rect, if forward { after } else { before });
                }
            }
            Change::Layers {
                before,
                before_active,
                after,
                after_active,
            } => {
                let (layers, active) = if forward {
                    (after, *after_active)
                } else {
                    (before, *before_active)
                };
                // Snapshots were valid when taken, so this only fails on corruption;
                // then the document is left as it is rather than crashing.
                if doc.replace_layers(layers.clone()).is_ok() {
                    doc.set_active(active);
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Step {
    pub label: &'static str,
    pub change: Change,
}

#[derive(Debug)]
pub struct History {
    undo: VecDeque<Step>,
    redo: Vec<Step>,
    bytes: usize,
    budget: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::with_budget(DEFAULT_BUDGET_BYTES)
    }
}

impl History {
    pub fn with_budget(budget: usize) -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
            bytes: 0,
            budget,
        }
    }

    pub fn push(&mut self, label: &'static str, change: Change) {
        self.redo.clear();
        self.bytes = self.undo.iter().map(|s| s.change.bytes()).sum();
        self.bytes += change.bytes();
        self.undo.push_back(Step { label, change });
        // Keep at least the newest step even if it alone is over budget.
        while self.bytes > self.budget && self.undo.len() > 1 {
            if let Some(old) = self.undo.pop_front() {
                self.bytes = self.bytes.saturating_sub(old.change.bytes());
            }
        }
    }

    pub fn undo(&mut self, doc: &mut Document) -> Option<&'static str> {
        let step = self.undo.pop_back()?;
        step.change.apply(doc, false);
        self.bytes = self.bytes.saturating_sub(step.change.bytes());
        let label = step.label;
        self.redo.push(step);
        Some(label)
    }

    pub fn redo(&mut self, doc: &mut Document) -> Option<&'static str> {
        let step = self.redo.pop()?;
        step.change.apply(doc, true);
        self.bytes += step.change.bytes();
        let label = step.label;
        self.undo.push_back(step);
        Some(label)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.bytes = 0;
    }

    /// Undo steps from oldest to newest (used to replay a time-lapse).
    pub fn steps(&self) -> impl Iterator<Item = &Step> {
        self.undo.iter()
    }

    pub fn len(&self) -> usize {
        self.undo.len()
    }

    pub fn is_empty(&self) -> bool {
        self.undo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paint(doc: &mut Document, hist: &mut History, x: u32, c: Rgba) {
        let rect = Rect::new(x, 0, 1, 1);
        let before = doc.active().unwrap().raster.read_rect(rect);
        doc.active_mut().unwrap().raster.set(x, 0, c);
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

    #[test]
    fn undo_redo_is_byte_exact() {
        let mut doc = Document::new(3, 1, Rgba::WHITE).unwrap();
        let original = doc.clone();
        let mut hist = History::default();
        paint(&mut doc, &mut hist, 0, Rgba::BLACK);
        paint(&mut doc, &mut hist, 1, Rgba::BLACK);
        let edited = doc.clone();
        assert_eq!(hist.undo(&mut doc), Some("Paint"));
        assert_eq!(hist.undo(&mut doc), Some("Paint"));
        assert_eq!(doc, original);
        assert_eq!(hist.undo(&mut doc), None);
        hist.redo(&mut doc);
        hist.redo(&mut doc);
        assert_eq!(doc, edited);
        assert!(hist.redo(&mut doc).is_none());
    }

    #[test]
    fn new_change_clears_redo() {
        let mut doc = Document::new(3, 1, Rgba::WHITE).unwrap();
        let mut hist = History::default();
        paint(&mut doc, &mut hist, 0, Rgba::BLACK);
        hist.undo(&mut doc);
        assert!(hist.can_redo());
        paint(&mut doc, &mut hist, 1, Rgba::BLACK);
        assert!(!hist.can_redo());
    }

    #[test]
    fn budget_drops_oldest_steps() {
        let mut doc = Document::new(3, 1, Rgba::WHITE).unwrap();
        // Each step stores 2 pixels × 4 bytes = 8 bytes.
        let mut hist = History::with_budget(20);
        for x in 0..3 {
            paint(&mut doc, &mut hist, x, Rgba::BLACK);
        }
        assert_eq!(hist.len(), 2);
    }

    #[test]
    fn layer_snapshots_undo() {
        let mut doc = Document::new(2, 2, Rgba::WHITE).unwrap();
        let mut hist = History::default();
        let before = doc.layers().to_vec();
        doc.add_layer().unwrap();
        hist.push(
            "New layer",
            Change::Layers {
                before,
                before_active: 0,
                after: doc.layers().to_vec(),
                after_active: 1,
            },
        );
        hist.undo(&mut doc);
        assert_eq!(doc.layers().len(), 1);
        hist.redo(&mut doc);
        assert_eq!(doc.layers().len(), 2);
        assert_eq!(doc.active_index(), 1);
    }
}
