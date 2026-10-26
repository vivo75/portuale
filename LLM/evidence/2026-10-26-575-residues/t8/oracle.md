# Task 8 oracle — the Portage mechanism behind `or_branch_vdb_deselected_set` and G0/G1/G2 (#580)

Reference: Portage **3.0.83**. `3rdparty/portage` is at tag `portage-3.0.83`, and
`cmp` shows that its `lib/_emerge/depgraph.py`, `lib/_emerge/resolver/backtracking.py` and
`lib/portage/dep/dep_check.py` are byte-identical to the host's
`/usr/lib/python3.14/site-packages`. All line numbers below refer to that tag. Paths: `D` =
`lib/_emerge/depgraph.py`, `B` = `lib/_emerge/resolver/backtracking.py`, `DC` =
`lib/portage/dep/dep_check.py`, `PT` = `lib/_emerge/resolver/package_tracker.py`.

Harness: pmtest is read-only. Every probe **copies** `pmtest/fixtures` into a scratch
`$W` and uses the copy as ROOT, vdb and repos. Portage gets a scratch configroot with
absolute repo paths, which is the `LLM/evidence/2026-10-25-563/probe-p563.sh` recipe.
Scripts are in `t8/playground/`. Trimmed captures are in `t8/captures/`, each with its
full argv. Raw `--debug` dumps (up to 20 MB each) are not kept; the scripts regenerate
them. The common options are `--ignore-default-opts --getbinpkg=n --usepkg=n --color=n -pv`,
with `--debug` where stated and `PYTHONHASHSEED=0`. portuale binary: `target/release/portuale`
at `7eba6c92`, built 2026-10-26 22:25. After all runs, `git status` in pmtest is clean.

## 0. Premise check (review claim) — confirmed

- `_slot_operator_update_backtrack` (D:2415). `slot_operator_mask_built` is set only for a
  `not child.installed` child (D:2441-2442) and a `not dep.parent.installed` parent
  (D:2443-2444). An installed parent or child gets `slot_operator_replace_installed`
  instead (D:2450-2462).
- `_slot_operator_unsatisfied_backtrack` (D:2896) follows the same split: mask_built for a
  non-installed parent (D:2921-2924), replace_installed for an installed parent
  (D:2928-2933).
- `Backtracker._feedback_config`. Only `slot_operator_mask_built` is copied into
  `runtime_pkg_mask` (B:237-240). `slot_operator_replace_installed` goes to its own set
  (B:241-242).
- Result: an installed parent never enters `runtime_pkg_mask` through the slot-operator
  paths. On the 6bdb765 cell the installed parent is not even a graph node (see C1b).

## A. Every read and write of `runtime_pkg_mask`

An exhaustive grep over `lib/` and `bin/`, excluding tests, finds it in two files only:
D (15 hits) and B (16 hits).

