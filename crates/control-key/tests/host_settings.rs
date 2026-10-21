//! The settings folder a host gives at start-up (HarmonyOS: the app sandbox).
//!
//! Its own process, because the folder is set once per process and the unit tests in this crate
//! check the platform's own detection.

use std::path::PathBuf;

#[test]
fn host_settings_dir_is_used_and_holds_the_key_file() {
    let host = PathBuf::from("/sandbox/haps/entry/files/wordcraft-settings");
    wordcraft_control_key::set_settings_dir(host.clone());
    assert_eq!(wordcraft_control_key::settings_dir(), Some(host.clone()));

    // The key file lives inside it, named after the instance as on the desktop.
    assert_eq!(wordcraft_control_key::key_path(&host, 7981), host.join("control-key.local-7981"));
}
