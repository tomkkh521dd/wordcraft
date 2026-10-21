//! The painter's contract: geometry, clipping, blending, texture sampling, and never panicking on
//! hostile input. Pixels are asserted directly — no golden images to attribute or maintain.

use std::sync::Arc;

use egui::epaint::{ImageDelta, Mesh, Primitive, Vertex};
use egui::{ClippedPrimitive, Color32, ColorImage, ImageData, Rect, TextureId, TextureOptions, TexturesDelta, pos2, vec2};
use smallvec::smallvec;

use crate::{Painter, Target};

/// A clip rectangle far larger than any test frame.
fn whole() -> Rect {
    Rect { min: pos2(-1.0e4, -1.0e4), max: pos2(1.0e4, 1.0e4) }
}

fn frame(width: u32, height: u32) -> Vec<u8> {
    vec![0; (width as usize) * (height as usize) * 4]
}

fn target(pixels: &mut [u8], width: u32, height: u32) -> Target<'_> {
    Target { pixels, width, height }
}

/// A quad in points, corners `(x0, y0)`–`(x1, y1)`, one colour and one uv.
fn quad(x0: f32, y0: f32, x1: f32, y1: f32, colour: Color32, uv: egui::Pos2) -> Mesh {
    let vertices = vec![
        Vertex { pos: pos2(x0, y0), uv, color: colour },
        Vertex { pos: pos2(x1, y0), uv, color: colour },
        Vertex { pos: pos2(x1, y1), uv, color: colour },
        Vertex { pos: pos2(x0, y1), uv, color: colour },
    ];
    Mesh { indices: vec![0, 1, 2, 0, 2, 3], vertices, texture_id: TextureId::Managed(0) }
}

/// The same quad, with the texture coordinate spanning `uv0`–`uv1` across it.
fn quad_uv(x0: f32, y0: f32, x1: f32, y1: f32, colour: Color32, uv0: egui::Pos2, uv1: egui::Pos2) -> Mesh {
    let vertices = vec![
        Vertex { pos: pos2(x0, y0), uv: uv0, color: colour },
        Vertex { pos: pos2(x1, y0), uv: pos2(uv1.x, uv0.y), color: colour },
        Vertex { pos: pos2(x1, y1), uv: uv1, color: colour },
        Vertex { pos: pos2(x0, y1), uv: pos2(uv0.x, uv1.y), color: colour },
    ];
    Mesh { indices: vec![0, 1, 2, 0, 2, 3], vertices, texture_id: TextureId::Managed(0) }
}

fn clipped(mesh: Mesh, clip: Rect) -> ClippedPrimitive {
    ClippedPrimitive { clip_rect: clip, primitive: Primitive::Mesh(mesh) }
}

fn texture(painter: &mut Painter, id: TextureId, size: [usize; 2], pixels: Vec<Color32>, options: TextureOptions) {
    let mut delta = TexturesDelta::default();
    delta.set.insert(id, smallvec![ImageDelta { image: ImageData::Color(Arc::new(ColorImage::new(size, pixels))), options, pos: None }]);
    painter.apply_textures(&delta);
    // A TexturesDelta must be emptied once applied (its Drop asserts that).
    delta.clear();
}

#[test]
fn an_opaque_quad_fills_its_area() {
    let mut painter = Painter::new();
    let (w, h) = (8u32, 4u32);
    let mut pixels = frame(w, h);
    let red = Color32::from_rgb(255, 0, 0);
    painter.paint(target(&mut pixels, w, h), 1.0, &[clipped(quad(0.0, 0.0, 8.0, 4.0, red, egui::pos2(0.0, 0.0)), whole())]);
    assert!(pixels.as_chunks::<4>().0.iter().all(|p| p == &[255, 0, 0, 255]), "every pixel is red");
    assert_eq!(painter.stats().triangles, 2);
    assert_eq!(painter.frames(), 1);
}

#[test]
fn the_clip_rectangle_limits_the_drawing() {
    let mut painter = Painter::new();
    let (w, h) = (8u32, 4u32);
    let mut pixels = frame(w, h);
    let white = Color32::WHITE;
    let clip = Rect { min: pos2(0.0, 0.0), max: pos2(4.0, 2.0) };
    painter.paint(target(&mut pixels, w, h), 1.0, &[clipped(quad(0.0, 0.0, 8.0, 4.0, white, pos2(0.0, 0.0)), clip)]);
    let at = |x: u32, y: u32| pixels[((y * w + x) * 4) as usize..][..4].to_vec();
    assert_eq!(at(0, 0), [255, 255, 255, 255]);
    assert_eq!(at(3, 1), [255, 255, 255, 255]);
    assert_eq!(at(4, 0), [0, 0, 0, 0], "right of the clip");
    assert_eq!(at(0, 2), [0, 0, 0, 0], "below the clip");
}