| Location | R/W | What goes in / what reads it |
|---|---|---|
| B:17, B:26 | W (init) | `BacktrackParameter.runtime_pkg_mask = {}`, a dict `pkg -> {reason: data}` |
| B:56-59, B:68 | R | deepcopy (nested dicts copied); `__eq__` used for node dedupe in `_add` (B:127) |
| B:123, B:144-177 | R | `_check_runtime_pkg_mask` (bug 375573). A node is dropped if a `"slot conflict"` entry's every conflict parent is itself masked. Entries holding `"missing dependency"` or `"slot_operator_mask_built"` are always valid (B:150-155). |
| B:185-195 | **W** | `_feedback_slot_conflict`: for each choice in `backtrack_data`, a new node with `mask_steps+1` and `runtime_pkg_mask[pkg]["slot conflict"] = parent_atoms` (B:192). Only the first conflict is used (B:178-183). |
| B:197-206 | **W** | `_feedback_missing_dep`: `runtime_pkg_mask[dep.parent]["missing dependency"] = {(dep.parent, dep.root, dep.atom)}` (B:203) |
| B:237-240 | **W** | `slot_operator_mask_built` (non-installed only, see §0) is merged in |
| B:247-256 | **W** (del) | `prune_rebuilds` removes the `slot_operator_mask_built` reasons again |
| B:262-279 | — | `feedback`: `"config"`, then **either** `"slot conflict"` **or** `"missing dependency"` (B:276-279), never both in one restart |
| D:710 | R | `_dynamic_config._runtime_pkg_mask = backtrack_parameters.runtime_pkg_mask` (the next pass sees the node's masks) |
| D:1544-1579 (:1549) | R | `_get_missed_updates`. Every non-installed masked pkg with a lower chosen instance becomes a missed update, typed by its reason. This is the source of the `WARNING ... skipped due to a dependency conflict` block (`"slot conflict"`) and the `!!! ... triggered by backtracking` block (`"missing dependency"`), via `_show_missed_update` D:1581-1604, D:1606-1664. |
| D:2172-2180 | R | `_process_slot_conflict`: debug-only "backtracking loop detected" message, no effect on control flow |
| D:2356 | R | `_slot_change_probe`: skips a masked unbuilt child |
| D:3049 | R | `_iter_similar_available`: skips masked pkgs. Feeds `_slot_confict_backtrack`'s similar_pkgs (D:2252), `_downgrade_probe` (D:2967, used by dep_zapdeps' downgrade guards) and the slot-operator probes. |
| D:3463-3470 | R | `_add_dep`: slot-operator unsatisfied probe only if `dep.parent not in runtime_pkg_mask` |
| D:3478-3487 | R | `_add_dep`: an installed parent that is **already** masked and has no match anywhere gives `_initially_unsatisfied_deps` and returns 1 (no backtrack) |
| D:3505-3515 | R | `_add_dep`, bug 693836. If a pkg matching the atom is masked **only** for a slot conflict with no conflict atoms: `_skip_restart`, return 0 |
| D:6606-6612 | R | `_show_unsatisfied_dep`: renders `masked by: backtracking: <reasons>` |
| **D:7910-7912** | **R** | `_wrapped_select_pkg_highest_available_imp`: `if pkg in runtime_pkg_mask: continue`. **The only selection-time consumer.** Chain: `_select_pkg_highest_available` D:7261 → `_select_pkg_highest_available_imp` D:7281 → `_wrapped_…_imp` D:7543/7562. `_pkg_visibility_check` (D:7590) does **not** read the mask. |
| D:12387-12392 | R | `_backtrack_depgraph`: debug print of each node's mask |

Not consulted: `_select_pkg_from_graph` (D:8538) and `_select_pkg_from_installed`
(D:8561). These are the complete-mode selectors (D:8704-8710), so a masked *installed*
instance can stay graphed. The k3 trace shows this: `!!! backtracking loop detected:
paired-1.0 (installed)`.

**The "missing dependency" path, end to end.**
1. In `_add_dep` (D:3364), `dep_pkg` is None (D:3410).
2. Optional or ignored priority returns 1 (D:3411-3413). `allow_unsatisfied` returns 1
   (D:3419-3421). A complete-mode installed parent beyond depth with nothing installed
   returns 1 (D:3438-3453).
3. `_unsatisfied_deps_for_display` (D:3455).
4. If backtracking is on: the slot-operator probe (D:3462-3470), the installed-parent
   compat (D:3478-3487), then a reselect without USE (D:3494-3499; a USE-only miss does
   not backtrack), then the bug-693836 suppression (D:3505-3515).
5. `_backtrack_infos["missing dependency"] = dep`, `_need_restart` (D:3517-3518), and
   **return 0** (D:3535).
6. `_create_graph` returns 0 (D:3280). `_resolve` returns `0, favorites` at D:5691-5693,
   **before** `self.altlist()` (D:5696). `altlist` (D:9353-9354) is what runs
   `_resolve_conflicts` → `_complete_graph` + `_process_slot_conflicts` (D:9473-9485). So
   **a pass with a missing dep never processes slot conflicts**.
