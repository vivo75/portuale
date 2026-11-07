# Cluster C diagnosis (2026-11-06): #609 #611 #613

Portage 3.0.83. Captures: `/tmp/claude-1000/-home-vivo-repo-PORTUALE-portuale/3c60fa85-0c1a-42e7-b42c-5a4d843557bb/scratchpad/cap/<cell>.{portage,portuale}.txt`.
Argv: `env -u EMERGE_DEFAULT_OPTS PORTAGE_CONFIGROOT=<cfg> ROOT=$FX ... --ignore-default-opts --getbinpkg=n --usepkg=n --color=n`.
D = `_emerge/depgraph.py`. All closable; none is a Portage quirk (one unconfirmed candidate at the end).

## #609 — reproduced, bigger than filed
Portage `_create_graph` pops `_dep_stack` LIFO and returns 0 on first failure (D:3269-3284; `_add_dep` D:3535 with escapes D:3411-3470, 3478-3486).
`--backtrack=0 r592atop`: Portage never walks `ax-2`, no slot conflict; portuale prints one.
Extra shapes: `--autounmask=n r594ctop/etop/qtop`, `r594htop` — portuale prints an extra `maskeddep` block; q/h count 1/20 vs Portage 0/20.
Cause: `fail_fast_truncate` (`resolve/replay.rs:122-226`) replays the LIFO walk but is only called from `run_breakage_clean_pass` (`replay.rs:112`); `has_report` ignores `masked_deps` and clears them (`replay.rs:184`). Cross-parent count: `maybe_missing_dep_backtrack` keeps first trigger in portuale order; `trigger_preempted_by_use_miss` handles one parent only.
Fix: (1) call it on every final Aborted result (skip complete mode / allow_unsatisfied escapes); (2) understand `masked_deps`; (3) arm `missing_dep_trigger` only when the replay's first failure is that edge.
Tests: fixtures `r592a*`, `r594q*` exist; add `r594h*`; CASES + pinned tests + `tests/tests_first_unsatisfied_dep.rs`. Risk medium; L0 on bed.

## #611
(a) masked_with_iuse (D:6736-6750, 6918-6929, 7015-7043): `use_unsat_candidates_for_atom` (`portage-repo/src/use_unsat.rs:128-321`, skips masked at `:199-201`) → return None when a masked version has all required flags. Callers: `unresolved_dep.rs:332,:673`, `top_level_unsatisfied.rs:320`, `solver_bridge.rs:4480`. Needs #609 truncation to keep `masked_deps`.
(b) `--autounmask-keep-keywords=n` (D:7474-7586): `relax_until_usable` (`resolve/pretend.rs:1187-1252`, `usable` `:1840-1867`) ignores missing IUSE → a version lacking a required flag is not usable when the atom has USE deps. portuale's "exists but is masked by KEYWORDS" note is its own invention (`display/entry_line.rs:539`, `top_level_unsatisfied.rs:355`).
(c) package.mask file + comment (`show_masked_packages` D:12632-12690): four render sites (`problem_reports.rs:127-141`, `resolve_phases.rs:17-22`, `display/entry_line.rs:478-485`, `masked_deps.rs:958`); share `portage_profile::masking_file_comment`; add license line. Corpus drift: review then bless.
(d) mostly done by #662; left: license reason (`!!! The following installed packages are masked` D:8995-8999, 11414-11432). Keywords never reported for installed (`Package.py:506-513`) — #662 residue wording wrong.
(e) `@selected` row missing in chains (`argument_label` `resolve/ctx.rs:39-41`; D:6272-6470): carry nested set path.
New: Portage prints USE-change block (D:11296) before masked block (D:11435); portuale reverse (`pretend/mod.rs:601` vs `:610`). Only `r594qtop` probed.

## #613
Default options: Portage 0/20, abk0a-3, `USE="x*"`, slot-conflict block; portuale 1/20, abk0a-2, `USE="y*"`, skipped-update warning, `x y`. With `-aby`: Portage 2/N, portuale 1/N.
Cause: `consumer_pin_conflict_may_feed_back` (`feedback/reverse_dep_pins.rs:122-136`) refuses once USE folded; driver break (`driver.rs:132-176`) covers only SlotConflict/MissingDep, not RevDep.
Fix candidates: (i) extend the break to RevDep when pass grew an autounmask change reached before stop; (ii) allow consumer-pin conflict into the multi-choice step. Check `t8x` counter-shape. Pins: abk0d with world-B ROOT; flip two 1/2 pins (test_emerge_pretend_contract.py:811, :10083, :10141) to 2/2. Risk high; do last.

## Order recommended: #611a → #609 → #611c/e → #611b → #613.
Roots: R1 (#609), R2 (#611a/b), R3 (display port), R4 (#613).
Unconfirmed Portage slip: `get_masking_status(..., use=self._pkg_use_enabled)` passes the method not its result (D:11418-11420).
