//! `--ask` prompts and their terminal handling.

use crate::pretend::*;

/// Real `sys.stdin.isatty()` (`actions.py:3922`'s own `--ask` gate).
pub(crate) fn stdin_is_tty() -> bool {
    // SAFETY: `isatty` is a pure syscall on a well-known, always-valid
    // fd (0); no pointers involved.
    unsafe { libc::isatty(libc::STDIN_FILENO) != 0 }
}

/// Real `_emerge/UserQuery.query` (`portage.output.userquery`'s
/// wrapper), the default `responses=["Yes", "No"]` case: prints the
/// bold `<question>` once (real `print(bold(prompt), end=" ")`), then
/// loops printing the bracketed, coloured choices (real `output.py:
/// 154-155`'s own `PROMPT_CHOICE_DEFAULT`/`PROMPT_CHOICE_OTHER` --
/// green/red) and reading a line, until it gets one that
/// case-insensitively prefix-matches "Yes" or "No" (a bare Enter always
/// matches "Yes") -- an unrecognized response reprints "Sorry,
/// response '...' not understood." and loops again, real's own
/// behavior, rather than giving up on the first bad answer. Returns
/// `true` for "Yes", `false` (after printing `Quitting.`, a caller-
/// side convenience real portage's own callers each print for
/// themselves) for "No" -- and `false` (after `ask_yes_no`'s own
/// `Interrupted.`, with no `Quitting.`) on EOF, a read error, or
/// SIGINT (backlog #240). Real `_emerge/UserQuery.query`
/// (`_emerge/UserQuery.py:74-76`) exits from *inside* `query` on
/// `EOFError`/`KeyboardInterrupt`, so the merge prompt's own
/// `== "No"` comparison and its `Quitting.` never run there either;
/// the caller still exits `128 + SIGINT` through the same arm a "No"
/// takes below.
/// TTY gating happens once, globally, before
/// `ask` is ever `true` at all (`run()`'s own `stdin_is_tty` check
/// right after CLI parsing) -- by the time this runs, stdin is already
/// known to be a real terminal.
///
/// `--ask-enter-invalid` (real `UserQuery.query`'s `enter_invalid`)
/// makes a bare Enter loop back for a real answer instead of matching
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

/// The prompt half of `ask_confirm`, without the decline side effect:
/// the same bold `<question>` + `[Yes/No]` loop, but a "No" (or EOF)
/// answer just returns `false` with nothing printed -- real
/// `UserQuery.query` itself prints nothing on "No" either (each real
/// caller decides what a decline means). Backlog #231 needs this for
/// the "read the news items while calculating dependencies?" prompt
/// (real `_emerge/actions.py:4271-4281`): answering "No" there skips
/// the `eselect` spawn and continues into `action_build`, it does not
/// quit the run.
///
/// Returns `None` on EOF, a read error, or SIGINT (after printing
/// real `UserQuery.query`'s own `Interrupted.`,
/// `_emerge/UserQuery.py:74-76`): the news prompt (backlog #234)
/// turns that into real's `sys.exit(128 + SIGINT)` before
/// `action_build`, while `ask_confirm` folds it into a quiet decline
/// (no `Quitting.`, backlog #240 -- real exits from inside `query`).
///
/// SIGINT handling is scoped to the prompt read itself (backlog
/// #240): `PromptSigintGuard` installs a handler that only records
/// the signal, and `read_prompt_line` turns it into `None`; the
/// previous disposition is restored before this returns, so SIGINT
/// during a merge, a build phase, or a resolve keeps doing exactly
/// what it does today.
pub(crate) fn ask_yes_no(color: &Colorizer, question: &str) -> Option<bool> {
    use std::io::Write;
    // Installed before the first print, so a ^C typed the moment the
    // prompt exists is already handled (the prompt text reaching the
    // terminal strictly implies the handler is active).
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

/// Backlog #240: real `_emerge/UserQuery.query` catches
/// `KeyboardInterrupt` out of `input()` (`_emerge/UserQuery.py:74-76`),
/// printing `Interrupted.` and exiting `128 + SIGINT`. Python installs
/// a SIGINT handler process-wide (its default one raises
/// `KeyboardInterrupt`); portuale instead installs this handler only
/// around the prompt read, so a merge, a build phase, or a resolve
/// keeps the default disposition (death by signal) exactly as today.
/// Set by the handler, consumed by `read_prompt_line`; cleared on
/// every install so a stale flag can never interrupt a later prompt.
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
/// `libc::read` in `read_prompt_line` observes `EINTR`) and restores
/// the previous disposition on drop. Covers the prints too, not just
/// the blocking read -- `read_prompt_line` re-checks the flag on
/// entry, since a ^C that lands while printing consumes its character
/// as the signal and no input byte will ever arrive for it. (One line
/// per raw `read` is a line-discipline guarantee in canonical mode,
/// so an answer written ahead can never be swallowed with the next
/// one -- the existing `Yes\n`+`No\n` tests rely on it.)
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

/// One line from fd 0 for the prompt loops: blocks in a raw
/// `libc::read` (a std `read_line` would retry `EINTR` internally and
/// never observe the scoped SIGINT) on the canonical-mode tty until
/// `\n`, EOF, a read error, or the guard's SIGINT flag. Returns
/// `Some(line)` (newline included when one arrived, like `read_line`;
/// callers trim) or `None` for a bare EOF (`Ok(0)` on an empty
/// buffer, real's `EOFError`), SIGINT (`EINTR` with the flag set,
/// real's `KeyboardInterrupt`), or any other read error (real's
/// `input()` failure arm, same `Interrupted.` treatment the old
/// `Err(_)` arm gave). A partial line pending at EOF is evaluated
/// like `read_line`'s `Ok(_)` arm. Invalid UTF-8 also yields `None`,
/// matching that old arm byte for byte; real decodes with replacement
/// and reprompts, a pre-existing divergence this slice keeps.
/// Line-oriented reader for the interactive prompts' stdin. One call
/// returns exactly one line (including its `\n`); anything past that
/// line is kept in userspace, and any further data already queued in
/// the kernel is drained into that same leftover on the way out.
///
/// Real is protected by Python's buffered `sys.stdin` (`input()` pulls
/// whatever the kernel has and serves the rest from the file object's
/// buffer); a raw `libc::read` is not, so without this the next prompt
/// answer sits in the kernel where any child spawned with inherited
/// stdin (`eselect`/`renice`/`ionice`, all `.status()`) can steal it --
/// the #259 root race, identified from the Z0 watchdog capture.
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
                    // Same as the old `read_prompt_line`: a prompt
                    // SIGINT aborts the read (`Interrupted.` upstream),
                    // any other EINTR just retries.
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
    // prompts are the only readers of it (the #259 leftover must
    // survive across successive `ask_*` calls).
    static PROMPT_LINE_BUFFER: std::sync::Mutex<PromptLineBuffer> =
        std::sync::Mutex::new(PromptLineBuffer::new());
    let mut buffer = PROMPT_LINE_BUFFER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    buffer.read_line(libc::STDIN_FILENO)
}

