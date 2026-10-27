# Task 10b S0: mapping Portage's `||` views onto portuale (2026-10-27)

Analysis only. HEAD `1a4c3117` (10a landed). Portage 3.0.83 in `3rdparty/portage`.
D = `lib/_emerge/depgraph.py`, DC = `lib/portage/dep/dep_check.py`,
PT = `lib/_emerge/resolver/package_tracker.py`, PD = `lib/portage/dep/__init__.py`.
Rust paths are under `crates/portage-repo/src/` unless they start with `crates/`.
Probes ran with `target/release/portuale` (mtime 14:09, built from the 10a tree before it
was committed at 14:30; not rebuilt) on a scratch copy of `pmtest/fixtures` in
`/var/tmp/claude-10b-s0` (deleted at the end).

## Summary of answers

| # | Answer |
|---|---|
| 1 | The walker has no persisted "was recursed" flag. Its decision is `Deep::recurses_at(depth)` in `enqueue_installed_deps` (`resolve/pass/unresolved_dep.rs:418-420`). Add a per-pass record there, and record the walker's `||` pick at `pop_disjunction` (`resolve/disjunction_stack.rs:204`, installed site `resolve_installed_group` :526, `disjunction_preference` call :616). The scan keeps the walker's recorded pick for a recursed consumer and never recomputes it. |
| 2 | `ctx.complete` alone is wrong: it is false in the `--deep` re-walk. Add a separate "Portage complete mode entered" signal. Details in section 2. |
| 3 | The PostMerge set cannot be passed through the existing args (`root` points at the vdb, with no way to subtract replaced packages). Add a required `view: OrView` field. All seven walker sites pass `Normal`, which delegates to today's functions. |
| 4 | The no-pin rule is "matches no installed (vdb) package", not "matches nothing in the PostMerge set". The sites are D:3419-3420 and D:8803-8809, and D:3438-3453 covers one case only. PostMerge only. |
| 5 | Portuale never gives sibling virtuals leaked atoms. The 9 perl `=5.42*` rows exist today only through the installed-arm heuristic (`rev_dep.rs:586`). |
| 6 | T1 gives 0/20 and T2 gives 4/20, both reachable as printed. |
| - | One plan correction (the no-pin site) and two gaps (slot-conflict complete mode, plain atoms that match nothing). No blocker. |

## 1. Depth and origin

### Portage

Whether a consumer is `Normal` is decided at the recursion gate in `_add_pkg`:
`recurse = deep is True or not self._too_deep(depth + 1)` (D:3834).
If the node is installed and not `recurse`, it goes to `_ignored_deps` (D:3838-3839).
It is pushed only when not `previously_added` (D:3841-3842). Portage keeps a depth per node
(`pkg.depth`, D:3806-3816) and this is where it is used.

### Portuale: where the decision is made

- `Deep::recurses_at(self, depth)` (`resolve/queue.rs:26`) is the same predicate:
  `Unlimited` is true, `Bounded(n)` is `depth < n`, `NotRequested` is false.
- The only caller is `enqueue_installed_deps` (`resolve/pass/unresolved_dep.rs:402`, the test at
  :418-420). It runs in `record_unresolved_dep` (:848) after `dedup_repeat_visit`, so a repeat
  visit never re-walks. This matches Portage's `previously_added` (D:3841).
- `ctx.deep` is `Unlimited` for `--emptytree` (`resolve/ctx.rs:238-245`), as in Portage
  (`create_depgraph_params.py:176-179`).
- No walker field records the decision. `PassState.resolved_depths` covers merge-bound nodes only
  (`pass_state.rs:85`), and the `GraphEntry.deps` list cannot tell "no deps" from "not walked".
  The scan, `reverse_dependency_constraints` / `constraints_from_consumer`
  (`resolve/rev_dep.rs:299`, `:493`), reads the whole vdb after the walk and has no per-consumer
  depth. It also cannot re-derive one, because Portage's depth is the minimum over all paths.

### Rule for the implementer

- Add `PassState.installed_walked: HashMap<(cat, pkg, version), HashSet<String>>`.
  An entry is inserted in the `if` branch of `enqueue_installed_deps`. The value is filled with
  the atoms chosen from its `||` groups.
