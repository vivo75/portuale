# crates/portage-repo/src/resolve/queue.rs
The walk queue.

## enum Deep
- Portage's `lib/_emerge/main.py` `"--deep": valid_integers`, `create_depgraph_params.py`'s `myparams["deep"]`, `depgraph.py`'s `_too_deep`/`_add_pkg`. Portage's default (`deep` absent from `myparams`, only set when `--deep` is present and non-zero) never walks an AlreadyInstalled package's deps -- portuale's pre-existing hardcoded behavior (see `resolve_pretend_graph`'s "A package's dependencies are only walked if..."), so `NotRequested` needed no new code. A bare `--deep` stores Python `True` (unlimited); `--deep=N` the integer `N`, rejected by Portage's `parser.error()` if negative. CLI parsing lives in `portuale/src/pretend.rs`.

## fn Deep::recurses_at
- Mirrors Portage's `recurse = deep is True or not self._too_deep(self._depth_increment(depth, n=1))`. A dependency discovered at `depth` lands at `depth + 1 <= n`.

## struct QueueItem
- `depth`/`owner` feed `Deep::recurses_at` and `GraphEntry::required_by` (see `resolve_pretend_graph`'s `required_by_map`).
- `unevaluated`: `None` when nothing changed or the atom was never conditional-evaluated (see `enqueue_flat_deps`); see `suggested_parent_use_candidate`.
- Track X Slice C (#242): the atom's own resolving root rides in `targets_running_root` (the child half); the two differ exactly for a cross-root group (a target-rooted owner's `BDEPEND` atom, or a running-rooted owner's `RDEPEND` atom). Portage installs the argument under the target `ROOT`.
- Track X Slice B (#242): Portage's `_add_pkg_deps` five-group `deps` queue (`depgraph.py:4255-4291`) resolves `RDEPEND`/`PDEPEND` against the target root, `DEPEND`/`BDEPEND`/`IDEPEND` against the running root, so the atom is selected against the running root's tree/installed db and its entry carries that root (`GraphEntry::targets_running_root`).
- `from_disjunction`: the `disj` partition of `enqueue_flat_deps` / `enqueue_dependencies`. Portage resolves the disjunction layer's group-level classification (`dep_check.py` soft 715-724: `all_available` vs `all_installed`/`some_installed` vs `other`) before per-atom selection; the chosen alternative's atoms each still need a visible tree candidate of their own -- an installed-only atom in the chosen alternative stays a reported miss (pinned by `test_or_group_other_installed_some_bin_beats_plain_other`: both `opartlya` and `opartlyb` are reported even though `opartlya` alone is installed).

## type SlotPullers
- Slice 4 uses the atom text to render the `pulled in by` lines; C3's `slot_conflict_mask_choices` uses the `(parent, version, atom)` triples to build each choice's `conflict_atoms`. See `resolve_pretend_graph`'s `slot_pullers`.

## fn or_group_universe
- Backlog #90 (S1). `enqueue_flat_deps` needs the split because the flattened list no longer says which tokens a `||` produced. Pathological overlap (a token both inside and outside) is documented, not handled.

## fn dep_running_tags
- Track X Slice B (#242). Native builds: `ESYSROOT` collapses to the running root, and the pre-EAPI-7 `DEPEND`-against-running-root branch lands in the same place, so no EAPI gate is needed. Scope-backlog's permanent non-gap for the running-rooted `PDEPEND` case, not reopened.
- Same per-key `use_reduce_flat` over the same `use_flags` as `enqueue_flat_deps`, so the same raw-text `contains` comparisons apply -- including the shared assumption that a merged-flatten token matches its per-key flatten textually.

## struct EnqueueFlatDepsArgs
- `buildtime_atoms`/`runtime_atoms`: `DEPEND`/`BDEPEND` vs `RDEPEND`/`PDEPEND`/`IDEPEND`. A `test?` dep is `DepPriority.optional`, never a hard cycle contributor.
- `bdepend_atoms`: the unit-test pin below also passes empty.
- `running_atoms` / `owner_targets_running_root`: a `test?` dep stays target-rooted.

## fn enqueue_flat_deps
- `flat_deps` is a `use_reduce_flat` / `use_reduce_flat_subset` result. A blocker atom can syntactically carry use-deps too (e.g. `!foo/bar[baz=]`). "Can't tell, so pass it through as-is" for unparseable tokens, same as the `parse_atom` fall-through below.
- Backlog #90 (S1): Portage pops each deplist last-declared-first (stack discipline). `flat_deps` already serializes plains before `||`-chosen (`resolve_disjunctions`); the partition only re-splits them. The orbtblocked shape (`|| (...) =1.0` must file 1.0 first, staged oracle).
- Track X Slice B fix (#242): bound on the raw token (same flatten family, same raw-text assumption; `||` groups never span keys). The bootstrap cycle (`rdrcyca <-> rdrcycb`) terminates like the old sidecar `seen` guard; the `cyc0z-3`/`cyc0y` cross-root `DEPEND` cycle still reports (the #207 flip, untouched).
- Track X Slice C (#242): a non-self bootstrap ring keeps Slice B's terminating scope.
- Portage's `blocker.priority.buildtime` (#143 S1): evaluated against the same raw text `buildtime_hard` uses (bound before `tok` moves into `atom_str`).
- `owner_installed: false`: the vdb-scan producer is `collect_unwalked_installed_blockers`.
