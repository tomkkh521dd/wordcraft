//! The app: the real WordCraft UI, on a surface, without a window system.
//!
//! ArkTS reports the surface size, the display density, input and focus; C++ hands them over the C
//! ABI; here they become [`egui::RawInput`]. `wordcraft-ui-egui` draws into an [`egui::Context`]
//! exactly as it does on the desktop, and [`wordcraft_ui_paint::Painter`] rasterises the result
//! into the RGBA frame C++ copies into the window buffer.

use std::time::Instant;

use egui::{Event, MouseWheelUnit, Pos2, RawInput, Rect, TouchPhase, vec2};
use wordcraft_doc::Pos;
use wordcraft_engine::{Selection, Session};
use wordcraft_ui_egui::WordApp;
use wordcraft_ui_paint::{Painter, Target};

use crate::config::{Config, Start, Theme};
use crate::input::{self, Button, KeyEvent, PointerKind, Scroll, modifiers};

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
/// What egui allows for a texture (the font atlas grows well past 2048 on a big UI).
const MAX_TEXTURE_SIDE: usize = 8192;

/// Whether the app drew a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameNeeded {
    No,
    Yes,
}

/// Where the last frame's time went, for the host renderer and the device's log.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameTimings {
    /// The UI pass: layout, painting into egui's shapes, and the app's own logic.
    pub ui_ms: f32,
    /// Turning shapes into meshes.
    pub tessellate_ms: f32,
    /// Rasterising the meshes into the frame.
    pub paint_ms: f32,
}

/// Everything the shell drives.
pub struct App {
    config: Config,
    notes: Vec<String>,
    ctx: egui::Context,
    ui: WordApp,
    painter: Painter,
    /// Input collected between frames, handed to egui with the next one.
    raw: RawInput,
    started: Instant,
    width: u32,
    height: u32,
    density: f32,
    pixels: Vec<u8>,
    frames: u64,
    events: u64,
    focused: bool,
    pointer: Option<Pos2>,
    pointer_down: bool,
    modifiers_state: egui::Modifiers,
    last: String,
    typed: String,
    composing: bool,
    scrolled: (f32, f32),
    timings: FrameTimings,
    /// What the previous frame's meshes looked like, to find the part that changed: a signature,
    /// the rectangle it covers, and the texture it samples (a re-uploaded texture changes what a
    /// mesh draws without changing the mesh).
    previous: Vec<(u64, egui::Rect, egui::TextureId)>,
    /// The part of the frame that changed since the previous one, in points (`None`: all of it).
    damage: Option<egui::Rect>,
    /// Where the text caret is, in points, refreshed after every UI pass: the platform's input
    /// method puts its candidate window here (and its absence means no text is being typed).
    caret: Option<egui::Pos2>,
    /// The height of the caret's line, in points.
    caret_height: f32,
    /// Where the shell's drag handle belongs, in points: the selection's focus end inside its
    /// line (the caret of a focus at the end of a wrapped line is on the next line, and the
    /// handle would follow it there), or the caret when the selection is collapsed.
    handle: Option<egui::Pos2>,
    /// The height of the handle's line, in points.
    handle_height: f32,
    /// A fling still in progress (points per second, positive = scrolling down and right), and the
    /// time it was last advanced.
    fling: (f32, f32),
    /// The selection the last painted frame was drawn with (see `render`).
    painted_selection: Selection,
    /// A document the platform asked to open at launch (a "open with" URI, or the document that
    /// was open last time), still on its way.
    startup: Option<std::sync::mpsc::Receiver<crate::host::Reply>>,
    last_frame: Option<Instant>,
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
        let ui = WordApp::new(Session::new(doc), crate::services::services());
        log::info!("started: {} paragraph(s), {} font folder(s) from the system", ui.session.doc.paragraph_count(), config.font_dirs.len());
        let mut app = Self {
            ctx: egui::Context::default(),
            ui,
            painter: Painter::new(),
            raw: RawInput::default(),
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
            modifiers_state: egui::Modifiers::default(),
            last: "none".into(),
            typed: String::new(),
            composing: false,
            scrolled: (0.0, 0.0),
            timings: FrameTimings::default(),
            previous: Vec::new(),
            damage: None,
            caret: None,
            caret_height: 0.0,
            handle: None,
            handle_height: 0.0,
            fling: (0.0, 0.0),
            painted_selection: Selection::caret(Pos::body(0, 0)),
            startup: None,
            last_frame: None,
            config,
            notes,
        };
        app.raw.max_texture_side = Some(MAX_TEXTURE_SIDE);
        app
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

