# Task 9 S0c — T2 (`swap-slotop`): how the built `p569prov:0/5.42=` atom acts (#580)

Reference: Portage 3.0.83, host `/usr/lib/python3.14/site-packages` (byte-identical to
`3rdparty/portage` at tag `portage-3.0.83`, `oracle.md` header). D = `lib/_emerge/depgraph.py`,
B = `lib/_emerge/resolver/backtracking.py`, PT = `lib/_emerge/resolver/package_tracker.py`.

Cell: `swap-slotop` from `t9/playground/probe-t9c.sh` (copy of `t8/playground/probe-t8c.sh`
that keeps the `backtracking try` / `runtime_pkg_mask:` / `to mask` lines, keeps the raw
dump in `$WORK`, runs portuale with `PORTUALE_BT_TRACE=1`, and builds only the two S0c/S0d
cells). Installed `p569virt-1` RDEPEND, vdb:
`|| ( =p569prov-5.40* =p569prov-5.42* =p569prov-5.38* ~p569core-1.0 ) dev-libs/p569prov:0/5.42=`
(ebuild and md5-cache carry `dev-libs/p569prov:=` instead of the built atom).
`WORK=/var/tmp/claude-s0c/work`, written `$W` below.

## Runs (full argv)

| capture | argv | rc | result |
|---|---|---|---|
| `captures/portage-c-swap-slotop.{trim,bt}.txt` | `PYTHONHASHSEED=0 PORTAGE_CONFIGROOT=$W/swap-slotop.cfg ROOT=$W/swap-slotop emerge --ignore-default-opts --getbinpkg=n --usepkg=n --color=n -pv --debug dev-libs/p569pull` | 0 | 4/20, `N p569pull-1`, skip row `p569prov:0/5.42=` only |
| `captures/instrument-swap-slotop.txt` | `PYTHONHASHSEED=0 PORTAGE_CONFIGROOT=$W/swap-slotop.cfg ROOT=$W/swap-slotop python3 -I playground/instrument.py --ignore-default-opts --getbinpkg=n --usepkg=n --color=n -pv dev-libs/p569pull` | 0 | same: 4/20, `N p569pull-1`, same skip row |
| `captures/addpkg-swap-slotop.txt` | same, with `playground/instrument-addpkg.py` | 0 | same |
| `captures/counterfactual-swap-slotop.txt` | same, with `playground/counterfactual-hide-installed.py` (NOT Portage: see t3-sequence.md) | 0 | 1/20, `U p569prov-5.44` + `N p569pull-2` |
| `captures/portuale-c-swap-slotop.{trim,full}.txt` | `PORTUALE_BT_TRACE=1 PYTHONHASHSEED=0 PORTAGE_CONFIGROOT=$W/swap-slotop ROOT=$W/swap-slotop portuale emerge --ignore-default-opts --getbinpkg=n --usepkg=n --color=n -pv dev-libs/p569pull` (binary `target/release/portuale`, 2026-10-27 07:13, tree at `41a7d941` + uncommitted scheduler edits) | 0 | 2/20, `N p569pull-1`, skip rows `:0/5.42=` **and** `=5.42*` |

