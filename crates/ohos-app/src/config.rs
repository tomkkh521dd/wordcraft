//! What the shell tells the app at start-up, read from the environment.
//!
//! The C++ shell cannot hand strings over without pointers, and this side forbids `unsafe`, so it
//! publishes them as environment variables with `setenv` before the app starts. That also means
//! the same configuration can be reproduced from `hdc shell` and in tests.

use std::ffi::OsString;
use std::path::PathBuf;

/// The app's sandbox folder for settings, preferences and logs (ArkTS `context.filesDir`).
pub const SETTINGS_DIR: &str = "WORDCRAFT_OHOS_SETTINGS_DIR";
/// The folders the system reports for fonts (`font.getUIFontConfig().fontDir`), as a path list.
pub const FONT_DIRS: &str = "WORDCRAFT_OHOS_FONT_DIRS";
/// The system language, `zh-Hans`/`en-US` style (`i18n.System.getSystemLanguage`).
pub const LOCALE: &str = "WORDCRAFT_OHOS_LOCALE";
/// `auto`, `light` or `dark` — the system's colour mode.
pub const THEME: &str = "WORDCRAFT_OHOS_THEME";
/// `sample` (the built-in demo document) or `empty`.
pub const START: &str = "WORDCRAFT_OHOS_START";

/// The interface's colour mode, following the system by default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    Auto,
    Light,
    Dark,
}

impl Theme {
    /// An unknown value reads as the default (a newer or older shell must not break start-up).
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::Auto,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

/// What document to open when the app starts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Start {
    /// The built-in sample document (the desktop app's `--sample`).
    #[default]
    Sample,
    /// Nothing open, the way the desktop app starts.
    Empty,
}

impl Start {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "empty" => Self::Empty,
            _ => Self::Sample,
        }
    }
}

/// The shell's start-up configuration.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    pub settings_dir: Option<PathBuf>,
    pub font_dirs: Vec<PathBuf>,
    pub locale: Option<String>,
    pub theme: Theme,
    pub start: Start,
}

impl Config {
    /// Read the environment the shell set up.
    pub fn from_env() -> Self {
        Self::from_lookup(|name| std::env::var_os(name))
    }

    /// The same from any lookup, so tests don't touch the process environment (setting variables
    /// is `unsafe` in edition 2024, and this crate forbids `unsafe`).
    pub fn from_lookup(mut get: impl FnMut(&str) -> Option<OsString>) -> Self {
        let non_empty = |v: Option<OsString>| v.filter(|v| !v.is_empty());
        let settings_dir = non_empty(get(SETTINGS_DIR)).map(PathBuf::from);
        // `split_paths` knows the platform's separator and leaves `C:\…` alone on Windows, which
        // keeps the host-side tests honest.
        let font_dirs =
            non_empty(get(FONT_DIRS)).map(|v| std::env::split_paths(&v).filter(|p| !p.as_os_str().is_empty()).collect()).unwrap_or_default();
        let locale = non_empty(get(LOCALE)).map(|v| v.to_string_lossy().into_owned());
        let theme = non_empty(get(THEME)).map(|v| Theme::parse(&v.to_string_lossy())).unwrap_or_default();
        let start = non_empty(get(START)).map(|v| Start::parse(&v.to_string_lossy())).unwrap_or_default();
        Self { settings_dir, font_dirs, locale, theme, start }
    }

    /// Hand the platform-independent parts to the crates that cache them, and describe what was
    /// done for the log. Order matters: the font catalog and the settings file are read on first
    /// use, so this must run before the first lookup.
    pub fn apply(&self) -> Vec<String> {
        let mut notes = Vec::new();
        if let Some(dir) = &self.settings_dir {
            wordcraft_control_key::set_settings_dir(dir.clone());
            notes.push(format!("settings folder: {}", dir.display()));
        } else {
            notes.push("settings folder: not given (preferences and logs stay in memory)".into());
        }
        if self.font_dirs.is_empty() {
            notes.push("system fonts: the platform's own folders".into());
        } else {
            let first = wordcraft_fonts::set_system_font_dirs(self.font_dirs.iter().cloned());
            notes.push(format!(
                "system fonts: {} folder(s) from the system{}",
                self.font_dirs.len(),
                if first { "" } else { ", given too late to be used" }
            ));
        }
        notes.push(format!("locale: {}", self.locale.as_deref().unwrap_or("(the environment's)")));
        notes.push(format!("theme: {}", self.theme.as_str()));
        notes.push(format!(
            "start: {}",
            match self.start {
                Start::Sample => "sample",
                Start::Empty => "empty",
            }
        ));
        notes
    }
}