    /// Whether something the app itself draws is covering the document (the context menu): the
    /// shell's Back gesture should close it rather than leave the app.
    pub fn overlay_open(&self) -> bool {
        self.ui.canvas.context_menu_open
    }

    /// The page view's zoom (1.0 = 100%), for tests and for the shell's logs.
    pub fn zoom(&self) -> f32 {
        self.ui.session.view.zoom
    }

    /// The document's file, as a path the platform can remember (the sandbox working copy).
    pub fn document_path(&self) -> Option<String> {
        self.ui.session.path.as_ref().map(|path| path.to_string_lossy().into_owned())
    }

    /// Whether the selection covers a range (a handle is drawn only when it does).
    pub fn selection_is_range(&self) -> bool {
        !self.ui.session.sel.is_collapsed()
    }

    /// The selection, for the crate's tests.
    #[cfg(test)]
    pub(crate) fn selection(&self) -> &Selection {
        &self.ui.session.sel
    }

    /// Where the text caret is in points (x, y) and how tall its line is, for the platform's input
    /// method; `None` when no text is being edited.
    pub fn caret(&self) -> Option<(f32, f32, f32)> {
        self.caret.map(|pos| (pos.x, pos.y, self.caret_height))
    }

    /// Where the shell's drag handle belongs in points (x, y) and how tall its line is; `None`
    /// when no text is being edited. Unlike the caret this sits at the selection's focus end
    /// *inside its line*, so a word that ends a wrapped line keeps its handle on that line.
    pub fn handle(&self) -> Option<(f32, f32, f32)> {
        self.handle.map(|pos| (pos.x, pos.y, self.handle_height))
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
        self.ui.session.doc.paragraph_count()
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

    /// What was typed since start-up.
    pub fn typed(&self) -> &str {
        &self.typed
    }

    pub fn composing(&self) -> bool {
        self.composing
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
        // The buffer now holds something else (a new allocation, or the old one at another size):
        // nothing of the previous frame can be reused, so the next frame must paint all of it.
        self.invalidate_frame();
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

    /// Forget what the previous frame looked like, so the next paint covers the whole window.
    fn invalidate_frame(&mut self) {
        self.previous.clear();
        self.damage = None;
    }

    fn resize_buffer(&mut self) {
        // A fresh allocation, not a `resize` of the old one: the bytes of a buffer of another size
        // are not a frame, and a partial repaint over them showed up as black areas with a few
        // white patches on the tablet.
        self.pixels = match frame_len(self.width, self.height) {
            Some(len) => vec![0; len],
            None => Vec::new(),
        };
        self.invalidate_frame();
    }

    pub fn focus(&mut self, focused: bool) {
        self.focused = focused;
        self.count(format!("focus {}", if focused { "in" } else { "out" }));
    }

    /// egui's `i.modifiers` — what the canvas reads to tell a Shift press from a plain one — only
    /// follows `Event::ModifiersChanged`, never the modifiers an input event carries (a desktop
    /// OS sends the change its own event too). Report every change the shell sees, or the touch
    /// handle's Shift press collapses the caret instead of extending the selection.
    fn update_modifiers(&mut self, modifiers: egui::Modifiers) {
        if modifiers != self.modifiers_state {
            self.raw.events.push(Event::ModifiersChanged(modifiers));
            self.modifiers_state = modifiers;
        }
    }

    /// A pointer (touch, pen or mouse) event at `x`, `y` in vp.
    pub fn pointer(&mut self, kind: PointerKind, x: f32, y: f32, button: Button, mods: i32) {
        let pos = Pos2::new(sane(x), sane(y));
        self.update_modifiers(modifiers::egui(mods));
        if kind == PointerKind::Down {
            // Touching the page stops a fling, the way it stops a scroll on paper.
            self.fling = (0.0, 0.0);
        }
        match kind {
            PointerKind::Down => {
                self.pointer = Some(pos);
                self.pointer_down = true;
                // A press moves the pointer first, so hover-based widgets (the ribbon) see it.
                self.raw.events.push(Event::PointerMoved(pos));
                self.raw.events.push(Event::PointerButton { pos, button: button.egui(), pressed: true, modifiers: self.modifiers_state });
            }
            PointerKind::Up => {
                self.pointer = Some(pos);
                self.pointer_down = false;
                self.raw.events.push(Event::PointerButton { pos, button: button.egui(), pressed: false, modifiers: self.modifiers_state });
            }
            PointerKind::Move => {
                self.pointer = Some(pos);
                self.raw.events.push(Event::PointerMoved(pos));
            }
            PointerKind::Leave | PointerKind::Cancel => {
                self.pointer = None;
                self.pointer_down = false;
                self.raw.events.push(Event::PointerGone);
            }
        }
        let mods = modifiers::describe(mods);
        self.count(format!(
            "pointer {} {} at ({:.1}, {:.1}){}{}",
            kind.as_str(),
            button.as_str(),
            pos.x,
            pos.y,
            if mods.is_empty() { "" } else { " " },
            mods
        ));
    }

    /// Scale the page view by `factor` (a pinch: 1.0 changes nothing), clamped to the zoom range the
    /// View tab offers. The caret, the selection and everything else stay where they are; only the
    /// drawing scale changes.
    pub fn zoom_by(&mut self, factor: f32) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        // `view.zoom` takes a percentage and clamps to 10–500%.
        let current = self.ui.session.view.zoom;
        let target = (current * factor).clamp(0.1, 5.0);
        if (target - current).abs() < 1.0e-4 {
            return;
        }
        let _ = self.ui.run("view.zoom", serde_json::json!({"value": target * 100.0}));
        self.count(format!("zoom {:.0}%", target * 100.0));
    }

    /// Start a fling from a gesture's release velocity (points per second, positive = scrolling
    /// down and right). It decays every frame until it stops or the page is touched.
    pub fn fling(&mut self, vx: f32, vy: f32) {
        let clamp = |v: f32| if v.is_finite() { v.clamp(-8000.0, 8000.0) } else { 0.0 };
        let (vx, vy) = (clamp(vx), clamp(vy));
        // A hand that let go while barely moving is not a fling.
        self.fling = if vx.abs() + vy.abs() < 200.0 { (0.0, 0.0) } else { (vx, vy) };
        self.last_frame = None;
    }

    /// Whether a fling is running (the shell keeps drawing frames while it is).
    pub fn flinging(&self) -> bool {
        self.fling.0 != 0.0 || self.fling.1 != 0.0
    }

    /// Advance a running fling by the time since the last frame: one wheel event per frame, with
    /// the velocity decaying, so the page comes to rest like a finger's throw.
    fn advance_fling(&mut self) {
        if !self.flinging() {
            return;
        }
        let now = Instant::now();
        let dt = self.last_frame.map_or(1.0 / 60.0, |last| now.duration_since(last).as_secs_f32().clamp(0.0, 0.05));
        self.last_frame = Some(now);
        let (vx, vy) = self.fling;
        let (dx, dy) = (vx * dt, vy * dt);
        if dx.abs() + dy.abs() >= 0.01 {
            self.scrolled = (self.scrolled.0 + dx, self.scrolled.1 + dy);
            self.raw.events.push(Event::MouseWheel {
                unit: MouseWheelUnit::Point,
                delta: vec2(dx, dy),
                phase: TouchPhase::Move,
                modifiers: self.modifiers_state,
            });
        }
        // Exponential decay, the way a scroller settles (about a second and a half from a fast throw).
        let decay = 1000.0_f32.powf(-dt / 0.25);
        self.fling = (vx * decay, vy * decay);
        if self.fling.0.abs() + self.fling.1.abs() < 30.0 {
            self.fling = (0.0, 0.0);
        }
    }

    /// A scroll gesture in vp, positive down and right (ArkUI's axis sign; egui's wheel is the
    /// same, positive down).
    pub fn scroll(&mut self, scroll: Scroll) {
        let (dx, dy) = (sane(scroll.dx), sane(scroll.dy));
        self.scrolled = (self.scrolled.0 + dx, self.scrolled.1 + dy);
        self.raw.events.push(Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: vec2(dx, dy),
            phase: TouchPhase::Move,
            modifiers: self.modifiers_state,
        });
        self.count(format!("scroll ({dx:.1}, {dy:.1})"));
    }

