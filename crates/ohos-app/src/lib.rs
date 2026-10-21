//! WordCraft for HarmonyOS NEXT: the app the HAP's native shell drives, without the C ABI.
//!
//! The shell (`apps/wordcraft-ohos` and the C++ in `ohos/entry/src/main/cpp`) reports the surface
//! size, the display density, input and time, and copies out finished RGBA frames. Everything else
//! — the egui UI, the document session, configuration, event bookkeeping — lives here, holding no
//! platform pointers, so this crate keeps `unsafe_code = "forbid"`.
//!
//! Layout of the port, and what each step builds, is in `docs/platform-ohos.md`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod app;
mod config;
mod input;
pub mod logging;

pub use app::{App, FrameNeeded, FrameTimings};
pub use config::{Config, Start, Theme};
pub use input::{Button, KeyEvent, PointerKind, Scroll, egui_key, modifiers};

#[cfg(test)]
mod tests;
