//! `log` records forwarded to the shell's logger (hilog on device, stdout in tests).
//!
//! The sink is an `extern "C"` function pointer, which is safe to hold and call: the shell owns
//! the bytes' lifetime (it copies them), and a null sink keeps everything on the floor.

use std::sync::Mutex;

use log::{Level, LevelFilter, Metadata, Record};

/// The shell's logger: a UTF-8 message, borrowed for the call only.
pub type Sink = extern "C" fn(*const u8, usize);

static SINK: Mutex<Option<Sink>> = Mutex::new(None);

struct Forward;

impl log::Log for Forward {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= Level::Debug
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!("{}: {}\n", record.level(), record.args());
        // A poisoned lock means a previous logger call panicked: drop the line rather than panic
        // (a word processor must never crash over a log message).
        if let Ok(guard) = SINK.lock()
            && let Some(sink) = *guard
        {
            sink(line.as_ptr(), line.len());
        }
    }

    fn flush(&self) {}
}

static FORWARD: Forward = Forward;

/// Point the `log` facade at the shell. Call once, before anything logs; later calls replace the
/// sink (so a restart in the same process keeps working).
pub fn set_sink(sink: Option<Sink>) {
    if let Ok(mut guard) = SINK.lock() {
        *guard = sink;
    }
    // `set_logger` fails only when a logger is already installed, which is exactly the second
    // call: the sink above is the part that changes.
    let _ = log::set_logger(&FORWARD);
    // Debug until the port settles: on a device, hilog is the only way to see what arrived.
    log::set_max_level(LevelFilter::Debug);
}
