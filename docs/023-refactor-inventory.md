# 023 refactor inventory — `backtracking_resolve` variable scopes and write sites

A0 slice of `docs/023-backtracking_resolve.md`. Ground truth for the A1–A3
struct split. Generated 2026-09-11 by `grep`/`awk` over
`rust/portage-repo/src/lib.rs` at merge `57ad2f5` (main `cea80b3` + plan
commit), with every write-site claim confirmed by reading the site.
Function: `backtracking_resolve`, lines **14873–17731** (~2859 lines).

Loop landmarks:

| landmark | lines |
|---|---|
| `'backtrack: loop {` opens | 15083 |
| per-pass lets (queue setup) | 15088–15234 |
| `'queue` walk | 15236–~17198 |
| post-walk dedup (`mergebound_cp_slots`, blocker attach, REQUIRED_USE early-`Err`) | 17209–17255 |
| `nvc_count` closure + decision chain (steps 1–7) | 17250–17525 |
| assembly (rebuild passes, trace dump, merge-order sort, autounmask coalesce, residuals, masked chains, `abort_outcome`, `GraphResult`) | 17527–17729 |

## Discrepancies vs the plan's estimates (both favour us)

- Plan §0 says **18** cross-pass `let mut`s. Actual: **15** at function
  scope (§1). The write-site finding still holds exactly.
- Plan §A0 says **8** `continue 'backtrack` sites. Actual: **7** code
  sites (§3) + 1 mention inside a comment (15064, documents the
  `autounmask_use_change_records` design). The §A2 decision-chain order
  (steps 1–7 + fall-through) is unchanged.

## 1. Function-scope `let`s — `ResolveCtx` vs `BacktrackParams`

Immutable (`let`, never reassigned): pure `ResolveCtx` candidates —
`config_root` 14874, `root` 14875, `atoms` 14876, `config` 14877,
`newuse` 14879, `changed_use` 14880, `nodeps` 14881, `update` 14882,
`deep` 14883 (shadow-rebound once at 14972, still immutable),
`excluded` 14884, `with_bdeps` 14885, `changed_deps` 14886,
`changed_slot` 14887, `with_test_deps` 14888, `changed_deps_report` 14889,
`selective` 14890, `autounmask_backtrack_enabled` 14902, `usepkg` 14903,
`usepkgonly` 14904, `binpkg_respect_use` 14905, `usepkg_exclude` 14906,
`usepkg_include` 14907, `rebuilt_binaries` 14908,
`rebuilt_binaries_timestamp` 14909, `newrepo` 14910, `buildpkgonly` 14911,
`root_deps_running_root` 14912, `distdir` 14913, `empty` 14914,
`getbinpkg` 14915, `ignore_built_slot_operator_deps` 14916,
`backtrack_max` 14917, `reinstall_atoms` 14918, `rebuild_if_new_slot` 14919,
`rebuild_if_unbuilt` 14920, `rebuild_if_new_rev` 14921,
`rebuild_if_new_ver` 14922, `rebuild_exclude` 14923, `rebuild_ignore` 14924,
`dynamic_deps` 14925, `implicit_system_deps` 14926, `complete` 14927,
`repos` 14929 (`find_repos(config_root)?` — fallible, so `ResolveCtx::new`
returns `Result`), `slot_op_reachable` 14937, `complete_locked_merges`
14949, `top_level` 14981, `top_level_cps` 14987, `local_binpkg` 15017.

Mutable (`let mut`): `BacktrackParams` candidates unless noted. Write
sites are exhaustive within 14873–17731 (pattern
`\b<name>\b\s*(=|\.(insert|push|entry|extend|clear|retain|get_mut|remove))`
plus manual `+=` check for the counter).

