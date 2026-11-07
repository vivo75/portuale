# Cluster A diagnosis (2026-11-06): #602 #603 #605 #617 #648

Portage 3.0.83 (host; playground from `3rdparty/portage` tag 3.0.83). Scratch captures:
`/tmp/claude-1000/-home-vivo-repo-PORTUALE-portuale/3c60fa85-0c1a-42e7-b42c-5a4d843557bb/scratchpad/clusterA-own/`.
Same argv on both: `env -u EMERGE_DEFAULT_OPTS PORTAGE_CONFIGROOT=$FX ROOT=$FX ... {emerge|portuale emerge} --ignore-default-opts --getbinpkg=n --usepkg=n --color=n <args>`.

## #602 — reproduced
Switches `--complete-graph-if-new-ver=n --complete-graph-if-new-use=n --rebuild-if-new-slot=n`.
`-p =z602prov-2 z602s z602a`: Portage `rR z602cons`, `r U z602prov-2`, backtrack 3/20; portuale only `U z602prov-2`, no rebuild.
Cause: `ResolveCtx::portage_complete` (`resolve/ctx.rs:232-248`) and `pretend/resolve_phases.rs:479-493` do not model `depgraph.py:9473-9480` (slot conflict while backtracking enables complete mode; `_accept_blocker_conflicts` false while backtracking, `9302-9314`).
Fix: per-pass "slot conflict seen and backtrack_max>0" signal (including direct-solved conflicts), OR into `portage_complete`, treat like `auto_enable` in `resolve_phases.rs`. Unmodelled: uninstall-task trigger `10399-10416`. Risk medium (bed re-grade). Closable; not a quirk.

## #603 — reproduced
`-p dev-libs/z603foo` (installed 3, world consumer `<foo-2`): Portage `U z603foo-3.1`, rc 0 (dep goes to `_initially_unsatisfied_deps`, `depgraph.py:8797-8806`); portuale `UD z603foo-1 [3]`, skip warning, backtrack 1/20.
Cause: vdb-match drop exists only for `||` arms (`resolve/rev_dep_or.rs:167-175,195-208`); `constraints_from_atom` (`rev_dep.rs:1169-1209`) pins plain atoms.
Fix: for unwalked consumers skip non-blocker atoms with no installed match. Risk medium-high; own bed run. Closable.

## #605 + new finding (unfiled)
Post-merge `||` view `[use]` must read the merge node's USE (`or_view.rs:225-231`; `GraphEntry.use_flags_display`, `entry.rs:67-69`), per `dep_check.py:492-495`.
Unfiled: unwalked installed consumer's `[use]` dep broken by an upgrade is a slot conflict in Portage (rc 1, "Change USE: +foo"; plain and `||`); portuale drops it (`rev_dep.rs:1318-1368`, rc 0). Fixing that without #605 makes a false conflict in variant A, so they ship together. Fixtures z605 variants A/C.

## #617 — not reproducible here
Portage reuses the lowest-version node of a slot (`_check_slot_conflict` `depgraph.py:3676-3690`, `package_tracker.py:254-255`); evidence `pmtest/differential-test-bed/logs/tmp-601-order-debug/real.txt:6715-6735`. Candidate portuale site `resolve/pass_helpers.rs:348-367` is a guess; needs bed S0. Record as quirk candidate.

## #648 — no oracle
Gates now at `rev_dep_or.rs:1063-1068`, `:1124-1130`, veto `:716-726`. Portage `dep_check.py:169-175,220,264`. Park at Tier 3.

## Side finding
`-puD @world`: portuale `N z603foo-1 [3]` where Portage prints `UD`; not isolated.
