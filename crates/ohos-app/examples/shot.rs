//! Render the HarmonyOS app's frame on the host, with no GPU and no device.
//!
//! `cargo run -p wordcraft-ohos-app --example shot -- out.ppm [width] [height] [density] [frames]`
//!
//! It drives exactly what the device drives (the same `App`, the same painter), so a layout or
//! drawing question can be answered on a laptop instead of through `hdc`. Writes a PPM (P6), which
//! every image tool reads and which needs no encoder.

use std::io::Write;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| "frame.ppm".into());
    let width: u32 = args.next().map(|v| v.parse().unwrap_or(1920)).unwrap_or(1920);
    let height: u32 = args.next().map(|v| v.parse().unwrap_or(2799)).unwrap_or(2799);
    let density: f32 = args.next().map(|v| v.parse().unwrap_or(1.5)).unwrap_or(1.5);
    let frames: usize = args.next().map(|v| v.parse().unwrap_or(4)).unwrap_or(4);

    let mut app = wordcraft_ohos_app::App::new();
    app.resize(width, height, density);
    // The UI installs its fonts over the first passes (as on the device); the shell keeps drawing.
    let mut painted = 0usize;
    for _ in 0..frames.max(1) {
        if app.render() == wordcraft_ohos_app::FrameNeeded::Yes {
            painted += 1;
        }
    }
    if painted == 0 {
        return Err(format!("no frame drawn at {width}x{height}").into());
    }

    let pixels = app.pixels();
    let mut file = std::io::BufWriter::new(std::fs::File::create(&out)?);
    write!(file, "P6\n{width} {height}\n255\n")?;
    // RGBA (premultiplied) → RGB over white, which is what the device compositor does with an
    // opaque surface.
    for px in pixels.as_chunks::<4>().0 {
        let a = u32::from(px[3]);
        let over = |c: u8| ((u32::from(c) * 255 + 255 * (255 - a)) / 255).min(255) as u8;
        file.write_all(&[over(px[0]), over(px[1]), over(px[2])])?;
    }
    file.flush()?;
    let t = app.timings();
    println!("last frame: ui {:.1} ms, tessellate {:.1} ms, paint {:.1} ms", t.ui_ms, t.tessellate_ms, t.paint_ms);
    println!("painted {painted} of {frames} passes (egui skips the frames nothing changed in)");
    println!(
        "{out}: {width}x{height} at {density}x, {} frames, {} triangles, {} textures",
        app.frames(),
        app.stats_json().split("\"triangles\":").nth(1).and_then(|s| s.split(',').next()).unwrap_or("?"),
        app.painter_textures()
    );
    Ok(())
}
