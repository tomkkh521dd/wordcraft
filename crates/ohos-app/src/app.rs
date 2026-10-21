//! The app: surface, input, configuration, the document session and the frame to present.
//!
//! Step 2 of the port draws [`test_pattern`] instead of the document, so a wrong buffer format,
//! stride, density or input path is visible on the device at a glance. From step 3 the same state
//! drives `wordcraft-ui-egui`; nothing here has to change for that.

use std::time::Instant;

use wordcraft_engine::Session;

use crate::config::{Config, Start};
use crate::input::{Button, KeyEvent, PointerKind, Scroll, modifiers};
use crate::test_pattern;

/// Sizes are clamped before any buffer is allocated: a surface reports whatever the compositor
/// says, and a hostile or buggy value must not become a huge allocation.
const MAX_DIM: u32 = 16384;
/// The largest frame buffer the app will allocate (128 MiB: an 8K surface at 2x still fits).
const MAX_BYTES: usize = 128 * 1024 * 1024;
/// Density outside this range is a platform bug; clamping keeps every scale finite.
const MIN_DENSITY: f32 = 0.5;
const MAX_DENSITY: f32 = 8.0;
/// Input coordinates are vp from the platform: finite, and within a sane range of any display.
const MAX_COORD: f32 = 1.0e6;

/// Whether the app drew a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameNeeded {
    No,
    Yes,
}

/// Everything the shell drives.
pub struct App {
    config: Config,
    notes: Vec<String>,
    /// The open document. Step 3 lays this out and paints it; for now its paragraph count is on
    /// the test pattern, which proves the engine runs on the device.
    session: Session,
    started: Instant,
    width: u32,
    height: u32,
    density: f32,
    pixels: Vec<u8>,
    frames: u64,
    events: u64,
    focused: bool,
    pointer: Option<(f32, f32)>,
    pointer_down: bool,
    pointer_button: Button,
    last: String,
    typed: String,
    composing: bool,
    scrolled: (f32, f32),
}

