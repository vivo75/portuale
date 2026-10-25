# crates/portage-repo/src/resolve/rev_dep.rs
Constraints that installed reverse dependencies put on an upgrade.

## fn is_built_slot_op
- Portage `_slot_operator_check_reverse_dependencies`'s own atom normalisation, applied to one atom an *installed* consumer records against a package this run wants to upgrade.
- Portage keeps the recorded atom as written, with one exception: a parent whose atom is a **built** slot-operator atom (`cat/pkg:S/SS=` -- operator `=` with both slot and sub-slot present, Portage `Atom.slot_operator_built`) "may need to be rebuilt, therefore discard its soname and built slot operator dependency components which are not necessarily relevant" (`depgraph.py:2494-2502`), which it does with `atom.with_slot("=")` -- keeping the version bound and dropping the slot/sub-slot. That distinction is the whole difference between "this upgrade needs a consumer rebuild" (fine, and what `slot_operator_rebuild_entries` already schedules) and "this upgrade breaks a consumer outright" (not fine).
- Portage's matcher ignores a bare `:=` (no slot named) exactly as portuale's does, so dropping the slot component textually is equivalent to rewriting it to `:=`. `[use-dep]` components are dropped too -- Portage evaluates them against the candidate's own USE, which this vdb-only check has no resolved USE for; a documented cut, and one that can only *allow* an upgrade Portage would block, never the reverse.

## fn live_miss_atom_supersedes
- Backlog #286. Portage's fail-fast walk evaluates an installed parent's live dep first (dynamic deps prepend it to the depstring -- `FakeVartree._apply_dynamic_deps`, confirmed on Portage's own `--debug` for the #276 shape: the installed parent's depstring is `<u276prov-2:= >=u276prov-1:0/1=` and the missing-dependency backtrack fires on the live atom, never the built one), so the built edge never reports when the live edge already failed. Portuale's BFS evaluates both and records first-wins, which picks the built atom -- hence the override, scoped to exactly that pair (built recorded, live candidate). Every other combination keeps first-wins, matching Portage's first-failed-edge report wherever only one shape exists.
- A live slot-operator atom is the `:=` family without a built `:S/SS=` binding.
- The bare slot-operator suffix form (`<cat/pkg-2:=`: an operator with no slot) never parses in `portage_dep` (its slot part hangs off a `:`), but Portage `Atom` accepts it -- and it is always live (no `:S/SS` binding to be built).