    /// A hardware key. The character it produced arrives as a [`Self::text`] call from the shell.
    pub fn key(&mut self, event: KeyEvent) {
        self.update_modifiers(modifiers::egui(event.modifiers));
        if let Some(key) = input::egui_key(event.code) {
            self.raw.events.push(Event::Key {
                key,
                physical_key: None,
                pressed: event.pressed,
                repeat: event.repeated,
                modifiers: self.modifiers_state,
            });
        }
        let mods = modifiers::describe(event.modifiers);
        self.count(format!(
            "key {} {}{}{}{}",
            event.code,
            if event.pressed { "down" } else { "up" },
            if event.repeated { " (repeat)" } else { "" },
            if mods.is_empty() { "" } else { " " },
            mods
        ));
        // Ctrl, Alt and Meta make a shortcut, never text. The character comes from the key code: the
        // platform's `keyText` is not the key's character (HarmonyOS's soft keyboard sends every
        // letter as `K`, and Ctrl+S does the same), so trusting it typed "K" for every key.
        let shortcut = event.modifiers & (modifiers::CTRL | modifiers::META | modifiers::ALT) != 0;
        if event.pressed
            && !shortcut
            && let Some(c) = input::character_for(event.code, event.modifiers)
        {
            self.text(c);
        }
    }

