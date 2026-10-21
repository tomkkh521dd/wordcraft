//! `cargo xtask ohos`: check (or build) the workspace for HarmonyOS / OpenHarmony.
//!
//! The target triples `aarch64-unknown-linux-ohos` (devices) and `x86_64-unknown-linux-ohos`
//! (the DevEco emulators) both report `target_os = "linux"` with `target_env = "ohos"`, so they use
//! the OHOS NDK's clang and sysroot. This command finds the SDK, hands cargo the linker and the
//! C toolchain, and checks every crate the app can use — so a dependency that quietly stops
//! building for the target fails here instead of on someone's machine.
//!
//! Where the SDK is, in order: `$OHOS_SDK_HOME`, `$HOS_SDK_HOME`, `$DEVECO_SDK_HOME`, else the
//! usual install folders. The NDK is either `<root>/native` (the standalone command-line SDK) or
//! `<root>/default/openharmony/native` (DevEco Studio's bundled SDK), and versioned folders
//! (`<root>/26.0.0/native`) are searched too.
//!
//! Usage: `cargo xtask ohos [--target aarch64|x86_64] [--build]`
//! `--build` links the cdylib and the binaries instead of only checking types.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{cargo, metadata, root, run};

/// The device target and the emulator target.
const TARGETS: &[(&str, &str)] = &[
    // (rust target triple, NDK triple)
    ("aarch64-unknown-linux-ohos", "aarch64-linux-ohos"),
    ("x86_64-unknown-linux-ohos", "x86_64-linux-ohos"),
];

/// Crates nothing on HarmonyOS uses: the desktop shell (eframe/winit/rfd, which have no OHOS
/// backend), the browser shell, and the workspace tooling.
const SKIP: &[&str] = &["wordcraft", "wordcraft-web", "xtask"];

pub fn run_cmd(rest: &[&str]) -> Result<(), String> {
    let build = rest.contains(&"--build");
    let only = rest.iter().position(|a| *a == "--target").and_then(|i| rest.get(i + 1).copied());
    let missing = missing_targets();
    if !missing.is_empty() {
        return Err(format!("missing Rust targets: {}. Install them with:\n  rustup target add {}", missing.join(", "), missing.join(" ")));
    }
    let ndk = find_ndk()?;
    println!("OHOS NDK: {}", ndk.display());
    let crates = ohos_set()?;
    let mut failed = Vec::new();
    for (target, ndk_triple) in TARGETS {
        if let Some(want) = only
            && !target.starts_with(want)
        {
            continue;
        }
        println!("\n{target}: {} crate(s), {}…", crates.len(), if build { "building" } else { "checking" });
        if let Err(e) = check_target(target, ndk_triple, &ndk, &crates, build) {
            eprintln!("{e}");
            failed.push(*target);
        }
    }
    if failed.is_empty() {
        println!("\nOHOS check: ok");
        Ok(())
    } else {
        Err(format!("{} target(s) failed: {}", failed.len(), failed.join(", ")))
    }
}

/// Workspace packages the HarmonyOS app links: every layer crate (the UI included), the paint
/// crate, and the OHOS shell itself.
fn ohos_set() -> Result<Vec<String>, String> {
    let crates = metadata()?;
    let members = crates["workspace_members"].as_array().ok_or("cargo metadata: no workspace_members")?;
    let packages = crates["packages"].as_array().ok_or("cargo metadata: no packages")?;
    let mut out = Vec::new();
    for pkg in packages {
        let id = pkg["id"].as_str().unwrap_or_default();
        if !members.iter().any(|m| m.as_str() == Some(id)) {
            continue;
        }
        let Some(name) = pkg["name"].as_str() else { continue };
        // The desktop and browser shells can't build for the target (winit/rfd have no OHOS
        // backend) and nothing the app links uses them; tooling is host-only.
        if SKIP.contains(&name) {
            continue;
        }
        out.push(name.to_string());
    }
    out.sort();
    if out.is_empty() {
        return Err("no crates to check".into());
    }
    Ok(out)
}

