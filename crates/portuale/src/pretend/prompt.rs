//! `--ask` prompts and their terminal handling.

use crate::pretend::*;

/// Portage's `--ask` TTY gate (`sys.stdin.isatty()`).
/// Portage: _emerge/actions.py
pub(crate) fn stdin_is_tty() -> bool {
    // SAFETY: `isatty` is a pure syscall on a well-known, always-valid
    // fd (0); no pointers involved.
    unsafe { libc::isatty(libc::STDIN_FILENO) != 0 }
}

/// The default `responses=["Yes", "No"]` case: prints the bold `<question>`
/// once, then loops on the bracketed choices until one prefix-matches "Yes"
/// or "No" (bare Enter matches "Yes"). Returns `true` for "Yes", `false`
/// (after printing `Quitting.`) for "No", and `false` on EOF, read error,
/// or SIGINT. TTY gating happens once, globally, before `ask` is ever true.
/// Portage: _emerge/UserQuery.py, query
///
/// `--ask-enter-invalid` makes a bare Enter loop back instead of matching
/// "Yes"; see `classify_yes_no`.
pub(crate) fn ask_confirm(color: &Colorizer, question: &str) -> bool {
    match ask_yes_no(color, question) {
        Some(true) => true,
        Some(false) => {
            println!("\nQuitting.\n");
            false
        }
        None => false,
    }
}

/// The prompt half of `ask_confirm`, without the decline side effect: a
/// "No" (or EOF) answer just returns `false` with nothing printed.
/// Returns `None` on EOF, read error, or SIGINT (after printing
/// `Interrupted.`); `ask_confirm` folds that into a quiet decline.
/// SIGINT handling is scoped to the prompt read itself: the previous
/// disposition is restored before this returns.
/// Portage: _emerge/UserQuery.py, query
pub(crate) fn ask_yes_no(color: &Colorizer, question: &str) -> Option<bool> {
    use std::io::Write;
    // Installed before the first print, so a ^C typed the moment the
    // prompt exists is already handled.
    let _sigint = PromptSigintGuard::install();
    print!("\n{} ", color.c("bold", question));
    loop {
        print!(
            "[{}/{}] ",
            color.c("PROMPT_CHOICE_DEFAULT", "Yes"),
            color.c("PROMPT_CHOICE_OTHER", "No")
        );
        let _ = std::io::stdout().flush();
        match read_prompt_line() {
            None => {
                println!("Interrupted.");
                return None;
            }
            Some(line) => match classify_yes_no(line.trim(), ask_enter_invalid()) {
                Some(true) => return Some(true),
                Some(false) => {
                    return Some(false);
                }
                None => print!("Sorry, response '{}' not understood. ", line.trim()),
            },
        }
    }
}

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

/// One line from fd 0 for the prompt loops: blocks in a raw `libc::read`
/// (a std `read_line` would retry `EINTR` internally and never observe
/// the scoped SIGINT) until `\n`, EOF, a read error, or the guard's SIGINT
/// flag. Returns `Some(line)` or `None` for bare EOF, SIGINT, or any other
/// read error. A partial line pending at EOF is evaluated like
/// `read_line`'s `Ok(_)` arm; invalid UTF-8 also yields `None`.
///
/// Line-oriented reader for the interactive prompts' stdin. One call
/// returns exactly one line (including its `\n`); anything past that
/// line is kept in userspace, and any further data already queued in
/// the kernel is drained into that same leftover on the way out: without
/// this the next prompt answer sits in the kernel where any child spawned
/// with inherited stdin can steal it.
pub(crate) struct PromptLineBuffer {
    pub(crate) leftover: Vec<u8>,
}

impl PromptLineBuffer {
    pub(crate) const fn new() -> Self {
        Self {
            leftover: Vec::new(),
        }
    }

    /// Split the first line out of `self.leftover`, if it holds one.
    pub(crate) fn take_line(&mut self) -> Option<String> {
        let pos = self.leftover.iter().position(|&b| b == b'\n')?;
        let line: Vec<u8> = self.leftover.drain(..=pos).collect();
        String::from_utf8(line).ok()
    }

