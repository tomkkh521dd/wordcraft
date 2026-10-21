//! The system font folders a host adds at start-up (HarmonyOS passes the ones the system reports).
//!
//! Its own process, because the list is set once per process and other tests in this crate look at
//! the platform's folders.

use std::path::PathBuf;

#[test]
fn injected_system_dirs_come_first_and_are_set_once() {
    let dir = std::env::temp_dir().join(format!("wc-host-fonts-{}", std::process::id()));
    let missing = dir.join("not-installed");
    assert!(wordcraft_fonts::set_system_font_dirs([dir.clone(), missing.clone()]));

    let dirs = wordcraft_fonts::system_font_dirs();
    assert_eq!(dirs.first(), Some(&dir), "injected folders are scanned first: {dirs:?}");
    assert_eq!(dirs.get(1), Some(&missing));

    // The catalog may already have been built, so a later list is ignored rather than half-applied.
    let late = PathBuf::from("/nowhere-at-all");
    assert!(!wordcraft_fonts::set_system_font_dirs([late.clone()]));
    let dirs = wordcraft_fonts::system_font_dirs();
    assert!(dirs.contains(&dir), "{dirs:?}");
    assert!(!dirs.contains(&late), "{dirs:?}");
}
