//! The RGBA8888 frame the painter draws into, and the pixel loops it needs.
//!
//! Everything works on whole rows (`&mut [u8]`) and four-byte chunks, not on indexes computed per
//! pixel: the frame is 5.4 MP, so a few nanoseconds per pixel is the difference between 3 fps and
//! 30.

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

pub(crate) struct Canvas<'a> {
    pixels: &'a mut [u8],
    width: usize,
    height: usize,
    stride: usize,
}

impl<'a> Canvas<'a> {
    pub(crate) fn new(target: Target<'a>) -> Option<Self> {
        if !target.is_consistent() {
            return None;
        }
        let width = target.width as usize;
        Some(Self { pixels: target.pixels, width, height: target.height as usize, stride: width * 4 })
    }

    pub(crate) fn width(&self) -> usize {
        self.width
    }

    pub(crate) fn height(&self) -> usize {
        self.height
    }

    /// One row as whole pixels. `None` when the row is off the frame.
    #[inline]
    pub(crate) fn row(&mut self, y: usize) -> Option<&mut [[u8; 4]]> {
        if y >= self.height {
            return None;
        }
        let start = y.checked_mul(self.stride)?;
        let end = start.checked_add(self.stride)?;
        self.pixels.get_mut(start..end).map(|row| row.as_chunks_mut::<4>().0)
    }

    /// The same colour across a span: the fast path for the rectangles a UI is made of.
    #[inline]
    pub(crate) fn blend_span(&mut self, y: usize, x0: usize, x1: usize, src: Color32) {
        if src.a() == 0 {
            return;
        }
        let x0 = x0.min(self.width);
        let x1 = x1.min(self.width);
        if x1 <= x0 {
            return;
        }
        let colour = src.to_array();
        let Some(row) = self.row(y) else { return };
        let Some(span) = row.get_mut(x0..x1) else { return };
        if colour[3] == 255 {
            for px in span.iter_mut() {
                *px = colour;
            }
        } else {
            for px in span.iter_mut() {
                over(px, colour);
            }
        }
    }

    /// Copy whole pixels from a texture row, replacing the destination (an opaque blit).
    ///
    /// `src` holds `count` texels; the destination must already be clipped to the row.
    #[inline]
    pub(crate) fn blit_row(&mut self, y: usize, x0: usize, src: &[Color32], opaque: bool) {
        let count = src.len();
        if count == 0 {
            return;
        }
        let x0 = x0.min(self.width);
        let x1 = (x0 + count).min(self.width);
        if x1 <= x0 {
            return;
        }
        let Some(row) = self.row(y) else { return };
        let Some(span) = row.get_mut(x0..x1) else { return };
        let source = &src[..x1 - x0];
        if opaque {
            for (px, texel) in span.iter_mut().zip(source) {
                *px = texel.to_array();
            }
        } else {
            for (px, texel) in span.iter_mut().zip(source) {
                over(px, texel.to_array());
            }
        }
    }

}

/// Premultiplied "source over": `out = src + dst * (1 - src.a)`.
#[inline]
pub(crate) fn over(dst: &mut [u8; 4], src: [u8; 4]) {
    let a = u32::from(src[3]);
    if a == 255 {
        *dst = src;
        return;
    }
    let inverse = 255 - a;
    for i in 0..4 {
        let value = u32::from(src[i]) + (u32::from(dst[i]) * inverse + 127) / 255;
        dst[i] = value.min(255) as u8;
    }
}
