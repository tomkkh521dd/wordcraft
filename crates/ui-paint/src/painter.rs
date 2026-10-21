//! The rasteriser: meshes in, pixels out.
//!
//! Three paths, in the order they are tried for each mesh:
//!
//! 1. **A 1:1 textured quad** (the document page and its tiles, inserted pictures): the texture is
//!    copied row by row, which is what almost all of the painted pixels are.
//! 2. **A uniform triangle** (backgrounds, borders, rules, white fills behind text): one span per
//!    scanline, no sampling.
//! 3. **Everything else** (glyph quads, rotated shapes, gradients): barycentric weights per pixel.
//!
//! The pixel loops live in [`crate::canvas`] and work on whole rows.

use egui::epaint::{Primitive, Vertex};
use egui::{ClippedPrimitive, Color32, Mesh, TextureFilter, TexturesDelta};

use crate::canvas::{Canvas, Target};
use crate::texture::{Store, Texture};

/// How much the last frame drew, for logs and tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub triangles: usize,
    pub pixels: u64,
    /// Pixels copied by the 1:1 blit path (part of `pixels`).
    pub blitted: u64,
}

/// Draws egui's meshes into a frame, keeping the textures egui uploads between frames.
#[derive(Default)]
pub struct Painter {
    store: Store,
    frames: u64,
    stats: Stats,
    /// Row scratch for the blit path, kept between frames so a frame allocates nothing.
    row: Vec<[u8; 4]>,
}

impl Painter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply the texture updates that came with a frame, and drop the ones the UI freed.
    pub fn apply_textures(&mut self, delta: &TexturesDelta) {
        self.store.apply(delta);
    }

    /// Textures kept between frames (the font atlas, images the UI uploaded).
    pub fn texture_count(&self) -> usize {
        self.store.len()
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// Draw one frame. `pixels_per_point` turns egui's points into the target's pixels.
    ///
    /// Paint callbacks (`Primitive::Callback`) need a renderer of their own and are skipped; this
    /// UI draws none.
    pub fn paint(&mut self, target: Target<'_>, pixels_per_point: f32, primitives: &[ClippedPrimitive]) {
        let ppp = if pixels_per_point.is_finite() && pixels_per_point > 0.0 { pixels_per_point } else { 1.0 };
        self.frames = self.frames.saturating_add(1);
        self.stats = Stats::default();
        let Some(mut canvas) = Canvas::new(target) else { return };
        for clipped in primitives {
            let Primitive::Mesh(mesh) = &clipped.primitive else { continue };
            let clip = clip_to_pixels(clipped.clip_rect, ppp, canvas.width(), canvas.height());
            if clip.is_empty() || mesh.indices.len() < 3 || mesh.vertices.is_empty() {
                continue;
            }
            let texture = self.store.get(mesh.texture_id);
            if texture.is_some() && self.row.len() < canvas.width() {
                self.row.resize(canvas.width(), [0; 4]);
            }
            if let Some(texture) = texture
                && blit_quad(&mut canvas, mesh, texture, clip, ppp, &mut self.row, &mut self.stats)
            {
                continue;
            }
            draw_mesh(&mut canvas, mesh, texture, clip, ppp, &mut self.stats);
        }
    }
}

/// The clip rectangle in whole pixels, clamped to the frame; `(x0, y0, x1, y1)` half-open.
fn clip_to_pixels(rect: egui::Rect, ppp: f32, width: usize, height: usize) -> Edges {
    let x0 = ((rect.min.x * ppp).floor()).clamp(0.0, width as f32) as usize;
    let y0 = ((rect.min.y * ppp).floor()).clamp(0.0, height as f32) as usize;
    let x1 = ((rect.max.x * ppp).ceil()).clamp(0.0, width as f32) as usize;
    let y1 = ((rect.max.y * ppp).ceil()).clamp(0.0, height as f32) as usize;
    Edges { x0, y0, x1: x1.max(x0), y1: y1.max(y0) }
}

#[derive(Clone, Copy, Debug)]
struct Edges {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}

impl Edges {
    fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }
}

