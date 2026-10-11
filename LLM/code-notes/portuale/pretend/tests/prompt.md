# crates/portuale/src/pretend/tests/prompt.rs
Prompt EOF/SIGINT behavior, the --ask privilege offer, and the prompt line buffer.

## fn ask_read_news_eof_exits_before_resolve_like_portage
- Backlog #234 (a): Portage's `_emerge/UserQuery.query` (`_emerge/UserQuery.py:74-76`) prints `Interrupted.` and exits `128 + SIGINT` on EOF -- the news prompt's `sys.exit` fires before `action_build`. EOF here is a VEOF byte (`Ctrl-D`) written to the pty master: the slave line discipline turns it into a zero-byte read, exactly how a terminal user types EOF into Portage's `input()` (which then raises `EOFError`). The master must stay open until the child exits -- closing it first makes the slave stop being a tty at all (Linux drops the termios once the last master closes), so the run would die in the up-front `--ask` isatty gate instead of reaching the prompt. Must print `Interrupted.`, exit 130, and never reach the resolve (`Calculating dependencies` stays absent); before the fix the run skipped the spawn, resolved, and only exited 130 at the later merge prompt.
- The pty buffers until the child prompts -- no synchronisation needed.

## fn ask_merge_sigint_prints_interrupted_and_exits_130_like_portage
- Backlog #240: a `^C` at the merge prompt exits from inside `query`, so the prompt's `== "No"` check and its `Quitting.` never run.

## fn ask_read_news_sigint_exits_before_resolve_like_portage
- Backlog #240, the news prompt (`_emerge/actions.py:4266-4288`): a `^C` there exits before `action_build`, like the #234 EOF arm, with no eselect spawn and no resolve.

## fn ask_config_select_sigint_prints_interrupted_and_exits_130_like_portage
- Backlog #240 follow-up: the config `Selection?` menu goes through the same `UserQuery.query` in Portage (`_emerge/actions.py:746` through `_emerge/UserQuery.py:74-76`), so a `^C` there prints `Interrupted.` and exits `128 + SIGINT` by exit -- `X`'s own `Quitting.` (`actions.py:747-748`) never runs. Two fake vdb entries make `dev-libs/seltest` match twice, reaching the menu with no ebuild work at all; same `^C`-through-the-pty delivery and prompt-synchronized timing as the merge-prompt test above.

## fn sigint_during_merge_still_dies_by_signal_like_before
- Backlog #240 scoping pin: the guard lives strictly inside the prompt functions, so a `^C` outside any prompt keeps the default disposition -- death by signal, exactly as before this slice. A promptless `emerge --oneshot` merge of `dev-libs/schedok` runs ebuild phases for well over a second after printing `>>> Emerging`, so a `^C` synchronized on that marker deterministically lands outside any prompt: no `Interrupted.` line, no exit code, `signal() == Some(SIGINT)`.

## fn ask_merge_eof_prints_interrupted_without_quitting_like_portage
- Backlog #240's second fix: EOF at the merge prompt printed a spurious `Quitting.` after `Interrupted.` (the `None` arm fell into the decline arm). Portage exits from inside `UserQuery.query` (`_emerge/UserQuery.py:74-76`), so only `Interrupted.` prints -- same VEOF delivery as the #234 news EOF test, at the later merge prompt instead.

## fn ask_pretend_offer_prompts_on_stdout_for_a_write_action
- Backlog #246: Portage's `_emerge/actions.py:3983-4006` offers a non-privileged caller `Would you like to add --pretend to options?` when `--ask` is set, instead of refusing outright. `emerge --ask --oneshot dev-libs/schedok` with an unowned ROOT must print `This action requires superuser access...` and the question on stdout (Portage's `writemsg_stdout` + `print(bold(prompt))`; only the `[Yes/No]` choices go to stderr, via readline's `input()` -- host probe 2026-09-29, `/usr/sbin/emerge` 3.0.82.2 as uid 1000, secpass 0, verbatim pty bytes for the No run: `This action requires superuser access...`, `[01mWould you like to add --pretend to options?[39;49;00m `, (stderr:) `[32;01mYes[39;49;00m/[31;01mNo[39;49;00m] `, rc 130, nothing else printed).
- The answer here is "No" (rc 130); the arms are pinned separately below.

## fn ask_pretend_offer_no_exits_130_like_portage
- Backlog #246, the "No" arm: Portage returns `128 + SIGINT` with nothing else printed (`actions.py:3997-4003` -- the `== "No"` comparison just returns, no notice, no resolve). Answering `n` must exit 130 with no `Interrupted.` line (that line is the EOF/SIGINT arm's, #240) and must never reach the resolve (`Calculating dependencies` stays absent); the old outright refusal (`superuser access is required` on stderr) must be gone on this path.