7. `_backtrack_depgraph` sends it to `feedback` (D:12415-12416), which calls
   `_feedback_missing_dep` (B:197-206) and masks `dep.parent`.
8. In the next pass, D:7910 skips the parent, so its puller re-selects a lower version. If
   only an argument wanted it, `_resolve` records `_unsatisfied_deps_for_display` and
   returns 0 **without restart** (D:5613-5616). That node is a dead end. When no node is
   left, `backtracking aborted after N tries` runs one last pass with
   `get_best_run()` (B:287-298: the deepest *terminal* node, i.e. no masks) and
   `allow_backtracking=False` (D:12420-12439). That pass prints the original problem.

**The first hop in every heal shape is a `"slot conflict"` mask, not a missing dep:**
`_process_slot_conflicts` (D:2123) runs `_solve_non_slot_operator_slot_conflicts` (D:1789;
silent solve, e.g. p491or "remove p491lib-2, keep p491lib-1"), then `_process_slot_conflict`
(D:2149-2218), then `_slot_confict_backtrack` (D:2220-2304), then
`_backtrack_infos["slot conflict"]` (D:2276-2279), then `_feedback_slot_conflict`.

`_slot_confict_backtrack` builds one choice per conflict pkg, plus the in-graph
`existing_node` (D:2227-2231). Lower versions are listed first (`sort(reverse=True)`,
D:2235, visited in reverse). Similar available pkgs share a choice (D:2248-2264). Choices
are ordered by `len(conflict_atoms)` (D:2273). The missing-dependency mask on the puller
is the **second** hop.

## B. `dep_zapdeps` (DC:339-818) on a `||` choice

- **Entry.** `dep_check` (DC:821) is called from `_select_atoms_highest_available` (D:6018)
  with `mydbapi=None` (D:6091-6093). So `dep_wordreduce` marks everything unsatisfied
  (DC:1056-1060), and DC:349 never short-circuits a `||` in the depgraph.
- **Two tree sets.**
  - **Normal walk:** `_filtered_trees` (D:762-830). `porttree` is
    `_dep_check_composite_db` (D:775-777), `vartree` is the **real vdb** (D:798-800), and
    `graph_db` is the PackageTracker fakedb (D:796, D:749).
  - **Complete mode:** `_select_atoms = _select_atoms_from_graph` (D:8705), which passes
    `_graph_trees` (D:6009-6016). There `porttree` = `vartree` = `graph_db` = the fakedb
    (D:764-768). The fakedb is `PackageTrackerDbapiWrapper` (PT:364). It **hides installed
    pkgs replaced by a same-slot merge node** (PT:230-252, `_replaced_by` PT:140-143). So
    in complete mode an arm satisfied only by an installed-but-replaced version is
    **unavailable**.
- **Availability** (`all_available`, DC:449-478). `avail_pkg = mydbapi.match_pkgs(atom.without_use)`
  (DC:469). In normal mode that is `_dep_check_composite_db.match_pkgs` (D:11964), which
  calls `_select_package`, then the D:7910 runtime_pkg_mask skip, then `_visible`
  (D:12051-12133). `_visible` masks:
  - installed pkgs the user asked to replace (D:12052-12053);
  - masked installed pkgs under `--update` without a visible ebuild (D:12054-12073);
  - lower-than-highest-visible choices in an empty slot when the highest also matches
    (D:12104-12122);
  - choices that would conflict with an in-graph pkg the atom also matches (D:12123-12132).

  So a **runtime_pkg_mask-masked** satisfier is simply not available: `all_available=False`.
- **Downgrade guards** (normal mode). `conflict_downgrade` (DC:480-490, bug 531656) and
  `installed_downgrade` (DC:531-541) put the choice in `other` (DC:634-635) when its
  avail_pkg is below the slot's highest graph/installed pkg and `_downgrade_probe` (D:2961)
  says the downgrade is undesirable.