#[test]
fn translucent_colour_blends_source_over() {
    let mut painter = Painter::new();
    let (w, h) = (2u32, 2u32);
    // Start from opaque blue, then draw half-transparent white over it.
    let mut pixels = frame(w, h);
    for p in pixels.as_chunks_mut::<4>().0 {
        *p = [0, 0, 255, 255];
    }
    let half_white = Color32::from_rgba_premultiplied(127, 127, 127, 127);
    painter.paint(target(&mut pixels, w, h), 1.0, &[clipped(quad(0.0, 0.0, 2.0, 2.0, half_white, pos2(0.0, 0.0)), whole())]);
    // 0.5 white over blue: red 127, green 127, blue 127 + 128 = 255.
    assert_eq!(&pixels[0..4], &[127, 127, 255, 255], "premultiplied source-over");
}

#[test]
fn geometry_is_scaled_by_pixels_per_point() {
    let mut painter = Painter::new();
    let (w, h) = (8u32, 8u32);
    let mut pixels = frame(w, h);
    // A 4x4-point quad at 2x covers the whole 8x8 target.
    painter.paint(target(&mut pixels, w, h), 2.0, &[clipped(quad(0.0, 0.0, 4.0, 4.0, Color32::WHITE, pos2(0.0, 0.0)), whole())]);
    assert!(pixels.as_chunks::<4>().0.iter().all(|p| p == &[255, 255, 255, 255]), "every pixel is white");
}

#[test]
fn a_texture_is_sampled_and_modulated_by_the_vertex_colour() {
    let mut painter = Painter::new();
    let id = TextureId::Managed(0);
    // Left column white, right column transparent, both 2x2.
    texture(&mut painter, id, [2, 2], vec![Color32::WHITE, Color32::TRANSPARENT, Color32::WHITE, Color32::TRANSPARENT], TextureOptions::NEAREST);
    assert_eq!(painter.texture_count(), 1);

    let (w, h) = (4u32, 4u32);
    let mut pixels = frame(w, h);
    // Sample only the white half (u in [0, 0.25)), tinted green by the vertex colour.
    let green = Color32::from_rgb(0, 255, 0);
    painter.paint(target(&mut pixels, w, h), 1.0, &[clipped(quad(0.0, 0.0, 4.0, 4.0, green, pos2(0.0, 0.0)), whole())]);
    assert_eq!(&pixels[0..4], &[0, 255, 0, 255], "white texel × green vertex");

    // Sample the transparent half: nothing is drawn.
    let mut pixels = frame(w, h);
    painter.paint(target(&mut pixels, w, h), 1.0, &[clipped(quad(0.0, 0.0, 4.0, 4.0, green, pos2(0.75, 0.0)), whole())]);
    assert_eq!(&pixels[0..4], &[0, 0, 0, 0], "a transparent texel leaves the frame alone");
}

#[test]
fn filtering_follows_the_texture_options() {
    let id = TextureId::Managed(0);
    let texels = vec![Color32::from_rgb(0, 0, 0), Color32::WHITE, Color32::from_rgb(0, 0, 0), Color32::WHITE];

    let mut nearest = Painter::new();
    texture(&mut nearest, id, [2, 2], texels.clone(), TextureOptions::NEAREST);
    let mut pixels = frame(2, 2);
    nearest.paint(target(&mut pixels, 2, 2), 1.0, &[clipped(quad(0.0, 0.0, 2.0, 2.0, Color32::WHITE, pos2(0.5, 0.5)), whole())]);
    let nearest_pixel = pixels[0..4].to_vec();
    assert!(nearest_pixel == vec![0, 0, 0, 255] || nearest_pixel == vec![255, 255, 255, 255], "one texel, not a mix: {nearest_pixel:?}");

    let mut linear = Painter::new();
    texture(&mut linear, id, [2, 2], texels, TextureOptions::LINEAR);
    let mut pixels = frame(2, 2);
    linear.paint(target(&mut pixels, 2, 2), 1.0, &[clipped(quad(0.0, 0.0, 2.0, 2.0, Color32::WHITE, pos2(0.5, 0.5)), whole())]);
    let mixed = pixels[0];
    assert!((100..=155).contains(&mixed), "bilinear mixes the neighbours: {mixed}");
}