- The value is filled in `pop_disjunction` (`disjunction_stack.rs:204-275`).
  `DeferredSite::Installed` has `owner` and `owner_version`.
  The `chosen` tokens (`resolve_installed_group` :526, `disjunction_preference` call :616,
  `tie_break` `promote_tied_alternative` :549) are inserted before `enqueue_chosen_atoms`.
- Carry the map through `PassResult` (`pass/assemble_pass.rs`). Pass it to
  `ReverseDependencyConstraintsArgs`, built at `feedback/reverse_dep_pins.rs:56` and
  `resolve/driver.rs:257`. The driver's fallback call must pass it too.

### Skip or reproduce

- For a walker-recursed consumer the scan **keeps the walker's recorded pick**. Every
  `||`-group atom outside the recorded set goes into `demoted`; plain atoms are untouched.
- Do not recompute the pick in the scan. Portage decides it at pop time against the graph as it
  stands then, and `entries_view` at :606 is `state.entries` at that moment. Final `pass.entries`
  differ.
- The cheaper alternative is to drop all `||` atoms of recursed consumers. It differs only when
  the walker's chosen arm is contradicted by an upgrade the walker did not itself conflict on.
  Not chosen, because it is less exact.
- Evidence that this is needed now: `emerge -pv --deep dev-libs/p589apull` on HEAD gives Portage's
  rows (`U prov-5.44`, `N core-1.0`, `N pull-2`) but ends rc 1 with a spurious slot conflict
  (`=p589aprov-5.42*` from the installed virt). The scan's installed-arm heuristic pins the arm
  the walker did not pick. Portage's `s0a-t1-deep` capture is rc 0 (`t9/captures/s0a-t1-deep.txt`).
- Known edge: Portage lowers `pkg.depth` on a later, shallower visit but does not re-push it, so
  a parked node stays PostMerge. Portuale's first-visit-wins dedup gives the same result.
  The opposite order (shallow first) records "recursed" first, also the same.

## 2. Complete mode per pass

### What the scan sees in each portuale run

`run_resolve_phases` (`crates/portuale/src/pretend/resolve_phases.rs:388-535`) makes up to two resolves:

| run | `complete` | seeds | `slot_op_reachable` / `world_reachable` | scan candidates |
|---|---|---|---|---|
| phase 1 (always) | false | none (`with_seeds=false`; `blocker_retry_seed_atoms` only) | empty | only consumers whose cp is in `graph_cps` (`rev_dep.rs:523`) |
| phase 2, `want_complete` (`--complete-graph`, or no `--deep` and auto-enable) | true | fed | non-empty | `reachable` (installed closure of the seeds, minus replaced) plus `graph_cps` |
| `--deep` re-walk, `else if auto_enable` | **false** | fed | non-empty | as phase 2 |
| final `get_best_run` pass (`driver.rs:218-232`) | same as its run | same | same | `backtrack_max=0` |

`live_reachable_consumers` returns the empty set when `slot_op_reachable` is empty
(`feedback/reverse_dep_pins.rs:16`). So `ctx.complete` is **not** a usable signal. It is false
in the `--deep` re-walk, which Portage runs in complete mode.

### Mapping from Portage's three sources

| Portage source | Portuale signal |
|---|---|
| `--complete-graph` explicit (`"complete"` in myparams; `create_depgraph_params.py:122-124`) | phase 2 with `complete_graph` set: `ctx.complete = true`, seeds fed |
| auto-detection (D:8635-8691), on by default, not gated by `--deep` | no `--deep`: phase 2 (`ctx.complete = true`). With `--deep`: the re-walk (`ctx.complete = false`, seeds fed) |
| slot conflict while backtracking is allowed (`_resolve_conflicts` D:9473-9480) | none today. Closest: `!pass.slot_conflicts.is_empty() && ctx.backtrack_max > 0`. D:9475-9476 turns it off in the final no-backtrack pass, which is `backtrack_max == 0`. `_accept_blocker_conflicts` is not modelled |

New field `ResolveCtx::portage_complete = req.complete || !req.config.complete_seed_atoms.is_empty()`
(`resolve/ctx.rs:336`). Do not reuse `ctx.complete`: `slot_expand.rs:55` and
`pass/initial_outcome.rs:53,63` also read it. The per-pass signal is
`portage_complete || (slot-conflict source)`.

Edge: with `--ignore-world`, `complete_seed_atoms` is empty even in the complete run. Then
`req.complete` still covers phase 2, but the `--deep` re-walk loses the signal. Document it.

