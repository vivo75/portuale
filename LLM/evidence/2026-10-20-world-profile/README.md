# `emerge -puDN @world` profile, 2026-10-20 (backlog #455 S0 first run, #457)

Host: this LXC, live Gentoo tree, `--getbinpkg` configured; the command ends
rc 1 with an autounmask report (the same shape as the strong review saw), so
this is the abort path, not the owner's 194 s completed plan. Wall clock
`/usr/bin/time`: **68.7 s before, 44.5 s after** the change below, stdout
byte-identical (`cmp`).

Method: `PORTUALE_PROFILE=1` phase timers (`portage_util::profile`, stderr
only) plus `eu-stack` samples every 0.4 s, aggregated with
`LLM/tools/profile_sample.sh` + `LLM/tools/stack_agg.py` (symbols demangled
with `llvm-cxxfilt`). Raw stacks and timer lines are in this directory.

## Phase timers (before)

| span | seconds |
|---|---|
| expand_targets | 0.15 |
| plan_resolve (binhost index refresh over wget dominates) | 9.8 |
| resolve_once (22 `run_pass`, ~1.43 s each, of which `collect_pass_blockers` 0.2) | 58.7 |
| ResolveCtx::new | 0.08 |
| merge order `select_nodes` | 0.02 |
| display and post | 0.004 |

The merge-order and context work the first S1 slices touched is not where
the time goes; the resolver passes are.

## Inclusive stack samples (167 before, 109 after)

| frame | before | after |
|---|---|---|
| `find_hard_cycles` (via `step_circular_dead_end`, `collect_feedback`) | 35% | gone |
| `installed_candidates_by_cp` (2100 `SLOT` reads per call) | 32% | gone |
| `resolve_pretend` (`reinstall_flags_for_use_change`, `binary_deps_changed`, `read_vdb_flag_set`) | 29% | 45% |
| `refresh_binhost_indexes` (wget, fixed cost) | 14% | 22% |
| `parse_atom` | 10% | 16% |

Change: `installed_candidates_by_cp` is memoised per root and VDB generation
(same contract as `all_installed_packages`), so the 2100 `SLOT` reads happen
once instead of once per caller (called from `build_digraph`, `alt_edges`
and every `dep_edge_satisfied_by_installed`).