| name | decl | scope verdict | write sites |
|---|---|---|---|
| `autounmask_suggest_keywords` | 14893 | cross-pass (cleared once by breakage step) | 17431 `= false` |
| `autounmask_suggest_use` | 14894 | cross-pass | 17432 `= false` |
| `autounmask_suggest_license` | 14895 | cross-pass | 17433 `= false` |
| `autounmask_suggest_masks` | 14896 | cross-pass | 17434 `= false` |
| `missing_dep_masked` | 14998 | cross-pass latch (never cleared) | 17473 `.insert` |
| `reverse_dep_masked` | 15003 | cross-pass latch (never cleared) | 17511 `.insert` |
| `dropped_pins` | 15009 | cross-pass accumulator (never cleared; deduped by `contains`) | 17518 `.push` |
| `slot_constraints` | 15029 | cross-pass accumulator (entries removed only by trial revert) | 17269 `get_mut`+`retain`, 17335 `entry`, 17406 `entry`, 17512 `entry` |
| `backtrack_iteration` | 15030 | cross-pass counter; **read in-walk** at 15189, 15305 (`resolver_debug() && … == 0`, A3 trap) | 17344, 17417, 17438, 17450, 17474, 17522 (`+= 1`) |
| `mask_phase` | 15045 | cross-pass (`MaskPhase` enum declared 15038–15044, stays inside `BacktrackParams` per plan) | 17260, 17274, 17281, 17415 (`=`); read as guard at 17294, 17356, 17429, 17449, 17466, 17505 and in-walk at 15808 |
| `mask_trial_spent` | 15046 | cross-pass latch | 17416 `= true` |
| `mask_negatives` | 15047 | cross-pass (trial scratch: written step 3, judged/cleared step 1) | 17273 `.clear`, 17413 `=` |
| `pre_trial_nvc` | 15048 | cross-pass (trial scratch, same lifecycle) | 17414 `=` |
| `autounmask_use_config` | 15059 | cross-pass **but written in-walk** (overlay in A3) | in-walk 15588, 16294 (`.entry`); post-walk 17435 `.clear` |
| `autounmask_use_change_records` | 15068 | cross-pass **but written in-walk** (overlay in A3) | in-walk 15615, 16330 (`.push`); post-walk 17436 `.clear` |
| `backtrack_config` | 15072 | cross-pass (`None` until first growth; rebuilt on growth) | 17437 `= None`, 17453 `= Some(c)` |
| `autounmask_use_broke` | 15080 | cross-pass latch **but written in-walk** (overlay `use_broke` in A3) | in-walk 15585, 16306 (`= true`); post-walk: only read (17429) |
| `autounmask_disabled` | 15081 | cross-pass latch | 17430 `= true` |

Result: exactly the plan's "three" — `autounmask_use_config`,
`autounmask_use_change_records`, `autounmask_use_broke` — are written
in-walk, at the two near-duplicate sites (§4). The other 12 cross-pass
`mut`s are written only in the post-walk decision block (17260–17523).

## 2. Loop-scope per-pass `let`s — `PassState` / `PassResult`

Declared at 8-space indent inside `'backtrack`, re-created every pass.
All `mut` except `config` (15088, per-pass shadow) and `pprovided_refs`
(15218, immutable borrow of `pprovided_atoms`).

| name | decl | read by decision chain / assembly? |
|---|---|---|
| `config` (shadow) | 15088 | everywhere (`config` from here on) |
| `visited_atoms` | 15097 | walk-local |
| `resolved_slots` | 15104 | walk-local (feeds `slot_conflicts` records) |
| `other_outcomes` | 15108 | walk-local |
| `root_deps_build_seen` | 15117 | walk-local |
| `slot_want` | 15123 | steps 2 (17301) and 7 (17508) |
| `slot_pullers` | 15130 | step 3 (17382), residuals (17692) |
| `autounmask_grew` | 15135 | step 5 (17449); set in-walk 15626, 16352 |
| `missing_dep_trigger` | 15140 | step 6 (17465 `.take()`) |
| `entries` | 15142 | everything downstream |
| `required_use_violations` | 15155 | early-`Err` 17245 |
| `slot_conflicts` | 15156 | steps 1–3, residuals 17687, `GraphResult` 17716 |
| `masked_deps` | 15160 | chains 17699, outcome 17707, `GraphResult` 17726 |
| `nvc_dep_atoms` | 15164 | outcome 17710 |
| `changed_deps_report_seen` | 15173 | walk-local |
| `changed_deps_report_entries` | 15174 | `GraphResult` 17717 |
| `queue` | 15184 | walk-local |
| `pending_blockers` | 15208 | blocker attach 17234 |
| `pprovided_atoms` | 15211 | `GraphResult` 17719 |
| `autounmask_keyword_changes` | 15214 | `GraphResult` 17720 |
| `autounmask_use_changes` | 15215 | coalesce 17623–17661, `GraphResult` 17721 |
| `autounmask_license_changes` | 15216 | `GraphResult` 17722 |
| `autounmask_mask_changes` | 15217 | `GraphResult` 17723 |
| `required_by_map` | 15228 | walk-local (feeds `required_by`) |
| `edge_kind_map` | 15234 | `find_hard_cycles` 17599 |

24 per-pass `mut`s + 2 immutables, matching the plan's "24".

Post-walk locals (decision/assembly scratch, not `PassResult` unless
listed): `mergebound_cp_slots` 17209, `nvc_count` closure 17250 (plan
trap (a): make it a free fn in A2), `progressed` 17298, `negatives`
17361, `added` 17404/17506, `slot_op_rebuilds`/`abi_rebuilds` 17532,
`buildpkgonly_deps_unsatisfied` 17573, `circular_deps` 17599,
`large_cycle_count`/`cycle_display` 17606, `outcome` 17705.

