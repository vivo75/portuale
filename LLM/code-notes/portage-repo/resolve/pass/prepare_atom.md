# crates/portage-repo/src/resolve/pass/prepare_atom.rs
Parse and dedup one queued atom, recording its edges.

## fn prepare_queued_atom
- Portage `dep_check.py:1052` (dependency) and `depgraph.py:5497-5615` (top-level target); Portage removes the dep from the deplist.

- Track X Slice B (backlog #242): this atom's resolving root -- Portage `_dep_expand`'s `root_config` (`depgraph.py:4928`). The flag implies `cross_root().is_some()` (tags are only ever set when the roots differ); same-root runs always resolve here.

- The soft running edges are soft via the running-`BDEPEND`-soft rule in `enqueue_flat_deps` (e.g. the `rdrcyca <-> rdrcycb` bootstrap cycle); cut mirrors the old sidecar `root_deps_build_seen` guard (`resolve_root_deps_build_entries`: "the second edge back into an in-progress package just yields no further entry", losing one `required_by` edge by design). The hard running edge back is the `cyc0z-3`/`cyc0y` cross-root `DEPEND` cycle (the #207 flip) and still records its edge so the cycle reports; a same-root soft revisit (a `RDEPEND` diamond's second owner) still records too (its `--json` `required_by` lists both).

- Track X Slice C (#242): slot/installed dedup is root-qualified since Slice B; top-level atoms are target-rooted like their flag.
- Portage `DepPriority.satisfied` (`_emerge/DepPriority.py`): a build-time dep already provided by an installed package gets a *satisfied* priority, which `_serialize_tasks`' `DepPrioritySatisfiedRange` can ignore when breaking a cycle, so it is not an unbreakable edge. Only a genuinely unsatisfied build-time dep counts as `has_hard`, matching this map's own "unsatisfied build-time dep" contract. Under `--emptytree` both ends of the `app-arch/xz-utils` <-> `app-portage/elt-patches` build-time cycle are still installed, so Portage orders them by installed versions rather than reporting a circular dependency.
- Backlog #250: Portage's satisfaction test is `vardb.match_pkgs(atom)`, which honours the atom's use-deps against the installed instance's recorded USE -- an installed `foo` built `-bar` does not satisfy `foo[bar]`, so the edge stays unbreakable; the version/slot-only lookup used to let a USE-mismatched instance hide a genuine build-time cycle. Track X Slice B (#242): satisfaction is tested against this atom's own resolving root (Portage `DepPriority.satisfied` reads that root's vdb).

- Backlog #277: Portage narrates `Child:`/`Parent Dep:` at every `_add_pkg` call; the single-resolution invariant means same text, same child. `parent_atoms` feeds only the trace dump.

- `emerge --pretend --debug` Stage 3: Portage `_wrapped_select_pkg_highest_available_imp`'s candidate list, every candidate that matched this atom, first pass only. Format is Portage's own `f"{type_name + ':':>10} {cpv}::{repo}\n"` (`depgraph.py:8347`), in `dbs` order (ebuild, then binary, then installed), to stderr.

- Backtracking: this atom is recorded as one of the constraints pulling `cat/pkg` (Portage `_select_pkg_highest_available` sees the whole atom set for a package, not just the first); slice 4 renders the `(Argument)` line via `slot_collision_handler`.

