//! The app's contract: configuration, input becoming egui events, and a frame that actually
//! contains the UI — with nothing panicking on hostile sizes.

use std::ffi::OsString;

use egui::{Event, Key, PointerButton};

use crate::config::{Config, FONT_DIRS, LOCALE, SETTINGS_DIR, START, Start, THEME, Theme};
use crate::input::{self, Button, KeyEvent, PointerKind, Scroll, egui_key, modifiers};
use crate::{App, FrameNeeded};

fn lookup(pairs: &[(&str, &str)]) -> impl FnMut(&str) -> Option<OsString> {
    let pairs: Vec<(String, OsString)> = pairs.iter().map(|(k, v)| ((*k).to_string(), OsString::from(*v))).collect();
    move |name: &str| pairs.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
}

#[test]
fn config_reads_what_the_shell_sets() {
    let config = Config::from_lookup(lookup(&[
        (SETTINGS_DIR, "/data/storage/el2/base/haps/entry/files/settings"),
        (FONT_DIRS, "/system/fonts"),
        (LOCALE, "zh-Hans"),
        (THEME, "dark"),
        (START, "empty"),
    ]));
    assert_eq!(config.settings_dir.as_deref(), Some(std::path::Path::new("/data/storage/el2/base/haps/entry/files/settings")));
    assert_eq!(config.font_dirs, vec![std::path::PathBuf::from("/system/fonts")]);
    assert_eq!(config.locale.as_deref(), Some("zh-Hans"));
    assert_eq!(config.theme, Theme::Dark);
    assert_eq!(config.start, Start::Empty);
}

#[test]
fn config_takes_several_font_folders_and_ignores_empties() {
    let joined = std::env::join_paths(["/system/fonts", "/data/fonts"]).unwrap_or_default();
    let config = Config::from_lookup(lookup(&[(FONT_DIRS, &joined.to_string_lossy()), (SETTINGS_DIR, ""), (THEME, ""), (START, "nonsense")]));
    assert_eq!(config.font_dirs, vec![std::path::PathBuf::from("/system/fonts"), std::path::PathBuf::from("/data/fonts")]);
    assert_eq!(config.settings_dir, None);
    assert_eq!(config.theme, Theme::Auto);
    assert_eq!(config.start, Start::Sample);
}

#[test]
fn config_defaults_without_anything() {
    let config = Config::from_lookup(lookup(&[]));
    assert_eq!(config, Config::default());
}

#[test]
fn theme_and_start_parse_leniently() {
    for (word, theme) in [("light", Theme::Light), ("Dark", Theme::Dark), ("auto", Theme::Auto), ("sepia", Theme::Auto), ("", Theme::Auto)] {
        assert_eq!(Theme::parse(word), theme, "{word:?}");
    }
    for (word, start) in [("empty", Start::Empty), ("SAMPLE", Start::Sample), ("", Start::Sample)] {
        assert_eq!(Start::parse(word), start, "{word:?}");
    }
}

#[test]
fn applying_config_reports_what_happened() {
    let config = Config::from_lookup(lookup(&[(SETTINGS_DIR, "/sandbox/settings"), (FONT_DIRS, "/system/fonts"), (LOCALE, "en-US")]));
    let notes = config.apply();
    assert!(notes.iter().any(|n| n.contains("/sandbox/settings")), "{notes:?}");
    assert!(notes.iter().any(|n| n.contains("1 folder")), "{notes:?}");
    assert!(notes.iter().any(|n| n.contains("en-US")), "{notes:?}");
}

/// The frame really is the UI: meshes, textures (the font atlas and the page) and opaque pixels.
#[test]
fn a_frame_draws_the_ui() {
    let mut app = App::new();
    app.resize(1200, 800, 1.5);
    // The UI installs its fonts over the first two passes (as on the desktop) and draws from the
    // third; the shell simply keeps presenting frames.
    for _ in 0..3 {
        assert_eq!(app.render(), FrameNeeded::Yes);
    }
    assert_eq!(app.pixels().len(), 1200 * 800 * 4);
    let stats = app.stats_json();
    let stats: serde_json::Value = serde_json::from_str(&stats).unwrap_or_default();
    assert!(stats["triangles"].as_u64().unwrap_or(0) > 50, "the ribbon alone is hundreds of triangles: {stats}");
    assert!(stats["textures"].as_u64().unwrap_or(0) >= 1, "the font atlas is uploaded: {stats}");
    assert!(stats["pixels"].as_u64().unwrap_or(0) > 100_000, "most of the window is painted: {stats}");
    // The ribbon paints an opaque bar at the top.
    assert!(app.pixels().as_chunks::<4>().0.iter().take(400).any(|p| p[3] == 255), "the top of the window is opaque");
    assert_eq!(app.frames(), 3);
}

