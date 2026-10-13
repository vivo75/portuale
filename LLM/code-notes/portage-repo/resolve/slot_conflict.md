# crates/portage-repo/src/resolve/slot_conflict.rs
Slot conflicts: records, merging and rendering.

## struct SlotConflict
- The atom that reached this slot *second* doesn't accept the version the *first* one already caused to be resolved (and recursed into). Two atoms requesting *different* slots of the same package (e.g. `dev-lang/python:3.11` and `dev-lang/python:3.12`) are normal, valid coexistence, not a conflict at all -- Portage, and portuale (see `resolve_pretend_graph`), treat them so.
- Same "report, don't enforce" spirit as `BlockerConflict`: Portage's own depgraph treats an unresolved slot conflict as fatal and refuses to proceed; portuale instead reports it and keeps going (using whichever version was resolved first). `--pretend` itself never touches anything, so nothing here is truly "fatal" to calculate.
- `instances`: Portage `slot_collision_handler`'s per-`slot_atom` `(pkg, parent_atoms)` data, used to render the `!!! Multiple package instances ...` block.

## struct SlotConflictInstance
- `use_display`: Portage `pkg_use_display(pkg, ...)` for a scheduled-for-merge instance -- the `USE="…"` (+ one `VAR="…"` per non-hidden `USE_EXPAND` group) pairs on its own `(… ebuild scheduled for merge) USE="…" pulled in by` header line. Every `IUSE` flag, enabled-first, `( )`-wrapped for profile force/mask (same `build_use_expand_display` the `--info` block uses). For an installed instance the same pairs built from the vdb's own `IUSE`/`USE` (see `installed_use_display_for`).
- `installed`: Portage's installed nomerge node -- an already-installed version kept in the graph by a residual consumer pin (see `build_residual_slot_conflicts`).

## struct SlotConflictParent
- One parent that pulled a `SlotConflictInstance` into the graph. See `SlotConflictInstance::parents`.
- `parent_cpv`: `cat/pkg-ver:slot/sub_slot::repo` for a dependency, or empty for a top-level atom (rendered `<atom> (Argument)`).
- `use_display`: Portage `pkg_use_display(parent, ...)` -- the parent package's own resolved `USE="…"` on its `<atom> required by (<parent_cpv>, …) USE="…"` line. Empty for a top-level `(Argument)` parent (Portage renders a bare `""` for a non-`Package` parent). For an installed parent the vdb's own `IUSE`/`USE` (see `installed_use_display_for`).
- `installed`: Portage's installed nomerge parent -- an already-installed package whose recorded atom the merge breaks. Always false for merge-vs-merge conflicts.

## fn slot_conflict_meta
- The same re-lookup `slot_changed` / `deps_changed` already do. Slice 4's slot-collision block uses it to print the Portage `(<cpv>:<slot>/<sub>::<repo>, ...)` form for every conflicting instance and every parent.

## enum InstalledSide
- #57's installed-instance collisions (Portage `_add_pkg` feeds installed nomerge nodes to `_package_tracker` too, so one slot holding an installed node plus a merge node is an ordinary slot collision).