fn check_target(target: &str, ndk_triple: &str, ndk: &Path, crates: &[String], build: bool) -> Result<(), String> {
    let llvm = ndk.join("llvm").join("bin");
    let clang = exe(&llvm, "clang");
    let clangxx = exe(&llvm, "clang++");
    let sysroot = ndk.join("sysroot");
    if !clang.is_file() {
        return Err(format!("no clang at {}", clang.display()));
    }
    if !sysroot.is_dir() {
        return Err(format!("no sysroot at {}", sysroot.display()));
    }
    // The SDK path usually holds spaces ("C:\Program Files\…"), and `RUSTFLAGS` is split on
    // whitespace, so the target and sysroot go in a small wrapper the linker setting names.
    let cc = wrapper(&clang, ndk_triple, &sysroot, target, "cc")?;
    let cxx = wrapper(&clangxx, ndk_triple, &sysroot, target, "cxx")?;
    let mut cmd = cargo();
    cmd.arg(if build { "build" } else { "check" }).args(["--target", target]);
    for c in crates {
        cmd.args(["-p", c]);
    }
    let key = target.to_uppercase().replace('-', "_");
    let triple_key = target.replace('-', "_");
    cmd.env(format!("CARGO_TARGET_{key}_LINKER"), &cc)
        .env(format!("CC_{triple_key}"), &cc)
        .env(format!("CXX_{triple_key}"), &cxx)
        .env(format!("AR_{triple_key}"), exe(&llvm, "llvm-ar"))
        .env("CARGO_TERM_COLOR", "always");
    run(cmd, &format!("cargo {} --target {target} ({} crates)", if build { "build" } else { "check" }, crates.len()))
}

/// Writes a compiler wrapper that adds the NDK triple and sysroot, and returns its path.
fn wrapper(compiler: &Path, ndk_triple: &str, sysroot: &Path, target: &str, kind: &str) -> Result<PathBuf, String> {
    let dir = build_dir().join("ohos-toolchain");
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let name = format!("{kind}-{target}");
    let path = if cfg!(windows) { dir.join(format!("{name}.cmd")) } else { dir.join(format!("{name}.sh")) };
    let body = if cfg!(windows) {
        // `%*` keeps the quoting rustc gave each argument (paths with spaces).
        format!("@echo off\r\n\"{}\" --target={ndk_triple} --sysroot=\"{}\" %*\r\n", compiler.display(), sysroot.display())
    } else {
        format!("#!/bin/sh\nexec \"{}\" --target={ndk_triple} --sysroot=\"{}\" \"$@\"\n", compiler.display(), sysroot.display())
    };
    std::fs::write(&path, body).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("cannot make {} executable: {e}", path.display()))?;
    }
    Ok(path)
}

/// Where cargo builds (`CARGO_TARGET_DIR` when set, as packaging and parallel agents do).
fn build_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| root().join("target"))
}

fn exe(dir: &Path, name: &str) -> PathBuf {
    let mut p = dir.join(name);
    if cfg!(windows) {
        p.set_extension("exe");
    }
    p
}

/// Rust targets this toolchain can't build for yet (empty when rustup can't say: cargo will).
fn missing_targets() -> Vec<&'static str> {
    let rustup = std::env::var("RUSTUP").unwrap_or_else(|_| "rustup".into());
    let Ok(out) = Command::new(rustup).args(["target", "list", "--installed"]).output() else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let installed = String::from_utf8_lossy(&out.stdout);
    TARGETS.iter().map(|(t, _)| *t).filter(|t| !installed.lines().any(|l| l.trim() == *t)).collect()
}

