//! WordCraft for HarmonyOS NEXT: the app the HAP's native shell drives, without the C ABI.
//!
//! The shell (`apps/wordcraft-ohos` and the C++ in `ohos/entry/src/main/cpp`) reports the surface
//! size, the display density, input and time, and copies out finished RGBA frames. Everything else
//! — configuration, event bookkeeping, the document session, and the UI from step 3 on — lives
//! here, holding no platform pointers, so this crate keeps `unsafe_code = "forbid"`.
//!
//! Layout of the port, and what each step builds, is in `docs/platform-ohos.md`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod app;
mod config;
mod input;
pub mod logging;
mod test_pattern;

pub use app::{App, FrameNeeded};
pub use config::{Config, Start, Theme};
pub use input::{Button, KeyEvent, PointerKind, Scroll, modifiers};

#[cfg(test)]
mod tests;