Each env also sets `PORTAGE_RUNNING_ROOT=$W/swap-slotop DISTDIR=$W/swap-slotop/distfiles
PORTAGE_TMPDIR=$W/tmp` (the script's `run`). The `instrument*.py` hooks wrap
`depgraph.select_files`, `_process_slot_conflict`, `_slot_conflict_backtrack_abi`,
`_slot_operator_update_probe[_slot_conflict]`, `_slot_confict_backtrack`, `_add_pkg`,
`_want_installed_pkg` and `Backtracker._add`, printing only; they change nothing (same
rc, count and rows as the plain `--debug` run).

Reading aid: `backtracking try N` is printed **before** `display_problems()` of the
previous pass and before the next node's `runtime_pkg_mask:` (D:12378-12393), so in the
`--debug` dump "try N" heads the problems of pass N-1 and the mask of pass N.

## 1. Is the built atom a parent atom on the installed node? Yes

- Pass 0. The `||` is popped in complete mode and selects `=dev-libs/p569prov-5.40*`
  (`trim.txt:60008-60011`, `Candidates: ['=dev-libs/p569prov-5.40*']`), which matches no
  package, so `=5.42*` is **not** a parent atom (as `oracle.md` B found for T1).
- The remaining atoms `dev-libs/p569prov:=` and `dev-libs/p569prov:0/5.42=` are walked in
  complete mode (`trim.txt:905-915`). In complete mode `_select_pkg_from_graph` (D:8538,
  set at D:8709) reads the PT fakedb, which hides installed 5.42 because the 5.44 merge
  node replaces it (PT:249-252). `:0/5.42=` therefore has no match, lands in
  `_unsatisfied_deps`, and `_complete_graph`'s loop (D:8803-8835) re-reads the **raw vdb**
  (`vardb.match_pkgs`, D:8805-8806) and adds installed 5.42 as a graph node with that dep
  (D:8834). `addpkg-swap-slotop.txt`:
  ```
  T9 === pass 0
  T9 _want_installed_pkg(dev-libs/p569prov-5.42) -> True
  T9 _add_pkg masked=False dev-libs/p569prov-5.42(installed) parent=dev-libs/p569virt-1 atom=dev-libs/p569prov:0/5.42= complete_mode=True select_package=_select_pkg_from_graph chain=_complete_graph:8834 <- _resolve_conflicts:9482 <- altlist:9354 <- _resolve:5696 <- ...
  ```
  The D:8816-8832 slot-operator rebuild probe is skipped because
  `_want_installed_pkg(5.42)` is True (D:8818), so the add happens unconditionally.
- The conflict on `dev-libs/p569prov:0` then has the slot nodes `[5.44 (ebuild), 5.42
  (installed)]`. Its parent atoms before D:2189 (`instrument-swap-slotop.txt`):
  ```
  parent_atoms[dev-libs/p569prov-5.44(ebuild)]=["(dev-libs/p569pull-2(ebuild), '=dev-libs/p569prov-5.44*')", "(dev-libs/p569virt-1(installed), 'dev-libs/p569prov:=')"]
  parent_atoms[dev-libs/p569prov-5.42(installed)]=["(dev-libs/p569virt-1(installed), 'dev-libs/p569prov:0/5.42=')"]
  ```
  `slot_parent_atoms` is the union over both nodes (D:2161-2166). D:2189-2197 then
  matches every atom against every node with `atom.match(pkg.with_use(...))` (D:2193),
  sub-slot included:
  - `p569prov:=` (unbuilt, no sub-slot) matches both nodes, so it is added to 5.42's set
    and never becomes a conflict atom;
  - `:0/5.42=` fails 5.44 **by sub-slot** (5.44 is `0/5.44`): `conflict_atoms[(virt,
    :0/5.42=)] = {5.44}`;
  - `=5.44*` fails installed 5.42: `conflict_atoms[(pull-2, =5.44*)] = {5.42i}`.

  So both nodes are conflict packages (D:2199-2200).

## 2. Does `_slot_conflict_backtrack_abi` run? Yes; it finds nothing

Gate: `conflict_pkgs` non-empty, `_allow_backtracking`, and not
`_accept_blocker_conflicts()` (D:2202-2206). It is called once per conflict package
(D:2208-2214) and scans **all** conflict atoms each time, not only those of `pkg`
(D:2305). It skips non-`Package` parents (D:2308), unbuilt parents (D:2311), and atoms
that are neither soname nor `package and slot_operator_built` (D:2314). Only
`(virt-1 installed, :0/5.42=)` passes. For each slot node outside that atom's conflict set
(only installed 5.42) it calls `_slot_operator_update_probe_slot_conflict` (D:2317-2327,
def D:2468). Pass 0 and pass 1 both log:
```
_slot_operator_update_probe_slot_conflict(parent=dev-libs/p569virt-1(installed), atom=dev-libs/p569prov:0/5.42=, child=dev-libs/p569prov-5.42(installed)) -> None
_slot_conflict_backtrack_abi(pkg=dev-libs/p569prov-5.44(ebuild), ...) -> False
_slot_operator_update_probe_slot_conflict(parent=dev-libs/p569virt-1(installed), atom=dev-libs/p569prov:0/5.42=, child=dev-libs/p569prov-5.42(installed)) -> None
_slot_conflict_backtrack_abi(pkg=dev-libs/p569prov-5.42(installed), ...) -> False
```
No `slot_conflict_abi` config entry is filed (D:2210-2212), so both packages stay in
`remaining` and go to `_slot_confict_backtrack` (D:2215-2218). The pass-end infos are only
`['slot conflict']`.

## 3. The tries

| pass | `runtime_pkg_mask` in | what happens | feedback / nodes (B) | dead-end kind |
|---|---|---|---|---|
| 0 | — | conflict 5.44 vs installed 5.42; abi → False ×2; `_slot_confict_backtrack` builds choice[0] = mask 5.44, conflict atoms `{(virt, :0/5.42=)}`; choice[1] = mask 5.42 ebuild (similar pkg, D:2253-2264) + 5.42 installed, conflict atoms `{(pull-2, =5.44*)}`; equal length, stable sort keeps version order (D:2235, D:2273) | both added (B:186-195); DFS pops choice[1] first (B:136) | restart (slot conflict) |
| 1 | 5.42e, 5.42i (`slot conflict`, parents `(pull-2, =5.44*)`) | the masked installed 5.42 is re-added by D:8803-8834 (`addpkg`: `_add_pkg masked=True ... chain=_complete_graph:8834`), the same conflict forms (`!!! backtracking loop detected` D:2171-2181, `trim.txt:137187`); choices: [5.44 `{(virt,:0/5.42=)}`], [5.42i `{(pull-2,=5.44*)}`] (5.42 ebuild is no longer a similar pkg: masked, D:3049) | node {5.42e, 5.42i, 5.44} ADD; node {5.42e, 5.42i} REJECT duplicate of the current node (B:126; the re-set mask value is identical, B:192-194) | restart (slot conflict) |
| 2 | 5.42e, 5.42i, 5.44 | no `p569prov` is selectable (D:7910); `p569pull-2`'s `=5.44*` is a missing dependency (D:3517-3518); `_create_graph` returns 0 before `altlist` (D:5691-5693), so no slot-conflict processing | node {…, pull-2: missing dependency} REJECT by `_check_runtime_pkg_mask` (B:123, B:160-174: the 5.42 entries' only parent `pull-2` is now masked) | dead end, no child |
| 3 | 5.44 (`slot conflict`, parents `{(virt-1 installed, :0/5.42=)}`) — pass 0's choice[0] | `p569pull-2`'s `=5.44*` missing (D:3517) | node {5.44, pull-2 missing} ADD (5.44's parent `virt-1` is not masked) | restart (missing dep) |
| 4 | 5.44, pull-2 (`missing dependency`) | `p569pull` → `p569pull-1` → `=5.42*` → installed 5.42; success | — (loop breaks on `success`, D:12407-12413) | success |

`backtracked` is incremented once per failed pass (D:12414-12418), giving 4.
`instrument-swap-slotop.txt`, the Backtracker lines:
```
T9 Backtracker._add mask_steps=1 masks=["dev-libs/p569prov-5.44(ebuild):['slot conflict']"] -> ADD explore
T9 Backtracker._add mask_steps=1 masks=["dev-libs/p569prov-5.42(ebuild):['slot conflict']", "dev-libs/p569prov-5.42(installed):['slot conflict']"] -> ADD explore
T9 Backtracker._add mask_steps=2 masks=[... 5.42(ebuild), 5.42(installed), 5.44(ebuild) ...] -> ADD explore
T9 Backtracker._add mask_steps=2 masks=[... 5.42(ebuild), 5.42(installed) ...] -> REJECT duplicate (B:126)
T9 Backtracker._add mask_steps=3 masks=[... 5.42(ebuild), 5.42(installed), 5.44(ebuild), p569pull-2:['missing dependency'] ...] -> REJECT _check_runtime_pkg_mask (B:123,B:173)
T9 Backtracker._add mask_steps=2 masks=["dev-libs/p569prov-5.44(ebuild):['slot conflict']", "dev-libs/p569pull-2(ebuild):['missing dependency']"] -> ADD explore
Dependency resolution took 5.07 s (backtrack: 4/20).
[ebuild  N     ] dev-libs/p569pull-1::testrepo to $W/swap-slotop/ 0 KiB
```

## 4. Why the skip block has only `p569prov:0/5.42=`

`_get_missed_updates` (D:1544-1579) walks the final `runtime_pkg_mask`, skips installed
packages (D:1552-1555), and for the non-installed masked 5.44 (a lower 5.42 is chosen,
D:1558-1567) prints the stored `slot conflict` parent atoms (D:1573-1577). Pass 4's mask
for 5.44 is choice[0]'s conflict-atom set, `{(virt-1 installed, dev-libs/p569prov:0/5.42=)}`
(`trim.txt:154478`). `=5.42*` was never a parent atom (§1), and `p569prov:=` matches 5.44
(§1), so neither is a conflict atom.

## 5. Where portuale diverges

`captures/portuale-c-swap-slotop.full.txt`:
```
BT try=1 kind=Feedback:MissingDep
BT   mask dev-libs/p569prov neg=!=dev-libs/p569prov-5.44 reason=SlotConflict { parents: [(("dev-libs", "p569virt", "1"), "dev-libs/p569prov:0/5.42="), (("dev-libs", "p569virt", "1"), "=dev-libs/p569prov-5.42*")] }
BT try=2 kind=Feedback:MissingDep
BT   mask dev-libs/p569pull neg=!=dev-libs/p569pull-2 reason=MissingDependency { parent: ("dev-libs", "p569pull"), atom: "=dev-libs/p569prov-5.44*" }
BT try=3 kind=Settle
Dependency resolution took (backtrack: 2/20).
```
- **Try 1 = the divergence.** portuale files one mask node (the
  `dropped_pin_mask_restart` single choice, `resolve/feedback/reverse_dep_pins.rs:50`) for
  5.44 only. That is Portage's pass-0 **choice[0]**, which Portage explores *third*
  (pass 3). portuale never files choice[1] (mask 5.42 ebuild + installed), so Portage's
  passes 1-2 have no counterpart: 2/20 against 4/20.
- The mask's parents carry `=5.42*`, because portuale keeps the installed-matching `||`
  arm (`or_branch_vdb_deselected_set`, `reverse_dep_pins.rs:317`), while Portage selected
  `=5.40*` (§1). Hence the extra skip row. Since the built atom reaches the mask only as
  `built_render` of a pin (review finding 2, `rev_dep.rs:35-41`), once P1 drops `=5.42*`
  without P2 nothing opposes 5.44 (review finding 2; not re-run here).
- portuale try 2 = Portage pass 3's feedback, portuale try 3 = Portage pass 4.

## Rules for Task 10a

1. **The installed instance is a slot node, and the built atom is its parent verbatim.**
   A selected consumer atom that the post-merge view cannot satisfy but the raw vdb can
   adds the installed instance to the conflict, with that atom as its parent (D:8803-8806,
   D:8814, D:8834). The residual record's installed-side parents must hold the built
   `cat/pkg:S/SS=` atom **with its sub-slot**, as written in the vdb, not the bare name
   that `reverse_dep_constraint_atom` produces (`rev_dep.rs:35-41`). On T2 that atom is
   the only consumer conflict parent.
2. **Conflict atoms are computed over the union of every slot node's parents, by
   full-atom match.** `all_parents` = the union of the parent atoms of every slot node,
   merge and installed (D:2161-2166). For each node, every atom in the union that matches
   it, sub-slot and USE included (D:2193), joins that node's parents. Every atom that
   does not match it is a conflict atom for that node (D:2195-2197, D:2241-2246). So an
   unbuilt `:=` atom from the same consumer is never a conflict atom when it matches both
   nodes. A `||` arm that was not selected is not an atom at all. This widens
   `slot_conflict_mask_choices`, which today reads only `slot_pullers`
   (`backtrack_params.rs:66-78`; review finding 3).
3. **ABI probe first, per conflict package.** For each conflict package, try
   `_slot_conflict_backtrack_abi` (D:2208-2214). It considers only built `Package`
   parents with a soname or `slot_operator_built` atom (D:2308-2315), and probes each
   other slot node outside that atom's conflict set (D:2317-2327). A hit files the
   rebuild (`slot_conflict_abi` config, D:2210-2212) and removes the package from
   `remaining`. On T2 every probe returns None, so both packages go to
   `_slot_confict_backtrack`. portuale's counterpart is `step_slot_conflict_abi`
   (`feedback/mod.rs:317`; review finding 7), and it must see the record.
4. **Choice list, in this order:**
   - conflict packages plus the in-graph `existing_node` (D:2222-2231);
   - sorted by version, highest first (D:2235);
   - each choice is the similar packages followed by the package itself
     (D:2248-2266). Similar packages are available, not installed, not runtime-masked
     and outside the conflict set (D:3043-3050), and each one has a non-empty
     conflict-atom subset;
   - a stable ascending sort by the conflict-atom count of the last element (D:2273);
   - one node per choice, in list order (B:186-195). DFS pops the last one first
     (B:136).

   On T2: [mask 5.44 {`:0/5.42=`}], [mask 5.42 ebuild + 5.42 installed {`=5.44*`}].
   The 5.42 choice is explored first.
5. **Only the first conflict of a pass is fed back** (B:178-183). A pass that ends on a
   missing dependency does not process slot conflicts (D:5691-5696).
6. **Node rejection drives the count.**
   - A child equal to an existing node is dropped (B:96-97, B:126). The pass-1 "mask
     installed 5.42" choice equals the current node.
   - A child whose slot-conflict entry has every parent masked is dropped (B:144-176).
     The pass-2 missing-dep child is dropped because `p569pull-2` is the only parent of
     the 5.42 entries.
   - portuale has both checks (`backtracker.rs:65`, `:114-122`).
7. **What ends the run.**
   - Success breaks the loop (D:12407-12413).
   - A config change also breaks it (`need_config_change`, D:12409).
   - So does `backtracked >= max_retries` (D:12410).
   - Otherwise each failed pass costs one try (D:12414-12418).
   - When the stack is empty without success, `get_best_run` and a final
     `allow_backtracking=False` pass follow (D:12420-12439). T2 does not reach this.
8. **The skip row is the choice's stored conflict atoms.** The final mask entry of the
   non-installed package carries exactly the conflict atoms of the choice that masked it
   (B:191-194). The renderer prints those (D:1573-1577) and never prints installed
   masks (D:1552-1555). On T2 that is the single `p569prov:0/5.42=` row.
9. **Off switches.** Without backtracking, or with `_accept_blocker_conflicts()`
   (D:2202-2206), there is neither the ABI probe nor `_slot_confict_backtrack`. A node
   with `mask_steps > max_depth` is never added (B:126).

## Not established

- **Why `_slot_operator_update_probe_slot_conflict` returns None on T2.** Observed in
  both passes, not traced inside `_slot_operator_update_probe` (D:2591). A guess, labelled
  as a guess: the probe looks for a parent replacement that accepts a *different* child,
  and here the child it gets (installed 5.42) already satisfies the atom. A T2 variant
  where the probe succeeds (a rebuildable consumer ebuild) was not built.
- **Where the depstring carries both `p569prov:=` and `:0/5.42=`.** It is seen at
  `trim.txt:906`, coming from the ebuild `:=` plus the vdb built atom, but it is not
  traced to the dynamic-deps code. Rule 2 makes the `:=` atom harmless here, but a shape
  where only the unbuilt `:=` exists was not probed.
- **The order of the slot nodes when they have equal versions.** `conflict_pkgs.sort`
  uses Package ordering (D:2235), and no shape with an installed instance and an ebuild
  of the same version in conflict was probed.