    /// One line from `fd`, or `None` on EOF with nothing buffered or on
    /// a read error. Blocking only until the first line is complete (or
    /// EOF/error).
    pub(crate) fn read_line(&mut self, fd: i32) -> Option<String> {
        if let Some(line) = self.take_line() {
            self.drain_available(fd);
            return Some(line);
        }
        let mut chunk = [0u8; 256];
        loop {
            let n = unsafe { libc::read(fd, chunk.as_mut_ptr() as *mut libc::c_void, chunk.len()) };
            if n < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    // A prompt SIGINT aborts the read; any other EINTR retries.
                    if PROMPT_SIGINT.load(std::sync::atomic::Ordering::SeqCst) {
                        return None;
                    }
                    continue;
                }
                return None;
            }
            if n == 0 {
                break;
            }
            self.leftover.extend_from_slice(&chunk[..n as usize]);
            if let Some(line) = self.take_line() {
                self.drain_available(fd);
                return Some(line);
            }
        }
        if self.leftover.is_empty() {
            None
        } else {
            let line: Vec<u8> = std::mem::take(&mut self.leftover);
            String::from_utf8(line).ok()
        }
    }

    /// Pull whatever else is already queued into `self.leftover` so it
    /// cannot be read by anyone else sharing the fd. Non-blocking; the
    /// fd's flags are restored on the way out.
    pub(crate) fn drain_available(&mut self, fd: i32) {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 {
            return;
        }
        if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } != 0 {
            return;
        }
        let mut chunk = [0u8; 256];
        loop {
            let n = unsafe { libc::read(fd, chunk.as_mut_ptr() as *mut libc::c_void, chunk.len()) };
            if n <= 0 {
                break;
            }
            self.leftover.extend_from_slice(&chunk[..n as usize]);
        }
        unsafe {
            libc::fcntl(fd, libc::F_SETFL, flags);
        }
    }
}

pub(crate) fn read_prompt_line() -> Option<String> {
    if PROMPT_SIGINT.load(std::sync::atomic::Ordering::SeqCst) {
        return None;
    }
    // Process-wide because fd 0 is process-wide and the interactive
    // prompts are the only readers of it.
    static PROMPT_LINE_BUFFER: std::sync::Mutex<PromptLineBuffer> =
        std::sync::Mutex::new(PromptLineBuffer::new());
    let mut buffer = PROMPT_LINE_BUFFER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    buffer.read_line(libc::STDIN_FILENO)
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

/// The explicit-`responses` menu (the `--ask` package menu of
/// `action_config`): loops printing `Selection? `, accepting `1`..`n`
/// (the 0-based index) or `x`/`X` (`None`; the caller quits), reprompting
/// on anything else. No colour: the caller already printed the list.
/// Portage: _emerge/UserQuery.py, query
pub(crate) fn ask_select(n: usize) -> Option<usize> {
    use std::io::Write;
    // Same scoped SIGINT as `ask_yes_no`.
    let _sigint = PromptSigintGuard::install();
    loop {
        print!("\nSelection? ");
        let _ = std::io::stdout().flush();
        let Some(line) = read_prompt_line() else {
            println!("Interrupted.");
            return None;
        };
        let a = line.trim();
        if a.is_empty() {
            // An empty response prefix-matches the first response ("1"),
            // unless `--ask-enter-invalid` rejects it.
            // Portage: _emerge/UserQuery.py, query
            if ask_enter_invalid() {
                print!("Sorry, response '' not understood.");
                continue;
            }
            return Some(0);
        }
        match a.parse::<usize>() {
            Ok(i) if i >= 1 && i <= n => return Some(i - 1),
            _ => {
                if a.eq_ignore_ascii_case("x") {
                    println!("\nQuitting.\n");
                    return None;
                }
                print!("Sorry, response '{a}' not understood.");
            }
        }
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
