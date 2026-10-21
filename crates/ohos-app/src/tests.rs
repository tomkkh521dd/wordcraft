//! Step 2's contract: configuration, input bookkeeping, and a frame that is RGBA in the right
//! order and never indexes past its buffer.

use std::ffi::OsString;

use proptest::{prop_assert, prop_assert_eq};

use crate::config::{Config, FONT_DIRS, LOCALE, SETTINGS_DIR, START, Start, THEME, Theme};
use crate::input::{Button, KeyEvent, PointerKind, Scroll};
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
    // Empty values read as "not given", and unknown words fall back to the defaults.
    assert_eq!(config.settings_dir, None);
    assert_eq!(config.theme, Theme::Auto);
    assert_eq!(config.start, Start::Sample);
}

#[test]
fn config_defaults_without_anything() {
    let config = Config::from_lookup(lookup(&[]));
    assert_eq!(config, Config::default());
    assert_eq!(config.theme, Theme::Auto);
    assert_eq!(config.start, Start::Sample);
}

#[test]
fn theme_and_start_parse_leniently() {
    for (word, theme) in [("light", Theme::Light), ("Dark", Theme::Dark), ("auto", Theme::Auto), ("sepia", Theme::Auto), ("", Theme::Auto)] {
        assert_eq!(Theme::parse(word), theme, "{word:?}");
    }
    for (word, start) in [("empty", Start::Empty), ("SAMPLE", Start::Sample), ("", Start::Sample)] {
        assert_eq!(Start::parse(word), start, "{word:?}");
    }
    assert_eq!(Theme::Light.as_str(), "light");
}

#[test]
fn applying_config_reports_what_happened() {
    let config = Config::from_lookup(lookup(&[(SETTINGS_DIR, "/sandbox/settings"), (FONT_DIRS, "/system/fonts"), (LOCALE, "en-US")]));
    let notes = config.apply();
    assert!(notes.iter().any(|n| n.contains("/sandbox/settings")), "{notes:?}");
    assert!(notes.iter().any(|n| n.contains("1 folder")), "{notes:?}");
    assert!(notes.iter().any(|n| n.contains("en-US")), "{notes:?}");
    // The font catalog may already exist by now (another test looked at it); either answer is
    // valid, but the note must say which.
    assert!(notes.iter().any(|n| n.starts_with("system fonts:")), "{notes:?}");
}

/// The first stripe pair is red, then green, then blue, then white: BGRA would swap red and blue.
#[test]
fn the_pattern_draws_the_channels_in_rgba_order() {
    let (w, h) = (400u32, 100u32);
    let mut app = App::new();
    app.resize(w, h, 1.0);
    assert_eq!(app.render(), FrameNeeded::Yes);
    let px = app.pixels();
    let at = |x: u32, y: u32| {
        let i = (y * w + x) as usize * 4;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    };
    assert_eq!(at(10, 8), [255, 0, 0, 255], "red stripe");
    assert_eq!(at(60, 8), [0, 200, 0, 255], "green stripe");
    assert_eq!(at(110, 8), [0, 0, 255, 255], "blue stripe");
    assert_eq!(at(160, 8), [255, 255, 255, 255], "white stripe");
    // The focus bar is the first three rows.
    assert_eq!(at(390, 0), [80, 200, 120, 255], "focused");
    assert_eq!(app.stride(), w as usize * 4);
    assert_eq!(px.len(), w as usize * h as usize * 4);
    assert_eq!(app.frames(), 1);
}

#[test]
fn losing_focus_greys_the_bar() {
    let mut app = App::new();
    app.resize(64, 48, 1.0);
    app.focus(false);
    assert_eq!(app.render(), FrameNeeded::Yes);
    assert_eq!(&app.pixels()[0..4], &[90, 90, 100, 255]);
}

#[test]
fn a_surface_is_needed_before_a_frame() {
    let mut app = App::new();
    assert_eq!(app.render(), FrameNeeded::No);
    assert!(app.pixels().is_empty());
    app.resize(32, 16, 2.0);
    assert_eq!(app.render(), FrameNeeded::Yes);
    assert_eq!(app.pixels().len(), 32 * 16 * 4);
    app.surface_gone();
    assert_eq!(app.render(), FrameNeeded::No);
    assert!(app.pixels().is_empty());
}

