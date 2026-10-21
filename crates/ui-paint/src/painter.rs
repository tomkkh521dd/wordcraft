//! The rasteriser: meshes in, pixels out.

use egui::epaint::{Primitive, Vertex};
use egui::{ClippedPrimitive, Color32, Mesh, TextureFilter, TexturesDelta};

use crate::canvas::{Canvas, Target};
use crate::texture::{Store, Texture};

/// How much the last frame drew, for logs and tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub triangles: usize,
    pub pixels: u64,
}

/// Draws egui's meshes into a frame, keeping the textures egui uploads between frames.
#[derive(Default)]
pub struct Painter {
    store: Store,
    frames: u64,
    stats: Stats,
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
        let (width, height) = (target.width as usize, target.height as usize);
        let Some(mut canvas) = Canvas::new(target) else { return };
        for clipped in primitives {
            let Primitive::Mesh(mesh) = &clipped.primitive else { continue };
            let clip = clip_to_pixels(clipped.clip_rect, ppp, width, height);
            if clip.is_empty() || mesh.indices.len() < 3 || mesh.vertices.is_empty() {
                continue;
            }
            let texture = self.store.get(mesh.texture_id);
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
        let colour = shade(v[0].color, texture, v[0].uv);
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
            let px = x as f32 + 0.5;
            let at = egui::pos2(px, py);
            let w = [edge(p[1], p[2], at), edge(p[2], p[0], at), edge(p[0], p[1], at)];
            let inside = w.iter().all(|w| *w >= 0.0) || w.iter().all(|w| *w <= 0.0);
            if !inside {
                continue;
            }
            let weights = [w[0] / area, w[1] / area, w[2] / area];
            let colour = lerp_color([v[0].color, v[1].color, v[2].color], weights);
            let uv = egui::pos2(
                v[0].uv.x * weights[0] + v[1].uv.x * weights[1] + v[2].uv.x * weights[2],
                v[0].uv.y * weights[0] + v[1].uv.y * weights[1] + v[2].uv.y * weights[2],
            );
            canvas.blend(x, y, shade(colour, texture, uv));
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

/// The vertex colour over the texture (both premultiplied), as egui's shader does.
fn shade(colour: Color32, texture: Option<&Texture>, uv: egui::Pos2) -> Color32 {
    match texture {
        Some(texture) => modulate(colour, sample(texture, uv)),
        None => colour,
    }
}

fn modulate(a: Color32, b: Color32) -> Color32 {
    let mul = |x: u8, y: u8| ((u32::from(x) * u32::from(y) + 127) / 255) as u8;
    Color32::from_rgba_premultiplied(mul(a.r(), b.r()), mul(a.g(), b.g()), mul(a.b(), b.b()), mul(a.a(), b.a()))
}

fn lerp_color(colours: [Color32; 3], weights: [f32; 3]) -> Color32 {
    let lerp = |channel: fn(&Color32) -> u8| {
        let v = f32::from(channel(&colours[0])) * weights[0]
            + f32::from(channel(&colours[1])) * weights[1]
            + f32::from(channel(&colours[2])) * weights[2];
        v.clamp(0.0, 255.0) as u8
    };
    Color32::from_rgba_premultiplied(lerp(Color32::r), lerp(Color32::g), lerp(Color32::b), lerp(Color32::a))
}

/// The texel at `uv`, filtered the way the texture asks for. Outside `[0, 1)` it clamps.
fn sample(texture: &Texture, uv: egui::Pos2) -> Color32 {
    let [w, h] = texture.size;
    if w == 0 || h == 0 || texture.pixels.len() < w.saturating_mul(h) {
        return Color32::WHITE;
    }
    let (u, v) = if uv.x.is_finite() && uv.y.is_finite() { (uv.x, uv.y) } else { (0.0, 0.0) };
    let texel = |x: usize, y: usize| texture.pixels.get(y.min(h - 1) * w + x.min(w - 1)).copied().unwrap_or(Color32::WHITE);
    if matches!(texture.options.magnification, TextureFilter::Nearest) {
        let x = (u * w as f32).floor().clamp(0.0, (w - 1) as f32) as usize;
        let y = (v * h as f32).floor().clamp(0.0, (h - 1) as f32) as usize;
        return texel(x, y);
    }
    // Bilinear, in premultiplied space (which is what the stored pixels are).
    let x = u * w as f32 - 0.5;
    let y = v * h as f32 - 0.5;
    let x0 = x.floor();
    let y0 = y.floor();
    let fx = x - x0;
    let fy = y - y0;
    let at = |dx: f32, dy: f32| {
        let xi = (x0 + dx).clamp(0.0, (w - 1) as f32) as usize;
        let yi = (y0 + dy).clamp(0.0, (h - 1) as f32) as usize;
        texel(xi, yi)
    };
    let channel = |a: Color32, b: Color32, t: f32, f: fn(&Color32) -> u8| f32::from(f(&a)) * (1.0 - t) + f32::from(f(&b)) * t;
    let (p00, p10, p01, p11) = (at(0.0, 0.0), at(1.0, 0.0), at(0.0, 1.0), at(1.0, 1.0));
    let bilinear = |f: fn(&Color32) -> u8| {
        let top = channel(p00, p10, fx, f);
        let bottom = channel(p01, p11, fx, f);
        (top * (1.0 - fy) + bottom * fy).clamp(0.0, 255.0) as u8
    };
    Color32::from_rgba_premultiplied(bilinear(Color32::r), bilinear(Color32::g), bilinear(Color32::b), bilinear(Color32::a))
}