### What the scan does with the signal

- `Normal` consumer (in `installed_walked`): the walker's recorded pick, whatever the signal.
- Not in `installed_walked`:
  - signal off: **no `||` pin at all** (never evaluated, `views.md` `s0a-t1-nocomplete`);
  - signal on: `PostMerge`.
- Phase 1 matters. The scan runs there over the parked installed nodes (`graph_cps`).
  Today it pins them even though Portage would not evaluate them.
  Evidence on HEAD (p589a family, T1):
  - `--complete-graph-if-new-ver=n --complete-graph-if-new-use=n --rebuild-if-new-slot=n`:
    4/20, `N pull-1` plus a skip block. Portage (p569 capture `s0a-t1-nocomplete`): `U prov-5.44`
    plus `N pull-2`, no pin.
  - `--complete-graph` explicit: HEAD prints only `N p589apull-2` (no `U p589aprov-5.44`).
    The wrong phase-1 heal locks the merges for phase 2. Expect this to resolve with 10b.
    Verify it in the RED.
- For the slot-conflict source, phase 1 has `reachable = {}`, so only parked consumers can be
  evaluated. World-only consumers need seeds from `config.blocker_retry_seed_atoms`
  (`resolve_phases.rs:190`) or `ctx.blocker_retry_closure`. Gap, see the final list.

### RED per source (fixtures all exist; Portage captures marked)

1. `--complete-graph` (explicit): `--complete-graph-if-new-ver=n --complete-graph-if-new-use=n
   --rebuild-if-new-slot=n --complete-graph -p dev-libs/p589apull`. Expected `U p589aprov-5.44`
   plus `N p589apull-2`, 0/20. Capture: p569 `s0a-t1-cg`. Needs a p589a Portage capture.
2. Auto-detection, no `--deep`: plain T1 `-p dev-libs/p589apull`. Portage `fixtures-portage-T1-aprov.txt`.
3. Auto-detection, `--deep`: `-p --deep dev-libs/p589apull` (the Normal case, with the re-walk).
   Expected `U prov-5.44`, `N core-1.0`, `N pull-2`, rc 0. Capture: p569 `s0a-t1-deep`.
4. No complete mode: the three `=n` flags only. Expected `U prov-5.44` plus `N pull-2`, no pin. Capture p569 `s0a-t1-nocomplete`.
5. Slot conflict while backtracking: no existing fixture shows it. It needs a conflict among new
   packages only, so that auto-enable stays off, plus a world-only installed consumer whose `||`
   is contradicted. Do not build it in 10b. Record as a gap with a backlog number.
6. World-only PostMerge consumer under `--deep`: `--deep =dev-libs/p589aprov-5.44` with
   `dev-libs/p589avirt` in world (p569 capture `s0a-revdep-deep-inworld`). Needs a p589a capture.

## 3. The PostMerge availability set

### Portage

- `_graph_trees`: `porttree` = `vartree` = the tracker fakedb (D:764-771).
  `package_tracker.match(root, atom)` is graph nodes plus installed packages not in `_replaced_by`
  (PT:230-252). A node replaces an installed package of the same slot atom **or** the same cpv
  (PT:137-143). An installed package that is itself a node is in `_cp_pkg_map`, so it stays
  available even though it replaces itself.
- Every probe in `dep_zapdeps` reads this one set: availability (DC:469-475),
  `all_installed` / slot (DC:597-616), the unavailable bins (DC:664, 681, 718), and both
  downgrade guards (DC:490, DC:541).
- `want_update_pkg` is false in complete mode (D:7359).

### Portuale: building the set

Per `(root, cp)`:

- nodes = `pass.entries` with a version: `merge_bound_cpv(e)` (New/Upgrade/Downgrade/Reinstall;
  `resolve/merge_bound.rs:21`) plus `AlreadyInstalled{version}`.
  Not `Uninstall`, not `NoVisibleCandidate`.
- survivors = `installed_refs(root, cat, pkg)` (`installed.rs:99`) that no merge-bound entry replaces.
  Replaced means: entry slot equals the ref slot, or entry version equals the ref version.
  `GraphEntry.oldbest` (`entry.rs:66`) is the display list; use it as a cross-check, not as the source.
- available(atom) = the atom matches a node or a survivor.
  Blockers are vacuously available, as today in `atom_currently_satisfiable`.

### Can `disjunction_preference` take it through its existing args?

