# S3 findings (plan 01.477, 2026-10-22): why the installed libplasma-6.7.5 is not a missing-dep mask

Host pair, same day: portuale `emerge -uDpvN @world` (trace `PORTUALE_BT_TRACE=1`) against Portage
`emerge -uDpvN @world --debug` (`s0/pd.out`, `s0/pm.txt`). Before S3: 371 packages, `backtrack: 20/20`.
Portage: 374, `5/20`.

## Answer to H1 and H2

Both hypotheses hold; each one hid the other.

**H2 (installed instance ignores its `[installed]` mask) -- confirmed, two selection sites.**
Pass with `!=kde-plasma/plasma-activities-6.7.5[installed]` in its constraints (try 3 masks-in):
the installed libplasma-6.7.5 edge `=kde-plasma/plasma-activities-6.7.5*:6/7=` still resolved to
`AlreadyInstalled 6.7.5`, so the installed parent never failed and no missing-dep trigger was filed
for it. The two sites that returned the installed instance without the mask filter:
- `crates/portage-repo/src/resolve/pretend.rs:1272` (usepkg/`ebuild_db_dropped` installed fallback in
  the candidate fallback, `need_fallback`); fixed by filtering with `installed_version_masked`.
- `crates/portage-repo/src/resolve/pass/initial_outcome.rs:170` (`_minimize_children`-style group
  pick, `resolve_initial_outcome`, predicting each sibling's outcome; a NoVisibleCandidate sibling
  took the installed fallback unmasked). This is the site that actually produced the
  `AlreadyInstalled` seen at the walk (instrumented: only this site fired for `plasma-activities-6.7.5`
  in the failing passes). Fixed with the same filter, against `bp.masked_negatives_for(key)`.
Portage: `depgraph.py` `_iter_match_pkgs` / `_runtime_pkg_mask` keyed by package object.

**H1 (owner is `cat/pkg` only, so the wrong parent instance is blamed) -- confirmed.**
`QueueItem.owner` is `(cat, pkg)` (`queue.rs:46`). `maybe_missing_dep_backtrack`
(`resolve/pass/unresolved_dep.rs:53`) looked up `state.entries.find(cat, pkg)`, which returns the
first entry of that name: the merge-bound `Upgrade -> 6.8.0`. The trigger was therefore
`!=kde-plasma/libplasma-6.8.0` for a dependency that belongs to the installed 6.7.5
(instrumented: entries for libplasma were `[Upgrade 6.8.0, AlreadyInstalled 6.7.5]`).
Fix: `QueueItem.owner_version` (`queue.rs:47-49`, set at `enqueue.rs:483` for installed owners and
`queue.rs:297` for merge-bound owners, `None` for seeds in `pass/seed_queue.rs`), copied into
`PassState.expanding_owner_version` at each queue pop (`pass/mod.rs:148`, cleared before
`pop_disjunction` at `pass/mod.rs:136`). `maybe_missing_dep_backtrack` now picks the entry whose
version matches (`unresolved_dep.rs:80-95`), falls back to the old `cat/pkg` lookup when no
version is known, and for an installed parent files `!=cat/pkg-<installed version>[installed]`
(`unresolved_dep.rs:114-125`). Portage: `_slot_operator_unsatisfied_backtrack` / `_feedback_missing_dep`
mask `dep.parent` (`depgraph.py:2881`, `backtracking.py:197`).

Result of H1 + H2 together (host, same day):
- trigger is now `owner=kde-plasma/libplasma neg=!=kde-plasma/libplasma-6.7.5[installed]
  atom==kde-plasma/plasma-activities-6.7.5*:6/7=` -- Portage's P4 mask, first filed in try 3.
- try 3 feedback is still SlotConflict (libdisplay-info-0.4.0), matching Portage's P2.
- try 4 is a `Config` (the built slot-op probe `file_unsat_probe_results` replaces installed parents),
  matching Portage's probe-driven `_slot_operator_unsatisfied_backtrack` (Config, no runtime mask).

## Remaining divergence (not fixed in S3)

- The probe fans out. `feedback/missing_dep.rs` (probe call at ~L27) files the replacement set for
  every installed puller of plasma-activities (skrooge, gwenview, kmail, kio-extras, libplasma,
  plasma-workspace, systemsettings, ...), whereas Portage's `_slot_operator_unsatisfied_backtrack`
  seeds only `dep.parent`. Experiment: restricting that set to the trigger owner gives 379 packages,
  9/20, 19 tries -- still not Portage. Not kept: it changes every built-slot-op probe, which is S3/S4 scope
  for the pullers and the reinstall set. Residue: a new backlog item (see backlog).
- After try 4 the missing-dep trigger keeps re-firing (tries 5-8) and the kwin/mesa `mask_built`
  masks are dropped and re-added (tries 6-7). Portage reaches P4 (the missing-dep mask) once the probe
  returns False; portuale's probe keeps returning True while the replace set grows.

## Trajectory now (before the rest of S3 / S4)

| run | packages | backtrack | first divergence from Portage P1..P6 |
|---|---|---|---|
| before S3 (committed S2) | 371 | 20/20 | try 3: masks 6.8.0 too (Portage keeps 6.8.0) |
| after S3 (H1+H2) | 380 | 9/20 | try 4 is Config (Portage P3 is the probe Config too); P4 mask lands at try 3 but the probe's larger replace set keeps the walk going: P5/P6 not reached as Portage does |
| Portage | 374 | 5/20 | P1 (kwin/mesa mask_built + plasma-activities 6.7.5/6.6.6-r1/installed) |

Note the try-3 mask sequence now matches Portage P1 and P2 and the missing-dep mask is the Portage P4
mask; the package count still differs (380 vs 374) because of the probe fan-out above.