impl App {
    /// Start with what the shell put in the environment.
    pub fn new() -> Self {
        let config = Config::from_env();
        let notes = config.apply();
        for note in &notes {
            log::info!("{note}");
        }
        let doc = match config.start {
            Start::Sample => wordcraft_engine::sample::sample_document(),
            Start::Empty => wordcraft_doc::Document::new(),
        };
        let session = Session::new(doc);
        log::info!("started: {} paragraph(s), {} font folder(s) from the system", session.doc.paragraph_count(), config.font_dirs.len());
        Self {
            config,
            notes,
            session,
            started: Instant::now(),
            width: 0,
            height: 0,
            density: 1.0,
            pixels: Vec::new(),
            frames: 0,
            events: 0,
            focused: true,
            pointer: None,
            pointer_down: false,
            pointer_button: Button::Primary,
            last: "none".into(),
            typed: String::new(),
            composing: false,
            scrolled: (0.0, 0.0),
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// What start-up did, for the shell's log (and the diagnostics screen to come).
    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    pub fn elapsed_s(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn events(&self) -> u64 {
        self.events
    }

    pub fn last_event(&self) -> &str {
        &self.last
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn density(&self) -> f32 {
        self.density
    }

    /// The open document's paragraph count.
    pub fn paragraphs(&self) -> usize {
        self.session.doc.paragraph_count()
    }

    /// How many system font folders the shell reported.
    pub fn font_dirs(&self) -> usize {
        self.config.font_dirs.len()
    }

    pub fn focused(&self) -> bool {
        self.focused
    }

    pub fn pressed(&self) -> bool {
        self.pointer_down
    }

    /// The frame's bytes, RGBA8888, `width * 4` per row. Valid until the next call.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub fn stride(&self) -> usize {
        usize::try_from(self.width).unwrap_or(0).saturating_mul(4)
    }

    /// The surface's size in pixels, and the display's density.
    pub fn resize(&mut self, width: u32, height: u32, density: f32) {
        let width = width.min(MAX_DIM);
        let height = height.min(MAX_DIM);
        let density = if density.is_finite() { density.clamp(MIN_DENSITY, MAX_DENSITY) } else { 1.0 };
        if (width, height) == (self.width, self.height) && (density - self.density).abs() < f32::EPSILON {
            return;
        }
        let bytes = frame_len(width, height);
        if bytes.is_none_or(|b| b > MAX_BYTES) {
            // Too large to be a real surface: stay unsized rather than allocate on a bad value.
            log::error!("refusing a {width}x{height} surface ({} bytes)", bytes.unwrap_or(0));
            self.width = 0;
            self.height = 0;
            self.density = density;
            self.pixels = Vec::new();
            return;
        }
        self.width = width;
        self.height = height;
        self.density = density;
        self.resize_buffer();
        log::info!("surface: {width}x{height} at {density}x");
    }

    /// The surface went away (the page was hidden, or the component was removed): drop the buffer.
    /// The next surface starts empty, as a fresh `onSurfaceCreated` does.
    pub fn surface_gone(&mut self) {
        self.width = 0;
        self.height = 0;
        self.pixels = Vec::new();
        log::info!("surface gone");
    }

    fn resize_buffer(&mut self) {
        match frame_len(self.width, self.height) {
            Some(len) => self.pixels.resize(len, 0),
            None => self.pixels = Vec::new(),
        }
    }

    pub fn focus(&mut self, focused: bool) {
        self.focused = focused;
        self.count(format!("focus {}", if focused { "in" } else { "out" }));
    }

    /// A pointer (touch, pen or mouse) event at `x`, `y` in vp.
    pub fn pointer(&mut self, kind: PointerKind, x: f32, y: f32, button: Button, mods: i32) {
        let (x, y) = (sane(x), sane(y));
        match kind {
            PointerKind::Down => {
                self.pointer = Some((x, y));
                self.pointer_down = true;
                self.pointer_button = button;
            }
            PointerKind::Up => {
                self.pointer = Some((x, y));
                self.pointer_down = false;
            }
            PointerKind::Move => self.pointer = Some((x, y)),
            PointerKind::Leave | PointerKind::Cancel => {
                self.pointer = None;
                self.pointer_down = false;
            }
        }
        let mods = modifiers::describe(mods);
        self.count(format!("pointer {} {} at ({x:.1}, {y:.1}){}{}", kind.as_str(), button.as_str(), if mods.is_empty() { "" } else { " " }, mods));
    }

    pub fn scroll(&mut self, scroll: Scroll) {
        let (dx, dy) = (sane(scroll.dx), sane(scroll.dy));
        self.scrolled = (self.scrolled.0 + dx, self.scrolled.1 + dy);
        self.count(format!("scroll ({dx:.1}, {dy:.1}) total ({:.1}, {:.1})", self.scrolled.0, self.scrolled.1));
    }

    pub fn key(&mut self, event: KeyEvent) {
        let mods = modifiers::describe(event.modifiers);
        self.count(format!(
            "key {} {}{}{}{}",
            event.code,
            if event.pressed { "down" } else { "up" },
            if event.repeated { " (repeat)" } else { "" },
            if mods.is_empty() { "" } else { " " },
            mods
        ));
        if let Some(c) = event.text.filter(|_| event.pressed) {
            self.text(c);
        }
    }

    /// Text from the keyboard or the input method.
    pub fn text(&mut self, c: char) {
        // A long paste is fine; the string is only for the log and the diagnostics screen.
        if self.typed.len() < 4096 {
            self.typed.push(c);
        }
        self.count(format!("text {c:?}"));
    }

    /// The input method started or finished composing (the composed text arrives through
    /// [`Self::text`]).
    pub fn ime_composing(&mut self, composing: bool) {
        self.composing = composing;
        self.count(format!("ime {}", if composing { "composing" } else { "committed" }));
    }

    /// What was typed since start-up (the diagnostics screen shows a line of it).
    pub fn typed(&self) -> &str {
        &self.typed
    }

    pub fn composing(&self) -> bool {
        self.composing
    }

    fn count(&mut self, what: String) {
        self.events = self.events.saturating_add(1);
        if self.events.is_multiple_of(120) {
            log::debug!("{what}");
        }
        self.last = what;
    }

    /// Draw the frame into the pixel buffer. `Yes` when there is something to present.
    pub fn render(&mut self) -> FrameNeeded {
        if self.width == 0 || self.height == 0 {
            return FrameNeeded::No;
        }
        if self.pixels.len() != self.stride().saturating_mul(usize::try_from(self.height).unwrap_or(0)) {
            self.resize_buffer();
        }
        let density = self.density;
        let view = test_pattern::View {
            time_s: self.elapsed_s(),
            density,
            frames: self.frames,
            events: self.events,
            paragraphs: self.session.doc.paragraph_count(),
            font_dirs: self.config.font_dirs.len(),
            pointer_px: self.pointer.map(|(x, y)| (x * density, y * density)),
            pointer_down: self.pointer_down,
            focused: self.focused,
        };
        if !test_pattern::paint(&mut self.pixels, self.width, self.height, &view) {
            return FrameNeeded::No;
        }
        self.frames = self.frames.saturating_add(1);
        FrameNeeded::Yes
    }

    /// A one-line summary for the shell's `stats()` (hilog, ArkTS logs, and a future About box).
    pub fn stats_json(&self) -> String {
        let pointer = match self.pointer {
            Some((x, y)) => serde_json::json!([x, y]),
            None => serde_json::Value::Null,
        };
        serde_json::json!({
            "width": self.width,
            "height": self.height,
            "density": self.density,
            "frames": self.frames,
            "events": self.events,
            "focused": self.focused,
            "pointer": pointer,
            "pressed": self.pointer_down,
            "paragraphs": self.session.doc.paragraph_count(),
            "fontDirs": self.config.font_dirs.len(),
            "theme": self.config.theme.as_str(),
            "locale": self.config.locale,
            "typed": self.typed.chars().rev().take(24).collect::<String>().chars().rev().collect::<String>(),
            "last": self.last,
        })
        .to_string()
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

/// Input coordinates from the platform: finite and bounded.
fn sane(v: f32) -> f32 {
    if v.is_finite() { v.clamp(-MAX_COORD, MAX_COORD) } else { 0.0 }
}

/// The bytes one frame of a surface takes, or `None` when the size can't be one.
fn frame_len(width: u32, height: u32) -> Option<usize> {
    let w = usize::try_from(width).ok()?;
    let h = usize::try_from(height).ok()?;
    w.checked_mul(4)?.checked_mul(h)
}
