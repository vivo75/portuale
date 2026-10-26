# crates/portuale/src/pretend/merge_list.rs
The counts and summary lines under the merge list, and the redisplay before a merge.

## fn show_merge_list_summary
- The `Total: …` counters line ports `output.py::display`'s `if self.conf.verbosity == 3: self.print_verbose(...)` (with a leading blank line); `--quiet` forces verbosity to 1, so `-pvq` suppresses the line `-pv` shows. Suppressed with an empty aborted list (Portage never calls `display()` there, so no counters exist); the cycle partial counts over its own rows only.
- Backlog #206 S2: the circular display runs forced-verbose, so the counters count the rendered rows (`rendered_rows` from the tree walk), not the entries: Portage's `_PackageCounters` accumulate per displayed merge row; `nomerge`, already-installed and disclosure rows contribute nothing, matching Portage's `set_pkg_info` gating.
- Backlog #185: the single blank line after the list is `actions.py:526`; `show_merge_list` (not the bare gate) is the condition since Portage's `print()` only runs when the list branch ran.
- Portage `output.py::display`: `if self.conf.verbosity == 3: self.print_verbose(...)` -- the `Total: …` counters line, printed after every entry (and blocker) line, only under `-v`, for the tree/columns/flat layouts alike. Portage emits `f"\n{self.counters}\n"` (a leading blank line).
- Backlog #185: Portage `_emerge/actions.py:526` prints one blank line after the merge list, before the `--ask` prompt (or the scheduler, when nothing prompts).
- `--ask` needs none here: `ask_confirm` already opens with that same single blank line (Portage's `print()` before `UserQuery.query`), so printing another would double it.

## fn render_tree_rows
- Backlog #131 S1: the tree-mode order is Portage's reversed tree-mode retlist; one extra schedule+select pass per `--tree` run, invisible to the flat list.
- Backlog #245: cross-root cycle partial only -- Portage hands `handler.merge_list` (the stuck remainder drained in digraph insertion order) straight to `display` without re-serializing, and `print_tree` walks the reverse of its retlist; the single-root cycle keeps the re-serialization.

## fn render_merge_rows
- Backlog #68/#72 B2: rows whose replacement waits on their owner print right after that package line (Portage appends them to `print_msg` at the solved blocker's merge-list position, `output.py:119-121`).
- Backlog #19 Slice 4: the aborted partial list carries no `NoVisibleCandidate` entries (masked/unsat partials are empty; the cycle remainder is merge-bound). Non-abort resolves (including `Complete` graphs that still carry NVC entries, e.g. build-root-only misses) render those lines in the main loop as before. Slice 5 reshapes the re-emit into the `AbortReason` error block following the partial list. Backlog #185: pretend-only, since the unconditional resolution-phase block took over the non-`--pretend` preamble -- Portage's resolution-phase prints run before success/failure is known (`actions.py:460-462`).
- Backlog #135 (c) (O7: un-suppress): Portage prints the merge-list preamble and the timing line and then aborts (`These are the packages ...` + `Calculating dependencies ... done!` + `Dependency resolution took N s (backtrack: N/M).` + blank, all stdout, before the abort block on stderr). Determinism cut (lessons-of-backlog-ops: nondeterministic values ride `--json` only, never stdout; the byte-identical repeated-run invariant is a jointly-owned gate): the wall-clock seconds are omitted. O7's "timing line" is the signal half, not the seconds; re-open with the owner to print Portage seconds.
- Backlog #453 D: the mid-resolve abort preamble opens with Portage's leading blank line (`_start_resolution_display` writes `"\n" + banner`, `depgraph.py:12122`); the non-tty capture of `--pretend dev-libs/missingdep` confirmed it. The top-level all-masked arm already had it via `show_resolution_header_with_counts`.
- Backlog #59 (b): Portage prints the timing line on every non-quiet run.
- Backlog #244: Portage's `display_problems` prints the slot-conflict block before the leftover `_unsatisfied_deps_for_display` item (`depgraph.py:11113-11280`).
- Backlog #80: unresolved rows with no owner entry join the blocker group (flat and tree alike -- Portage's `Display.blockers` is mode-independent). Unresolved rows are never column-suppressed, so no `columns` gate.
- Backlog #195 (round 2): a dependency the resolver collected into a parent USE flip (record-and-fail, Portage `_apply_parent_use_changes` dropping the item from `_unsatisfied_deps_for_display`) renders no row at all -- the USE-changes block below is its whole output, and the `!!! note:` fallback in `print_entry_line` would duplicate it.
- Portage `Display.print_blockers()`: the collected `[blocks B ...]` lines, printed as one group after every package line and before the counters.

## fn print_json_result
- The `--json` output, which replaces the merge list.
- `has_autounmask_changes` is bound once next to the `--autounmask-only` re-show gate (backlog #271).
- Backlog #62: the slot-conflict rule; `--fetchonly`/`--fetch-all-uri` carve-out (still portuale-unimplemented options, rc 2 before reaching here).
- Backlog #19 Slice 4: gated like the text path; the `Aborted` arm there fires first, so the `--json` arm only matters for `--json`.
- Same non-zero-on-autounmask-changes contract as the text path below (a `--json` consumer's exit-code check must see the "config still needs editing" signal too). `--autounmask-only` stays exit 0 (its whole point is "just show the changes").
- `--autounmask-only` is escape 1 (its whole point is "just show the changes", exit 0); backtracking disabled plus `--buildpkgonly`/`--nodeps` is escape 2 (`_accept_blocker_conflicts`). See the text-path gate for the full grounding and the `--fetchonly`/`--fetch-all-uri` carve-out.

## fn show_pprovided_notice
- Ports `display_problems()`' block for a directly-requested atom matching `package.provided` (`depgraph.py:11192-11235`), matching Portage's `display_problems()`-before-`display()` order. A documented divergence: the `@world`/`@selected` "A) B) C)" solution text is never reached (see `GraphResult::pprovided_atoms`).

## fn show_resolution_header
- Backlog #185 fix round 1: `backtrack_depgraph`'s `finally:` completes the notice plus the timing line after resolving, success or failure alike.
- Provenance: container probe m185 (Portage 3.0.81.3, `localhost/test-portuale:latest`, `emerge --oneshot --usepkgonly probe/probe-a` in six shapes; the gating is identical in 3.0.82.2 by source read) plus the 3.0.82.2 source for exact text. The m185 probe (3.0.81.3) shows two spaces before `...`; the 3.0.82.2 source (`f"{notice} ..."` + `" done!\n"`) shows one -- ported from the pinned source, pending the coordinator's 3.0.82.2 bed confirmation (see the m185 report).
- `--fetchonly` is unimplemented here (rc 2), so only the `built` arm is reachable.
- `_show_resolution_report` (`depgraph.py:12134-12146`): `Dependency resolution took N.NN s (backtrack: B/M).`
- Portage `depgraph.py:: _start_resolution_display` prints the header (when `--ask`, `--tree` or `--verbose` is given and `--quiet` is absent -- `--nodeps` is NOT consulted, the `spinner.mode = QUIET` assignment at `depgraph.py:12129-12130` comes after the header block) and starts the spinner notice before resolving, and `backtrack_depgraph`'s `finally:` completes the notice plus the timing line after (success or failure alike). `--quiet` suppresses everything; `--nodeps` suppresses only the notice and the timing line (QUIET mode: `end_notice()` reports nothing to complete).
- Portage `_start_resolution_display` (`depgraph.py:12087-12127`): a leading blank line, then `These are the packages that would be ...` -- `in reverse order` for `--tree` (a plain `:` under `--unordered-display`), `fetched` under `--fetchonly` and `built` under `--buildpkgonly` (`--fetchonly` is unimplemented here, rc 2, so only the `built` arm below is reachable), else `merged, in order:` -- then a blank line.
- Portage `stdout_spinner` STATIC mode (piped stdout, Portage `stdout_spinner.py:105-145`): `begin_notice("Calculating dependencies")` writes the notice plus ` ...`, and `end_notice()` completes it with ` done!`.
- Backlog #351 S7: the body is `show_resolution_header_with_counts(opts, restarts, max)` so the top-level all-masked abort — which exits via `Err` before any `GraphResult` exists — prints the identical bytes with zero restarts over the run's backtrack max (the argument dies on first dequeue, S0 a01/a04 show `0/20`); the `--quiet`/`--nodeps`/`--ask` gates are shared, not re-decided. The `--pretend` all-masked arm stays silent per Q6 (see `top_level_abort_notice_shown`).
- Backlog #453 A: with colour on, the banner is `darkgreen` (`depgraph.py:12122-12127` wraps the whole line) and the timing seconds keep Portage's escape pair around the cut (`took \x1b[32m\x1b[39;49;00m s`); see `timing_line`. The mid-resolve abort preamble (`abort_preamble`) shares both.
- Backlog #462: a `--pretend` run whose resolve completed with autounmask changes prints the same header before the rows -- Portage's `_display_autounmask` calls `_show_merge_list()` per change (`depgraph.py:10726` etc.), which runs `display()` (header + rows) before the change blocks, even though the run then fails. `pretend_changes` threads that arm through the shared gates (`--quiet` still suppresses everything, `--nodeps` still drops notice + timing, `--tree` still says "in reverse order"); aborted resolves keep their own preamble arms and `--json` stays machine-readable (no header on the early-return JSON path).

## fn show_merge_list_summary circular warning (#517 (d))
- Portage warns about a bad `PORTAGE_CIRCULAR_MAX_USE_FLAGS` at handler construction, before the merge-list rows print — so the forced re-display warns ahead of its rows, not inside the error block. The `json` report path has no rows; its branch warns for itself.

## fn render_tree_rows circular order (#517 (b))
- Portage's circular re-display walks `handler.merge_list` (the reduced leaf-drain order) through the same `display()` reversal; `display_entries` (the abort `partial`) already is that reduced order, so it goes in as-is and `print_tree`'s reversal produces Portage's walk. Portage: `depgraph.py`, `_show_circular_deps`.
