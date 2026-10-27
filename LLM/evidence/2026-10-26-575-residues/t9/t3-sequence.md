# Task 9 S0d — T3 (`orig-noslotop`): Portage's try-by-try sequence (#580)

Reference and abbreviations as in `t2-abi.md` (Portage 3.0.83; D, B, PT).

Cell: `orig-noslotop` from `t9/playground/probe-t9c.sh`. Installed `p569virt-1` RDEPEND
(the same text in ebuild, md5-cache and vdb):
`|| ( =dev-libs/p569prov-5.42* =dev-libs/p569prov-5.40* =dev-libs/p569prov-5.38* ~dev-libs/p569core-1.0 )`.
There is no slot-operator atom, and the installed arm is listed first.
`WORK=/var/tmp/claude-s0c/work`, written `$W` below.

## Runs (full argv)

| capture | argv | rc | result |
|---|---|---|---|
| `captures/portage-c-orig-noslotop.{trim,bt}.txt` | `PYTHONHASHSEED=0 PORTAGE_CONFIGROOT=$W/orig-noslotop.cfg ROOT=$W/orig-noslotop emerge --ignore-default-opts --getbinpkg=n --usepkg=n --color=n -pv --debug dev-libs/p569pull` | 0 | 4/20, `N p569pull-1`, skip row `=dev-libs/p569prov-5.42*` |
| `captures/instrument-orig-noslotop.txt` | `PYTHONHASHSEED=0 PORTAGE_CONFIGROOT=$W/orig-noslotop.cfg ROOT=$W/orig-noslotop python3 -I playground/instrument.py --ignore-default-opts --getbinpkg=n --usepkg=n --color=n -pv dev-libs/p569pull` | 0 | same |
| `captures/addpkg-orig-noslotop.txt` | same, with `playground/instrument-addpkg.py` | 0 | same |
| `captures/counterfactual-orig-noslotop.txt` | same, with `playground/counterfactual-hide-installed.py` (**modified Portage**, see §2) | 0 | **1/20, `U p569prov-5.44` + `N p569pull-2`** |
| `captures/portuale-c-orig-noslotop.{trim,full}.txt` | `PORTUALE_BT_TRACE=1 PYTHONHASHSEED=0 PORTAGE_CONFIGROOT=$W/orig-noslotop ROOT=$W/orig-noslotop portuale emerge --ignore-default-opts --getbinpkg=n --usepkg=n --color=n -pv dev-libs/p569pull` | 0 | 2/20, `N p569pull-1`, skip row `=5.42*` |

The extra environment variables are the same as in `t2-abi.md`. The non-debug count matches
`t8/captures/portage-c-orig-noslotop.nodebug.txt` (4/20).

## 1. The sequence

Slot nodes in every conflict pass: `p569prov-5.44` (ebuild, pulled by `p569pull-2`'s
`=5.44*`) and installed `p569prov-5.42`. In complete mode the `||` selects `=5.42*`
(trim `Candidates`). The fakedb hides the replaced 5.42 (PT:249-252), so the dep goes
unsatisfied. `_complete_graph` then re-adds 5.42 from the raw vdb with that dep
(D:8803-8806, D:8834; `addpkg`: `atom==dev-libs/p569prov-5.42* ... chain=_complete_graph:8834`).
The atoms are `(virt-1, =5.42*)`, which fails 5.44, and `(pull-2, =5.44*)`, which fails
5.42. `_slot_conflict_backtrack_abi` returns False for both packages: no parent atom is
soname or `slot_operator_built` (D:2314), so no probe is called (instrument: there is no
`_slot_operator_update_probe_slot_conflict` line on T3).