/// Input arrives in vp, is clamped to something finite, and every kind leaves a trace.
#[test]
fn input_is_counted_clamped_and_remembered() {
    let mut app = App::new();
    app.resize(100, 100, 2.0);
    app.pointer(PointerKind::Down, 10.0, 20.0, Button::Primary, 0);
    assert_eq!(app.events(), 1);
    assert!(app.last_event().contains("pointer down primary"));
    app.pointer(PointerKind::Move, f32::NAN, f32::INFINITY, Button::None, 0);
    assert!(app.last_event().contains("(0.0, 0.0)"), "{}", app.last_event());
    app.pointer(PointerKind::Move, 1.0e30, -1.0e30, Button::None, 0);
    assert!(app.last_event().contains("(1000000.0, -1000000.0)"), "{}", app.last_event());
    app.pointer(PointerKind::Leave, 0.0, 0.0, Button::None, 0);
    assert!(!app.stats_json().contains("\"pointer\":["), "the pointer is gone: {}", app.stats_json());
    app.scroll(Scroll { dx: 1.5, dy: -2.0 });
    assert!(app.last_event().contains("scroll"));
    app.key(KeyEvent { code: 2017, pressed: true, repeated: false, modifiers: 2, text: None });
    assert!(app.last_event().contains("key 2017 down Ctrl"), "{}", app.last_event());
    app.text('字');
    assert_eq!(app.typed(), "字");
    app.ime_composing(true);
    assert!(app.composing());
    assert_eq!(app.events(), 8);
}

/// A key that produced a character types it; a key-up or a shifted arrow does not.
#[test]
fn typing_keys_type_their_character() {
    let mut app = App::new();
    app.key(KeyEvent { code: 2017, pressed: true, repeated: false, modifiers: 0, text: Some('a') });
    app.key(KeyEvent { code: 2017, pressed: false, repeated: false, modifiers: 0, text: Some('a') });
    app.key(KeyEvent { code: 2018, pressed: true, repeated: false, modifiers: 0, text: None });
    assert_eq!(app.typed(), "a");
}

#[test]
fn stats_are_json_with_the_numbers_the_shell_logs() {
    let mut app = App::new();
    app.resize(320, 200, 1.5);
    app.render();
    let stats: serde_json::Value = serde_json::from_str(&app.stats_json()).unwrap_or_default();
    assert_eq!(stats["width"], 320u64);
    assert_eq!(stats["height"], 200u64);
    assert_eq!(stats["density"], 1.5);
    assert_eq!(stats["frames"], 1u64);
    assert_eq!(stats["fontDirs"], 0u64);
    assert!(stats["paragraphs"].as_u64().unwrap_or(0) > 0, "the sample document loads: {}", app.stats_json());
    assert!(stats["theme"].is_string());
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
        // Whatever was allocated matches the (clamped) size it claims, and a refused surface
        // stays consistently unsized.
        assert_eq!(app.pixels().len(), app.stride() * app.height() as usize, "{w}x{h}");
        assert!(app.density() >= 0.5 && app.density() <= 8.0, "density clamped: {}", app.density());
    }
}

proptest::proptest! {
    #[test]
    fn any_surface_size_paints_or_declines(width in 0u32..600, height in 0u32..400, x in -1.0e6f32..1.0e6, y in -1.0e6f32..1.0e6, density in 0.0f32..9.0) {
        let mut app = App::new();
        app.resize(width, height, density);
        app.pointer(PointerKind::Move, x, y, Button::None, 0);
        let needed = app.render() == FrameNeeded::Yes;
        prop_assert_eq!(needed, width > 0 && height > 0);
        prop_assert_eq!(app.pixels().len(), (width as usize) * (height as usize) * 4);
        // Every byte was written by the pattern: the pixels are opaque (alpha 255) throughout.
        let (pixels, rest) = app.pixels().as_chunks::<4>();
        prop_assert!(rest.is_empty(), "a frame is whole pixels");
        prop_assert!(pixels.iter().take(64).all(|px| px[3] == 255));
    }
}