## 3. `continue 'backtrack` sites in source order (the `next_step` chain)

| # | line | guard (verbatim condition shape) | plan step |
|---|---|---|---|
| 1 | 17275 | `MaskPhase::Trying` arm, trial rejected (`!slot_conflicts.is_empty()` or `nvc_count > pre_trial_nvc`); sets `Reverting` | (1) judge trial |
| 2 | 17345 | `mask_phase == None && !slot_conflicts.is_empty() && backtrack_iteration < backtrack_max`, `progressed` (solvable-conflict growth added a new want) | (2) solvable growth |
| 3 | 17418 | `mask_phase == None && !mask_trial_spent && !slot_conflicts.is_empty() && backtrack_iteration < backtrack_max`, `added` (trial negatives added) | (3) mask trial |
| 4 | 17439 | `autounmask_use_broke && !autounmask_disabled && mask_phase == None` (unconditional retry; budget-exempt) | (4) breakage |
| 5 | 17454 | `autounmask_grew && mask_phase == None && backtrack_iteration < backtrack_max` | (5) growth re-run |
| 6 | 17475 | `missing_dep_trigger.take()` is `Some` `&& mask_phase == None && backtrack_iteration < backtrack_max` | (6) missing-dep |
| 7 | 17523 | `mask_phase == None && backtrack_iteration < backtrack_max`, `added` (a new reverse-dep pin entered `slot_constraints`) | (7) reverse-dep |
| — | — | fall-through: rebuild passes → trace dump → `topological_merge_order` → assembly → `return Ok(GraphResult)` | settle |

Note step 4 is budget-exempt (no `backtrack_max` check, unconditional
`+= 1` at 17438) — C2's `mask_steps` alignment must decide whether it
stays exempt; flagged for C2, not A.

## 4. The two in-walk cross-pass write sites (A3 overlay targets)

- **`'parent_flip` block (~15433–15627).** A `[use]`-dep fails against an
  already-resolved slot but a `package.use` flip could fix it:
  opposite-flip check → `autounmask_use_broke = true` (15585) + break;
  else insert flips (15588–15595), record `(atom, token)`-deduped change
  (15611–15625), `autounmask_grew = true` (15626), `continue 'queue`.
  On conflict it `break`s *before inserting anything*.
- **Already-resolved-slot re-check (~16290–16354).** Same shape, one
  behavioural difference: non-conflicting flags are inserted and only
  the conflicting one sets `autounmask_use_broke` (16294–16312); record
  + `autounmask_grew` only if `newly` *and*
  `autounmask_backtrack_enabled` (16313–16353). The plan's "prove
  equivalence with a fixture or preserve both" stands — preserve both.

Overlay reads-within-pass that must consult overlay-then-`bp`: the
15579–15584 opposite-flip check and the 15591 `bucket.get(f)` /
16297 `bucket.get(f)` lookups.

## 5. A2 trap resolutions (from reading, no code changed)

- (a) `nvc_count` (17250) is a closure over nothing but its arg — free-fn
  candidate, confirmed.
- (b) `backtrack_config: Option<Config>` rebuilt from `req.config` at
  17451–17453 lives in `next_step` step 5; the 15088 shadow
  (`backtrack_config.as_ref().unwrap_or(config)`) is per-pass and stays
  at the top of `run_pass`.
- (c) `build_residual_slot_conflicts` (17687, extending `slot_conflicts`
  *after* the decision chain) belongs in `assemble_result`, **not**
  `PassResult` — decided, not surfaced: no `continue` follows it, so it
  cannot influence a retry.

## 6. Baseline gate (A0 acceptance, first half)

- `cargo fmt --check`: clean.
- `cargo clippy --release --all-targets`: zero warnings.
- `cargo test --release`: all suites `ok`, 0 failed.
- `python3 -m pytest tests -q`: 1522 passed, 2 skipped, 3 xfailed;
  5 failures in `tests/test_portuale.py`, all pre-existing on the clean
  tree per the Slice 5 commit (interactive/TTY-adjacent):
  `test_emerge_resume_replays_the_saved_mergelist`,
  `test_emerge_ask_prompts_before_a_real_merge_and_honours_the_answer`,
  `test_emerge_ask_prompts_before_a_real_unmerge`,
  `test_emerge_deselect_ask_prompts_before_rewriting_world`,
  `test_emerge_config_runs_pkg_config_from_the_vdb`.
- L0 raw baseline: `TEST/logs/l0-baseline-023-A0/` (copy of
  `l0-20260911T144609Z`, 120 per-probe portuale outputs; gitignored).
  Code-identical to this branch: `git diff cea80b3 HEAD -- rust/
  python/ tests/ fixtures/` is empty.