## fn build_slot_conflict
- Portage `slot_collision_handler`'s `(pkg, parent_atoms)` per `slot_atom`. Each `slot_pullers` entry files under whichever instance its atom matches (under A when it matches both -- a bare atom pulled the resolved one).
- `installed_side` (#57): an installed nomerge node's sub-slot/repo, `USE=` display and use-dep flag sets come from the vdb (`installed_refs` / `installed_use_display_for` / `installed_pkg_iuse_and_use`, exactly what `build_residual_slot_conflicts` uses) instead of from the repo candidate carrying the same version, and its instance renders `(… installed in '<root>')`. `None` for merge-vs-merge conflicts.
- An installed instance's own vdb `(sub_slot, repo)`: the repo the *installed* copy came from can differ from whatever currently provides that version, and a version no longer in any repo has no candidate at all -- both of which `slot_conflict_meta` would render as empty strings.
- The match strings carry the sub-slot (Portage matches puller atoms against full packages, not slot-only strings): without it, a *built* slot-operator puller (`cat/pkg:S/SS=`) matches neither instance and vanishes from the notice entirely -- which also starves the `need_rebuild` scan of exactly the parents it exists for. A missing sub-slot (version gone from every repo) keeps the old slot-only shape.
- USE-aware filing: Portage lists every puller and display-filters by reason; portuale's two-instance model files by match instead, so a USE-mismatching parent must land on the instance it actually pulls (e.g. `>=T-1.0[x]` under 1.0, never under the x-off 2.0 it version-matches). Without this, USE parents file under the wrong instance and vanish from the notice (their atom satisfies the other side).
- Unparseable atoms (soname and friends never reach pullers, but be permissive): file by string match.

## fn slot_conflict_puller_cpv
- The Portage `(<cpv>:<slot>/<sub>::<repo>, …)` form.

## fn record_slot_conflict
- Portage keeps one collision handler per conflicting slot, accumulating every puller; portuale pushes per mismatching atom, so a second distinct atom hitting the same pair (e.g. two USE-deps pulling the older version) must re-render the record from the pullers recorded so far -- which `build_slot_conflict` already does from the live `slot_pullers` map, making the merge a pure replace. Records that differ in any key element are unrelated and still append (the pre-existing multi-slot behavior is untouched).

## fn merge_same_slot_conflicts
- #142 S2 F1: `record_slot_conflict` files one record per mismatching atom, so a three-version contention lands as two two-instance records (pg0's `b c a` order: `(X-3,X-2)` + `(X-3,X-1)`); solving each independently keeps a version a later atom rejects, and the repoint loop's second removal finds no entry and vanishes silently -- the row settles at X-2 against B's `<X-2` bound. The joint record carries every contending instance with per-instance parents re-filed from the live pullers (a puller files under *every* instance its atom fully satisfies -- the pairwise `hits_a else hits_b` rule of `build_slot_conflict` generalized to n-way), so the solver sees the whole contention exactly like Portage's per-slot collision handler and keeps the uniquely jointly-satisfying version from any walk order. Singleton groups pass through untouched; a merged record inherits the first record's `resolved_version`/`conflicting_atom` (render fields, only read when the conflict survives the solve).
- The same structural probe `direct_solve_atom_matches` matches puller atoms against (see its doc comment): tree slot/sub-slot, USE checked against the instance's own resolved flags. #291: an installed instance carries its own vdb sub-slot (`build_slot_conflict` read it there); the tree's metadata for the same version can differ, or be absent, and would drop a built `:S/SS=` puller from the instance it actually pulls.

## fn synthesize_surviving_conflict_entries
- Backlog #148. The walk graphs one entry per `(category, package, slot)` (the `resolved_slots` conflict arm records the `SlotConflict` and moves on), so a surviving conflict's other instances have a notice but no merge row -- while Portage prints a row per instance (bed `l0-fx-20260922T192350Z`, cell `-D dev-libs/slotusegroup`: Portage schedules both `slotusetarget-1.0` and `-2.0`, portuale only `1.0`, notices identical, rc 1 both sides).
- The solver's inputs -- and every solved cell -- are byte-identical to before: a dissolved conflict has no row owed (the removal machinery already repoints or deletes the loser's row), and only a conflict Portage would print before giving up gains rows.
- Slot / sub-slot / repo / USE come from the same tree-candidate reads the notice uses (`refresh_entry_use_display` on a one-entry slice, the #141 precedent -- existing entries are untouched), `required_by` from the same cp-union the post-walk fill gives its siblings, and scheduler edges resolve to it through the USE-narrowed `match_candidates`, so it orders like a walked node. Deliberately *not* carried over: the instance's own deps (walking them would need a second walk -- the same documented non-leaf staleness as the #142 S2 F3 keeper-blocker flattening), its blockers (a second-instance blocker is a new number, not this item), and the ebuild-metadata row decorations (`interactive`, fetch-restrict / `download_files`, `keyword_mask`, `provenance`, binary `source`) -- all default to the plain-undecorated `New` shape. Each is a filed-later residue if a bed ever shows a twin that needs it, not silent scope growth here.
- File beside its siblings: Portage prints the conflicting instances adjacently (`1.0` then `2.0` at the head of the S0.1 capture), and discovery order settles scheduler ties.

## fn build_residual_slot_conflicts
- Portage `_complete_graph`'s end-of-walk unsatisfied-dep loop (`depgraph.py:8770+`) adds the installed package satisfying a broken deep dependency as a nomerge node, so the run reports it as a slot collision against the merged version instead of silently breaking it -- merge instance vs installed instance, exit 1. Portuale reports the same shape without carrying the nodes.
- Verified live against Portage 3.0.82.2 (`=paired-2.0` over a keeper pinning `=paired-1.0`, and the needer/othermod triangle): Portage prints the merge list with the upgrade *and* the conflict block, exit 1 -- where portuale used to mask the version into invisibility (`!!! no visible ebuild`) or fail the top-level atom outright.
- `dropped` accumulates across backtrack passes, so a pin can outlive the reason it was recorded: its consumer may be merge-bound in the final graph (e.g. a `:=` rebuild seeded by a later pass). `reverse_dependency_constraints`'s own "consumer being replaced" skip applies against the final `entries` (Portage `depgraph.py:2512-2522`).
- Fold every dropped consumer pin for the group into the installed instance's parents (Portage lists each installed consumer beside the hard pullers already filed above). A pin belongs to a record only when it guards that record's own installed version.

## fn residual_conflict_for_pin
- A pin matching no installed version means the consumer was already broken (Portage's `initially_unsatisfied` skip). Parents unioned across pins.
- Backlog #222: a `[use]`-carrying pin (a USE-break dropped pin) version-matches the merge by construction -- `match_from_list` never reads use-deps, same as Portage's -- so the holds check must also evaluate the pin's use-deps against the merge's own decided USE (Portage `violated_conditionals`, the same check that fired the pin). A merge whose USE still breaks the pin stays reported; an uncomputable merge USE falls back to the version-only check, today's behaviour. A pin the final version satisfies after all reconciles silently too (Portage reconciles silently).
- The installed version the pin preserves: the highest installed one it matches (Portage's `matches[-1]`).
- Merge parents: this pass's hard pullers matching the merged version (plus every top-level Argument puller -- Portage files an `(Argument)` parent under a reason exactly when the other side is installed, which a residual always is).
