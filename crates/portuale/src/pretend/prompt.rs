//! `--ask` prompts and their terminal handling.

use crate::pretend::*;

/// Set by the handler, consumed by `read_prompt_line`; cleared on every
/// install so a stale flag can never interrupt a later prompt.
/// Portage: _emerge/UserQuery.py, query
pub(crate) static PROMPT_SIGINT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// The handler behind `PromptSigintGuard`: records the signal and
/// returns. An atomic store is async-signal-safe; nothing else runs
/// here, so interrupting allocator locks or stdio buffers is safe.
pub(crate) extern "C" fn prompt_sigint_handler(_sig: libc::c_int) {
    PROMPT_SIGINT.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// RAII SIGINT scope for the prompt reads: installs
/// `prompt_sigint_handler` (without `SA_RESTART`, so the raw
/// `libc::read` observes `EINTR`) and restores the previous disposition
/// on drop. Covers the prints too, not just the blocking read.
pub(crate) struct PromptSigintGuard {
    pub(crate) old: libc::sigaction,
}

impl PromptSigintGuard {
    pub(crate) fn install() -> Self {
        // SAFETY: `sigaction`/`sigemptyset` touch only the two local
        // structs; the handler is a plain function pointer valid for
        // the process lifetime.
        unsafe {
            let mut old: libc::sigaction = std::mem::zeroed();
            let mut new: libc::sigaction = std::mem::zeroed();
            new.sa_sigaction = prompt_sigint_handler as *const () as usize;
            libc::sigemptyset(&mut new.sa_mask);
            new.sa_flags = 0;
            // Clear before installing, so a SIGINT landing in between is not lost.
            PROMPT_SIGINT.store(false, std::sync::atomic::Ordering::SeqCst);
            assert_eq!(
                libc::sigaction(libc::SIGINT, &new, &mut old),
                0,
                "sigaction(SIGINT) failed"
            );
            Self { old }
        }
    }
}

impl Drop for PromptSigintGuard {
    fn drop(&mut self) {
        // SAFETY: restores the disposition saved at install; the
        // pointer is the guard's own field, valid until drop returns.
        unsafe {
            libc::sigaction(libc::SIGINT, &self.old, std::ptr::null_mut());
        }
    }
}

/// `--ask-enter-invalid`: process-wide because the flag is CLI-global and
/// only read by the interactive prompts. `run` resets it at entry and sets
/// it while parsing, so in-process callers can't leak it between invocations.
pub(crate) static ASK_ENTER_INVALID: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub(crate) fn ask_enter_invalid() -> bool {
    ASK_ENTER_INVALID.load(std::sync::atomic::Ordering::Relaxed)
}

/// The matching half of `ask_confirm`'s loop, split out so it's directly
/// unit-testable without stdin. Case-insensitive prefix match, `Yes`
/// first; a bare Enter matches `Yes` unless `--ask-enter-invalid` is set.
/// Portage: _emerge/UserQuery.py, query
pub(crate) fn classify_yes_no(answer: &str, enter_invalid: bool) -> Option<bool> {
    if answer.is_empty() {
        return if enter_invalid { None } else { Some(true) };
    }
    let lower = answer.to_ascii_lowercase();
    if "yes".starts_with(lower.as_str()) {
        Some(true)
    } else if "no".starts_with(lower.as_str()) {
        Some(false)
    } else {
        None
    }
}

/// A short pause before a removal starts, so Ctrl-C can still abort.
/// `CLEAN_DELAY=0` skips it. Honours the `CLEAN_DELAY` env var directly.
/// Portage: _emerge/unmerge.py, countdown
pub(crate) fn clean_delay_countdown() {
    let secs: u64 = std::env::var("CLEAN_DELAY")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(5);
    if secs == 0 {
        return;
    }
    print!(">>> Waiting {secs} seconds before starting...\n>>> (Control-C to abort)...\n");
    use std::io::Write;
    let _ = std::io::stdout().flush();
    std::thread::sleep(std::time::Duration::from_secs(secs));
}