    /// Text from the keyboard or the input method.
    pub fn text(&mut self, c: char) {
        if self.typed.len() < 4096 {
            self.typed.push(c);
        }
        self.raw.events.push(Event::Text(c.to_string()));
        self.count(format!("text {c:?}"));
    }

    /// The input method started or finished composing. The committed text arrives through
    /// [`Self::text`]; the preedit (candidate) text has no channel yet, so nothing is drawn for it
    /// until the IME bridge is built.
    pub fn ime_composing(&mut self, composing: bool) {
        self.composing = composing;
        self.count(format!("ime {}", if composing { "composing" } else { "committed" }));
    }

    /// The input method's composing text (pinyin before it becomes characters). The UI draws it
    /// at the caret, underlined, until [`Self::ime_commit`] replaces it with the characters.
    pub fn ime_preedit(&mut self, text: &str) {
        self.raw.events.push(Event::Ime(egui::ImeEvent::Preedit { text: text.to_string(), active_range_chars: None }));
        self.count(format!("ime preedit {text:?}"));
    }

    /// The input method's committed text: one undoable insert, however many characters.
    pub fn ime_commit(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.raw.events.push(Event::Ime(egui::ImeEvent::Commit(text.to_string())));
        self.count(format!("ime commit {text:?}"));
    }

    fn count(&mut self, what: String) {
        self.events = self.events.saturating_add(1);
        if self.events <= 40 || self.events.is_multiple_of(120) {
            log::debug!("{what}");
        }
        self.last = what;
    }

    /// Ask the platform for the document to open at launch (an "open with" URI from another app,
    /// or the document that was open when the app was last closed). The shell calls this once its
    /// bridge is ready, so nothing is queued before there is someone to answer.
    pub fn ask_startup_document(&mut self) {
        if self.startup.is_none() {
            self.startup = Some(crate::services::startup_document());
        }
    }