- **Bins** (DC:378-402). `preferred_in_graph` is `preferred_installed` is
  `preferred_any_slot`: **one list** (DC:379-380). Then `preferred_non_installed`,
  `unsat_use_*`, `other_installed`, `other_installed_some`, `other_installed_any_slot`,
  `other`.
  - "Installed" is **cp/slot level, not version level**: `vardb.match(Atom(atom.cp))`
    (DC:603-608) and slot (DC:610-616).
  - `all_in_graph` means a match is in the digraph (DC:637-649).
  - Unavailable choices are sorted by `vardb.match(atom)` (DC:713-736). In complete mode
    `vardb` is the fakedb, so "installed" means "present after the merge".
  - Inside a bin, the listed order holds, except for promotions of all-installed-slots
    choices, upgrades and all-in-graph choices (DC:745-809).
  - The pick is the first available choice in bin order. Failing that, the first choice in
    bin order (DC:812-816).
- **Parent atoms.** Only `mycheck[1]` (the selected atoms) leave `_select_atoms`. `||`
  groups are queued (`_queue_disjunctive_deps` D:4872, popped by `_pop_disjunction`
  D:4912) and re-run through `_select_atoms`. Selected atoms become `Dependency`s →
  `_add_dep` → `_add_pkg` → `_add_parent_atom` (D:3654/3659/3787/3793, def D:4151) and
  `_add_slot_operator_dep` (D:3832, def D:4158). **A non-selected branch never becomes a
  parent atom or a `_slot_operator_deps` entry.** `_slot_operator_trigger_reinstalls`
  (D:3104) only ranges over the latter.
- **Live probe** (`playground/which-trees.py` wraps `_select_atoms_highest_available`):
  ```
  PORTAGE_CONFIGROOT=$W/swap-noslotop.cfg ROOT=$W/swap-noslotop python3 playground/which-trees.py --ignore-default-opts --getbinpkg=n --usepkg=n --color=n -pv dev-libs/p569pull
  T8PROBE parent=dev-libs/p569virt-1 complete_mode=True trees=_graph_trees depstring=[['||', Atom('=dev-libs/p569prov-5.40*'), Atom('=dev-libs/p569prov-5.42*'), ...]] selected=['=dev-libs/p569prov-5.40*']
  Dependency resolution took 3.40 s (backtrack: 0/20).
  [ebuild     U  ] dev-libs/p569prov-5.44 ... [5.42 ...]   /   [ebuild  N     ] dev-libs/p569pull-2
  ```
  The installed consumer's `||` is decided in **complete mode against `_graph_trees`**.
  Its installed-matching arm `=5.42*` is **not** chosen: 5.42 is replaced by the 5.44
  merge node, so no arm is available, and the first arm in `other_installed_any_slot`
  wins. The chosen `=5.40*` matches nothing (`_initially_unsatisfied_deps`, D:3438-3453),
  so this consumer forms **no conflict at all**.

## C. The heuristics, shape by shape

### C1 `or_branch_vdb_deselected_set` (reverse_dep_pins.rs:317)
**Shape** (code-note, `tests_p563.rs:344`, contract
`test_skip_block_keeps_the_built_atom_and_drops_non_selected_or_branches`): an installed
consumer whose `||` has an arm that matches an installed instance. Every other arm is
demoted and never pins. The demotion is version-level (`or_member_matches_installed`).

**C1a, scan site, p569 heal (`captures/p569-heal.trim.txt`).** Portage chooses
`Candidates: ['=dev-libs/p569prov-5.42*']` (complete mode). The slot-conflict parents are
then `p569virt-1 =…5.42*`, `p569prov:0/5.42=` and `p569prov:=`. The outcome matches
portuale, but **only because the installed-matching arm is also listed first**.

