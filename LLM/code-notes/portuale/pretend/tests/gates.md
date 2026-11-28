# crates/portuale/src/pretend/tests/gates.rs
Display and early-return gates for non-pretend runs.

## fn nonpretend_merge_list_shown_only_when_portage_shows_it
- Backlog #185: Portage's `_emerge/actions.py:464-469` (`mergelist_shown` branch) plus the m185 container probe (Portage 3.0.81.3, `emerge --oneshot --usepkgonly probe/probe-a` in six shapes; gating identical in 3.0.82.2 by source read): a non-`--pretend` run shows the merge list only with `--ask`, `--tree` or `--verbose`, and never with `--quiet` unless `--ask` is also given. `--pretend` always shows it.
- (pretend, ask, tree, verbose, quiet) -> shown.

## fn autounmask_only_reshows_the_merge_list_under_pretend
- Backlog #271: Portage's `_display_autounmask` re-shows the merge list via `_show_merge_list` (`depgraph.py:10625`, `:10686`, `:10488`) even on the `--autounmask-only` early-return path (`actions.py:456-458`) -- whenever it has changes to report.
- (pretend, autounmask_only, has_changes) -> re-shown.
- No changes (a plain package): still nothing, like Portage (no `_show_merge_list` call fires from an empty change loop). The control path shows the list through the normal gate, not this carve-out.
- Non-`--pretend` keeps the old suppression (unprobed this slice; Portage has no pretend gate here, so this is a cut).

## fn nothing_to_merge_fires_only_for_a_shown_nonpretend_empty_plan
- Backlog #225: Portage's `_emerge/actions.py:464-521` -- the early return fires only inside the display-flag branch of a real (non-`--pretend`) run, and only when `mergecount == 0`.
- (pretend, show_merge_list, mergecount) -> early return.
- A plain run (no display flags) never takes this branch: `mergecount` stays `None` and `Scheduler` still writes (per #179, possibly empty). Anything merge-bound proceeds to the prompt / scheduler.

## fn selective_noop_deferred_only_for_selective_non_oneshot_world_runs
- Backlog #232: Portage's `_emerge/actions.py:514-516` -- the deferral (no message, rotation + world record, no `Scheduler`) fires only when the run is `selective` without `--oneshot` / `--onlydeps` and a world-candidate favorite exists. The second parameter is Portage's folded gate variable (`actions.py:257`: `--oneshot` or `--onlydeps`), so one `true` covers both flags.
- (selective, oneshot_or_onlydeps, has_world_candidates) -> deferred.
- `--oneshot` -- or `--onlydeps`, which Portage folds into the same gate variable (`actions.py:257`) -- keeps the deferral at bay (the #225 shape), with or without world candidates. A non-selective all-noop run is the plain `Nothing to merge` shape even with world candidates around. No world candidate (bare args, or only `@world`/`@system`) is the plain `Nothing to merge` shape even when selective.

## fn nonpretend_resolution_display_splits_header_from_notice_under_nodeps
- Backlog #185 fix round 1: Portage's `_emerge/depgraph.py::_start_resolution_display` (`depgraph.py:12086-12132`) prints the header without consulting `--nodeps` (the `spinner.mode = QUIET` assignment at `:12129-12130` comes after the header block), while the `Calculating dependencies ... done!` notice and the timing report are suppressed under `--nodeps` (QUIET spinner: `end_notice()` reports nothing to complete) or `--quiet`.
- (ask, tree, verbose, quiet) -> header shown; (quiet, nodeps) -> notice + timing shown.
- The three review shapes (`--verbose` in each): `--nodeps` non-pretend -> header shown, calc/timing hidden; plain -> both shown; `--quiet` -> neither.

## fn top_level_all_masked_abort_prints_the_notice_on_and_off_pretend
- Backlog numbers removed from the comment (original text): Plan #351 S7 (351-ABORT): a non-`--pretend` run that aborts on a top-level all-masked target prints the resolution notice on its way out; the `--pretend` arm stays silent (Q6) and every other resolve error shape is out of scope. Oracle: Portage 3.0.82.2 A cells `LLM/evidence/2026-10-15-346-s0/a/` (`a01` and `a02` -p both print; backlog #406).
- Backlog numbers removed from the comment (original text): A plain miss prints the notice too (backlog #470); the USE-flag variant keeps today's silence.

## fn abort_preamble_opens_with_the_banner_blank_line
- Backlog numbers removed from the comment (original text): Backlog #453 D: the mid-resolve abort preamble opens with Portage's leading blank line (`_start_resolution_display` writes `"\n" + banner`). Oracle: Portage 3.0.82.2 non-tty capture of `--pretend dev-libs/missingdep` (staged fixtures).
- Backlog numbers removed from the comment (original text): Backlog #621: under `--quiet` Portage prints no resolution display at all -- `_start_resolution_display` skips the banner (`depgraph.py:12219`: `"--quiet" not in myopts`) and forces the spinner QUIET (`:12259-12260`), so `end_notice()` is false and neither the notice completion nor the report prints. Oracle: Portage 3.0.83 `--pretend --quiet dev-libs/missingdep` on the fixture tree: empty stdout, rc 1.
- Backlog numbers removed from the comment (original text): Backlog #453 A: with colour on, the banner is `darkgreen` and the cut seconds keep Portage's empty escape pair (determinism: the seconds themselves are still omitted). Oracle: the `--color=y` pipe capture of `--pretend dev-libs/maskedpkg`.
- Backlog numbers removed from the comment (original text): Backlog #614: `--debug` sets PORTAGE_DEBUG=1, which puts the spinner in QUIET mode; neither the notice completion nor the report is printed, only the banner.

## fn timing_line_prints_seconds_when_given
- Backlog numbers removed from the comment (original text): Backlog #803: on a terminal the timing line carries the real seconds (`took 1.33 s`) instead of the cut `took  s` that read as broken; the cut forms stay for non-terminal runs (the contract suite).

## fn pretend_changes_show_header_only_for_completed_pretend_text_runs
- Backlog numbers removed from the comment (original text): Backlog #462: a `--pretend` run whose resolve completed with autounmask changes prints the resolution header before the rows (Portage's `_show_merge_list` before the change blocks). `--json` keeps stdout machine-readable; an aborted resolve keeps its own preamble arms; a non-pretend run goes through the existing `!pretend` arm.