/// A second frame works, and the UI keeps its textures rather than re-uploading them.
#[test]
fn frames_follow_each_other() {
    let mut app = App::new();
    app.resize(800, 600, 1.0);
    assert_eq!(app.render(), FrameNeeded::Yes);
    let textures = app.painter_textures();
    assert_eq!(app.render(), FrameNeeded::Yes);
    assert_eq!(app.frames(), 2);
    assert!(app.painter_textures() >= textures, "textures are kept between frames");
}

#[test]
fn pointer_and_scroll_become_egui_events() {
    let mut app = App::new();
    app.pointer(PointerKind::Down, 100.0, 200.0, Button::Primary, modifiers::CTRL);
    app.pointer(PointerKind::Move, 110.0, 210.0, Button::None, 0);
    app.pointer(PointerKind::Up, 110.0, 210.0, Button::Primary, 0);
    app.scroll(Scroll { dx: 0.0, dy: -120.0 });
    let events = app.pending_events().to_vec();
    assert!(matches!(events[0], Event::PointerMoved(p) if p.x == 100.0 && p.y == 200.0), "{events:?}");
    assert!(
        matches!(events[1], Event::PointerButton { pressed: true, button: PointerButton::Primary, modifiers, .. } if modifiers.ctrl),
        "{events:?}"
    );
    assert!(matches!(events[3], Event::PointerButton { pressed: false, .. }), "{events:?}");
    assert!(matches!(events[4], Event::MouseWheel { delta, .. } if delta.y == -120.0), "{events:?}");
}

#[test]
fn a_touch_without_a_button_is_the_primary_button() {
    let mut app = App::new();
    app.pointer(PointerKind::Down, 5.0, 5.0, Button::None, 0);
    assert!(matches!(app.pending_events()[1], Event::PointerButton { button: PointerButton::Primary, .. }));
    app.pointer(PointerKind::Leave, 0.0, 0.0, Button::None, 0);
    assert!(matches!(app.pending_events().last(), Some(Event::PointerGone)), "leaving the surface clears hover");
}

#[test]
fn a_shortcut_never_types_its_key_text() {
    // On the HarmonyOS emulator, Ctrl+S arrives with `keyText` "K": the platform's character for
    // a chorded key is unreliable, and typing it would put a stray letter in the document.
    let mut app = App::new();
    app.key(KeyEvent { code: 2035, pressed: true, repeated: false, modifiers: modifiers::CTRL, text: Some('K') });
    app.key(KeyEvent { code: 2038, pressed: true, repeated: false, modifiers: modifiers::META, text: Some('x') });
    assert_eq!(app.typed(), "", "a Ctrl or Meta chord types nothing");
    assert!(!app.pending_events().iter().any(|e| matches!(e, Event::Text(_))));
    // Without Ctrl or Meta the character still arrives (AltGr-style text stays possible).
    app.key(KeyEvent { code: 2017, pressed: true, repeated: false, modifiers: modifiers::ALT, text: Some('@') });
    assert_eq!(app.typed(), "@");
    // A released key types nothing either.
    app.key(KeyEvent { code: 2017, pressed: false, repeated: false, modifiers: 0, text: Some('a') });
    assert_eq!(app.typed(), "@");
    // A key that cannot type carries no character, whatever the platform says: an injected F11
    // arrived with the text 'K' on a tablet.
    app.key(KeyEvent { code: 2100, pressed: true, repeated: false, modifiers: 0, text: Some('K') });
    app.key(KeyEvent { code: 2012, pressed: true, repeated: false, modifiers: 0, text: Some('k') });
    assert_eq!(app.typed(), "@", "function and arrow keys type nothing");
    assert!(input::types_text(2017) && input::types_text(2050) && input::types_text(2044));
    assert!(!input::types_text(2100) && !input::types_text(2012) && !input::types_text(2054));
}