**Portage counterpart = `dep_zapdeps` in complete mode over `_graph_trees`**
(D:8705 → D:6015 → DC:339). It is *not* "installed-preferred".

Discriminator (`playground/probe-t8c.sh`): tree ebuild, md5-cache and vdb are edited the
same way so that dynamic deps cannot mask the edit. Same argv on both managers:
`-pv dev-libs/p569pull`.

| cell (p569virt RDEPEND) | Portage | portuale |
|---|---|---|
| orig-noslotop `\|\| ( =5.42* =5.40* =5.38* ~core )` | rc 0, backtrack 4/20, `N p569pull-1`, skip row `=5.42*` | rc 0, 2/20, `N p569pull-1`, skip row `=5.42*` |
| **swap-noslotop** `\|\| ( =5.40* =5.42* =5.38* ~core )` | **rc 0, 0/20, `U p569prov-5.44` + `N p569pull-2`, no skip block** | rc 0, 2/20, `N p569pull-1` + skip row `=5.42*` — **merge list differs** |
| swap-slotop (same `\|\|` + `p569prov:=`, vdb `:0/5.42=`) | rc 0, 4/20, `N p569pull-1`, skip rows: `p569prov:0/5.42=` only | rc 0, 2/20, `N p569pull-1`, skip rows: `:0/5.42=` **and `=5.42*`** |

(`captures/{portage,portuale}-c-*.trim.txt`, `portage-c-*.nodebug.txt`,
`which-trees-*.txt`.)

The 4/20 against 2/20 count comes from `_slot_confict_backtrack`'s choice order. With
equal conflict-atom counts, masking the installed 5.42 is explored first and dead-ends
twice (`captures/portage-c-orig-noslotop.trim.txt` runtime_pkg_mask sequence). The
fixture p569-heal (with the slot-op atom) gives 2/20 on both sides
(`counts-nodebug.txt`).

**C1b, probe site, pmtest 6bdb765 (`captures/p491or-world.trim.txt`).** Cell:
`-puDv --backtrack=4 @world`, world `[p491orcon, p491keep]`. Portage: rc 0,
`U p491orcon-2`, p491lib-1 kept (the slot conflict p491lib-2 vs installed is silently
solved by `_solve_non_slot_operator_slot_conflicts`: "remove p491lib-2"). `p491orcon-1`
appears **0 times** in the 6.6 MB debug dump: the installed parent is replaced by its
update, so it is never a graph node, its `||` is never evaluated, and it has no
`_slot_operator_deps` entry for `_slot_operator_trigger_reinstalls` (D:3104) to probe.
portuale gives the same merge rows (rc 0, 0/4).

The deselected arm `=dev-libs/p491lib-2:6/7=` matches **no package** (p491lib-2 is
`6/8`). So even for a walked installed parent, dep_zapdeps would put it in `other` by
availability (DC:475-478, DC:713-736), not by installed preference.

**Counterpart:** "parent not in the graph" on the commit's own cell, and dep_zapdeps
availability for the hypothetical walked shape. The unit pin
(`tests_p491.rs:198`) feeds `p491orcon-1` as an `AlreadyInstalled` entry, which is a
shape the Portage cell does not reach.

**C1c, R4 (bug 531656).** Portage (`captures/r4-orchoice.trim.txt:112-115`, normal
mode) chooses the installed `mllablgl`'s
`|| ( dev-libs/mllabltk:= <dev-libs/mlocaml-4.02 )` as `Candidates:
['dev-libs/mllabltk:=']`. That is the **non-installed** arm: `installed_downgrade`
(DC:531-541 → DC:634-635) demotes `<mlocaml-4.02` because mlocaml-4.02.1 is in the graph.
`or_branch_vdb_deselected_set` applies the opposite rule here: it demotes `mllabltk:=`
(not installed) and keeps `<mlocaml-4.02` (matches installed 4.01.0). Only G2 then stops
that pin (C4). Final rows on both managers: `r U mlocaml-4.02.1`, `N mllabltk-8.06.0`,
`rR mllablgl-1.05`, backtrack 1/20.