/// The newest NDK found, from the environment first and then the usual install folders.
fn find_ndk() -> Result<PathBuf, String> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for var in ["OHOS_SDK_HOME", "HOS_SDK_HOME", "DEVECO_SDK_HOME", "OHOS_NDK_HOME"] {
        if let Some(v) = std::env::var_os(var).filter(|v| !v.is_empty()) {
            roots.push(PathBuf::from(v));
        }
    }
    // The workspace may carry a checkout-relative SDK (CI unpacks it here).
    roots.push(root().join("ohos-sdk"));
    if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            roots.push(PathBuf::from(local).join("Huawei").join("Sdk"));
        }
        roots.push(PathBuf::from(r"C:\Program Files\Huawei\DevEco Studio\sdk"));
    } else if cfg!(target_os = "macos") {
        roots.push(PathBuf::from("/Applications/DevEco-Studio.app/Contents/sdk"));
    } else {
        roots.push(PathBuf::from("/opt/ohos-sdk"));
        roots.push(PathBuf::from("/usr/local/ohos-sdk"));
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join("ohos-sdk"));
        }
    }
    let mut found: Vec<PathBuf> = Vec::new();
    for r in &roots {
        // `<root>/native` (command-line SDK), `<root>/default/openharmony/native` (DevEco).
        for candidate in [r.join("native"), r.join("default").join("openharmony").join("native")] {
            if is_ndk(&candidate) {
                found.push(candidate);
            }
        }
        // `<root>/<version>/native` and `<root>/<version>/openharmony/native`.
        let Ok(entries) = std::fs::read_dir(r) else { continue };
        for e in entries.flatten() {
            for candidate in
                [e.path().join("native"), e.path().join("openharmony").join("native"), e.path().join("default").join("openharmony").join("native")]
            {
                if is_ndk(&candidate) {
                    found.push(candidate);
                }
            }
        }
    }
    // Newest last by the version folder's name (26.0.0 sorts after 9.0.0 only with a numeric
    // compare), so compare the numeric parts.
    found.sort_by_key(|p| version_key(p));
    found.pop().ok_or_else(|| {
        format!(
            "no OHOS NDK found. Set OHOS_SDK_HOME to the SDK folder (one holding `<version>/native` \
             or `default/openharmony/native`). Looked in: {}",
            roots.iter().map(|r| r.display().to_string()).collect::<Vec<_>>().join(", ")
        )
    })
}

fn is_ndk(dir: &Path) -> bool {
    dir.join("llvm").join("bin").is_dir() && dir.join("sysroot").is_dir()
}

/// The numeric parts of the version folder above the NDK (`.../26.0.0/native` → [26, 0, 0]).
fn version_key(ndk: &Path) -> Vec<u32> {
    ndk.ancestors()
        .filter_map(|p| p.file_name().and_then(|n| n.to_str()))
        .find(|n| n.chars().next().is_some_and(|c| c.is_ascii_digit()) && n.contains('.'))
        .map(|n| n.split('.').filter_map(|part| part.parse().ok()).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_folders_sort_numerically() {
        let newer = version_key(Path::new("/sdk/26.0.0/native"));
        let older = version_key(Path::new("/sdk/9.0.0/native"));
        assert!(newer > older, "{newer:?} vs {older:?}");
        assert_eq!(newer, vec![26, 0, 0]);
        // The DevEco layout has no version folder: an empty key, so it sorts first (and loses).
        assert!(version_key(Path::new("/dev/eco/default/openharmony/native")).is_empty());
    }

    #[test]
    fn targets_cover_devices_and_emulators() {
        assert!(TARGETS.iter().any(|(t, _)| *t == "aarch64-unknown-linux-ohos"));
        assert!(TARGETS.iter().any(|(t, _)| *t == "x86_64-unknown-linux-ohos"));
        assert!(TARGETS.iter().all(|(_, ndk)| ndk.ends_with("-linux-ohos")));
    }

    #[test]
    fn the_desktop_and_web_shells_are_not_checked() {
        // They can't build for the target (winit/rfd have no OHOS backend) and nothing links them.
        assert!(SKIP.contains(&"wordcraft"));
        assert!(SKIP.contains(&"wordcraft-web"));
    }
}
