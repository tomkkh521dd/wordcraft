//! The RGBA8888 frame the painter draws into, and the two operations it needs.

use egui::Color32;

/// A frame to draw into: RGBA8888, `width * 4` bytes per row, premultiplied alpha.
///
/// Rows are packed (no padding), which is what the HarmonyOS native window buffer copy expects.
pub struct Target<'a> {
    pub pixels: &'a mut [u8],
    pub width: u32,
    pub height: u32,
}

impl Target<'_> {
    /// Whether the buffer matches the size (a hostile size must not index past it).
    pub(crate) fn is_consistent(&self) -> bool {
        let Some(len) = (self.width as usize).checked_mul(self.height as usize).and_then(|p| p.checked_mul(4)) else {
            return false;
        };
        self.width > 0 && self.height > 0 && self.pixels.len() == len
    }
}

/// The pixel loop, kept apart from the geometry so it can be tightened later without touching the
/// rasteriser.
pub(crate) struct Canvas<'a> {
    pixels: &'a mut [u8],
    width: usize,
    height: usize,
}

impl<'a> Canvas<'a> {
    pub(crate) fn new(target: Target<'a>) -> Option<Self> {
        if !target.is_consistent() {
            return None;
        }
        Some(Self { pixels: target.pixels, width: target.width as usize, height: target.height as usize })
    }

    fn row(&mut self, y: usize, x: usize) -> Option<&mut [u8]> {
        let index = y.checked_mul(self.width)?.checked_add(x)?.checked_mul(4)?;
        self.pixels.get_mut(index..index.checked_add(4)?)
    }

    /// One premultiplied "source over" pixel.
    #[inline]
    pub(crate) fn blend(&mut self, x: usize, y: usize, src: Color32) {
        let alpha = src.a();
        if alpha == 0 || x >= self.width || y >= self.height {
            return;
        }
        let Some(dst) = self.row(y, x) else { return };
        if alpha == 255 {
            dst.copy_from_slice(&src.to_array());
            return;
        }
        let inverse = 255 - alpha as u32;
        for (channel, out) in dst.iter_mut().enumerate() {
            let over = (src.to_array()[channel] as u32) + (u32::from(*out) * inverse + 127) / 255;
            *out = over.min(255) as u8;
        }
    }

    /// The same colour across a span: the fast path for the rectangles a UI is made of.
    #[inline]
    pub(crate) fn blend_span(&mut self, y: usize, x0: usize, x1: usize, src: Color32) {
        if y >= self.height {
            return;
        }
        let x0 = x0.min(self.width);
        let x1 = x1.min(self.width);
        if x1 <= x0 {
            return;
        }
        if src.a() == 255 {
            // Opaque: write the four bytes straight through, row-wise.
            let colour = src.to_array();
            for x in x0..x1 {
                if let Some(dst) = self.row(y, x) {
                    dst.copy_from_slice(&colour);
                }
            }
            return;
        }
        for x in x0..x1 {
            self.blend(x, y, src);
        }
    }
}