No. Its probes read the **tree** (`atom_currently_satisfiable` :234,246 via
`gate.repo_candidates`, `masked_deps.rs:14`) and the **vdb** by `root`
(`atom_cp_installed` :211,344,363, `atom_matches_installed`, `atom_installed_in_slot_of`).
`entries` only feeds `atoms_all_in_graph` and the downgrade guard. Nothing can subtract a replaced
installed package or hide the tree.

So add a required `view: OrView { Normal, PostMerge(&PostMergeSet) }` to `DisjunctionPreferenceArgs`
(`or_preference.rs:129`) and to `PromoteTiedAlternativeArgs` (:381). The tie-break reads the same
tree and vdb probes via `highest_available_candidate_ignoring_use`, `atom_cp_installed`,
`atom_installed_in_slot_of` and `atoms_all_in_graph`.

Byte-identical forward walk: the seven walker sites (`disjunction_stack.rs:404,616`,
`pass/enqueue_deps.rs:480`, `slot_expand.rs:574`, `enqueue.rs:604`, `blockers.rs:463`, plus the
`promote_tied_alternative` callers) pass `OrView::Normal`. Route every probe through one small
`Avail` helper whose `Normal` arm calls the existing functions unchanged. Also update the two tests
in `tests/tests_162.rs:727,1280`. The full gate proves identity.

PostMerge arms of the helper:

- availability: `PostMergeSet.matches`;
- `all_installed`, `all_installed_slots`: the set (a `virtual/` cp counts as installed);
- `all_in_graph`: nodes only;
- downgrade guards (:26): `avail_pkg` and `highest_in_slot` from the set. An arm that only a
  replaced installed version satisfies leaves at DC:475 before either guard (`views.md` item 6);
- `want_update` stays false (already pinned).

Selection API: `installed_consumer_or_selection(args) -> HashSet<String>` (the arm texts not
selected), in `or_preference.rs`. Internally it calls `use_reduce_flat_disjunctive` on the consumer's
tokens from `reduced_dep_tree`. This is the existing path used by `resolve_installed_group`. It gives the
chosen atoms directly, so deselected = flat atoms minus chosen.

Deviation from the plan: with recorded picks for `Normal` consumers, the scan only ever calls this with
`PostMerge`. Keep `Normal` in the enum for the walker. RED 3 of the plan
(`consumer_or_normal_walk_downgrade_guard...`) is then a scan-level test on the R4 keeper cell
(a recorded pick is used) rather than a unit test of the function.

## 4. The arm that matches nothing

- Portage forms no pin in two places. Both ask the **vdb**, not the post-merge set:
  - `_complete_graph` walks with `allow_unsatisfied=True` (D:8795-8797, D:3419-3420), then for
    each unsatisfied dep tests `vardb.match_pkgs(dep.atom)` and files "initially unsatisfied" when
    empty (D:8803-8809). This is the main site for PostMerge consumers.
  - D:3438-3453 is the same rule for `_create_graph` calls with `allow_unsatisfied=False` in
    complete mode, for parents with depth `UNREACHABLE` or `>= requested_depth`. It is the narrower
    site. The plan names only this one.
- If the vdb has a match but the post-merge set has none, Portage adds the installed package and
  reports a slot collision (D:8814-8834). That is today's pin. So the rule is:
  **drop a selected arm only if it matches no installed instance.**
- Normal: Portage's walk pulls the arm in (`N p589acore`). The walker owns it; the scan keeps the recorded pick.
- Why vdb and not "matches nothing in the set": T3 (`p589cvirt`, `=5.42*` first) selects `=5.42*`,
  which the vdb matches, so it must still pin (Portage 4/20 with the `=5.42*` row). T1's
  `=5.40*` matches no installed package, so it drops.
- Where in `rev_dep.rs`: in `constraints_from_consumer` (:586), after the selection, add to
  `demoted` every selected arm atom for which `best_installed_matching`-style vdb matching
  (`graph_lookup.rs:53`) is empty. `constraints_from_atom` (:612-614 doc) then skips it before the
  `failing` loop. PostMerge only.
- Note: plain (non-`||`) atoms that match no installed package follow the same Portage rule and
  pin today. Out of scope for 10b (L0 risk). File as a residue.

## 5. Q19 sibling virtuals

### Where portuale does it today

