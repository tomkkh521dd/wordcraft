//! The texture store: egui hands over new and changed textures, and takes them back when freed.

use std::collections::HashMap;

use egui::epaint::{ImageData, ImageDelta};
use egui::{Color32, TextureId, TextureOptions, TexturesDelta};

pub(crate) struct Texture {
    pub(crate) size: [usize; 2],
    /// sRGBA, premultiplied, row-major.
    pub(crate) pixels: Vec<Color32>,
    pub(crate) options: TextureOptions,
}

#[derive(Default)]
pub(crate) struct Store {
    textures: HashMap<TextureId, Texture>,
}

impl Store {
    /// Apply a frame's texture changes before painting it.
    pub(crate) fn apply(&mut self, delta: &TexturesDelta) {
        for (id, deltas) in &delta.set {
            for d in deltas {
                self.upsert(*id, d);
            }
        }
        for id in &delta.free {
            self.textures.remove(id);
        }
    }

    pub(crate) fn get(&self, id: TextureId) -> Option<&Texture> {
        self.textures.get(&id)
    }

    pub(crate) fn len(&self) -> usize {
        self.textures.len()
    }

    fn upsert(&mut self, id: TextureId, delta: &ImageDelta) {
        let ImageData::Color(image) = &delta.image;
        let [w, h] = image.size;
        let Some(count) = w.checked_mul(h) else { return };
        if count == 0 || image.pixels.len() < count {
            return;
        }
        let options = delta.options;
        match delta.pos {
            // A whole new texture, or one replaced outright.
            None => {
                let mut pixels = image.pixels.clone();
                pixels.truncate(count);
                self.textures.insert(id, Texture { size: [w, h], pixels, options });
            }
            // A partial update: copy the patch into the existing texture, clipped to it.
            Some([x0, y0]) => {
                let Some(texture) = self.textures.get_mut(&id) else { return };
                let (tw, th) = (texture.size[0], texture.size[1]);
                for y in 0..h {
                    let ty = y0 + y;
                    if ty >= th {
                        break;
                    }
                    for x in 0..w {
                        let tx = x0 + x;
                        if tx >= tw {
                            break;
                        }
                        if let (Some(src), Some(dst)) = (image.pixels.get(y * w + x), texture.pixels.get_mut(ty * tw + tx)) {
                            *dst = *src;
                        }
                    }
                }
                texture.options = options;
            }
        }
    }
}
