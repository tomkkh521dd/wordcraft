//! The first frame: a test pattern that proves the whole chain on a device.
//!
//! Step 2 of the port puts pixels on a HarmonyOS surface before any UI exists, so a wrong stride,
//! channel order, density or input path shows up as a picture that is obviously wrong rather than
//! as a subtly broken document:
//!
//! - top left, four stripes: **red, green, blue, white** — in that order. Blue where red belongs
//!   means the buffer is BGRA, not RGBA.
//! - top right, a checkerboard — a wrong stride skews the squares into a rhombus.
//! - the very top, 3 px: **green when the surface has focus**, grey when it does not.
//! - upper middle, an orange bar whose length grows with the events received — input reaches the
//!   app at all.
//! - middle: one square per paragraph in the open document (the engine runs on the device), and
//!   one cyan square per system font folder the shell reported.
//! - a yellow line sweeping left to right — frames advance.
//! - a crosshair at the last pointer position (violet, or yellow while pressed) — coordinates and
//!   density are right.
//! - bottom left: ten 10-vp squares, so a screenshot shows whether vp-to-pixel scaling is right.

/// What to draw. Everything here is already in the units the picture needs.
pub(crate) struct View {
    pub time_s: f64,
    pub density: f32,
    pub frames: u64,
    pub events: u64,
    pub paragraphs: usize,
    pub font_dirs: usize,
    /// The last pointer position in pixels.
    pub pointer_px: Option<(f32, f32)>,
    pub pointer_down: bool,
    pub focused: bool,
}

const BG: [u8; 4] = [24, 26, 34, 255];
const FOCUS: [u8; 4] = [80, 200, 120, 255];
const UNFOCUSED: [u8; 4] = [90, 90, 100, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 200, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];
const CHECKER_A: [u8; 4] = [220, 220, 225, 255];
const CHECKER_B: [u8; 4] = [40, 42, 52, 255];
const EVENT_BAR: [u8; 4] = [240, 150, 40, 255];
const PARAGRAPH: [u8; 4] = [150, 120, 240, 255];
const FONT_DIR: [u8; 4] = [60, 220, 230, 255];
const SWEEP: [u8; 4] = [250, 230, 60, 255];
const POINTER: [u8; 4] = [255, 60, 160, 255];
const POINTER_DOWN: [u8; 4] = [255, 255, 80, 255];
const RULER: [u8; 4] = [120, 130, 150, 255];

/// Draw the pattern into `pixels` (RGBA8888, `width * 4` bytes per row).
///
/// False when the buffer and the size disagree, in which case nothing is drawn: a hostile size
/// (0, or one whose row doesn't fit) must never index out of bounds.
pub(crate) fn paint(pixels: &mut [u8], width: u32, height: u32, view: &View) -> bool {
    let Some(mut canvas) = Canvas::new(pixels, width, height) else { return false };
    canvas.paint(view);
    true
}

struct Canvas<'a> {
    px: &'a mut [u8],
    w: i64,
    h: i64,
    /// Bytes per row (`w * 4`).
    stride: usize,
}

impl<'a> Canvas<'a> {
    fn new(px: &'a mut [u8], width: u32, height: u32) -> Option<Self> {
        let w = i64::from(width);
        let h = i64::from(height);
        let stride = usize::try_from(w).ok()?.checked_mul(4)?;
        if stride == 0 || h <= 0 || px.len() != stride.checked_mul(usize::try_from(h).ok()?)? {
            return None;
        }
        Some(Self { px, w, h, stride })
    }

