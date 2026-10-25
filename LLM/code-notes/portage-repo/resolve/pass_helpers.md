# crates/portage-repo/src/resolve/pass_helpers.rs
Small helpers of the resolver pass.

## fn overlay_use_want
- Phase A3 (023): exactly as the old direct-write-then-read did.

## fn onlydeps_walk_dep_key
- Backlog #194. Portage's `bin/ebuild.sh` `depend` phase unsets `IDEPEND` below EAPI 8 (`portage/eapi.py:135`), so a stale or hand-written cache entry can still *carry* `IDEPEND` under EAPI 7 (Portage's `egencache` never emits one). Portage's `depgraph.py:4186-4193` blanks an `--onlydeps` root's `RDEPEND`/`PDEPEND` under `--onlydeps-with-rdeps=n`, and its `IDEPEND` too unless `--onlydeps-with-ideps` is `y`/`True`.
- Extracted (rather than left as `run_pass`'s inline closure) so the adversarial-cache path stays pinned after the fixtures stopped carrying one.
- Original comment citations: Portage: eapi.py:135; depgraph.py:4186-4193

## fn onlydeps_runtime_keys
- Backlog #194. Portage's `DepPriority`: an atom in a build-time key but no walked run-time key is `buildtime_hard`, an unbreakable merge-order edge. Under `--onlydeps --onlydeps-with-rdeps=n --onlydeps-with-ideps=y` on EAPI 8, `IDEPEND` is still walked, so it still classifies.

## fn minimize_children
- #236 B. Portage's `_minimize_children` (`_emerge/depgraph.py:4751-4854`): installed packages first, then the rest, each group ascending by version (bug 631894); a package is dropped when every atom matching it also matches another survivor; each atom binds to the highest survivor it matches (`child_pkgs.sort(); child_pkgs[-1]`).
- Matching is Portage's `findAtomForPackage(pkg, modified_use=self._pkg_use_enabled(pkg))`. Installed `mesa`'s live `libdisplay-info:=[abi_x86_32(-),abi_x86_64(-)]` and built `libdisplay-info:0/3=[...]` both bind installed `0.3.0` (#107: same check `best_installed_matching` makes). #266: `>=foo-1[bar]` alongside `<foo-3` collapses onto the lower pick when that pick builds `bar`, stays split otherwise; same pair the walk's own selection applies. A version with no ebuild candidate (binary-only pick) only ever matches its own selection.
- Atoms without a package pass through like Portage's `(atom, None)` yield.
- The `atom_pkg_graph.child_nodes` shape; same-version tie goes to the highest `repo_priority` (the walk's own tie-break).
- #266's `_pkg_use_enabled` distinction: built vs not-yet-built package.

## fn arg_pinned_installed_reuse
- Backlog #285. Portage's `_wrapped_select_pkg_highest_available_imp`'s `find_existing_node` pass (`lib/_emerge/depgraph.py:7854`, the `for find_existing_node in True, False` loop). Portage looks up `_package_tracker.match(root, pkg.slot_atom, installed=False)` (`:8180-8215`); `slot_atom` is `cp:slot` (`_emerge/Package.py:139`, so `1/1` and `1/2` share it). The winner returns directly (`:8370-8371`, mirrored at add time by `_check_slot_conflict`'s reuse, `:3532-3547`, in `_add_pkg`, `:3661-3670`).
- The slice shape (`=app-misc/mmprov-2 app-misc/mmcons` under `--update --deep`, Portage 3.0.82.2 probed 2026-10-02): the `=mmprov-2` argument puts installed `mmprov-2:1/1` in the graph, and after the missed-slot-abi backtrack (`_slot_operator_update_backtrack`, `:2400-2452`) the rebuilt mmcons' dep `app-misc/mmprov:=` reuses that node (soft/`runtime_slot_op` edge) instead of re-selecting the highest slot-1 version (mmprov-3).
- Portuale's reversed queue order (#90 S1): the consumer's dep resolves before the pinning argument. A missing cache key means uncomputed; the replace set and every other `BacktrackParams` input can change the verdict per pass.
- Same-`slot_atom` requirement: a pin in another slot never diverts an upgrade (e.g. `=mmprov-1` still upgrades slot 1). Portage iterates the tracker descending. The `[use]` check is the same one `dependency_avoid_update_candidate` runs. `resolve_pretend` writes no global state, so the speculative resolutions are side-effect free.
- "Ebuild type is the last resort", then repo priority (the pop site's `resolved` pick).
- Portage consults the packages already added to the graph before picking the highest available one: for the current db's first match it looks up `_package_tracker.match(root, pkg.slot_atom, installed=False)` (`:8180-8215`) -- the in-graph nodes in that (main-)`slot_atom` (`slot_atom` is `cp:slot`, `_emerge/Package.py:139`, so `1/1` and `1/2` share it) -- and reuses the node when the dep atom matches it (`atom.match(e_pkg)`), unless a higher version is available in a *different* slot (then the node is "irrelevant" and the highest wins).
- For every same-cp top-level atom (blockers skipped) the helper speculatively re-resolves it exactly as the walk would (`is_top_level = true`, same flags and constraints) and keeps its `AlreadyInstalled` verdicts as `(version, main slot)` pairs -- the instances the arguments pin in the graph, whatever the walk order.
- Portage only keeps it when it satisfies the dep atom (the "only keeps it when it satisfies the dep atom" half of `atom.match`).
- The dep atom matching the pinned `cat/pkg-ver:slot/sub` string (Portage's `atom.match`, the "only keeps it when it satisfies the dep atom" half), and -- for `[use]` deps -- the pinned instance's own recorded vdb `USE` satisfying them (the same check [`dependency_avoid_update_candidate`] runs).

## fn onlydeps_walk_dep_key
- #348 S5: the resolve-time EAPI gate on `IDEPEND` is gone — the key reads presence-based. Portage gates only at generation (`bin/ebuild.sh` unsets it below EAPI 8, so a consistent cache never carries a stale one) and walks whatever string the cache holds (`edepend[k] = metadata[k]`, `depgraph.py:4155-4157`; S0 cell `d-eapimm-pret`: file EAPI 8, cache EAPI 7 + `IDEPEND`, Portage pulls the dep). The `eapi_has_idepend` plumbing (`OnlydepsWalkDepKeyArgs`, `OnlydepsRuntimeKeysArgs`, `select_dep_tokens`, `record_merge_order_deps`, `classify_dep_atoms`) went with it.

## fn onlydeps_walk_dep_key
- #348 S5: the resolve-time `eapi_has_idepend` gate is removed — `IDEPEND` reads presence-based (see `enqueue_deps.md`). Only the `--onlydeps` blanking remains here.

## gate threading (#571)
- `minimize_children` carries the gate into the conflict-version display strings and the ebuild-by-version map.