### C2 G0 (doomed pass: fresh autounmask USE / use_overlay, masked, USE-unsat or plain-miss disclosure)
**Shape** (`tests_p563.rs:274`, `LLM/evidence/2026-10-25-570/`): healable `p569pull` plus
unrelated `abort-masked-mid`, whose dep `maskeddep` is package.mask'd.

Portage (`captures/g0-combo.trim.txt`, rc 1, 1/20):
- try 0 reaches `backtracking due to unsatisfied dep: parent abort-masked-mid-1.0, atom
  dev-libs/maskeddep` and **never prints `backtracking due to slot conflict`**;
- try 1 has `runtime_pkg_mask {abort-masked-mid-1.0: missing dependency}`, so the argument
  fails (`masked by: backtracking: missing dependency`, D:5613-5616) with no restart;
- `backtracking aborted after 1 tries`, then the `get_best_run` rerun prints only the
  maskeddep disclosure.

**Portage counterpart = pass ordering plus the dead end, not a gate.**
- A missing dep returns 0 from `_create_graph` (D:3535 → D:3280 → D:5691-5693) before
  `altlist` → `_resolve_conflicts` → `_process_slot_conflicts` (D:5696, D:9473-9485). So
  the slot conflict is never fed back in that pass.
- Autounmask or REQUIRED_USE changes stop the backtrack loop outright:
  `need_config_change` (D:11838-11885: `_success_without_autounmask`,
  `_required_use_unsatisfied` D:11844-11845, slot-conflict-handler changes D:11849-11865,
  `_have_autounmask_changes` D:11866-11883) → `break` at D:12407-12411.

The heal is **deferred, not forbidden**. Counter-shape `playground/probe-t8d.sh` (scratch
`t8x-2` needs maskeddep, `t8x-1` needs nothing; argv `-pv dev-libs/p569pull dev-libs/t8x`):
Portage rc 0, 3/20, `N t8x-1` + `N p569pull-1`, with the slot-conflict skip block,
`t8x` skipped and `triggered by backtracking p569pull`. The heal fires in a later pass.
portuale matches (rc 0, 3/20, same rows), because G0 is per-pass. Divergence seen only in
**trailer order** (`captures/g0x-trailer-order.txt`): Portage prints full missing-dep
entries, then the abbreviated `triggered by backtracking` block (D:1613-1664). portuale
prints the abbreviated block first. portuale also omits ` for <ROOT>` on the slot headers
(D:1631-1632, D:1661-1662). Both are display-only and outside #580.