    fn pixel(&mut self, x: i64, y: i64, c: [u8; 4]) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return;
        }
        let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y)) else {
            return;
        };
        let Some(index) = y.checked_mul(self.stride).and_then(|row| row.checked_add(x.checked_mul(4)?)) else {
            return;
        };
        if let Some(slot) = self.px.get_mut(index..index.saturating_add(4)) {
            slot.copy_from_slice(&c);
        }
    }

    /// Fill the half-open box, clamped to the canvas. Empty boxes draw nothing.
    fn fill(&mut self, x0: i64, y0: i64, x1: i64, y1: i64, c: [u8; 4]) {
        let (x0, x1) = (x0.clamp(0, self.w), x1.clamp(0, self.w));
        let (y0, y1) = (y0.clamp(0, self.h), y1.clamp(0, self.h));
        for y in y0..y1 {
            for x in x0..x1 {
                self.pixel(x, y, c);
            }
        }
    }

    fn paint(&mut self, view: &View) {
        let (w, h) = (self.w, self.h);
        self.fill(0, 0, w, h, BG);

        // Focus, along the very top.
        self.fill(0, 0, w, 3, if view.focused { FOCUS } else { UNFOCUSED });

        // Channel order: red, green, blue, white, left to right.
        let band = (h / 8).max(1);
        let half = w / 2;
        for (i, c) in [RED, GREEN, BLUE, WHITE].into_iter().enumerate() {
            let x0 = i64::try_from(i).unwrap_or(0) * (half / 4);
            let x1 = if i == 3 { half } else { x0 + half / 4 };
            self.fill(x0, 3, x1, band, c);
        }

        // Stride: a checkerboard whose squares must stay square.
        let cell = (w / 120).max(6);
        let mut y = 3;
        while y < band {
            let mut x = half;
            while x < w {
                let odd = (x / cell + y / cell) % 2 == 1;
                self.fill(x, y, (x + cell).min(w), (y + cell).min(band), if odd { CHECKER_A } else { CHECKER_B });
                x += cell;
            }
            y += cell;
        }

        // Sweep: proves frames advance.
        let phase = (view.time_s.rem_euclid(2.0) / 2.0).clamp(0.0, 1.0);
        let sweep = (phase * w as f64) as i64;
        self.fill(sweep, h / 4, sweep + 3, h * 3 / 4, SWEEP);

        // Events received: grows, then resets (64 steps).
        let steps = i64::try_from(view.events % 64).unwrap_or(0);
        self.fill(0, h * 3 / 8, steps * (w / 64), h * 3 / 8 + (h / 16).max(4), EVENT_BAR);

        // One square per paragraph in the open document, and one per reported font folder.
        let side = (h / 32).max(4);
        let row = h / 2;
        for i in 0..view.paragraphs.min(24) {
            let x = i64::try_from(i).unwrap_or(0) * (side + 2);
            self.fill(x, row, x + side, row + side, PARAGRAPH);
        }
        let row2 = row + side + 6;
        for i in 0..view.font_dirs.min(24) {
            let x = i64::try_from(i).unwrap_or(0) * (side + 2);
            self.fill(x, row2, x + side, row2 + side, FONT_DIR);
        }

        // Ten squares of 10 vp: the density in the picture.
        let vp = |n: i64| ((n as f64) * f64::from(view.density)).round() as i64;
        let unit = (vp(10)).max(1);
        let base = h - unit - vp(8).max(1);
        for i in 0..10i64 {
            let x = vp(8) + i * (unit + 2);
            self.fill(x, base, x + unit, base + unit, RULER);
        }

        // Pointer.
        if let Some((px, py)) = view.pointer_px {
            let (x, y) = ((px as i64).clamp(0, w - 1), (py as i64).clamp(0, h - 1));
            let c = if view.pointer_down { POINTER_DOWN } else { POINTER };
            self.fill(x - 8, y, x + 9, y + 1, c);
            self.fill(x, y - 8, x + 1, y + 9, c);
        }

        // Frames drawn, as a small bar under the top band (a second, independent animation).
        let frames_bar = i64::try_from(view.frames % 120).unwrap_or(0);
        self.fill(0, band + 6, frames_bar * (w / 120), band + 6 + (h / 48).max(3), WHITE);
    }
}
