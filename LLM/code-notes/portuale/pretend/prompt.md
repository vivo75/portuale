# crates/portuale/src/pretend/prompt.rs
`--ask` prompts and their terminal handling.

## fn stdin_is_tty
- Ports Portage's `sys.stdin.isatty()` (`actions.py:3922`'s `--ask` gate).

## fn ask_confirm
- Ports Portage's `_emerge/UserQuery.query` (`portage.output.userquery`'s wrapper), the default `responses=["Yes", "No"]` case: prints the bold `<question>` once (Portage's `print(bold(prompt), end=" ")`), then loops printing the bracketed, coloured choices (Portage's `output.py:154-155` `PROMPT_CHOICE_DEFAULT`/`PROMPT_CHOICE_OTHER` -- green/red) and reading a line, until one case-insensitively prefix-matches "Yes" or "No" (a bare Enter always matches "Yes") -- an unrecognized response reprints "Sorry, response '...' not understood." and loops again, Portage's own behavior, rather than giving up on the first bad answer. `false` (after printing `Quitting.`) is a caller-side convenience Portage's own callers each print for themselves. On EOF, a read error, or SIGINT returns `false` (after `ask_yes_no`'s own `Interrupted.`, with no `Quitting.`) (backlog #240). Portage's `_emerge/UserQuery.query` (`_emerge/UserQuery.py:74-76`) exits from *inside* `query` on `EOFError`/`KeyboardInterrupt`, so the merge prompt's own `== "No"` comparison and its `Quitting.` never run there either; the caller still exits `128 + SIGINT` through the same arm a "No" takes below. TTY gating happens once, globally, before `ask` is ever `true` at all (`run()`'s own `stdin_is_tty` check right after CLI parsing).
- `--ask-enter-invalid` (Portage's `UserQuery.query`'s `enter_invalid`) makes a bare Enter loop back for an answer instead of matching "Yes".

## fn ask_yes_no
- The same bold `<question>` + `[Yes/No]` loop, but a "No" (or EOF) answer just returns `false` with nothing printed -- Portage's `UserQuery.query` itself prints nothing on "No" either (each Portage caller decides what a decline means). Backlog #231 needs this for the "read the news items while calculating dependencies?" prompt (Portage's `_emerge/actions.py:4271-4281`): answering "No" there skips the `eselect` spawn and continues into `action_build`, it does not quit the run.
- Returns `None` on EOF, a read error, or SIGINT (after printing Portage's `UserQuery.query`'s own `Interrupted.`, `_emerge/UserQuery.py:74-76`): the news prompt (backlog #234) turns that into Portage's `sys.exit(128 + SIGINT)` before `action_build`, while `ask_confirm` folds it into a quiet decline (no `Quitting.`, backlog #240 -- Portage exits from inside `query`).
- SIGINT handling is scoped to the prompt read itself (backlog #240): `PromptSigintGuard` installs a handler that only records the signal, and `read_prompt_line` turns it into `None`; the previous disposition is restored before this returns, so SIGINT during a merge, a build phase, or a resolve keeps doing exactly what it does today.

## static PROMPT_SIGINT
- Backlog #240: Portage's `_emerge/UserQuery.query` catches `KeyboardInterrupt` out of `input()` (`_emerge/UserQuery.py:74-76`), printing `Interrupted.` and exiting `128 + SIGINT`. Python installs a SIGINT handler process-wide (its default one raises `KeyboardInterrupt`); portuale instead installs this handler only around the prompt read, so a merge, a build phase, or a resolve keeps the default disposition (death by signal) exactly as today.

## struct PromptSigintGuard
- `read_prompt_line` re-checks the flag on entry, since a ^C that lands while printing consumes its character as the signal and no input byte will ever arrive for it. (One line per raw `read` is a line-discipline guarantee in canonical mode, so an answer written ahead can never be swallowed with the next one -- the existing `Yes\n`+`No\n` tests rely on it.)

## fn read_prompt_line / struct PromptLineBuffer
- Returns `Some(line)` (newline included when one arrived, like `read_line`; callers trim) or `None` for a bare EOF (`Ok(0)` on an empty buffer, Portage's `EOFError`), SIGINT (`EINTR` with the flag set, Portage's `KeyboardInterrupt`), or any other read error (Portage's `input()` failure arm, same `Interrupted.` treatment the old `Err(_)` arm gave). Invalid UTF-8 yields `None`, matching that old arm byte for byte; Portage decodes with replacement and reprompts, a pre-existing divergence this slice keeps.
- Portage is protected by Python's buffered `sys.stdin` (`input()` pulls whatever the kernel has and serves the rest from the file object's buffer); a raw `libc::read` is not, so without the leftover buffer the next prompt answer sits in the kernel where any child spawned with inherited stdin (`eselect`/`renice`/`ionice`, all `.status()`) can steal it -- the #259 root race, identified from the Z0 watchdog capture.
- The prompt text reaching the terminal strictly implies the handler is active.
- The #259 leftover must survive across successive `ask_*` calls (the buffer is process-wide because fd 0 is process-wide).

## static ASK_ENTER_INVALID
- Portage's `main.py` boolean flag, consumed by `UserQuery.query`'s `enter_invalid` argument. Threading it through every unmerge/config/deselect helper would buy nothing.

## fn classify_yes_no
- Split out because the loop itself, and `ask_confirm`'s own `--ask` TTY gate, can't be driven from a test -- feeding it piped input would trip the very `stdin_is_tty` check this backlog item added, matching Portage's own identical limitation. Portage's `UserQuery.query` (`_emerge/UserQuery.py:63-72`): the response is only matched against the responses when it is non-empty **or** `enter_invalid` is off (`if response or not enter_invalid:`), then by case-insensitive prefix (`response.upper() == key[:len(response)].upper()`), `Yes` first. So a bare Enter (an already-trimmed empty line) matches `Yes` unless `--ask-enter-invalid` is set.

## fn ask_select
- Ports Portage's `_emerge/UserQuery.query` with an explicit `responses` list (`action_config`'s `--ask` package menu, `responses = [str(i) for i in 1..=n] + ["X"]`): returns the 0-based index, or `None` on `x`/`X` (the caller exits 130). Same "loop until valid" behavior `ask_confirm` has. No colour here: Portage's own default `colours = [bold]` only applies to `prompt` (there isn't one for this bare numeric menu -- the caller already printed the numbered list before calling this).
- Backlog #240: same scoped SIGINT as `ask_yes_no` -- Portage's `UserQuery.query` exits `128 + SIGINT` on `KeyboardInterrupt` for this menu too (the config `Selection?` prompt).
- Loops printing `Selection? ` and reading a line, accepting `1`..`n` (returns the 0-based index) or `x`/`X` (returns `None` -> the caller exits 130), reprompting on anything else -- same Portage "loop until valid, Portage UserQuery.query" behavior `ask_confirm` now has.

## fn clean_delay_countdown
- Ports Portage's `_emerge/unmerge.py:639` `countdown(int(settings["CLEAN_DELAY"]), ">>> Unmerging")` (default `CLEAN_DELAY=5`). Same "read the var at the CLI boundary" shortcut as `PORTAGE_TMPDIR` etc.
- `countdown(int(settings["CLEAN_DELAY"]), ">>> Unmerging")` -- a short pause (default `CLEAN_DELAY=5`) before a real unmerge starts, so a `Ctrl-C` can still abort. `CLEAN_DELAY=0` skips it.
