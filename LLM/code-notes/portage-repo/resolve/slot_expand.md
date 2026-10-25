# crates/portage-repo/src/resolve/slot_expand.rs
Re-expanding a resolved slot when USE changes.

## struct DeltaAtomCleanlyReusesResolvedSlotArgs
- `atom_running`: Track X Slice B (#242) -- this delta atom's own resolving root (its queue tag): the speculative selection and the slot lookup both run against it, so a running-root atom never reuses a target-root slot (and vice versa).

## fn delta_atom_cleanly_reuses_resolved_slot
- Backlog #218 (L0 20260928T063310Z): eager clean-reuse classification for delta atoms.
- Portage's rule (`3rdparty/portage`, `lib/_emerge/depgraph.py`): the flipped package was already *walked* pre-flip, so a newly-gated atom that names an already-graphed slot never re-enters Portage's `_dep_stack` (`if not previously_added: dep_stack.append(pkg)`, `_add_pkg`, :3820) and records no `_add_parent_atom` owner row (`:3583-3604`).
- Live-verified on `gui-libs/gtk:4`: Portage walks freetype with `harfbuzz` off (its `Children` are bzip2 + libpng only) and the `harfbuzz` flip lands later via pango's `freetype-2.5.0.1:2[harfbuzz,...]` atom -- so Portage's graph carries no freetype -> harfbuzz edge at all.
- Portuale's delta queues through the normal machinery, so without this filter a delta atom that merely re-names a settled slot still pops: the pop folds the flipped package into `required_by_map` (plus the edge-kind map, slot want-lists and pullers), and `build_digraph`'s `required_by` fallback then synthesizes exactly the phantom edge Portage never has -- reordering Kahn's walk for unrelated packages (gtk:4 went set-identical but #30 -> #22 with findings byte-identical, and even dropping the narrowed new-target `deps` edges did not move it).
- Each text-new delta atom is pre-resolved here with the pop site's own arguments (`resolve_pretend` writes no global state -- the unparsed-token counter fires at the pop site, not inside it -- and its inputs are all pass-fixed (`ctx`, `config`, `union_constraints`), so the verdict cannot drift between here and the pop). A genuinely new slot is the aucascleaf case (queue it); a version conflict is recorded by the pop site (queue it).
- The already-resolved version's own sub-slot is included exactly like the pop site (a built `cat/pkg:0/2=` puller matches no `:slot`-only string).
- The pop site's bound-instance side record would fire: queue it.
- `match_from_list` matched version + slot only: a `[flag]` use-dep still has to be satisfied by the settled package, else the pop site folds a flip (possibly a nested cascade). Only a satisfied atom is a clean reuse.
- Complete mode diverts dependency atoms around `resolve_pretend` (graph-or-installed only); never second-guess that branch.
- The winning-candidate pick the pop site resolves the slot from ("ebuild type is the last resort", then repo priority).
- `resolved_slots` only ever indexes the four merge-bound outcomes (the pop site's own `unreachable!`); conservative here, never a drop.

## fn expand_resolved_slot_with_flipped_use
- Backlog #218. Portage's rule (`3rdparty/portage`, `lib/_emerge/depgraph.py`): `_add_dep` only pushes onto `_dep_stack` (`_add_pkg`, :3820: `if not previously_added: dep_stack.append(pkg)`), and `_create_graph` (:3254-3271) pops that stack LIFO -- so `aucasctop`'s two deps are both *added* before either is *walked*, `aucasclate` pops first, and its `aucascmid[cascade]` atom flips the still-unwalked `aucascmid` through `_pkg_use_enabled(pkg, target_use)` (:7669), which records `_needed_use_config_changes` synchronously. When `aucascmid` then pops, `_add_pkg_deps` (:4151) reduces `cascade? ( dev-libs/aucascleaf )` with the flip already in force, so `aucascleaf` joins the graph in the same attempt (live-verified: `fixtures/abort-captures/dev-libs_aucasctop.pretend-debug.stdout` shows one continuous walk -- `aucascmid` added `USE="-cascade"`, flipped by `aucasclate`'s dep, then walked with `cascade?` live).
- Portuale fuses resolve+expand at pop time (FIFO), so the flipped package's `flag?`-gated deps were already flattened pre-flip by the time the later atom's already-resolved-slot re-check folds the flip. Only with `--autounmask-backtrack=y` does the driver additionally re-drive the whole walk (`autounmask_grew`); the default keeps Portage's single attempt.
- Additive only, by design: atoms the pre-flip expansion already queued stay queued (a flag turned *off* cannot unqueue them mid-pass -- the narrowing half is reconciled by the next whole-walk pass, same as the pre-#218 behaviour). A no-op unless the flip actually reveals new atoms, so every non-autounmask walk is untouched.
- Portage never flips a built package (`_pkg_use_enabled` returns the baked USE for `pkg.built`, `depgraph.py:7676`; `can_adjust_use = not pkg.built`, `:8131`): the delta only ever applies to a fresh ebuild entry, which is also the only shape whose md5-cache metadata this re-reads below. (`resolved_slots` only ever indexes New/Upgrade/Downgrade/Reinstall outcomes, so no outcome check.)
- The flipped package's own depth (Portage `pkg.depth`): the delta atoms queue one deeper, exactly where the fresh expansion put its own.
- The same key-inclusion rule as the fresh expansion (an ebuild source, so only the `--buildpkgonly`-without-`--deep` runtime blanking applies). Portage `RDEPEND`-first discovery order for the merge-order edges, same `portage_order_keys` as the fresh expansion. The same build-time/run-time classification as the fresh expansion (an ebuild source always classifies from both key sets).
- The same `--root-deps` running-root split as the fresh expansion (Track X Slice B: flag-on coinciding-roots only -- the unified per-group walk owns every cross-root atom).

## fn enqueue_flipped_tokens
- Compared in evaluated form, the same text `visited_atoms` and the queue itself carry.
- L0 20260928T063310Z: a text-new atom that cleanly re-resolves to its already-settled slot (`delta_atom_cleanly_reuses_resolved_slot`) is dropped before any bookkeeping -- queueing it would record the flipped package as an owner at pop and synthesize the phantom edge Portage never has. The genuinely-new survivors are untouched. Track X Slice B (#242): the tag is raw-keyed -- `tok` here is still the raw survivor (evaluated only for the lookup below).
- Narrowing (L0 20260928T054625Z): unioning the whole flipped flatten would also re-add edges for already-graphed targets (a `||` re-choice under the later queue state, an evaluated-form difference), which over-constrained Kahn's walk and reordered unrelated packages (gui-libs/gtk:4 went set-identical but order-shifted). The flipped package's genuinely new deps still get their edge (and cycle classification).

## fn enqueue_test_deps_for_flip
- `--with-test-deps` follow-up, same gating as the fresh expansion (top-level packages only): the flipped USE may newly enable `test?` deps the pre-flip `use_flags` hid. Same genuinely-new filter as the main delta: the pre-flip expansion already queued this package's test deps under the old USE (evaluated here against the flipped test uselist, like the fresh path evaluates at queue time).
- Track X Slice B (#242): a `test?` dep stays target-rooted (same gate as the fresh expansion).

## fn flatten_deps_for_flip
- Track X Slice B fix (#242): `BDEPEND` alone, for the same running-`BDEPEND`-soft rule the fresh expansion applies (see `enqueue_flat_deps`'s own `bdepend_atoms` doc comment).
- Track X Slice B (#242): the flipped entry's own per-group roots -- same rule as the fresh expansion (a flipped running-root entry re-resolves its build groups against the running root).
- Track X Slice C (#242): the node key carries the flipped entry's own root (Portage's node-identity keying); the gate reads the whole graph (same slices the bins would see on a same-root run).

## fn expand_resolved_slot_with_flipped_use, `||` deferral (backlog #398)
- The flip splits its tokens with `split_off_disjunctions` on the flipped USE
  before flattening: plains go through the unchanged flatten/filter/enqueue
  pipeline (closures never fire on `||`-free input), groups defer on the
  disjunctive stack as a `Flip` site. The deferred entry is pushed before the
  plains enqueue so the genuinely-new early return cannot drop it.
- `or_universe` still folds over the full token list (groups included), and
  the root-deps satisfied/unsatisfied sets are computed once and shared
  between the plains filter and the Flip site.

## gate threading (#571)
- Selection pools, `root_deps_satisfied_atoms`, the `DisjunctionPreferenceArgs`/`PromoteTiedAlternativeArgs` sites and the `BuiltEqualsShiftUnrebuildableArgs` chain carry `ctx.gate()`.
