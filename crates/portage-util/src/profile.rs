//! Phase timers behind `PORTUALE_PROFILE=1` (backlog #455, plan S0).
//!
//! `span("name")` returns a guard that prints `[profile] name: <seconds>` to
//! stderr when dropped, only when the variable is set (read once). Off by
//! default: one cached bool check and no output, so it never changes
//! `emerge` output.
//!
//! While the pretend plan cache captures a run's stderr into the stored
//! bytes (backlog #474), spans raised inside the capture are buffered and
//! printed when the capture ends, so the cached bytes stay span-free
//! (a timing line in the bytes would replay stale numbers and fail
//! `PORTUALE_CACHE_VERIFY=1` on every hit).

use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// Whether `PORTUALE_PROFILE` is set to anything but empty or `0`.
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("PORTUALE_PROFILE").is_ok_and(|v| !v.is_empty() && v != "0"))
}

/// While set, dropping a [`span`] buffers its line instead of printing it;
/// the capturer drains the buffer after restoring the real fds.
static CAPTURING: AtomicBool = AtomicBool::new(false);

/// Buffered span lines, oldest first. Only ever non-empty while profiling
/// with an active capture; a poisoned mutex just drops the line.
static BUFFERED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Begin or end span buffering for a stderr capture.
pub fn set_capturing(on: bool) {
    CAPTURING.store(on, Ordering::Relaxed);
}

/// Take all buffered span lines, oldest first.
pub fn take_buffered() -> Vec<String> {
    BUFFERED
        .lock()
        .map(|mut lines| std::mem::take(&mut *lines))
        .unwrap_or_default()
}

fn emit_or_buffer(line: String) {
    if CAPTURING.load(Ordering::Relaxed) {
        if let Ok(mut lines) = BUFFERED.lock() {
            lines.push(line);
        }
    } else {
        eprintln!("{line}");
    }
}

/// Reports the time from [`span`] to drop.
pub struct Span {
    name: String,
    start: Option<Instant>,
}

/// Start timing `name`; a no-op guard when profiling is off.
pub fn span(name: impl Into<String>) -> Span {
    if enabled() {
        Span {
            name: name.into(),
            start: Some(Instant::now()),
        }
    } else {
        Span {
            name: String::new(),
            start: None,
        }
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        if let Some(start) = self.start {
            emit_or_buffer(format!(
                "[profile] {}: {:.3}",
                self.name,
                start.elapsed().as_secs_f64()
            ));
        }
    }
}