## fn ask_read_news_without_eselect_prints_portages_hint
- Backlog #231 (c): Portage catches the spawn's `OSError` (eselect missing) and prints `Please install eselect to use this feature.` (`actions.py:4284-4287`). `PATH` holds only an empty dir, so the spawn must fail; the "Yes" is still consumed and the declined merge prompt still exits 130.

## fn classify_yes_no_prefix_matches_case_insensitively_and_rejects_garbage
- --ask-enter-invalid: a bare Enter is not a match, a real answer still is (Portage UserQuery.query's own `if response or not enter_invalid` gate).

## fn prompt_line_buffer_drains_the_kernel_so_a_child_cannot_steal_the_next_answer
- Part 2 of the same race: after the first line is served the kernel must hold nothing -- otherwise a child that inherits the fd and reads it (Portage's helper shape) steals the next answer and the next prompt hangs until the #259 watchdog fires. That is exactly the Z0 capture: both answers pre-written, the news prompt answered, the merge prompt hung with no input left.
- A pty, not a pipe: the canonical line discipline is what queues the second answer in the kernel in the Portage capture.
- The kernel must be drained: a non-blocking read gets EAGAIN, which is exactly what a stealing child would see.

## fn pty_watchdog_kills_a_hung_child_and_fails_the_test
- Backlog #259's proof: a child that never exits is killed at the bound and the test fails with the `watchdog:` prefix the race chase greps for -- instead of hanging the whole `cargo test` run the way the #259 incident did (20 hours on a pty-blocked child).
- `sleep` is the stand-in for the #259 child stuck in `read_prompt_line`'s blocking read: it will never exit on its own. A 1 s bound keeps the test itself fast.
- SAFETY: this is the only test setting the watchdog variable, and the value is read once per `wait_pty_*` call in this thread's own child. SAFETY: paired with the `set_var` above in the same test.

## fn ask_declined_writes_no_resume_list
- #179: Portage's `_emerge/actions.py:525-536` returns `128 + SIGINT` on a "No" answer before `Scheduler` runs -- a declined run writes nothing (and rotates nothing: the rotation sits after the prompt). `--ask` needs a real TTY (the up-front isatty gate), so the decline is answered through a pty; the stale multi-item list must come back byte-identical.
- The pty buffers until the child prompts -- no synchronisation needed.

## fn ask_pretend_offer_yes_continues_as_pretend_like_portage
- Backlog #246, the "Yes" arm: Portage sets `opts["--pretend"] = True`, pops `--ask`, and the run continues as a pretend run (`actions.py:4004-4006`). Host probe 2026-09-29 (`printf 'y\n' | script -qec "/usr/sbin/emerge --ask --depclean"`): after the offer the run prints the depclean notice, `Calculating dependencies ... done!`, and continues -- with no second prompt, since `--ask` is gone. Here answering `y` must resolve the pretend merge list (`dev-libs/schedok-1.0`) with rc 0 and must not prompt again (`Would you like to merge these packages?` stays absent -- `--ask` is no longer in effect).

## fn ask_true_spelling_hits_the_tty_gate_like_portage
- Backlog #234 (b): `--ask=True` parses as ask-on (Portage's `true_y_or_n` choices + `in true_y` normalization, `main.py:321-322,802-805`), so a non-terminal stdin hits Portage's `actions.py:3920-3926`'s gate exactly like a bare `--ask` (before the fix `--ask=True` died as an unrecognized option).

## fn ask_without_a_tty_exits_immediately_instead_of_prompting
- Portage's `actions.py:3920-3926`: "--ask" is rejected outright, up front, when stdin isn't a real terminal -- there would be nobody able to answer a prompt piped from a non-interactive source. Checked right after CLI parsing finishes, before any atom resolution, so this fires even for an atom that wouldn't otherwise resolve.

## fn prompt_line_buffer_returns_exactly_one_line_per_call
- #259's root race, chased from the Z0 watchdog capture (2026-09-30): `read_prompt_line` is a raw `libc::read` with no userspace leftover, so a multi-line chunk becomes ONE answer and a line left in the kernel is stealable by any child spawned with inherited stdin (`eselect`/`renice`/`ionice` all use `.status()`). Portage is protected by Python's buffered `sys.stdin` (`input()` drains the kernel and serves leftovers from userspace). Part 1: one line per call.