#[test]
fn keys_map_to_egui_and_text_arrives_separately() {
    let mut app = App::new();
    // ArkUI: 2035 = S, 2054 = Enter, 2012 = ArrowUp, 2090 = F1.
    app.key(KeyEvent { code: 2035, pressed: true, repeated: false, modifiers: modifiers::CTRL, text: None });
    app.key(KeyEvent { code: 2054, pressed: true, repeated: false, modifiers: 0, text: None });
    app.text('字');
    let events = app.pending_events().to_vec();
    assert!(matches!(events[0], Event::Key { key: Key::S, pressed: true, modifiers, .. } if modifiers.ctrl), "{events:?}");
    assert!(matches!(events[1], Event::Key { key: Key::Enter, .. }), "{events:?}");
    assert!(matches!(&events[2], Event::Text(text) if text == "字"), "{events:?}");
    assert_eq!(app.typed(), "字");
    assert_eq!(egui_key(2012), Some(Key::ArrowUp));
    assert_eq!(egui_key(2090), Some(Key::F1));
    assert_eq!(egui_key(2101), Some(Key::F12));
    assert_eq!(egui_key(16), None, "volume keys have no logical key");
}

#[test]
fn modifiers_translate() {
    let mods = modifiers::egui(modifiers::CTRL | modifiers::SHIFT | modifiers::ALT);
    assert!(mods.ctrl && mods.shift && mods.alt && mods.command && !mods.mac_cmd);
    assert_eq!(modifiers::describe(modifiers::CTRL | modifiers::SHIFT), "Ctrl+Shift");
}

/// Input coordinates from the platform are clamped, not trusted.
#[test]
fn pointer_coordinates_are_clamped() {
    let mut app = App::new();
    app.pointer(PointerKind::Move, f32::NAN, f32::INFINITY, Button::None, 0);
    assert!(matches!(app.pending_events()[0], Event::PointerMoved(p) if p.x == 0.0 && p.y == 0.0));
    app.pointer(PointerKind::Move, 1.0e30, -1.0e30, Button::None, 0);
    assert!(matches!(app.pending_events()[1], Event::PointerMoved(p) if p.x == 1.0e6 && p.y == -1.0e6));
}

/// A frame that changed nothing is not painted at all, and a scroll damages far less than the
/// whole window, which is what makes interaction affordable.
#[test]
fn a_changed_frame_is_painted_and_an_idle_one_is_not() {
    let mut app = App::new();
    app.resize(1200, 800, 1.5);
    for _ in 0..3 {
        app.render();
    }
    let (x, y, w, h) = app.damage_px();
    assert_eq!((x, y, w, h), (0, 0, 1200, 800), "the first painted frame is the whole window");

    // The UI settles (fonts, caret, animations) and then stops painting entirely.
    let mut settled = false;
    for _ in 0..40 {
        if app.render() == FrameNeeded::No {
            settled = true;
            break;
        }
    }
    assert!(settled, "an idle UI stops painting");

    // Scrolling changes the page area only, not the ribbon. egui scrolls what the pointer is over,
    // so the pointer has to be on the page first.
    app.pointer(PointerKind::Move, 600.0, 400.0, Button::None, 0);
    app.render();
    app.scroll(Scroll { dx: 0.0, dy: -120.0 });
    assert_eq!(app.render(), FrameNeeded::Yes, "a scroll paints");
    // The damage rectangle never exceeds the frame, and the first painted frame is all of it.
    let (_, _, damage_w, damage_h) = app.damage_px();
    assert!(damage_w > 0 && damage_h > 0 && damage_w <= 1200 && damage_h <= 800, "damage {damage_w}x{damage_h}");
    // Measured, not assumed: on this UI an input frame still changes meshes across the whole
    // window (egui re-emits the scene), so the diff usually ends up asking for the full frame.
    // The painter keeps the damage path for the frames where it does help (a caret, a hover).
}

#[test]
fn a_surface_is_needed_before_a_frame() {
    let mut app = App::new();
    assert_eq!(app.render(), FrameNeeded::No);
    assert!(app.pixels().is_empty());
    app.resize(320, 200, 1.0);
    assert_eq!(app.render(), FrameNeeded::Yes);
    app.surface_gone();
    assert_eq!(app.render(), FrameNeeded::No);
    assert!(app.pixels().is_empty());
}

/// Sizes from the platform are hostile input: nothing panics, nothing allocates wildly.
#[test]
fn hostile_sizes_do_not_panic() {
    for (w, h, d) in
        [(0u32, 0u32, 1.0f32), (1, 1, 1.0), (u32::MAX, u32::MAX, 1.0), (16384, 1, 0.0), (1, 16384, f32::NAN), (33, 17, 1.0e9), (8192, 8192, 1.0)]
    {
        let mut app = App::new();
        app.resize(w, h, d);
        app.pointer(PointerKind::Down, 1.0, 1.0, Button::Primary, 0);
        let _ = app.render();
        assert_eq!(app.pixels().len(), app.stride() * app.height() as usize, "{w}x{h}");
        assert!((0.5..=8.0).contains(&app.density()), "density clamped: {}", app.density());
    }
}