| pass | mask set in (`runtime_pkg_mask`) | what happens | children (B) | dead-end kind |
|---|---|---|---|---|
| 0 | — | slot conflict. Choices (D:2220-2273): c0 = mask 5.44 `{(virt, =5.42*)}`; c1 = mask 5.42 ebuild (similar, D:2253-2264) + 5.42 installed `{(pull-2, =5.44*)}`. The counts are equal (1 and 1), and the stable sort keeps version order. | c0, c1 added. c1 is popped first (B:136). | restart: slot conflict |
| 1 | 5.42 ebuild, 5.42 installed: `slot conflict {(pull-2, =5.44*)}` | The masked installed 5.42 **re-enters the graph** through D:8834 (`_add_pkg masked=True`). The same conflict forms again (`!!! backtracking loop detected`, D:2171-2181, `bt.txt`). Choices: [5.44 `{(virt,=5.42*)}`], [5.42 installed `{(pull-2,=5.44*)}`]. The 5.42 ebuild drops out of the similar list because it is masked (D:3049). | {5.42e, 5.42i, 5.44} ADD. {5.42e, 5.42i} is REJECTED as a duplicate of the current node (B:126). | restart: slot conflict (a loop) |
| 2 | 5.42e, 5.42i, 5.44 | Every `p569prov` is masked at selection (D:7910). `p569pull-2`'s `=5.44*` is unsatisfied, so this is a missing dependency (D:3517-3518). `_create_graph` returns 0, and `altlist` never runs (D:5691-5696). | The child {…, pull-2: missing dependency} is REJECTED by `_check_runtime_pkg_mask` (B:123, B:160-174): the only parent of the 5.42 entries is now masked. | **dead end, no child** |
| 3 | 5.44: `slot conflict {(virt-1 installed, =5.42*)}` (pass 0's c0) | `p569pull-2`'s `=5.44*` is a missing dependency (D:3517). | {5.44, pull-2 missing} ADD (5.44's parent `virt-1` is not masked) | restart: missing dep |
| 4 | 5.44 + pull-2 (`missing dependency`) | `p569pull-1` is selected and success follows (D:12407-12413) | — | success |

`backtracked` counts passes 0-3 (D:12414-12418): **4/20**. The skip row is pass 4's 5.44
entry, `{(virt-1, =5.42*)}` (D:1544-1579). `instrument-orig-noslotop.txt` shows the same
`Backtracker._add` lines as T2: ADD, ADD, ADD, REJECT duplicate, REJECT
`_check_runtime_pkg_mask`, ADD.

T2 and T3 have **the same sequence**. Only the consumer atom differs: the built
`:0/5.42=` on T2, the `||` arm `=5.42*` on T3.

## 2. Does Portage's 4/20 need the masked installed instance to stay in the graph? Yes, for the rows too, not just the count

- **The mechanism is not the complete-mode selectors, strictly.** In pass 1,
  `_select_pkg_from_graph` (D:8538) finds no match in the fakedb (the replaced 5.42 is
  hidden, PT:249-252), so it does not return the masked package. My hook on D:8538 and
  D:8561 never fired with a masked result. The instance comes back through
  `_complete_graph`'s unsatisfied-deps loop: `vardb.match_pkgs(dep.atom)` reads the raw
  vdb (D:8805-8806) and `_add_pkg(pkg, dep)` adds it (D:8834), and **neither step checks
  `_runtime_pkg_mask`**. `_runtime_pkg_mask` is read only at selection (D:7910) and by
  the similar-pkg iterator (D:3049). The review's wording ("the complete-mode selectors
  keep it", review finding 8) is right on the effect but names the wrong line. The
  "backtracking loop detected" print is debug-only (D:2171-2181), as `oracle.md` A notes.
- **Counterfactual.** `playground/counterfactual-hide-installed.py` changes Portage so
  that, at D:8834 only, a masked installed package is treated as absent: the dep goes to
  `_initially_unsatisfied_deps`, as D:8807-8809 do when the vdb has no match. Result:
  ```
  T9 CF: hide masked dev-libs/p569prov-5.42(installed) for =dev-libs/p569prov-5.42*
  Dependency resolution took 8.63 s (backtrack: 1/20).
  [ebuild     U  ] dev-libs/p569prov-5.44:0/5.44::testrepo [5.42:0/5.42::testrepo] to $W/orig-noslotop/ 0 KiB
  [ebuild  N     ] dev-libs/p569pull-2::testrepo to $W/orig-noslotop/ 0 KiB
  ```
  T2 gives the same rows (`counterfactual-swap-slotop.txt`, 1/20). If the installed
  instance is hidden, the first popped choice (mask 5.42) *succeeds* by breaking the
  consumer. The run then merges the upgrade, which is the opposite of Portage's heal.
- **portuale today.** `MaskChoice.installed` (`backtrack_params.rs:13-16`) makes the
  feedback add a `!=cat/pkg-ver[installed]` entry next to the version mask
  (`backtracker.rs:214-230`).
  - It is honoured by the installed-instance selection readers:
    `candidate_is_installed` (`pretend.rs:1884-1900`) and `installed_version_masked`
    (`pretend.rs:509-519`; callers `pretend.rs:804, 851, 1140, 1265, 1523, 1550`,
    `slot_conflict_backtrack.rs:123`, `outcome_fallbacks.rs:123, 142`).
  - The residual-record builder reads the raw vdb: `installed_refs` in
    `residual_conflict_for_pin`, `slot_conflict.rs:678`. That is Portage's D:8805
    counterpart, and it ignores masks.
  - Whether a P2 record re-forms in the pass after a `[installed]` mask depends on
    whether the consumer pin is still *dropped* in that pass rather than treated as
    already broken or not formed. **That is not established** (§4).
- **Verdict for 10a.** To reproduce 4/20 and `N p569pull-1`, the pass after the "mask
  installed 5.42" choice must form the same conflict record again, with the installed
  instance and its consumer parent. That conflict then feeds back
  [5.44] / [5.42 installed], and the second child is a duplicate. The installed mask
  must **not** stop the residual record or its consumer pin. If it does, the T2/T3 rows
  flip to the counterfactual's `U`+`N` (a target moving away from Portage, a stop
  condition). This is **in scope for 10a**, not a §R residue: without it 10a does not
  reach its own T2/T3 targets.

## 3. Where portuale diverges

`captures/portuale-c-orig-noslotop.full.txt`:
```
BT try=1 kind=Feedback:MissingDep
BT   mask dev-libs/p569prov neg=!=dev-libs/p569prov-5.44 reason=SlotConflict { parents: [(("dev-libs", "p569virt", "1"), "=dev-libs/p569prov-5.42*")] }
BT try=2 kind=Feedback:MissingDep
BT   mask dev-libs/p569pull neg=!=dev-libs/p569pull-2 reason=MissingDependency { parent: ("dev-libs", "p569pull"), atom: "=dev-libs/p569prov-5.44*" }
BT try=3 kind=Settle
Dependency resolution took (backtrack: 2/20).
```
- The divergence is at **portuale try 1 against Portage pass 0's feedback**. portuale
  files the single `dropped_pin_mask_restart` node (`reverse_dep_pins.rs:50`, called
  from `:575`) for 5.44. That is Portage's c0. Portage's c1 subtree (passes 1 and 2:
  the loop pass and the rejected missing-dep child) has no counterpart.
- After that the two runs line up: portuale try 2 = Portage pass 3, try 3 = pass 4. The
  rows and the skip row already match Portage. Only the count differs, 2 against 4.

## Rules for Task 10a

1. **Conflict record.**
   - Slot nodes: the merge instance and the installed instance. The installed instance
     is re-read from the raw vdb whenever a selected consumer atom is unsatisfied in the
     post-merge view (D:8803-8806, D:8834), **whether or not it is runtime-masked**.
   - Installed-side parents: the consumer's selected atoms that match it (D:4151 via
     D:8834).
   - Merge-side parents: its pullers, plus any consumer atom that matches it (D:2189-2194).