/// A quad drawn one texture pixel per screen pixel: copy its rows.
///
/// egui draws every image and every page tile as such a quad, and they hold most of the frame's
/// pixels, so this runs before the general triangle path. `false` means "not this case", and the
/// caller draws the triangles instead.
fn blit_quad(canvas: &mut Canvas<'_>, mesh: &Mesh, texture: &Texture, clip: Edges, ppp: f32, row: &mut [[u8; 4]], stats: &mut Stats) -> bool {
    if mesh.vertices.len() != 4 || mesh.indices.len() != 6 {
        return false;
    }
    let v: [&Vertex; 4] = [&mesh.vertices[0], &mesh.vertices[1], &mesh.vertices[2], &mesh.vertices[3]];
    // The vertex colour modulates the texture; the opaque white egui uses for images is the case a
    // straight copy can take.
    if !v.iter().all(|vertex| vertex.color == Color32::WHITE) {
        return false;
    }
    let xs: [f32; 4] = v.map(|vertex| vertex.pos.x * ppp);
    let ys: [f32; 4] = v.map(|vertex| vertex.pos.y * ppp);
    let (x0, x1) = (xs.iter().copied().fold(f32::INFINITY, f32::min), xs.iter().copied().fold(f32::NEG_INFINITY, f32::max));
    let (y0, y1) = (ys.iter().copied().fold(f32::INFINITY, f32::min), ys.iter().copied().fold(f32::NEG_INFINITY, f32::max));
    if !(x0.is_finite() && x1.is_finite() && y0.is_finite() && y1.is_finite()) || x1 - x0 < 1.0 || y1 - y0 < 1.0 {
        return false;
    }
    let (u0, u1) = (v.iter().map(|q| q.uv.x).fold(f32::INFINITY, f32::min), v.iter().map(|q| q.uv.x).fold(f32::NEG_INFINITY, f32::max));
    let (t0, t1) = (v.iter().map(|q| q.uv.y).fold(f32::INFINITY, f32::min), v.iter().map(|q| q.uv.y).fold(f32::NEG_INFINITY, f32::max));
    let [tw, th] = texture.size;
    if tw == 0 || th == 0 || texture.pixels.len() < tw.saturating_mul(th) {
        return false;
    }
    // One texel per pixel, and the quad's corners on whole pixels: anything else needs filtering.
    let width_px = x1 - x0;
    let height_px = y1 - y0;
    let texels_w = (u1 - u0) * tw as f32;
    let texels_h = (t1 - t0) * th as f32;
    if (texels_w - width_px).abs() > 0.5 || (texels_h - height_px).abs() > 0.5 {
        return false;
    }
    if (x0 - x0.round()).abs() > 0.01 || (y0 - y0.round()).abs() > 0.01 || (x1 - x1.round()).abs() > 0.01 || (y1 - y1.round()).abs() > 0.01 {
        return false;
    }

    // Destination, clipped to the frame and the clip rect.
    let dx0 = (x0.round().max(0.0) as usize).max(clip.x0);
    let dx1 = (x1.round().max(0.0) as usize).min(clip.x1).min(canvas.width());
    let dy0 = (y0.round().max(0.0) as usize).max(clip.y0);
    let dy1 = (y1.round().max(0.0) as usize).min(clip.y1).min(canvas.height());
    if dx1 <= dx0 || dy1 <= dy0 {
        return false;
    }
    let texel_x0 = u0 * tw as f32;
    let texel_y0 = t0 * th as f32;
    let opaque = false; // the page tiles have alpha; blending is still a per-pixel 4-byte op
    for y in dy0..dy1 {
        let ty = ((texel_y0 + (y as f32 - y0)) as isize).clamp(0, th as isize - 1) as usize;
        let source_row = texture.pixels.get(ty * tw..(ty + 1) * tw);
        let Some(source_row) = source_row else { continue };
        let x_offset = dx0 as f32 - x0; // pixels into the quad
        let count = dx1 - dx0;
        for (i, slot) in row.iter_mut().take(count).enumerate() {
            let tx = ((texel_x0 + x_offset + i as f32) as isize).clamp(0, tw as isize - 1) as usize;
            *slot = source_row.get(tx).map(|c| c.to_array()).unwrap_or([0, 0, 0, 255]);
        }
        canvas.blit_row(y, dx0, &row[..count], opaque);
        stats.pixels = stats.pixels.saturating_add(count as u64);
        stats.blitted = stats.blitted.saturating_add(count as u64);
    }
    true
}