## fn reverse_dep_constraint_atom
- Portage `_slot_operator_check_reverse_dependencies`' own constraint
  normalisation (`lib/_emerge/depgraph.py:2494-2502`): a **built**
  slot-operator atom (`cat/pkg:S/SS=`) strips to the bare cp for
  *matching* (textually equivalent to `with_slot("=")` here, since the
  matcher ignores a bare `:=` either way). `[use-dep]` components are
  dropped too (allow-only; see `is_built_slot_op`'s notes).
- Backlog #568 deliberately does NOT change this function: its result is
  the constraint the resolver records (`constraints_from_atom`) and the
  key `or_member_matches_pin` / `dropped_pin_mask_restart` match pins by.
  The built atom survives for *rendering* only, as `RevDepPin.built_render`
  (attached post-scan in `reverse_dependency_constraints`), so every
  mask/restart decision is byte-identical with the field present or
  absent. If the strip ever looks wrong for constraints too, that is a
  separate bed-wide change — stop and re-open instead of touching it.

## fn relax_built_slot_operator_atom
- Portage `Atom.with_slot("=")` as `_slot_operator_check_reverse_dependencies` applies it (`lib/_emerge/depgraph.py:2494-2502`): a parent whose atom is a **built** slot-operator atom "may need to be rebuilt, therefore discard its soname and built slot operator dependency components" -- the recorded `:S/SS=` binding is relaxed to bare `:=`, keeping the version operator, any `[use]` deps and any `::repo`. The relaxed atom schedules a consumer rebuild, never a withhold: `>=dev-libs/provpkg-1.0:0/1=` still accepts the `0/2`-slot `provpkg-2.0` candidate, while `<dev-libs/r25lib-2.0:0/1=` still rejects `r25lib-2.0`.
- Unlike `reverse_dep_constraint_atom` (which drops the `=` as well as the slot, equivalently, for *matching*), this keeps the operator so the result stays a faithful `with_slot("=")` rendering.
- `is_built_slot_op` guarantees both are present; the `:S/SS=` needle is parser-derived, so it occurs verbatim (slot names are ASCII, and no `[use]`/`::repo` tail can contain that shape).

## fn built_slot_operator_rebuild_trigger
- Portage's `_minimize_children` (`lib/_emerge/depgraph.py:4751-4840`) never lets such an atom contend the selection against the probe's upgrade -- both atoms collapse onto the installed instance first, and the update probe then replaces it (scheduling consumer rebuilds) exactly when this predicate holds. Recording it as a slot conflict instead feeds the direct solve a removal verdict Portage never renders (the slotop `@world` cells: no `provpkg-2.0`, no consumer `rR` rows). A version veto (`<dev-libs/r25lib-2.0:0/1=`, the r25 consumer pin) still mismatches the merge under the relaxed form, so that conflict records normally and the withhold survives.
- An unparseable atom is never a trigger (fall back to recording, today's behaviour).

## struct RevDepPin
- Portage's complete graph carries the consumer as a nomerge node (`_add_pkg` of the installed package); portuale carries the `(cp, atom, consumer)` triple instead, which is everything the backtrack loop and the residual-conflict report need from the node.
- `raw_atom`: matching always uses `atom`; the skip-conflict notice renders what Portage renders -- the consumer's own recorded text (S2/#92 oracle: qemu's `~...-202408[qemu_softmmu_targets_x86_64(+)]`).
- `built_render` (backlog #568): built `:S/SS=` atoms the consumer
  recorded against `cp`, verbatim. Never constrains — collected into a
  per-pass side table (`BuiltSeenMap`) while scanning (even when the
  stripped constraint matches everything and yields no pin of its own)
  and attached to the consumer's pins post-scan, so the skip-block
  printer renders Portage's both-forms block (`_show_missed_update`
  prints every non-matching `_parent_atoms` entry verbatim). Sorted and
  deduped; not part of the `seen` dedup key, so mask/restart matching is
  untouched. Ordering note: Portage collects those parents into a `set`
  (`depgraph.py:2116-2121`), so its intra-block order is hash order and
  varies run to run — portuale renders the built form first,
  deterministically.

## fn rev_dep_pin_holdable
- Portage's `_select_pkg_highest_available` seeing "the whole atom set for a package, not just the first" (`depgraph.py`), with the installed consumer's own atom among them.
- Live shape (verified against Portage 3.0.82.2): an installed `keeper-1.0` recording `=paired-1.0` while the run merges `paired-2.0` for an explicit `=paired-2.0` argument or a `>=paired-2.0` dependency -- Portage prints `U paired-2.0` plus the conflict block pairing the merge instance against the installed `paired-1.0` pulled in by keeper; portuale used to mask `2.0` away entirely (`!!! no visible ebuild`, or a fatal top-level failure).
- A pin (and each hard atom) applies to a slot group only when its own slot is unset or equal, so a pin on one slot never vetoes an upgrade in another. A drop happens only on proven joint unsatisfiability.
- A hard atom that cannot even be parsed cannot constrain anything; fall back to enforcing rather than reasoning about it.
- Backlog #210: withholding keeps the installed instance, so the installed instances count as candidates too -- a same-version slot-change reinstall has no *repo* candidate at the old slot (only the installed instance still satisfies the pin), while an upgrade always does (the old version stays visible). Without this every slot-change pin would drop for the residual report instead of withholding.

## fn is_build_time_dep_key
- Backlog #91 (S1): the dep-key axis of the reverse-dependency pin scan. Portage empties (or marks `optional`) the *build-time* keys of a built package (`DEPEND`/`BDEPEND`, `depgraph.py:4194-4247`) while the runtime keys (`RDEPEND`/`IDEPEND`/`PDEPEND`) always form hard parent edges -- so only a build-time pin is unconditionally ignorable (D1); a runtime pin from an in-graph consumer withholds (host qemu/edk2, hermetic `whblocker` cell).

## fn reverse_dependency_constraints
- Portage `depgraph._complete_graph` reaching `_slot_operator_check_reverse_dependencies`.
- `hard_want`: every atom text that targeted the package, top-level arguments included. See `rev_dep_pin_holdable` for the joint-satisfiability rule and its live grounding. Portage merges the hard-required version and reports the broken consumer against the installed instance, exit 1.
- Once any installed package would change version, slot or USE, `_complete_graph` auto-enables complete mode (`depgraph.py:8592-8648`) and re-seeds the walk from `@world`/`@selected`/`@system` (`_initial_arg_list` plus the required sets, `depgraph.py:8677-8731`), pulling every installed package **reachable from those seeds over the installed dependency graph** in as nomerge nodes -- not the whole installed universe (#54: `keeper`'s `=paired-1.0` pin is outside every target closure in the hermetic fixture and Portage never walks to it there; it only shows up when a competing hard requirement keeps `paired`'s installed instance itself in the graph, or when `keeper` is reachable from `@world`). Each reachable consumer contributes its recorded `*DEPEND` atoms to `_parent_atoms`, and a candidate has to satisfy every parent atom on its package -- so an installed consumer nowhere near the requested atom's own dependency tree can still veto an upgrade, as long as it is in that reachable closure. Live example this exists for: `net-libs/rest` does not reach `dev-libs/weston` through the requested atom's own tree, but both are `@world`-reachable, and weston's recorded `<media-libs/libdisplay-info-0.4.0:0/3=` is why Portage leaves `media-libs/libdisplay-info` at `0.3.0` while portuale used to upgrade it to `0.4.0`.
- Portuale reaches the same answer without carrying the extra ~1400 installed nodes: it scans the vdb for the consumers of each upgrade-bound `cat/pkg` directly, keeps only the ones `reachable` names or that are themselves a graph entry this pass (Portage's "or is a dep of a node already in the graph" case), and hands the surviving atoms back to the `'backtrack` loop as ordinary `slot_constraints` -- which is exactly what those parent atoms are. The re-resolve picks the highest candidate that satisfies the surviving atoms (here: the installed `0.3.0`, so the entry settles as `AlreadyInstalled`), and drops whatever the rejected version had dragged in. `reachable` is `ResolveCtx::slot_op_reachable` (`required_set_reachable_cps` over `@world ∪ @selected ∪ @system ∪` this run's own argument cps) -- the same reachable closure that gates the slot-operator-rebuild scan (`slot_operator_rebuild_scan`), computed once per resolve, not a second walker.
- Only `Upgrade`/`Downgrade` entries move a package's version out from under an existing consumer -- plus a `Reinstall` whose slot or sub-slot changed without a revbump (backlog #210): Portage `_complete_graph` treats that shape as a version change (`depgraph.py:8611-8618`, "slot/sub-slot change without revbump gets similar treatment to a version change") and re-seeds the required sets so the consumers' recorded atoms constrain selection. A `New` into a fresh slot leaves the old slot's consumers alone, and any other `Reinstall` keeps the version.
- Consumers Portage would not enforce are skipped: one whose own `cat/pkg` this run is already replacing (Portage's "this parent may need to be eliminated due to a slot conflict, so its dependencies aren't necessarily relevant", `depgraph.py:2512-2522`), one this run uninstalls to solve a blocker conflict (Portage skips a parent that `is installed and _in_blocker_conflict`, bug 612772 -- its pin cannot veto the upgrade whose blocker removes it), and one matched by `--exclude`. Portage's further `_upgrade_available` / direct-cycle escapes sit behind `if not self._too_deep(parent.depth)`, and a consumer that only entered via `_complete_graph` carries `_UNREACHABLE_DEPTH`, for which `_too_deep` is unconditionally true (`depgraph.py:7369`) -- so for exactly the consumers this function finds, Portage skips those escapes too.
- A top-level argument that settles `AlreadyInstalled` is never a source (backlog #210): it is a no-op request, not a digraph node in Portage, so its recorded atoms never become parent atoms -- even though it carries a graph entry here (`graph_cps`) and complete-mode arg-seeding pulls it into `reachable`. A consumer the required sets (`@world ∪ @selected ∪ @system`) reach stays a source, whether or not it was also requested.
- Returns `(cat/pkg, atom)` pairs to add to `slot_constraints`; empty (and the vdb scan skipped entirely) when nothing is being upgraded.
- Returns `(enforced, dropped)` pin lists, each sorted and deduped by `(cp, atom, consumer)`. Enforced pins feed `slot_constraints` exactly as before; dropped pins feed the residual installed-instance conflict report instead of masking the hard-required version into invisibility.
- `upgrading`: (`cat/pkg`, slot) -> the candidate string this run would install, whether from an Upgrade/Downgrade or a same-version slot-change reinstall.
- `already_installed_top_level` (backlog #210): every `cat/pkg` this pass leaves installed that was directly requested.
- `being_removed` (backlog #91 S1): consumers this pass uninstalls (bug 612772). A removal entry lands in `entries` through the blocker phase before this scan runs, which would otherwise make the removed consumer look "walked" (`graph_cps`) and let its runtime pin veto the very upgrade whose blocker removes it (the `whpuller`-only twin: `whtarget-1.0`'s `Uninstall` entry vs its `~whblocker-1.0` pin).
- `graph_cps`: every `cat/pkg` this pass walked, any outcome -- Portage's "or is a dep of a node already in the graph" reachability path (`_complete_graph` deep-walks the required sets, but a package the requested atoms' own tree already pulled in is a graph node regardless of that walk).
- `use_breaks` (backlog #222): (`cat/pkg`, slot) -> decided merge state for every same-version USE-change reinstall -- a `Reinstall` whose version never moves, so the stripped-constraint check always holds, but whose USE flip can still break an installed consumer's `[use]` bound (upstream `test_complete_graph.py` pg0: the `icu` flip breaks installed `cgp0q`'s `[-icu]` dep). Portage fails exactly that shape in its complete-mode end-of-walk unsatisfied-dep loop (`depgraph.py:8751-8791`), having enabled complete mode on the USE change (`complete_if_new_use`, `:8620-8637` -- portuale's `complete_graph_auto_enable` already re-resolves with `complete = true` for it, so these sources are collected only in complete mode). Upgrade/Downgrade/slot-change entries are deliberately NOT sources: their version machinery owns the break, and a USE interplay there (the #91 qemu shapes) belongs to the slot-operator probe's rebuild domain, not to a failure pin.
- `dep_keys`: Portage `_add_pkg_dep_string` empties `DEPEND`/`BDEPEND` for a *built* package (an installed one always is) unless `--with-bdeps` asks for them.
- `seen`: (constrained cp, pin atom, pinning consumer) already emitted this pass.
- `meta_memo`: per-pass `Effective`-layer cache, mirroring Portage's once-per-instance `FakeVartree` overlay (`installed_dep_string`).

## fn constraints_from_consumer
- #54: Portage only ever sees a consumer if `_complete_graph`'s required-set walk reaches it, or it is itself a dep of a node already in the graph.
- Backlog #91 (S1): Portage walks an installed consumer's *effective* deps -- the live ebuild metadata (plus the vdb's own built `:=` atoms, backlog #25 S1b) when the installed version is still in the tree (`FakeVartree._apply_dynamic_deps`, bug #368725), the raw vdb record when it is gone or EAPI-gated out -- so a pin the live ebuild no longer carries is invisible to Portage (the #79 `revdepconsumer` shape: vdb `RDEPEND="<revdeptarget-2.0"`, live ebuild empty, Portage upgrades). Same layer choice as `collect_unwalked_installed_blockers` (#77).

## fn constraints_from_atom
- Backlog #210 (verbatim vs stripped probe): a same-version slot-change reinstall re-checks the recorded atom verbatim -- Portage's `_complete_graph` re-seed constrains `_select_pkg_highest_available` with the required sets' atoms as-is (`depgraph.py:8677-8754`), built `:S/SS=` bindings included. The Upgrade/Downgrade path keeps the stripped constraint (the update probe's rule, `_slot_operator_check_reverse_dependencies`, `depgraph.py:2494-2502`, backlog #24 S5: a built binding is a rebuild trigger there, never a withhold).
- #296: an atom with an explicit slot constrains that slot's instance only; an upgrade in another slot leaves it standing (`bdhold`'s `bdprov:0` is not broken by `bdprov-3` arriving in slot 1).
- Backlog #222 (USE-break check): no version in any upgraded slot breaks the stripped constraint -- but the merge's USE is already decided -- Portage fails rather than reflipping it (the EAPI-8 oracle answers `success=False` with no `slot_collision_solutions`) -- so a broken USE pin is dropped for the residual installed-instance report, never enforced (enforcing the verbatim atom would let the re-resolve silently flip the USE back and succeed where Portage fails). The stripped probe must still hold the merge: otherwise the break is not USE at all (and a consumer already broken before the run is Portage's `initially_unsatisfied` skip). Built slot-operator pins stay out: their domain owns the version/USE interplay through rebuilds (#24), and build-time keys stay out like the D0 ignore (Portage empties a built package's `DEPEND`/`BDEPEND` before registering any parent atom, `depgraph.py:4194-4247`).
- Portage stores the parent atom evaluated against the parent's own USE (Portage `Atom.evaluate_conditionals` at edge time, `lib/portage/dep/__init__.py:1387`): the collision renderer classifies USE reasons from the unconditional forms alone (Portage `slot_collision.py:331-389`), so the pin carries the evaluated text (`[-icu]`, not `[!icu?]`) while `raw_atom` keeps the recorded verbatim form for notices.
- The enforced/dropped atom text: verbatim when a built slot-operator binding fired through a slot-change reinstall (withholding the reinstall keeps the recorded binding satisfied); the stripped constraint otherwise.
- Enforce when some failing slot can still hold the pin alongside the hard requirements; otherwise drop it for the residual report (the hard-required upgrade merges anyway). `hard_want` is every atom text that targeted the package this pass, top-level arguments included -- Portage's whole-atom-set selection. A slot-stripped (built-`:=`) pin constrains no slot in particular, like its normalised form; a plain slot pin stays scoped.
- Backlog #79 (D0 + the shared S0 with #78): Portage only ever consults such a pin from the slot-operator update probe (`_slot_operator_check_reverse_dependencies`, `depgraph.py:2472-2538` -- which parents that probe visits, which it skips). A holdable *plain* pin (no built `:=`) from a *build-time* key (`DEPEND`/`BDEPEND`) is therefore ignored entirely: a built package's build-time deps are `optional` edges (Portage `depgraph.py:4194-4247`, `optional=(pkg.built or ...)` at `:4277-4287`), emptied outright when `bdeps` is off, and never withhold an upgrade -- Portage merges without filtering on the pin and without reporting it (D1: `rdctarget-1.0`'s recorded `~rdcblocker-1.0` neither drops `rdcblocker-2.0` nor leaves a residual row -- the only `B` is the new version's own `RDEPEND` blocker). Enforcing it here is what silently settled the old version (D0's trace).
- Backlog #569: demoted `||` arms (see `or_branch_vdb_deselected_set`'s
  notes — computed once per depstring in `constraints_from_consumer`)
  return before any probe runs: they are no parent atoms, so neither the
  version check, the USE-break check, nor any pin may see them. The check
  compares the flat token and the stripped constraint, covering verbatim
  and normalised members alike.
- Backlog #568: a built atom on an upgrading cp is recorded into
  `built_seen` (verbatim, `[use]` stripped like the verbatim pin path)
  unconditionally — even when the stripped constraint matches everything
  and no pin of its own results. The post-scan attach in
  `reverse_dependency_constraints` is the only reader.
- Backlog #91 (S1) narrows that ignore to build-time keys: a holdable plain pin from a *runtime* key (`RDEPEND`/`IDEPEND`/`PDEPEND`, never emptied as a class) IS a slot-conflict parent edge once its consumer is in the graph, and Portage's direct solve keeps the installed instance (host qemu/edk2: `Slot conflict ... remove: ...-202608 / keep: ...-202408-5`; hermetic oracle: the `whblocker`/`whtarget`/`whpuller` fixtures -- runtime `~` pin from a non-replaced consumer withholds `whblocker-2.0`, while the unreachable-consumer twin merges with the uninstall/`b` rows). Enforcing it here as an ordinary slot constraint settles the same answer through the re-resolve. The three rejections of #91 §1 stand: no use-dep gating (qemu's own pin carries use-deps and withholds), no operator narrowing (any operator holds when runtime + in-graph), only the key axis moves. Still enforced: built slot-operator pins (the probe's own domain, #24's rebuilds) and every not-holdable pin (dropped for the residual report, keeper's shape).

## struct UseBreakSource
- The candidate string this run installs (same shape as `reverse_dependency_constraints`' own `upgrading` values), plus the merge's own effective USE set and declared IUSE -- Portage's `violated_conditionals(child_use, parent_use)` needs both sides.

## fn merge_use_state
- The candidate's own effective flags (`candidate_iuse_and_use` -- the profile-resolved set this run merges with, or the baked set for a binary). `None` when no readable candidate exists (an unreadable md5-cache entry); callers fall back to ignoring USE, today's behaviour.

## fn merge_use_changed_vs_installed
- Portage `_complete_graph`'s own USE-change trigger (`depgraph.py:8620-8637`). This is a node-vs-vdb comparison Portage makes unconditionally -- it does NOT go through `--newuse`/`--changed-use` (a bare, reasonless `[ebuild R]` for an explicitly-requested installed package carries no `changed_flags` yet still flips USE and still enables complete mode). `None` when the merge side is unreadable (no md5-cache entry); callers fall back to the entry's own `changed_flags`, today's behaviour.