2. **Conflict atoms.** For each node, every atom in the union of all slot nodes' parents
   that does not match the node (D:2161-2166, D:2193-2197, D:2241-2246). On T3 that is
   `{(virt, =5.42*)}` for 5.44 and `{(pull-2, =5.44*)}` for 5.42.
3. **ABI step.** Run it before the choices (D:2208-2214). On T3 it is a no-op: no built
   slot-operator or soname atom (D:2314).
4. **Choice order.**
   - Version-desc (D:2235), then a stable ascending sort on the conflict-atom count
     (D:2273).
   - The "mask installed X" choice also masks every available non-installed same-slot
     package that is not runtime-masked and whose conflict-atom subset is non-empty
     (D:2253-2264, D:3043-3050). Here that is the 5.42 ebuild in pass 0; in pass 1 it is
     masked, so it is left out.
   - One node per choice. The last choice is popped first (B:186-195, B:136).
5. **A repeated mask is a duplicate, not a new step.** Re-masking with the same parent
   atoms leaves the parameter unchanged, so the child equals the current node and is
   dropped (B:96-97, B:126, B:192-194). portuale's `params_equal` (`backtracker.rs:31`) and
   the reason overwrite (`backtracker.rs:197-200`) must keep this true. That means
   identical `parents` text for the re-formed record, and an `[installed]` entry that is
   not re-pushed (`backtracker.rs:221`).
6. **A missing-dep child whose mask makes an earlier conflict pointless is dropped**
   (B:144-176): every parent of a slot-conflict entry is masked.
7. **Pass order.** A pass that ends on a missing dependency feeds back only that
   dependency and processes no slot conflict (D:5691-5696, B:276-279). That is the P3
   rule at the P2 site.
8. **The run ends on success** (D:12407-12413), on a config change (D:12409), on
   `max_retries` (D:12410), or when the stack is empty: then `get_best_run` and a
   non-backtracking pass follow (D:12420-12439, B:287-298). Each failed pass counts once
   (D:12414-12418), including a pass whose only child is rejected (pass 2 here).
9. **Skip rows** come from the final mask's stored conflict atoms of the non-installed
   masked package (D:1544-1579, B:191-194). Installed masks are never shown
   (D:1552-1555).

## 4. Not established

- **Whether portuale's rev-dep scan still drops the consumer pin (and so forms the P2
  record) in a pass whose params hold `!=p569prov-5.42` + `!=p569prov-5.42[installed]`.**
  That decides whether Rule 1 needs code beyond `installed_refs`. It cannot be checked
  until P2 exists. A 10a RED should assert the pass-1 record directly: installed 5.42
  present, parents `(virt-1, =5.42*)`.
- **Whether Portage's pass 2 would differ if `p569pull` had a third version.** With one
  more puller version, the missing-dep child of the "all masked" node might survive.
  Not probed.
- `existing_node` is the lowest-version graph node of the slot (PT:248-256,
  `installed=False`). Here it is the installed 5.42 and already a conflict package, so
  D:2227-2231 adds nothing. A shape where the existing node matches every parent (bug
  692746) was not probed.