### C3 G1 (contradicted merge version must be hard-wanted by a merge-bound non-argument puller)
**Shape** (#54 K3 keeper, #222 cgp, #223 igw): the merge version is wanted only by
`(Argument)`.

Portage **does backtrack** and dead-ends:
- **k3-paired-arg** (`=dev-libs/paired-2.0`, keeper in world; `captures/k3-paired-arg.trim.txt`):
  - try 0: slot conflict; to mask `[paired-1.0 (ebuild), paired-1.0 (installed)]`;
  - try 1: 1.0 masked but the installed instance stays graphed (complete-mode selectors
    ignore the mask; "backtracking loop detected"), so a second conflict follows, which
    masks paired-2.0;
  - tries 2-3 have 2.0 masked, so the argument has no satisfier (D:5613-5616, no restart);
  - `backtracking aborted after 3 tries`, then the `get_best_run` rerun prints the residual
    `dev-libs/paired:0` conflict, rc 1.
- **k3-needer**: 4/20. The extra hop masks `needer-1.0` (missing dependency), then the
  `needer` argument fails.
- **igw-plain**: 3/20, same pattern.

**Portage counterpart = slot-conflict backtracking whose every choice ends in an argument
failure** (`_slot_confict_backtrack` D:2220 → B:185-195 → D:7910 → D:5613-5616), then the
backtracker runs dry and `get_best_run` + the final pass print the residual (B:287-298,
D:12420-12439). The outcome is equivalent to G1's "do not mask". The **count differs**
(`counts-nodebug.txt`): Portage k3-arg 3/20, k3-needer 4/20, igw 3/20 against portuale
0/20, 1/20, 0/20. Probably filtered by the pmtest pins (not checked). cgp (#222) was not
traced.

### C4 G2 (pin from a `||` branch whose other-package sibling is merge-bound)
**Shape:** R4 bug 531656 (`test_or_choice_avoids_downgrade_into_the_graphed_update`).

**Portage counterpart = `dep_zapdeps`**, not backtracking. In the normal walk,
`installed_downgrade`/`conflict_downgrade` (DC:480-490, DC:531-541, DC:634-635) push the
lower-version arm to `other`, so the sibling arm is chosen (C1c trace). In complete mode,
the replaced installed version is invisible in `_graph_trees` (PT:230-252) and an
all-in-graph sibling goes to `preferred_in_graph` (DC:637-649, DC:694-696). Either way the
pinned arm never becomes a `_parent_atoms` entry, so there is no slot conflict and no
mask. G2 and C1 are **two halves of one approximation** of the same `dep_zapdeps` call. In
R4 C1 gets the arm wrong and G2 corrects it at the restart site only.

## D. Conclusion

No single heuristic stands for `runtime_pkg_mask`-via-missing-dependency. One Portage
mechanism, with two parts, produces every outcome above:

1. **The installed reverse-dep consumer as a graph node.** Its deps run through
   `dep_zapdeps`: in complete mode against `_graph_trees` (D:8704-8710, D:6009-6016,
   D:749-771, DC:339-818), in the normal walk with the downgrade guards. Only the selected
   arm becomes `_parent_atoms` on the instance it selects (D:3565 `_add_pkg`, D:4151).
   This replaces `or_branch_vdb_deselected_set` and G2.
2. **Slot-conflict backtracking over that parent atom.** The chain runs
   `_process_slot_conflicts` (D:2123) → `_solve_non_slot_operator_slot_conflicts`
   (D:1789; the solvable case = today's enforced pins) → `_process_slot_conflict`
   (D:2149) → **`_slot_confict_backtrack` (D:2220-2304)** → `_feedback_slot_conflict`
   (B:185-195, `"slot conflict"` mask, one node per choice) → D:7910 in the next pass.
   The existing missing-dep second hop (D:3517 → B:197-206) and the backtracker's node
   tree with the `get_best_run` fallback (B:118-139, B:287-298, D:12378-12439) complete
   it. This replaces `dropped_pin_mask_restart` with its single mask choice and G1. G0
   becomes the natural consequence of D:5691-5696 ordering plus `need_config_change`
   (D:12409).

For portuale: the forward walk already has a dep_zapdeps-bin port
(`portage-use-reduce` `AltPreference`, `resolve/or_preference.rs`). The missing piece is
evaluating the rev-dep consumer's `||` with it against the **post-merge view** (complete
mode). After that, the dropped pins must enter the existing slot-conflict feedback as
parents of the installed instance, with Portage's choice list
(lower-first, similar-pkg grouping, sort by conflict-atom count, D:2227-2273).

**§P contradictions with the plan's Task 8 text:**
- (a) The candidate "missing-dependency path in `_add_dep` (D:3460-3510)" is only the
  **second** hop. Every heal and keeper trace starts with `backtracking due to slot
  conflict` (D:2220). The mechanism to port is `_slot_confict_backtrack`, not
  `_feedback_missing_dep` (portuale already has the latter).
- (b) "dep_zapdeps resolves each `||` installed-preferred" (code-notes of both files) is
  wrong. For the complete-graph consumers that carry these pins, dep_zapdeps runs against
  the post-merge `_graph_trees`, and "installed" there means cp/slot presence after the
  merge, so first-listed order decides (live probe above). In the normal walk the
  downgrade guards can prefer a non-installed arm (R4).
- (c) G1's premise "masking strands the argument, Portage aborts loudly instead" is
  right on the outcome but wrong on the mechanism and on the count. Portage backtracks
  3-4 times first.
- (d) Line drift: the plan's `backtracking.py:237-243` is B:237-240 (mask_built) and
  B:241-242 (replace_installed). D:3517 and B:197-205 are confirmed as B:197-206.

New target-cell candidates (none are in pmtest; scratch only): `swap-noslotop` (merge
list differs) and `swap-slotop` (one extra skip-block line). The backtrack-count gaps on
the G1 shapes and orig-noslotop are a third candidate if the pins compare counts.

## E. Not established

- Why Portage's p569virt `||` is popped only in complete mode (the disjunction stays
  queued while the installed universe is walked). It is observed (`which-trees.py`:
  `complete_mode=True`), not traced to a line.
- Whether a non-complete-mode evaluation of an installed consumer's `||` can ever carry a
  dropped pin. Not probed. R4 is the only normal-mode case seen.
- The cgp (#222) cells and the bed's 9 perl heals were not traced. The claim that the
  bed virtuals follow the orig-noslotop pattern (installed arm listed first) is a
  **guess** from the `virtual/perl-*` ebuild shape, not checked.
- Whether portuale honours `--dynamic-deps=n` in its rev-dep scan (`rev_dep.rs:557-561`
  reads a layer from `dynamic_deps`). The discriminators sidestep this by editing tree,
  cache and vdb together.
- Whether the trailer-order and ` for <ROOT>` differences (C2) are already filed.

## F. Traces (all under `t8/`)

- `playground/probe-t8.sh`, `--debug`. Cells:
  - `p563-heal` (`dev-libs/p563pull`);
  - `p569-heal`;
  - `g0-combo` (`dev-libs/p569pull dev-libs/abort-masked-mid`);
  - `k3-paired-arg` (`=dev-libs/paired-2.0`, world+keeper);
  - `k3-needer`;
  - `igw-plain` (`>=dev-libs/igw0x-2`, world+igw0a);
  - `r4-orchoice` and `r4-orchoice-bt0` (`-uD @world`, ad-hoc root);
  - `p491or-world` (`-uD --backtrack=4 @world`, the 6bdb765 vdb).

  Output: `captures/<cell>.{argv,rc,trim.txt}`. Raw files were trimmed by hand after the
  run, with a grep on resolver lines and `$W` substitution.
- Non-debug rc and backtrack counts for both managers on the same roots:
  `captures/counts-nodebug.txt`.
- `playground/probe-t8c.sh`, the `||`-order discriminator for both managers:
  `captures/{portage,portuale}-c-{swap-slotop,swap-noslotop,orig-noslotop}.*` and
  `portage-c-*.nodebug.txt`.
- `playground/which-trees.py`: `captures/which-trees-{swap-noslotop,orig-noslotop}.txt`.
- `playground/probe-t8d.sh`, the G0 counter-shape for both managers: `captures/g0x-*.txt`
  and `captures/g0x-trailer-order.txt`.
- p563 key lines (`captures/p563-heal.trim.txt`): try 0 `backtracking due to slot
  conflict ... package(s) to mask: [p563prov-5.44] ... parents: (p563con-1 installed,
  '=dev-libs/p563prov-5.42*'), (…, 'dev-libs/p563prov:0/5.42='), (…, 'dev-libs/p563prov:0='),
  (p563pull-2, '=dev-libs/p563prov-5.44*')`. Try 1: `backtracking due to unsatisfied dep:
  parent p563pull-2 atom =dev-libs/p563prov-5.44*`. Try 2: `runtime_pkg_mask
  {p563prov-5.44: slot conflict, p563pull-2: missing dependency}` → `N p563pull-1`. rc 0,
  2/20 on both managers.