/// `--ask-enter-invalid` (real `main.py`'s boolean flag, consumed by
/// `UserQuery.query`'s `enter_invalid` argument): process-wide because
/// the flag is CLI-global and only read by the interactive prompts, and
/// threading it through every unmerge/config/deselect helper would buy
/// nothing. `run` resets it at entry and sets it while parsing, so
/// in-process callers can't leak the flag between invocations.
pub(crate) static ASK_ENTER_INVALID: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub(crate) fn ask_enter_invalid() -> bool {
    ASK_ENTER_INVALID.load(std::sync::atomic::Ordering::Relaxed)
}

/// The matching half of `ask_confirm`'s own loop, split out so it's
/// directly unit-testable without any stdin involvement (the loop
/// itself, and `ask_confirm`'s own real `--ask` TTY gate, can't be
/// driven from a test -- feeding it piped input would trip the very
/// `stdin_is_tty` check this backlog item added, matching real
/// portage's own identical limitation). Real `UserQuery.query`
/// (`_emerge/UserQuery.py:63-72`): the response is only matched against
/// the responses when it is non-empty **or** `enter_invalid` is off
/// (`if response or not enter_invalid:`), then by case-insensitive
/// prefix (`response.upper() == key[:len(response)].upper()`), `Yes`
/// first. So a bare Enter (an already-trimmed empty line) matches `Yes`
/// unless `--ask-enter-invalid` is set.
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

/// Real `_emerge/UserQuery.query` with an explicit `responses` list
/// (`action_config`'s `--ask` package menu, `responses = [str(i) for i
/// in 1..=n] + ["X"]`): loops printing `Selection? ` and reading a
/// line, accepting `1`..`n` (returns the 0-based index) or `x`/`X`
/// (returns `None` -> the caller exits 130), reprompting on anything
/// else -- same real "loop until valid, real UserQuery.query" behavior
/// `ask_confirm` now has. No colour here: real's own default `colours =
/// [bold]` only applies to `prompt` (there isn't one for this bare
/// numeric menu -- the caller already printed the numbered list before
/// calling this).
pub(crate) fn ask_select(n: usize) -> Option<usize> {
    use std::io::Write;
    // Backlog #240: same scoped SIGINT as `ask_yes_no` -- real's
    // `UserQuery.query` exits `128 + SIGINT` on `KeyboardInterrupt`
    // for this menu too (the config `Selection?` prompt).
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
            // Real `UserQuery.query`'s prefix match makes an empty
            // response match the first response ("1"), unless
            // `--ask-enter-invalid` rejects it.
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

/// Real `_emerge/unmerge.py:639`: `countdown(int(settings["CLEAN_DELAY"]),
/// ">>> Unmerging")` -- a short pause (default `CLEAN_DELAY=5`) before a
/// real unmerge starts, so a `Ctrl-C` can still abort. `CLEAN_DELAY=0`
/// skips it. Portuale honours the `CLEAN_DELAY` env var directly (same
/// "read the var at the CLI boundary" shortcut as `PORTAGE_TMPDIR` etc.).
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