Nowhere explicitly. The nine perl virtuals that list `=5.44*` first keep a `=5.42*` pin today only
because `or_branch_vdb_deselected_set` (`rev_dep.rs:586`, defined at
`feedback/reverse_dep_pins.rs:295`) prefers the arm matching the installed instance. Plain 10b removes
that and the nine `=5.42*` rows per perl cell (`t9/s0b.md` section 3, "Predicted moves").
Portuale treats virtuals as ordinary nodes (`disjunction_stack.rs:458`), with no inline expansion.

### Portage

- `_expand_new_virtuals` runs a **separate** `dep_check` per virtual and returns that virtual's
  chosen atoms into the parent's list (DC:195-260). Every atom of the virtual's dep string is added to
  `atom_graph` under the virtual (DC:132, 141, 192).
- `_select_atoms_highest_available` (D:6122-6167) keeps a virtual's atoms whose `id()` is in
  `chosen_atom_ids`, the ids of all atoms chosen in this parent's call.
- With interning (PD:100-130, key at PD:1577-1595) equal atoms are one object.
- One call is one dep string of one parent: one dep key, with `parent in atom_graph`.
  The parent can be installed or merge-bound.
- The intern key: atom text (carries repo, USE, blocker), `_eapi`, `_extended_syntax`, `_build_id`,
  the unevaluated atom's (text, eapi), `orig_atom`'s (text, eapi), blocker overlap.
- Detail found here: `dep_check` passes `eapi=None` when the declaring package is **installed**
  (DC:870-871). So for installed virtuals EAPI never differs. EAPI can split the key only for
  non-installed (merge-bound) virtuals. Verified by reading only.

### Portuale data for the key

- Text: the atom after `use_reduce` (`flat_dep_atoms`, `installed.rs:656`; it returns a
  `HashSet`, so take order from `reduced_dep_tree`).
- Unevaluated form: `QueueItem.unevaluated` / `evaluate_atom_conditionals`.
- EAPI: `metadata["EAPI"]` through `installed_walk_metadata` (as used at `pass/unresolved_dep.rs:445`).
  Use `None` for an installed declaring package; the real EAPI for a merge-bound one.
- Parent grouping: the walker's `pass.slot_pullers` (target cp to pullers with atom;
  `queue.rs:~78`) for merge-bound and Normal parents. For PostMerge parents, the scan reads the
  parent's own dep string per key and finds its `virtual/*` atoms.
- Algorithm (new pre-pass in `rev_dep.rs`, per parent and dep key): collect the chosen atoms of the
  parent's plain atoms and of every virtual's own pick (the same selection as section 3). A virtual
  then pins every atom of its own dep string that is intern-equal to any chosen atom.

### REDs and fixtures