    /// Open the document the platform offered at launch, once it arrives.
    fn take_startup_document(&mut self) {
        let Some(receiver) = self.startup.as_ref() else { return };
        let reply = match receiver.try_recv() {
            Ok(reply) => reply,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.startup = None;
                return;
            }
        };
        self.startup = None;
        let Some(path) = reply.string("path").filter(|path| !path.is_empty()) else {
            return;
        };
        log::info!("opening {} at launch", path);
        let _ = self.ui.run("file.open", serde_json::json!({"path": path}));
    }

    /// Run one UI pass and paint it. `Yes` when there is something to present.
    pub fn render(&mut self) -> FrameNeeded {
        if self.width == 0 || self.height == 0 {
            return FrameNeeded::No;
        }
        let expected = self.stride().saturating_mul(usize::try_from(self.height).unwrap_or(0));
        if self.pixels.len() != expected {
            self.resize_buffer();
            if self.pixels.len() != expected {
                return FrameNeeded::No;
            }
        }

        self.take_startup_document();
        self.advance_fling();
        // A changed selection (a double click's word, a drag) repaints in full: the highlight spans
        // several meshes and a partial repaint left it patchy for a frame on the tablet.
        let selection = self.ui.session.sel.clone();
        if selection != self.painted_selection {
            self.painted_selection = selection;
            self.invalidate_frame();
        }
        let raw = self.take_raw_input();
        let had_input = !raw.events.is_empty();
        let ctx = self.ctx.clone();
        let ui = &mut self.ui;
        let started = Instant::now();
        let output = ctx.run_ui(raw, |root| {
            ui.logic(root.ctx());
            ui.ui(root);
        });
        let ui_ms = started.elapsed().as_secs_f32() * 1000.0;
        // Where the input method's candidate window belongs: the caret, when the document has one.
        let caret = wordcraft_ui_egui::canvas::caret_screen(&mut self.ui);
        self.caret = caret.map(|(pos, _)| pos);
        self.caret_height = caret.map_or(0.0, |(_, height)| height);
        // Where the shell's drag handle belongs: the selection's end inside its line.
        let handle = wordcraft_ui_egui::canvas::handle_screen(&mut self.ui);
        self.handle = handle.map(|(pos, _)| pos);
        self.handle_height = handle.map_or(0.0, |(_, height)| height);

        // Textures must land before the meshes that sample them, and the delta must be emptied
        // once applied (egui's Drop asserts on leftovers). Which textures were re-uploaded is
        // kept: a page the UI re-rendered in place (same `TextureId`) is invisible to the mesh
        // diff below, so those meshes are damaged by hand.
        let mut textures = output.textures_delta;
        let uploaded: Vec<egui::TextureId> = textures.set.keys().copied().collect();
        self.painter.apply_textures(&textures);
        textures.clear();

        // Nothing moved and egui asks for no repaint: the previous frame is still on screen, so
        // there is nothing to rasterise. (The UI pass above is ~1 ms; a painted frame is the
        // expensive one.)
        // A delay of "never" or "later" means egui is happy with what is on screen; only an
        // immediate repaint (an animation, a caret blink landing) needs a new frame now.
        // A texture uploaded this frame has to be rasterised now: the meshes sampling it may be
        // identical to the previous frame's, so this is the only frame that would ever show it.
        let wants_paint =
            had_input || !uploaded.is_empty() || output.viewport_output.get(&egui::ViewportId::ROOT).is_none_or(|v| v.repaint_delay.is_zero());
        if self.frames > 0 && !wants_paint {
            self.timings = FrameTimings { ui_ms, tessellate_ms: 0.0, paint_ms: 0.0 };
            return FrameNeeded::No;
        }

        let pixels_per_point =
            if output.pixels_per_point.is_finite() && output.pixels_per_point > 0.0 { output.pixels_per_point } else { self.density };
        let tessellating = Instant::now();
        let primitives = ctx.tessellate(output.shapes, pixels_per_point);
        let tessellate_ms = tessellating.elapsed().as_secs_f32() * 1000.0;
        let damage = self.take_damage(&primitives, &uploaded);
        if damage == Some(egui::Rect::NOTHING) {
            // Every mesh is identical to the previous frame's: the screen is already right.
            self.timings = FrameTimings { ui_ms, tessellate_ms, paint_ms: 0.0 };
            return FrameNeeded::No;
        }
        let painting = Instant::now();
        self.painter.paint_damaged(
            Target { pixels: &mut self.pixels, width: self.width, height: self.height },
            pixels_per_point,
            &primitives,
            damage,
        );
        let paint_ms = painting.elapsed().as_secs_f32() * 1000.0;
        self.timings = FrameTimings { ui_ms, tessellate_ms, paint_ms };
        self.frames = self.frames.saturating_add(1);
        if self.frames.is_multiple_of(300) {
            let stats = self.painter.stats();
            log::debug!(
                "{} frames, {} triangles, {}/{} px blitted, {} textures, ui {:.1} ms, tessellate {:.1} ms, paint {:.1} ms",
                self.frames,
                stats.triangles,
                stats.blitted,
                stats.pixels,
                self.painter.texture_count(),
                self.timings.ui_ms,
                self.timings.tessellate_ms,
                self.timings.paint_ms
            );
        }
        FrameNeeded::Yes
    }

    /// The input for this frame: what the shell reported since the last one, plus the state egui
    /// keeps per frame.
    fn take_raw_input(&mut self) -> RawInput {
        let ppp = self.density;
        let mut raw = std::mem::take(&mut self.raw);
        raw.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, vec2(self.width as f32 / ppp, self.height as f32 / ppp)));
        raw.time = Some(self.elapsed_s());
        raw.focused = self.focused;
        raw.max_texture_side = Some(MAX_TEXTURE_SIDE);
        raw.system_theme = match self.config.theme {
            Theme::Dark => Some(egui::Theme::Dark),
            Theme::Light => Some(egui::Theme::Light),
            Theme::Auto => raw.system_theme,
        };
        // egui reads the density and the window state from the viewport, not the raw input.
        let info = raw.viewports.entry(egui::ViewportId::ROOT).or_default();
        info.native_pixels_per_point = Some(ppp);
        info.focused = Some(self.focused);
        self.ui.raw_input_hook(&mut raw);
        // The next frame's scaffolding; `events` was taken above and stays empty until input comes.
        self.raw.max_texture_side = Some(MAX_TEXTURE_SIDE);
        self.raw.viewports.entry(egui::ViewportId::ROOT).or_default();
        raw
    }

    /// A one-line summary for the shell's `stats()` (hilog, ArkTS logs, and a future About box).
    pub fn stats_json(&self) -> String {
        let pointer = match self.pointer {
            Some(p) => serde_json::json!([p.x, p.y]),
            None => serde_json::Value::Null,
        };
        let stats = self.painter.stats();
        serde_json::json!({
            "width": self.width,
            "height": self.height,
            "density": self.density,
            "frames": self.frames,
            "events": self.events,
            "focused": self.focused,
            "pointer": pointer,
            "pressed": self.pointer_down,
            "paragraphs": self.paragraphs(),
            "fontDirs": self.config.font_dirs.len(),
            "theme": self.config.theme.as_str(),
            "locale": self.config.locale,
            "typed": self.typed.chars().rev().take(24).collect::<String>().chars().rev().collect::<String>(),
            "last": self.last,
            "triangles": stats.triangles,
            "pixels": stats.pixels,
            "blitted": stats.blitted,
            "textures": self.painter.texture_count(),
        })
        .to_string()
    }

    /// The events waiting for the next frame (tests look at them; the shell never does).
    #[doc(hidden)]
    pub fn pending_events(&self) -> &[Event] {
        &self.raw.events
    }

    /// Which part of the frame changed: a full repaint unless every mesh can be matched against
    /// the previous frame and the changes are small.
    ///
    /// egui hands over the whole scene each pass, so the diff is per mesh: same count, and a
    /// signature (geometry, uv, colour, texture) that mostly matches. A moved or edited mesh
    /// damages both its old and its new rectangle, so nothing stale is left behind.
    ///
    /// A mesh whose texture was re-uploaded this frame counts as changed even when the mesh
    /// itself is identical: the UI re-renders a page in place (a `TextureHandle::set` with the
    /// same `TextureId`), and without this the new page pixels would never be rasterised.
    fn take_damage(&mut self, primitives: &[egui::ClippedPrimitive], uploaded: &[egui::TextureId]) -> Option<egui::Rect> {
        use std::hash::{Hash, Hasher};
        let mut current: Vec<(u64, egui::Rect, egui::TextureId)> = Vec::with_capacity(primitives.len());
        for clipped in primitives {
            let egui::epaint::Primitive::Mesh(mesh) = &clipped.primitive else { continue };
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            mesh.texture_id.hash(&mut hasher);
            mesh.indices.hash(&mut hasher);
            for vertex in &mesh.vertices {
                vertex.pos.x.to_bits().hash(&mut hasher);
                vertex.pos.y.to_bits().hash(&mut hasher);
                vertex.uv.x.to_bits().hash(&mut hasher);
                vertex.uv.y.to_bits().hash(&mut hasher);
                vertex.color.to_array().hash(&mut hasher);
            }
            let mut rect = clipped.clip_rect;
            for vertex in &mesh.vertices {
                rect = rect.union(egui::Rect::from_min_max(vertex.pos, vertex.pos));
            }
            current.push((hasher.finish(), rect, mesh.texture_id));
        }

        let frame = egui::Rect::from_min_size(Pos2::ZERO, vec2(self.width as f32 / self.density, self.height as f32 / self.density));
        let damage = if self.previous.len() == current.len() && !current.is_empty() {
            let mut changed: Option<egui::Rect> = None;
            for (index, (signature, rect, texture)) in current.iter().enumerate() {
                let (old_signature, old_rect, _) = self.previous[index];
                let texture_changed = !uploaded.is_empty() && uploaded.contains(texture);
                if *signature != old_signature || texture_changed {
                    let both = rect.union(old_rect).expand(1.0);
                    changed = Some(changed.map_or(both, |acc| acc.union(both)));
                }
            }
            match changed {
                // A tiny change costs less to repaint exactly; a huge one is cheaper as a frame.
                Some(rect) if rect.area() < frame.area() * 0.6 && rect.intersects(frame) => Some(rect.intersect(frame)),
                Some(_) => None,
                None => Some(egui::Rect::NOTHING),
            }
        } else {
            None
        };
        self.previous = current;
        self.damage = damage;
        damage
    }

    /// The damage rectangle in pixels, `(x, y, width, height)`; the whole frame when everything
    /// changed (or nothing did, in which case nothing is presented).
    pub fn damage_px(&self) -> (u32, u32, u32, u32) {
        let ppp = self.density;
        match self.damage {
            Some(rect) if !rect.is_negative() && rect.width() > 0.0 && rect.height() > 0.0 => {
                let x = (rect.min.x * ppp).floor().max(0.0) as u32;
                let y = (rect.min.y * ppp).floor().max(0.0) as u32;
                let right = (rect.max.x * ppp).ceil().clamp(0.0, self.width as f32) as u32;
                let bottom = (rect.max.y * ppp).ceil().clamp(0.0, self.height as f32) as u32;
                (x.min(self.width), y.min(self.height), right.saturating_sub(x), bottom.saturating_sub(y))
            }
            Some(_) => (0, 0, 0, 0),
            None => (0, 0, self.width, self.height),
        }
    }

    /// Where the last frame's time went.
    pub fn timings(&self) -> FrameTimings {
        self.timings
    }

    /// Textures the painter is holding (the font atlas, the page tiles), for tests and logs.
    pub fn painter_textures(&self) -> usize {
        self.painter.texture_count()
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
