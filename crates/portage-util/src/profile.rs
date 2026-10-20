//! Phase timers behind `PORTUALE_PROFILE=1` (backlog #455, plan S0).
//!
//! `span("name")` returns a guard that prints `[profile] name: <seconds>` to
//! stderr when dropped, only when the variable is set (read once). Off by
//! default: one cached bool check and no output, so it never changes
//! `emerge` output.

use std::sync::OnceLock;
use std::time::Instant;

/// Whether `PORTUALE_PROFILE` is set to anything but empty or `0`.
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("PORTUALE_PROFILE").is_ok_and(|v| !v.is_empty() && v != "0"))
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
            eprintln!(
                "[profile] {}: {:.3}",
                self.name,
                start.elapsed().as_secs_f64()
            );
        }
    }
}
