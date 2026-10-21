//! The C ABI the HarmonyOS shell calls (C++ in `ohos/entry/src/main/cpp`).
//!
//! A HAP's native library is loaded by ArkTS, and its C++ half does the platform work: NAPI, the
//! native window buffer, hilog, input events. This crate is the seam. C++ calls `wc_*` functions
//! whose declarations live in `wc_abi.h`, and they drive an [`App`] from `wordcraft-ohos-app`.
//!
//! # `unsafe`
//!
//! This crate sets `unsafe_code = "deny"` rather than inheriting the workspace's `forbid`, and
//! only [`abi`] opts out, because exporting a symbol C can find requires `#[unsafe(no_mangle)]`,
//! which the lint counts. Nothing in the port dereferences a pointer:
//! arguments cross by value, configuration arrives through the environment, and pixels leave as a
//! `(pointer, length)` pair the shell copies out before its next call. The app itself
//! (`wordcraft-ohos-app`) keeps `unsafe_code = "forbid"`.
#![deny(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod abi;

pub use abi::*;
