//! How long a frame of WordCraft's UI takes to rasterise, split by what the frame is made of.
//!
//!   cargo run -p wordcraft-ui-paint --release --example bench
//!
//! The three cases are the ones a real frame mixes: the chrome's rectangles, the document page
//! drawn 1:1 from its tile bitmaps, and text (thousands of small quads sampling the font atlas).
//! Sizes follow the HarmonyOS tablet (1840x2650 at 2.25x = 1840x2650 pixels); pass a size and a
//! repeat count to measure something else.

use std::sync::Arc;
use std::time::Instant;

use egui::epaint::{ImageDelta, Mesh, Primitive, Vertex};
use egui::{ClippedPrimitive, Color32, ColorImage, ImageData, Pos2, Rect, TextureId, TextureOptions, TexturesDelta, pos2};
use smallvec::smallvec;
use wordcraft_ui_paint::{Painter, Target};

/// A quad spanning `(x0, y0)`–`(x1, y1)` in points, sampling `uv0`–`uv1` of the texture.
fn quad(x0: f32, y0: f32, x1: f32, y1: f32, uv0: Pos2, uv1: Pos2, colour: Color32, texture: TextureId) -> Mesh {
    let vertices = vec![
        Vertex { pos: pos2(x0, y0), uv: uv0, color: colour },
        Vertex { pos: pos2(x1, y0), uv: pos2(uv1.x, uv0.y), color: colour },
        Vertex { pos: pos2(x1, y1), uv: uv1, color: colour },
        Vertex { pos: pos2(x0, y1), uv: pos2(uv0.x, uv1.y), color: colour },
    ];
    Mesh { indices: vec![0, 1, 2, 0, 2, 3], vertices, texture_id: texture }
}

fn solid(x0: f32, y0: f32, x1: f32, y1: f32, colour: Color32) -> Mesh {
    let mut mesh = quad(x0, y0, x1, y1, pos2(0.0, 0.0), pos2(0.0, 0.0), colour, TextureId::Managed(0));
    mesh.texture_id = TextureId::Managed(0);
    mesh
}

fn whole_screen(width: u32, height: u32) -> Rect {
    Rect { min: Pos2::ZERO, max: pos2(width as f32, height as f32) }
}

fn clipped(mesh: Mesh, clip: Rect) -> ClippedPrimitive {
    ClippedPrimitive { clip_rect: clip, primitive: Primitive::Mesh(mesh) }
}

/// Time `repeat` frames and report the best and median milliseconds.
fn time(painter: &mut Painter, target: &mut [u8], width: u32, height: u32, primitives: &[ClippedPrimitive], repeat: usize) -> (f64, f64) {
    let mut times = Vec::with_capacity(repeat);
    for _ in 0..repeat {
        let started = Instant::now();
        painter.paint(Target { pixels: target, width, height }, 1.0, primitives);
        times.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = times.get(times.len() / 2).copied().unwrap_or(0.0);
    (times.first().copied().unwrap_or(0.0), median)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let width: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(1840);
    let height: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(2650);
    let repeat: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(20);

    let mut painter = Painter::new();
    let mut target = vec![0u8; width as usize * height as usize * 4];

    // A page-sized texture for the 1:1 blit (the document's tile bitmaps look like this).
    let page_w = width as usize;
    let page_h = height as usize;
    let mut page = Vec::with_capacity(page_w * page_h);
    for y in 0..page_h {
        for x in 0..page_w {
            page.push(Color32::from_rgb((x % 251) as u8, (y % 241) as u8, ((x + y) % 239) as u8));
        }
    }
    push_texture(&mut painter, TextureId::Managed(1), [page_w, page_h], page);

    // A small atlas, sampled by every glyph.
    let atlas = 512;
    let mut atlas_pixels = Vec::with_capacity(atlas * atlas);
    for y in 0..atlas {
        for x in 0..atlas {
            atlas_pixels.push(Color32::from_white_alpha(((x + y) % 255) as u8));
        }
    }
    push_texture(&mut painter, TextureId::Managed(2), [atlas, atlas], atlas_pixels);

    let clip = whole_screen(width, height);

    // (a) The chrome: 1000 opaque rectangles.
    let rects: Vec<ClippedPrimitive> = (0..1000)
        .map(|i| {
            let x = (i % 40) as f32 * (width as f32 / 40.0);
            let y = (i / 40) as f32 * (height as f32 / 25.0);
            clipped(solid(x, y, x + 24.0, y + 18.0, Color32::from_rgb(200, 210, 220)), clip)
        })
        .collect();

    // (b) The document page, drawn 1:1 from its texture.
    let page_quad =
        vec![clipped(quad(0.0, 0.0, width as f32, height as f32, pos2(0.0, 0.0), pos2(1.0, 1.0), Color32::WHITE, TextureId::Managed(1)), clip)];

    // (c) Text: 8000 glyph quads, 12x16 points each (a full page of 2.25x text).
    let glyphs: Vec<ClippedPrimitive> = (0..8000)
        .map(|i| {
            let col = i % 100;
            let row = i / 100;
            let x = 40.0 + col as f32 * 18.0;
            let y = 40.0 + row as f32 * 24.0;
            let u0 = ((i % 16) as f32) / 16.0;
            let v0 = ((i % 32) as f32) / 32.0;
            clipped(quad(x, y, x + 12.0, y + 16.0, pos2(u0, v0), pos2(u0 + 0.05, v0 + 0.04), Color32::BLACK, TextureId::Managed(2)), clip)
        })
        .collect();

    let pixels = (width as f64) * (height as f64) / 1.0e6;
    println!("frame {width}x{height} ({pixels:.1} MP), {repeat} frames each, best / median:");
    for (name, prims) in [("1000 opaque rects", &rects), ("one 1:1 page quad", &page_quad), ("8000 glyph quads", &glyphs)] {
        let (best, median) = time(&mut painter, &mut target, width, height, prims, repeat);
        println!("  {name:22} {best:6.2} ms / {median:6.2} ms");
    }
    let all: Vec<ClippedPrimitive> = rects.iter().chain(page_quad.iter()).chain(glyphs.iter()).cloned().collect();
    let (best, median) = time(&mut painter, &mut target, width, height, &all, repeat);
    println!("  {:22} {best:6.2} ms / {median:6.2} ms", "all of it (one frame)");
    println!("stats of the last frame: {:?}", painter.stats());
}

fn push_texture(painter: &mut Painter, id: TextureId, size: [usize; 2], pixels: Vec<Color32>) {
    let mut delta = TexturesDelta::default();
    delta.set.insert(
        id,
        smallvec![ImageDelta { image: ImageData::Color(Arc::new(ColorImage::new(size, pixels))), options: TextureOptions::NEAREST, pos: None }],
    );
    painter.apply_textures(&delta);
    delta.clear();
}
