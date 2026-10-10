# crates/portage-repo/src/resolve/pass/unresolved_dep.rs
Handles a dependency with no version to merge: backtracking feedback and its unsatisfied entry.

## fn record_unresolved_dep
- A `||` group whose chosen alternative has an unsatisfiable subtree yields to the next alternative; a top-level-selected version whose dependency is unmaskable yields to the next older version. Backlog #198: `akk0a-2` masked, `akk0a-1` picked; Portage `depgraph.py:3493-3503` + `backtracking.py:197-207` carry no top-level exemption (the mask names one cpv, never the requested package itself). One backtrack node at a time, like Portage.
- Portage (`depgraph.py:3473-3483`) does NOT missing-dep backtrack when only a USE change would satisfy the dep (`_select_package(dep.atom.without_use)` still finds a package); that path is the autounmask machinery's, and an unfixable `[flag]` dep reports as a plain NVC.
- C3: the probe consults the negatives, not the bare tree (`btconsumer-2.0` under a masked `bttarget-2.0` falls back to `btconsumer-1.0` on retry, the same downgrade the old puller pre-masking produced). Positives stay out (Portage selection has no enforcement counterpart); a USE-only failure satisfies the probe and stays out, as before.
- Backlog #276: Portage `_add_dep` (`depgraph.py:3456-3470`) maps to `runtime_pkg_mask` "missing dependency". See `is_installed_only_mask`; the next pass walks the same-version ebuild like Portage's second backtracking try masking the ebuild.
- Backlog #57, direction 1: `othermod`'s `<paired-2.0` landing on installed 1.0 while `needer`'s `>=paired-2.0` already graphed 2.0. Portage keeps the installed node and still merges the other instance; the `_solve_non_slot_operator_slot_conflicts` split belongs to the backtracker.
- The vdb `SLOT` read is reused by the slot-conflict record and the `other_outcomes` dedup key; `installed_refs` would re-list every version and read each `repository` too, and this runs for every `AlreadyInstalled` outcome in the walk (thousands on a `-puD @world`).
- Direction-1 forgiveness: Portage 3.0.82.2 `[sct, oldc, newc]`-only shape at `--backtrack=0`; Portage `_solve_non_slot_operator_slot_conflicts` (`depgraph.py:1774-2115`).
- Backlog #74 S2a: the old cp key let the first slot processed shadow the rest, dropping `llvm-core/clang:23`'s own build deps.
- Backlog #57 (`suppressed_nvc`): the argv order `othermod needer` used to settle on the masked try otherwise.
- Backlog #286: the repeat edge carries the atom the first edge lacked, and Portage's fail-fast walk would have hit it first. Both row builders read the new atom only; the cheap gate runs them only when the recorded atom is a supersedeable built one.
- Installed `deps`: live ebuild deps plus the vdb's own built `:=` atoms by default, the vdb snapshot under `--dynamic-deps=n` (backlogs #25 S1b display/recursion agreement, #26 A2 same-view construction, #86 recorded-repo view, never a priority search). The recursion walks `atom_root` (Portage `pkg.root`); see `enqueue_dependencies`.
- Masked disclosure is the Tier 2.20 half of finding S's top-level report; the bare line it replaces is `!!! no visible ebuild`.
- Parent-flip suppression: Portage `_unsatisfied_deps_for_display` (`depgraph.py:5820` `remaining_items`).
- Plain-miss shape: backlog #135 (d).
