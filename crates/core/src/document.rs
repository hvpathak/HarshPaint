//! A picture: a stack of layers that share one size.

use crate::canvas::{Raster, Rect};
use crate::color::{Rgba, mul_255, to_u8};
use crate::error::{Error, Result};

/// Most layers a document may have (keeps memory and `.hpaint` files bounded).
pub const MAX_LAYERS: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    /// 0.0 ..= 1.0
    pub opacity: f32,
    pub raster: Raster,
}

impl Layer {
    pub fn new(name: impl Into<String>, raster: Raster) -> Self {
        Self {
            name: name.into(),
            visible: true,
            opacity: 1.0,
            raster,
        }
    }

    fn alpha_scale(&self) -> u8 {
        to_u8(self.opacity.clamp(0.0, 1.0) * 255.0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    width: u32,
    height: u32,
    layers: Vec<Layer>,
    active: usize,
}

impl Document {
    /// A new picture with one opaque background layer.
    pub fn new(width: u32, height: u32, background: Rgba) -> Result<Self> {
        Ok(Self::from_raster(Raster::new(width, height, background)?))
    }

    pub fn from_raster(raster: Raster) -> Self {
        Self {
            width: raster.width(),
            height: raster.height(),
            layers: vec![Layer::new("Background", raster)],
            active: 0,
        }
    }

    /// Rebuilds a document from saved layers; all must match `width` × `height`.
    pub fn from_layers(width: u32, height: u32, layers: Vec<Layer>, active: usize) -> Result<Self> {
        if layers.is_empty() || layers.len() > MAX_LAYERS {
            return Err(Error::Format(format!(
                "a picture needs between 1 and {MAX_LAYERS} layers"
            )));
        }
        if layers
            .iter()
            .any(|l| l.raster.width() != width || l.raster.height() != height)
        {
            return Err(Error::Format(
                "a layer's size does not match the picture".into(),
            ));
        }
        Ok(Self {
            width,
            height,
            active: active.min(layers.len() - 1),
            layers,
        })
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

    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    pub fn layer(&self, index: usize) -> Option<&Layer> {
        self.layers.get(index)
    }

    pub fn layer_mut(&mut self, index: usize) -> Option<&mut Layer> {
        self.layers.get_mut(index)
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn set_active(&mut self, index: usize) {
        if index < self.layers.len() {
            self.active = index;
        }
    }

    pub fn active(&self) -> Option<&Layer> {
        self.layers.get(self.active)
    }

    pub fn active_mut(&mut self) -> Option<&mut Layer> {
        self.layers.get_mut(self.active)
    }

    /// Replaces every layer at once (resize, crop, rotate…); sizes must agree.
    pub fn replace_layers(&mut self, layers: Vec<Layer>) -> Result<()> {
        let Some(first) = layers.first() else {
            return Err(Error::Unavailable("a picture needs at least one layer"));
        };
        let (w, h) = (first.raster.width(), first.raster.height());
        *self = Document::from_layers(w, h, layers, self.active)?;
        Ok(())
    }

    // ---- Layer stack -------------------------------------------------------

    /// Adds a transparent layer above the active one and selects it.
    pub fn add_layer(&mut self) -> Result<()> {
        self.ensure_room()?;
        let raster = Raster::new(self.width, self.height, Rgba::TRANSPARENT)?;
        let name = format!("Layer {}", self.layers.len() + 1);
        self.insert_above_active(Layer::new(name, raster));
        Ok(())
    }

    pub fn duplicate_layer(&mut self) -> Result<()> {
        self.ensure_room()?;
        let Some(src) = self.active() else {
            return Err(Error::Unavailable("there is no layer to duplicate"));
        };
        let mut copy = src.clone();
        copy.name = format!("{} copy", src.name);
        self.insert_above_active(copy);
        Ok(())
    }

    pub fn delete_layer(&mut self) -> Result<()> {
        if self.layers.len() <= 1 {
            return Err(Error::Unavailable("a picture needs at least one layer"));
        }
        if self.active < self.layers.len() {
            self.layers.remove(self.active);
        }
        self.active = self.active.saturating_sub(1).min(self.layers.len() - 1);
        Ok(())
    }

    /// Moves the active layer up (towards the top) or down the stack.
    pub fn move_layer(&mut self, up: bool) -> Result<()> {
        let target = if up {
            self.active + 1
        } else {
            self.active
                .checked_sub(1)
                .ok_or(Error::Unavailable("the layer is already at the bottom"))?
        };
        if target >= self.layers.len() {
            return Err(Error::Unavailable("the layer is already at the top"));
        }
        self.layers.swap(self.active, target);
        self.active = target;
        Ok(())
    }

    /// Paints the active layer onto the one below it, then removes it.
    pub fn merge_down(&mut self) -> Result<()> {
        let below = self
            .active
            .checked_sub(1)
            .ok_or(Error::Unavailable("there is no layer below to merge into"))?;
        let upper = self.layers.remove(self.active);
        if let Some(lower) = self.layers.get_mut(below)
            && upper.visible
        {
            let scale = upper.alpha_scale();
            for (d, s) in lower
                .raster
                .pixels_mut()
                .iter_mut()
                .zip(upper.raster.pixels())
            {
                *d = s.scaled(scale).over(*d);
            }
        }
        self.active = below;
        Ok(())
    }

    fn ensure_room(&self) -> Result<()> {
        if self.layers.len() >= MAX_LAYERS {
            return Err(Error::Unavailable(
                "that's the most layers a picture can have",
            ));
        }
        Ok(())
    }

    fn insert_above_active(&mut self, layer: Layer) {
        let at = (self.active + 1).min(self.layers.len());
        self.layers.insert(at, layer);
        self.active = at;
    }

    // ---- Compositing -------------------------------------------------------

    /// The visible picture inside `rect`, bottom layer first.
    pub fn composite_rect(&self, rect: Rect) -> Vec<Rgba> {
        let r = rect.clip(self.width, self.height);
        let mut out = vec![Rgba::TRANSPARENT; r.area() as usize];
        for layer in self.layers.iter().filter(|l| l.visible && l.opacity > 0.0) {
            let scale = layer.alpha_scale();
            for dy in 0..r.h {
                let src = layer.raster.row(r.y + dy, r.x, r.w);
                let start = dy as usize * r.w as usize;
                let Some(dst) = out.get_mut(start..start + src.len()) else {
                    continue;
                };
                for (d, s) in dst.iter_mut().zip(src) {
                    if s.a != 0 {
                        *d = s.with_alpha(mul_255(s.a, scale)).over(*d);
                    }
                }
            }
        }
        out
    }

    /// The whole visible picture as one raster.
    pub fn flatten(&self) -> Result<Raster> {
        let mut out = Raster::new(self.width, self.height, Rgba::TRANSPARENT)?;
        out.write_rect(self.bounds(), &self.composite_rect(self.bounds()));
        Ok(out)
    }

    /// The visible colour at one pixel (colour picker).
    pub fn sample(&self, x: u32, y: u32) -> Option<Rgba> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.composite_rect(Rect::new(x, y, 1, 1)).first().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_add_move_merge_delete() {
        let mut doc = Document::new(4, 4, Rgba::WHITE).unwrap();
        doc.add_layer().unwrap();
        assert_eq!(doc.layers().len(), 2);
        assert_eq!(doc.active_index(), 1);
        doc.active_mut().unwrap().raster.set(0, 0, Rgba::BLACK);
        assert!(doc.move_layer(true).is_err());
        doc.move_layer(false).unwrap();
        assert_eq!(doc.active_index(), 0);
        doc.move_layer(true).unwrap();
        doc.merge_down().unwrap();
        assert_eq!(doc.layers().len(), 1);
        assert_eq!(doc.layer(0).unwrap().raster.get(0, 0), Some(Rgba::BLACK));
        assert!(doc.delete_layer().is_err());
        assert!(doc.merge_down().is_err());
    }

    #[test]
    fn composite_respects_visibility_and_opacity() {
        let mut doc = Document::new(2, 1, Rgba::WHITE).unwrap();
        doc.add_layer().unwrap();
        doc.active_mut().unwrap().raster.set(0, 0, Rgba::BLACK);
        assert_eq!(doc.sample(0, 0), Some(Rgba::BLACK));
        assert_eq!(doc.sample(1, 0), Some(Rgba::WHITE));
        doc.active_mut().unwrap().opacity = 0.5;
        let half = doc.sample(0, 0).unwrap();
        assert!((126..=129).contains(&half.r), "{half:?}");
        doc.active_mut().unwrap().visible = false;
        assert_eq!(doc.sample(0, 0), Some(Rgba::WHITE));
        assert_eq!(doc.sample(5, 5), None);
    }

    #[test]
    fn layer_count_is_capped() {
        let mut doc = Document::new(1, 1, Rgba::WHITE).unwrap();
        for _ in 1..MAX_LAYERS {
            doc.add_layer().unwrap();
        }
        assert!(doc.add_layer().is_err());
        assert!(doc.duplicate_layer().is_err());
    }

    #[test]
    fn from_layers_validates() {
        let a = Layer::new("a", Raster::new(2, 2, Rgba::WHITE).unwrap());
        let b = Layer::new("b", Raster::new(3, 2, Rgba::WHITE).unwrap());
        assert!(Document::from_layers(2, 2, vec![a.clone(), b], 0).is_err());
        assert!(Document::from_layers(2, 2, vec![], 0).is_err());
        let doc = Document::from_layers(2, 2, vec![a], 9).unwrap();
        assert_eq!(doc.active_index(), 0);
    }
}