#[test]
fn a_partial_texture_update_lands_in_the_right_place() {
    let mut painter = Painter::new();
    let id = TextureId::Managed(0);
    texture(&mut painter, id, [4, 1], vec![Color32::BLACK; 4], TextureOptions::NEAREST);
    let mut delta = TexturesDelta::default();
    delta.set.insert(
        id,
        smallvec![ImageDelta {
            image: ImageData::Color(Arc::new(ColorImage::new([1, 1], vec![Color32::WHITE]))),
            options: TextureOptions::NEAREST,
            pos: Some([2, 0]),
        }],
    );
    painter.apply_textures(&delta);
    delta.clear();

    let mut pixels = frame(4, 1);
    painter.paint(target(&mut pixels, 4, 1), 1.0, &[clipped(quad_uv(0.0, 0.0, 4.0, 1.0, Color32::WHITE, pos2(0.0, 0.5), pos2(1.0, 0.5)), whole())]);
    let row: Vec<u8> = pixels.to_vec();
    assert_eq!(&row[0..4], &[0, 0, 0, 255], "texel 0 stays black");
    assert_eq!(&row[8..12], &[255, 255, 255, 255], "texel 2 was replaced");
    assert_eq!(&row[12..16], &[0, 0, 0, 255], "texel 3 stays black");
}

#[test]
fn freed_textures_are_dropped() {
    let mut painter = Painter::new();
    let id = TextureId::Managed(0);
    texture(&mut painter, id, [1, 1], vec![Color32::WHITE], TextureOptions::NEAREST);
    assert_eq!(painter.texture_count(), 1);
    let mut delta = TexturesDelta::default();
    delta.free.insert(id);
    painter.apply_textures(&delta);
    delta.clear();
    assert_eq!(painter.texture_count(), 0);
    // Drawing with an unknown texture is the vertex colour alone, not a panic.
    let mut pixels = frame(1, 1);
    painter.paint(target(&mut pixels, 1, 1), 1.0, &[clipped(quad(0.0, 0.0, 1.0, 1.0, Color32::from_rgb(1, 2, 3), pos2(0.0, 0.0)), whole())]);
    assert_eq!(&pixels[0..4], &[1, 2, 3, 255]);
}

/// Nothing a platform or a UI can hand over may panic the painter.
#[test]
fn hostile_input_does_not_panic() {
    let mut painter = Painter::new();
    let nan = f32::NAN;
    let huge = 1.0e30f32;
    let cases = vec![
        quad(nan, 0.0, 4.0, 4.0, Color32::WHITE, pos2(0.0, 0.0)),
        quad(0.0, 0.0, huge, huge, Color32::WHITE, pos2(0.0, 0.0)),
        quad(4.0, 4.0, 0.0, 0.0, Color32::WHITE, pos2(0.0, 0.0)),
        Mesh { indices: vec![0, 1, 99], vertices: vec![], texture_id: TextureId::Managed(7) },
        Mesh { indices: vec![], vertices: vec![], texture_id: TextureId::Managed(7) },
        Mesh {
            indices: vec![0, 0, 0],
            vertices: vec![Vertex { pos: pos2(1.0, 1.0), uv: pos2(0.0, 0.0), color: Color32::WHITE }],
            texture_id: TextureId::Managed(7),
        },
    ];
    // A buffer that doesn't match the size is refused, not indexed.
    let mut short = vec![0u8; 7];
    painter.paint(Target { pixels: &mut short, width: 8, height: 8 }, 1.0, &[clipped(cases[0].clone(), whole())]);
    let mut empty = frame(0, 0);
    painter.paint(target(&mut empty, 0, 0), 1.0, &[]);
    // And every hostile case against a real frame, at a hostile scale too.
    for mesh in cases {
        let mut pixels = frame(8, 8);
        for ppp in [1.0f32, nan, huge, 0.0, -2.0] {
            painter.paint(target(&mut pixels, 8, 8), ppp, &[clipped(mesh.clone(), whole())]);
        }
    }
    // A clip that is inverted or outside the frame draws nothing.
    let mut pixels = frame(8, 8);
    let inverted = Rect { min: pos2(10.0, 10.0), max: pos2(-10.0, -10.0) };
    painter.paint(target(&mut pixels, 8, 8), 1.0, &[clipped(quad(0.0, 0.0, 8.0, 8.0, Color32::WHITE, pos2(0.0, 0.0)), inverted)]);
    assert!(pixels.iter().all(|b| *b == 0));
}

#[test]
fn a_paint_callback_is_skipped() {
    let mut painter = Painter::new();
    let mut pixels = frame(2, 2);
    let callback = ClippedPrimitive {
        clip_rect: whole(),
        primitive: Primitive::Callback(egui::PaintCallback { rect: Rect::from_min_size(pos2(0.0, 0.0), vec2(2.0, 2.0)), callback: Arc::new(()) }),
    };
    painter.paint(target(&mut pixels, 2, 2), 1.0, &[callback]);
    assert!(pixels.iter().all(|b| *b == 0), "nothing drawn, nothing crashed");
}