fn draw_mesh(canvas: &mut Canvas<'_>, mesh: &Mesh, texture: Option<&Texture>, clip: Edges, ppp: f32, stats: &mut Stats) {
    let vertices = &mesh.vertices;
    for [ia, ib, ic] in mesh.indices.as_chunks::<3>().0 {
        let (Some(a), Some(b), Some(c)) = (vertices.get(*ia as usize), vertices.get(*ib as usize), vertices.get(*ic as usize)) else {
            continue;
        };
        stats.triangles += 1;
        draw_triangle(canvas, [*a, *b, *c], texture, clip, ppp, stats);
    }
}

/// One triangle, in points, drawn with barycentric weights.
fn draw_triangle(canvas: &mut Canvas<'_>, v: [Vertex; 3], texture: Option<&Texture>, clip: Edges, ppp: f32, stats: &mut Stats) {
    let p: [egui::Pos2; 3] = v.map(|vertex| egui::pos2(vertex.pos.x * ppp, vertex.pos.y * ppp));
    let area = edge(p[0], p[1], p[2]);
    // Nothing to fill, or a hostile vertex: a NaN must not reach the loops below.
    if !area.is_finite() || area.abs() < 1.0e-6 || p.iter().any(|q| !q.x.is_finite() || !q.y.is_finite()) {
        return;
    }
    let min_x = p.iter().map(|q| q.x).fold(f32::INFINITY, f32::min).floor();
    let max_x = p.iter().map(|q| q.x).fold(f32::NEG_INFINITY, f32::max).ceil();
    let min_y = p.iter().map(|q| q.y).fold(f32::INFINITY, f32::min).floor();
    let max_y = p.iter().map(|q| q.y).fold(f32::NEG_INFINITY, f32::max).ceil();
    let x0 = (min_x.max(clip.x0 as f32) as usize).min(clip.x1);
    let x1 = (max_x.min(clip.x1 as f32).max(0.0) as usize).min(clip.x1);
    let y0 = (min_y.max(clip.y0 as f32) as usize).min(clip.y1);
    let y1 = (max_y.min(clip.y1 as f32).max(0.0) as usize).min(clip.y1);
    if x1 <= x0 || y1 <= y0 {
        return;
    }

    // Backgrounds, borders and rules have one colour and one texture coordinate throughout (the
    // white texel): sample once and fill spans, rather than per pixel.
    if v[0].color == v[1].color && v[1].color == v[2].color && v[0].uv == v[1].uv && v[1].uv == v[2].uv {
        let texel = match texture {
            Some(texture) => modulate(v[0].color.to_array(), sample(texture, v[0].uv)),
            None => v[0].color.to_array(),
        };
        let colour = Color32::from_rgba_premultiplied(texel[0], texel[1], texel[2], texel[3]);
        for y in y0..y1 {
            if let Some((left, right)) = span(p, y as f32 + 0.5, x0, x1) {
                canvas.blend_span(y, left, right, colour);
                stats.pixels = stats.pixels.saturating_add((right - left) as u64);
            }
        }
        return;
    }

    for y in y0..y1 {
        let py = y as f32 + 0.5;
        for x in x0..x1 {
            let at = egui::pos2(x as f32 + 0.5, py);
            let w = [edge(p[1], p[2], at), edge(p[2], p[0], at), edge(p[0], p[1], at)];
            let inside = w.iter().all(|w| *w >= 0.0) || w.iter().all(|w| *w <= 0.0);
            if !inside {
                continue;
            }
            let weights = [w[0] / area, w[1] / area, w[2] / area];
            let uv = egui::pos2(
                v[0].uv.x * weights[0] + v[1].uv.x * weights[1] + v[2].uv.x * weights[2],
                v[0].uv.y * weights[0] + v[1].uv.y * weights[1] + v[2].uv.y * weights[2],
            );
            let colour = lerp_color([v[0].color, v[1].color, v[2].color], weights);
            let texel = match texture {
                Some(texture) => modulate(colour, sample(texture, uv)),
                None => colour,
            };
            canvas.put(x, y, texel);
            stats.pixels = stats.pixels.saturating_add(1);
        }
    }
}

/// Twice the signed area of the triangle `(a, b, c)`.
#[inline]
fn edge(a: egui::Pos2, b: egui::Pos2, c: egui::Pos2) -> f32 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