- Orders: two installed virtuals, same parent, arms in different orders. One lists `=prov-5.44*`
  first and `=prov-5.42*` second; the sibling lists `=prov-5.42*` first. Expected: the first also
  carries `=5.42*` (Portage's attribution).
- EAPIs: the same two virtuals, but one is merge-bound with another EAPI. The `=5.42*` atoms are not
  equal then, so no leak.
- Existing fixtures do not cover either: `p589?virt` are single virtuals with one parent each.
  New fixtures are needed. Collision check done: no `p590*` or `q19*` names exist under
  `../pmtest/fixtures/repo` (only `p563`, `p569`, `p589a/b/c`). Suggest the `p590v*` family, with a
  fresh `ls` before adding. The EAPI RED needs a Portage capture to confirm the `eapi=None` reading.

## 6. T1 / T2 predictions

Probes on HEAD: T1 4/20 `N pull-1` plus skip row; T2 2/20 with two skip rows; T3 4/20 (as `t10a/counts.md`).

### T1 `p589apull` (Portage 0/20, `U p589aprov-5.44` + `N p589apull-2`)

- No `--deep`: auto-enable fires, so phase 2 runs with the signal on (section 2).
- `p589avirt` is not walker-recursed (`NotRequested`), so it is PostMerge (section 1).
- PostMerge set: 5.42 is replaced by the 5.44 node; `=5.40*`, `=5.38*` and `~p589acore` are absent
  (the fakedb has no tree). Nothing is available, so every arm falls to `other` and the first listed,
  `=5.40*`, is chosen (matches `views.md` `s0a-t1-nodeep`).
- `=5.40*` matches no installed instance, so it is dropped (section 4). No pin, no conflict.
- Phase 1 also forms no pin, because the signal is off there. Phase-1 locked merges are now right.
- Result: 0 restarts, rows as Portage. Parts that produce it: phase-1 no-pin, PostMerge view, the vdb no-match drop.

### T2 `p589bpull` (Portage 4/20, `N pull-1`, skip row `p589bprov:0/5.42=` only)

- Same selection: `=5.40*` is dropped, so the `=5.42*` row disappears (today's second row).
- The plain built atom `p589bprov:=` (`:0/5.42=`) still pins (10a) and is the only conflict parent.
- After 10a the two choices sort `5.42` first (counts 1 versus 2 atoms), which gives the 2/20
  settle-early path (`t10a/after/fx-p589bpull.err`). Removing the extra atom ties the counts, so
  the order becomes the T3 order (`5.44` first, `5.42` second) with Portage's sequence in
  `t9/t2-abi.md`: two conflict tries, two missing-dependency tries, settle.
- Prediction: 4/20 and one skip row. The tie-break reasoning is inferred; the RED confirms it.
- **Printed count:** T3 has the same trace shape on HEAD (tries 1-5, ending in a Settle) and prints
  4/20. The "one short" bug (#600, `driver.md` "exit count") only hits cells that end in a dead end
  on an argument (k3, igw, cgp VER, needer, triangle). T2 ends in Settle, so 4/20 is reachable as printed.

### T3 stays

`=5.42*` is the first arm, matches the vdb, so it stays pinned: 4/20 (guard against over-dropping).

## Plan for the implementer

Files and functions:
1. `resolve/pass_state.rs`, `resolve/pass/unresolved_dep.rs:418`, `resolve/disjunction_stack.rs:204`,
   `pass/assemble_pass.rs`: record `installed_walked` and the walker's picks; add to `PassResult`.
2. `resolve/ctx.rs`: `portage_complete`. `feedback/reverse_dep_pins.rs:54` and `resolve/driver.rs:257`:
   pass `installed_walked`, the signal and the PostMerge set into `ReverseDependencyConstraintsArgs`.
3. `resolve/or_preference.rs`: `OrView`, an `Avail` helper, the `view` field on both args structs,
   `installed_consumer_or_selection`. All walker sites and `tests/tests_162.rs` pass `Normal`.
4. `resolve/rev_dep.rs:586`: the three-way choice (recorded / PostMerge / none); drop vdb-unmatched
   selected arms; sibling-virtual pre-pass.
5. `feedback/reverse_dep_pins.rs`: remove G2 (`or_branch_superseded`, :`file_consumer_pin_conflicts`)
   and, unless the probe site keeps them, the helpers. S0f was skipped, so
   `slot_operator/installed_parent_update_probe.rs:129` keeps `or_branch_vdb_deselected_set`
   with its helpers.
6. Docs per plan, plus a note for the section 4 plan correction.

RED list (in order):
- Normal skip: `--deep p589apull` (rc 0, no conflict; currently rc 1).
- Phase-1 no pin: the three `=n` flags.
- Explicit `--complete-graph`; auto-detect without `--deep`; auto-detect with `--deep`.
- PostMerge picks first available arm; PostMerge keeps an unreplaced installed arm; "selected arm
  that matches no installed package forms no pin"; T3 keeps its `=5.42*` pin.
- R4 keeper (Normal, recorded pick, downgrade guard).
- `t1_swap_noslotop_merges_the_update_like_portage`, `t2_swap_slotop_matches_portage`.
- Q19 orders and EAPIs; world-only PostMerge consumer under `--deep`.
- pmtest CASES rows at the end of `CASES`; corpus drift only on T1.

## Cannot be mapped (or not fully)

No item blocks the mapping, so §P is not triggered. Record these:
1. Plan correction: D:3438-3453 is not the main no-pin site; D:8803-8809 is, and the test is
   vdb-match, not "matches nothing". Amend the plan text in 10b.
2. Slot-conflict complete mode (D:9473-9480): only approximable (`slot_conflicts` plus
   `backtrack_max > 0`); world-only consumers are invisible in phase 1. No fixture. Gap, needs a backlog item.
3. `--ignore-world` with `--deep`: the signal is lost (section 2).
4. Plain atoms that match no installed package still pin (section 4).
5. Q19 EAPI behaviour for installed virtuals (`eapi=None`) is from reading DC:870-871, not probed.
