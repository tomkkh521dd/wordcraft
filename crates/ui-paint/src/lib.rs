//! Draws egui's tessellated output into an RGBA8888 buffer on the CPU.
//!
//! The HarmonyOS app creates no GPU context (see `docs/platform-ohos.md`), so the UI is
//! rasterised here while `vello_cpu` rasterises the document pages. egui's output is small to draw:
//! triangles with a position in points, a normalised texture coordinate and a premultiplied sRGBA
//! colour, clipped to a rectangle. Blending is premultiplied "source over", in sRGB space — the
//! same result a GPU surface with premultiplied blending gives up to rounding, which keeps the
//! output close to the desktop window without a colour-space conversion on every pixel.
//!
//! Text comes through the same path: glyphs are quads sampling the font atlas, modulated by the
//! run's colour.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod canvas;
mod painter;
mod texture;

pub use canvas::Target;
pub use painter::{Painter, Stats};

#[cfg(test)]
mod tests;