/// The x range a scanline crosses inside the triangle, within `[x0, x1)`.
///
/// A convex triangle crosses a scanline twice (any more means the line runs through a vertex, and
/// the extremes are still the span).
fn span(p: [egui::Pos2; 3], y: f32, x0: usize, x1: usize) -> Option<(usize, usize)> {
    let mut left = f32::INFINITY;
    let mut right = f32::NEG_INFINITY;
    for i in 0..3 {
        let (a, b) = (p[i], p[(i + 1) % 3]);
        if (a.y <= y && b.y > y) || (b.y <= y && a.y > y) {
            let t = (y - a.y) / (b.y - a.y);
            let x = a.x + t * (b.x - a.x);
            left = left.min(x);
            right = right.max(x);
        }
    }
    if !left.is_finite() || !right.is_finite() || right <= left {
        return None;
    }
    let lo = (left.ceil().max(0.0) as usize).clamp(x0, x1);
    let hi = (right.ceil().max(0.0) as usize).clamp(x0, x1);
    (hi > lo).then_some((lo, hi))
}

fn modulate(colour: [u8; 4], texel: [u8; 4]) -> [u8; 4] {
    [
        ((u16::from(colour[0]) * u16::from(texel[0]) + 127) / 255) as u8,
        ((u16::from(colour[1]) * u16::from(texel[1]) + 127) / 255) as u8,
        ((u16::from(colour[2]) * u16::from(texel[2]) + 127) / 255) as u8,
        ((u16::from(colour[3]) * u16::from(texel[3]) + 127) / 255) as u8,
    ]
}

fn lerp_color(colours: [Color32; 3], weights: [f32; 3]) -> [u8; 4] {
    let lerp = |channel: fn(&Color32) -> u8| {
        let v = f32::from(channel(&colours[0])) * weights[0]
            + f32::from(channel(&colours[1])) * weights[1]
            + f32::from(channel(&colours[2])) * weights[2];
        v.clamp(0.0, 255.0) as u8
    };
    [lerp(Color32::r), lerp(Color32::g), lerp(Color32::b), lerp(Color32::a)]
}

/// The texel at `uv`, filtered the way the texture asks for. Outside `[0, 1)` it clamps.
fn sample(texture: &Texture, uv: egui::Pos2) -> [u8; 4] {
    let [w, h] = texture.size;
    if w == 0 || h == 0 || texture.pixels.len() < w.saturating_mul(h) {
        return [255, 255, 255, 255];
    }
    let (u, v) = if uv.x.is_finite() && uv.y.is_finite() { (uv.x, uv.y) } else { (0.0, 0.0) };
    let texel = |x: usize, y: usize| texture.pixels.get(y.min(h - 1) * w + x.min(w - 1)).map(|c| c.to_array()).unwrap_or([255, 255, 255, 255]);
    if matches!(texture.options.magnification, TextureFilter::Nearest) {
        let x = (u * w as f32).floor().clamp(0.0, (w - 1) as f32) as usize;
        let y = (v * h as f32).floor().clamp(0.0, (h - 1) as f32) as usize;
        return texel(x, y);
    }
    // Bilinear, in premultiplied space (which is what the stored pixels are).
    let x = u * w as f32 - 0.5;
    let y = v * h as f32 - 0.5;
    let xi = x.floor();
    let yi = y.floor();
    let fx = x - xi;
    let fy = y - yi;
    let at = |dx: f32, dy: f32| {
        let px = (xi + dx).clamp(0.0, (w - 1) as f32) as usize;
        let py = (yi + dy).clamp(0.0, (h - 1) as f32) as usize;
        texel(px, py)
    };
    let (p00, p10, p01, p11) = (at(0.0, 0.0), at(1.0, 0.0), at(0.0, 1.0), at(1.0, 1.0));
    let mut out = [0u8; 4];
    for i in 0..4 {
        let top = f32::from(p00[i]) * (1.0 - fx) + f32::from(p10[i]) * fx;
        let bottom = f32::from(p01[i]) * (1.0 - fx) + f32::from(p11[i]) * fx;
        out[i] = (top * (1.0 - fy) + bottom * fy).clamp(0.0, 255.0) as u8;
    }
    out
}
