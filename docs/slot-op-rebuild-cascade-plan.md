# Plan: slot-operator-rebuild — complete-graph mode + cascade + `r` marker

*Working plan — 2026-09-03.* **STATUS: COMPLETE (both slices shipped
2026-09-03).** Slice 1 (`required_set_reachable_cps`, commit `bc1fbaa`).
Slice 2 (the B rework: reachability-gated scan, sub-slot cascade to a
fixpoint, `r` marker, `[oldver]` bracket on a slot-shifted rebuild,
`str(Package)` "causing rebuilds:" rendering). Container-verified
byte-for-byte against real portage 3.0.82.2 —
`TEST/scripts/40-slotop-cascade.sh` diff is empty. Contract tests:
`test_slot_operator_rebuild_reinstalls_a_stale_equals_consumer`
(rewritten), `test_slot_operator_rebuild_cascades_through_a_multi_level_chain`
(new). See `docs/what-this-proves.md` "Increment 4".

One documented cut kept from the analysis below: a cascade rebuild's own
`RDEPEND`/`DEPEND` are **not** re-walked, so a genuinely *new* dependency
of a forced rebuild is missed (a pure sub-slot cascade — the only kind
the fixture exercises — has none).

## What the real container showed

Investigation via `TEST/` (`localhost/test-portuale`, real portage
3.0.82.2). Scenario: `dev-libs/scltarget` `1.0` (`SLOT=0/1`, installed) →
`2.0` (`SLOT=0/2`, in tree); `dev-libs/sclmid` tree ebuild `SLOT=0/2`
but vdb `SLOT=0/1`, `RDEPEND="dev-libs/scltarget:="` (vdb bound
`:0/1=`); `dev-libs/scltail` `RDEPEND="dev-libs/sclmid:="` (vdb bound
`:0/1=`).

`TEST/scripts/3x-slotop-*.sh` results — real `emerge -p dev-libs/scltarget`:

| `@world` contents | real portage output |
|---|---|
| *(empty)* | `[ebuild U] scltarget-2.0` — **no rebuild at all** |
| `scltail` (`scltail→sclmid→scltarget`) | `[ebuild r U] scltarget` · `[ebuild rR] sclmid` · `[ebuild rR] scltail` |
| `sclfwd` (forward-deps `scltarget` + `sclmid`) | `scltarget rU` · `sclmid rR` · `sclfwd rR` (not `scltail`) |
| `sclmid` + `scltail` | all three |

Portuale, every case: `[ebuild U] scltarget` · `[ebuild R] sclmid`
(one level, plain markers).

### Three gaps

1. **Trigger condition.** Real rebuilds a consumer **only if it is in
   the graph** — a member of `@world`/`@selected`/`@system`, or a
   transitive *forward* dependency of one, pulled in by
   `_complete_graph`'s deep re-walk of the required sets (which
   auto-enables when a merge changes an installed package's
   version/slot/USE). Portuale's `slot_operator_rebuild_entries` scans
   `all_installed_packages` unconditionally → it **over-fires** for a
   consumer that isn't reachable from any set (real case 1), and its
   `complete` flag only sets `deep = Unlimited`, never walking the sets.
2. **The cascade.** `sclmid` rebuilt lands at its *tree ebuild* sub-slot
   (`0/2`), which makes `scltail`'s built `sclmid:0/1=` stale in turn.
   Portuale reads the rebuilt consumer's sub-slot from the **vdb**
   (`read_vdb_slot`, always `0/1`) and never re-scans, so it misses
   every level past the first.
3. **The `r` display column.** Real tags every forced slot-op rebuild
   (and the triggering upgrade) with a red `r` (`PkgAttrDisplay.
   force_reinstall`, real `depgraph.py:5479` — the
   `@__auto_slot_operator_replace_installed__` set is yielded with
   `force_reinstall=True`). Portuale emits plain `U` / `R`. The existing
   `test_slot_operator_rebuild_reinstalls_a_stale_equals_consumer`
   encodes the plain form.

### Real's mechanism (approach B, confirmed)

`_slot_operator_trigger_reinstalls` (during `_serialize_tasks`) finds a
stale built slot-op dep whose parent is *in the graph* →
`_slot_operator_update_backtrack` adds the parent's replacement atom to
`slot_operator_replace_installed` (a `BacktrackParameter`) →
`_need_restart`. `_backtrack_depgraph` restarts; `_gen_reinstall_sets`
yields `@__auto_slot_operator_replace_installed__` (`force_reinstall=
True`) → the consumer is walked as a merge-bound `Reinstall` → its own
deps + its own stale built slot-op deps are found → another restart →
cascade to a fixpoint.

## Slices

### Slice 1 — complete-graph mode's required-set re-walk

Real `_complete_graph` (depgraph.py:8555–8760). Portuale already detects
the auto-enable condition (`complete_graph_auto_enable`) and re-resolves
with `complete = true`; today that only deepens `deep`. Extend it: when
`complete` and the target isn't already the world set, **also seed the
walk with `@world ∪ @selected ∪ @system`**, selection restricted to
graph-or-installed-not-replaced (real `_select_pkg_from_graph`), so those
installed packages and their forward deps land in `entries` as
`AlreadyInstalled` with `required_by` edges — no new merges by
themselves.

Observable surface (small): `--json` gains the extra `AlreadyInstalled`
entries + edges for an upgrade/reinstall run; `--changed-deps-report`
scans the wider set. Most `--pretend` fixtures (fresh installs) don't
auto-enable complete mode, so stay byte-identical. Verify against the
container (`emerge -p <upgrade-atom>` merge list unchanged; `-p --tree`
edge set matches).

### Slice 2 — slot-operator-rebuild B rework

- Gate the scan on **in-graph** consumers (the Slice-1 `entries` now
  include the reachable world/selected set).
- Feed each newly-found stale consumer into the `'backtrack` loop via a
  `slot_op_replace: HashMap<(cat,pkg), ForcedReinstall>` accumulator
  outside the loop; on growth, `continue 'backtrack`. On the re-walk,
  inject each as a merge-bound `Reinstall { slot_operator_rebuild: true,
  force_reinstall: true }`, re-bind its own `:=` deps to the graph's new
  sub-slots, and walk its `RDEPEND`/`DEPEND` (so a genuinely new dep is
  pulled and its own stale built slot-op deps feed the next iteration).
- `PretendOutcome::Reinstall` grows a `force_reinstall` bit → `pretend.rs`
  renders the red `r` column (real `PkgAttrDisplay`); the triggering
  upgrade entry also gets `r` when it matches a forced-reinstall atom.
- `abi_rebuilds` accumulates every cascade level.
- Rewrite `_slotbind_root` / its test for the new gating + `r`; add
  `_slotcascade_root` (the 3-level chain, chain in `@world`).
- **Container gate:** `TEST/scripts/NN-slotop-cascade.sh` — the
  `portuale -p` vs real `emerge -p` diff for the cascade fixture must be
  empty. Committed with the slice.

## TEST/ infrastructure

`TEST/create-container.bash` + `TEST/scripts/31-slotop-trigger.sh` are
committed now (independent of the slice) as the repeatable real-portage
cross-check for this area. `init.c` (referenced by
`create-container.bash`) is not in the repo — the image is prebuilt;
the script is kept for reproducibility.
