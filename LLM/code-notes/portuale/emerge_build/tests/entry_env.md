# crates/portuale/src/emerge_build/tests/entry_env.rs
Per-entry environment unit tests.

## fn buildpkgonly_entry_build_env_matches_package_env_and_folds_flags
- Backlog #129: Phase 2 S0 cells A/F oracle (a matched `CFLAGS`/`CC` wins over the run-wide value), applied to the third build path. Direct unit test of `buildpkgonly_entry_build_env` (no build), the same pattern `entry_build_env_resolves_full_use_expand_and_entry_identity` uses for the merge-scheduler sibling. An unmatched entry must not leak a matched entry's vars onto its neighbours.

## fn buildpkgonly_entry_build_env_folds_matched_features
- Backlog #98/#129: `FEATURES` folds in `[run-wide, pkg, calling-env]` order.

## fn entry_build_env_resolves_full_use_expand_and_entry_identity
- #37 S2: the S0 oracle's exact row set (`TEST/findings/l2.md` "S0 recon (#37)"). The profile layers replayed set implicit arch/elibc/kernel flags no package declares in IUSE; `archusepkg` declares `IUSE="amd64 riscv"`. Portage's `environ_filter`/`AA`-pop rows stay out (S0 findings).

## fn entry_resolved_features_folds_the_matched_env_file_value
- Backlog #98: the per-entry folded `FEATURES` list. A matched env file value folds onto the run-wide list in `[run-wide, pkg, calling-env]` order (S0 cells B/C); no match returns `None`, so the run-wide value, and every gate reading it, stays untouched. A pkg `-tok` does not prune a calling-env token.

## fn entry_build_env_rereads_profile_env_changed_after_resolve
- Backlog #332: Portage's `Scheduler._allocate_config` calls `config.reload()` for every package task (`Scheduler.py:1899-1913`), which re-reads `env.d` (`config.py:2691-2699`). `entry_build_env` is portuale's `_allocate_config` point; untouched `profile.env` takes the fast path (pre-#332 env exactly). Expected value from Portage source (`config.py:2691-2699`); the bed finding was `PT_HELPER_DOINS` missing from later packages.

## fn get (test helper)
- Later pairs win in both backends (`Command::envs` / successive `export`s), so it reads the *last* pair: the run-wide base carries empty `USE_EXPAND` placeholders that the per-entry rows override.
