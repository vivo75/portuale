# #601 Task 0b — the discriminator (2026-10-28)

Cell: `emerge -puvD @world` on the bed (world seeded with tmux, eix,
gentoolkit, blake3), Portage 3.0.83-r1, portuale
`d512cb05-dirty` + the trace lines added by this task. Controls on the
fixture tree: the deep `-puvD` run (the pinned #598 test's argv, with and
without `--debug`) and the brief's plain `-pv --update` run (with and
without `--debug`).

Everything below is measured from the captures in `captures/`:

| capture | what |
|---|---|
| `captures/task0b-control-bt-trace.txt` | control `-puvD --debug`, full `BT` trace (24 lines) |
| `captures/task0b-control-deep-nodebug-bt-trace.txt` | control `-puvD` (no `--debug`), full stderr (22 lines): identical `BT` lines, no `BT-RESULT` |
| `captures/task0b-control-plain-bt-trace.txt` | control `-pv --update` (plain, no `--deep`, no `--debug`), full stderr (34 lines) |
| `captures/task0b-control-plain-debug-bt-trace.txt` | the same plain run with `--debug`: the two `BT-RESULT` lines plus the debug walk |
| `captures/task0b-cell-bt-trace-excerpt.txt` | cell, the 26 tries (long lists curtailed), both `BT-ASSEMBLE`s, both `BT-RESULT`s, the `BT-DROP` summary |
| `captures/task0b-cell-pair-drop-detail.txt` | cell settling pass: masks, the full 34 scan pairs, both `BT-ASSEMBLE`s, both `BT-RESULT`s, all 34 distinct `BT-DROP` lines |
| `captures/task0b-cell-portage-block.txt` | Portage's complete 48-row block on the cell, verbatim, nothing elided |

## The instrument (added by this task, trace-only)

All gated on `PORTUALE_BT_TRACE=1`; `--debug` stdout is untouched (still
pinned by the contract suite).

- `resolve/feedback/mod.rs`, beside `BT replace-in`:
  `BT probe-in <len> <sorted params.probe_replaced>` and
  `BT abi-pass <none|Some(n) [pairs]>` — the pass's own display-pair stash.
- `resolve/assemble.rs`, after the pair union and `drop_masked_pairs`:
  `BT-ASSEMBLE abi taken=… scanned=… extended=… kept=… [pairs]`.
- `resolve/feedback/slot_conflict_backtrack.rs`, in `drop_masked_pairs`:
  `BT-DROP pair <provider> -> <consumer>: provider_off=… consumer_off=…`.
- `portuale/src/pretend/final_gates.rs`, in `print_resolver_debug`:
  `BT-RESULT slot_operator_replaced <len> <list>` and
  `BT-RESULT abi_rebuilds <len> <pairs>` — the post-assemble fields.

Gate note: `BT`/`BT-ASSEMBLE`/`BT-DROP` need only `PORTUALE_BT_TRACE=1`;
`BT-RESULT` lives inside `print_resolver_debug`'s `if resolver_debug()`
block, so it also needs `--debug` (that is the only print site; the fields
themselves live on `GraphResult`). `BT probe-in` reads the **incoming**
`params.probe_replaced` (like `BT replace-in`), so a filing made *during*
the settling pass is not in `probe-in` — it shows up only in
`BT-RESULT slot_operator_replaced`.

## The sets, measured

### Control (`p589bpull`, `-puvD --debug`)

```
try=1 Config  replace-in 0  probe-in 0  abi-pass Some(1) [("dev-libs/p589bprov-5.44","dev-libs/p589bvirt-1")]
try=2 Settle  replace-in 2 ["dev-libs/p589bprov","dev-libs/p589bvirt"]  probe-in 0  abi-pass Some(0) []
BT-ASSEMBLE abi taken=Some(0) scanned=0 extended=1 kept=1 [("dev-libs/p589bprov-5.44","dev-libs/p589bvirt-1")]
BT-RESULT slot_operator_replaced 0 []
BT-RESULT abi_rebuilds 1 [("dev-libs/p589bprov-5.44","dev-libs/p589bvirt-1")]
```

(`-puvD` without `--debug` emits exactly these `BT` lines — the only
difference is that the two `BT-RESULT` lines are absent, see
`captures/task0b-control-deep-nodebug-bt-trace.txt`. The fixture run
resolves twice — tries 1–2 and 3–4 — with identical sets both times.)

The brief's second control run, **plain** (`-pv --update`, no `--deep`),
is a different resolution and shows no display fields at all: 5 tries
(SlotConflict ×2, MissingDep ×2, Settle), `replace-in 0`, `probe-in 0`,
`abi-pass Some(0)`, `BT-ASSEMBLE … extended=0 kept=0`, and with `--debug`
`BT-RESULT abi_rebuilds 0`; the merge list is only
`[ebuild N] dev-libs/p589bpull-1` and there is no block
(`captures/task0b-control-plain-bt-trace.txt`,
`…-plain-debug-bt-trace.txt`). Without `--deep` the request never pulls
the provider upgrade, so the display path is not exercised — the plain
run is a negative control, not a second sighting of the pair.

- `probe_replaced`: **empty**, both tries → `result.slot_operator_replaced` empty.
- `result.abi_rebuilds`: **1 pair**, and it comes from the *conflict
  continuity* path, `assemble.rs:60 slot_conflict_abi_display_pairs`,
  not from the settling pass's scan (`abi-pass Some(0)`).

Consequence for `mechanism.md` point 7: the control proves the `r` column
is **not** read from `slot_operator_replaced` alone. It is the union of
`abi_rebuild_force_reinstall_cps(result)` (both sides of every
`result.abi_rebuilds` pair, `pretend/mod.rs:486`), `rebuild_if_reinstalls`
and `slot_operator_replaced` (`pretend/mod.rs:492/495`). In the control
`slot_operator_replaced` is empty and `r` still prints on both prov and virt.

Portage's own `r` is the same shape: `PkgAttrDisplay.force_reinstall` is
set when any atom in `_iter_atoms_for_pkg(pkg)` has `arg.force_reinstall`
(`depgraph.py:4988-5012`, `resolver/output.py:815-817`), and those are
exactly the `__auto_slot_operator_replace_installed__` /
`__auto_rebuild__` args — the set Portage's `_compute_abi_rebuild_info`
seeds from (`depgraph.py:940-950`).

### Cell (`@world`, `-puvD --debug`)

```
try=1  SlotConflict   replace-in 0   probe-in 0   abi-pass none
try=2…11 MissingDep   replace-in 0   probe-in 0   abi-pass none
try=12 Config         replace-in 0   probe-in 0   abi-pass none
try=13 Settle  masks-in [ … "!=dev-lang/perl-5.42.2[installed]" ]
       replace-in 34   probe-in 0   abi-pass Some(34) [34 pairs, providers: {"dev-lang/perl-5.44.0"}]
       BT-ASSEMBLE abi taken=Some(34) scanned=34 extended=68 kept=0 []
try=14 SlotConflict   replace-in 0   probe-in 0   abi-pass none
try=15…24 MissingDep  replace-in 0   probe-in 0   abi-pass none
try=25 Config         replace-in 0   probe-in 0   abi-pass none
try=26 Settle  same 34/0/34
       BT-ASSEMBLE abi taken=Some(34) scanned=34 extended=68 kept=0 []
BT-RESULT slot_operator_replaced 0 []
BT-RESULT abi_rebuilds 0 []
# 136 BT-DROP lines = 68 pairs × 2 assembles, every one
#   provider dev-lang/perl-5.44.0 provider_off=true consumer_off=false
```

- (a) `probe_replaced` / `slot_operator_replaced`: **empty on all 26
  tries**; it does not contain perl.
- (b) `result.abi_rebuilds` **is empty**, and it does not go empty
  *between* assembles: each settling assemble receives `Some(34)`
  (`taken=Some(34)`) and leaves with 0. It empties **inside**
  `assemble_result`, in `drop_masked_pairs` (`assemble.rs:67-71`).
- (c) Portage's block on the same run has **48 rows** under the header
  `dev-lang/perl-5.44.0:0/5.44::gentoo causes rebuilds for:`
  (`captures/task0b-cell-portage-block.txt`). Portuale's scan pairs cover
  34 of them.

## Verdict: candidate 2 holds; 1, 3 and 4 do not (as stated)

Testing the brief's four candidates in its order, by measurement:

1. **The display scan's scope — partly true, but not the cause of the
   empty block.** `scan_reachable`/`walked_installed` did not starve the
   scan: on the settling pass it produced **34** pairs, all naming
   `dev-lang/perl-5.44.0`. The pairs are then wiped by
   `drop_masked_pairs`. The scan's *`already`/`in_graph`* rule
   (`rebuild_scan.rs:158`) is a real, second-order narrowing: it skips
   installed consumers that are merge-bound for an upgrade in the same
   run, which is why portuale has 34 pairs where Portage has 48 rows
   (see below).
2. **`probe_replaced` is never extended for perl — HOLDS.** `probe-in`
   is 0 on every try of both runs. No filing of the installed perl
   (`dev-lang/perl-5.42.2`) ever happens; portuale's slot-conflict arm
   only masks it (`!=dev-lang/perl-5.42.2[installed]`, from
   `resolve/backtracker.rs:214-228`).
3. **`prune_rebuilds` cleared both sets — FALSIFIED.** No try in the cell
   trace has `prune=true` (26/26 `prune=false`), and the replace set on
   the settling pass is 34 — it was never cleared.
4. **The `virtual/perl-*` `||` providers make `slot_op_edge_for_atom`
   reject the edge — FALSIFIED.** `slot_op_edge_for_atom` produced all
   34 pairs on the settling pass; the edges were accepted.

The chain that actually produces the cell's empty display, measured
end to end:

1. The conflict on `dev-lang/perl` (installed `5.42.2:0/5.42` vs
   merge-bound `5.44.0:0/5.44`) is resolved by masking the installed
   instance: `!=dev-lang/perl-5.42.2[installed]`
   (`resolve/backtracker.rs:214-228`, `choice.installed`).
   `dev-lang/perl` never enters `slot_operator_replace_installed` or
   `probe_replaced`.
2. The rebuild scan still finds the 34 consumers bound by
   `dev-lang/perl:0/5.42=` and emits 34 display pairs
   (`abi-pass Some(34)`).
3. `assemble_result` unions the continuity pairs (34 more) →
   `extended=68`, then calls `drop_masked_pairs` (`assemble.rs:67-71`).
   That function drops a pair when either side is *masked-installed and
   not in the replace set* (`slot_conflict_backtrack.rs:142-167`;
   the `retain` is `:156-166`).
   Every pair's provider cp is `dev-lang/perl`; perl *is*
   masked-installed (mask `!=dev-lang/perl-5.42.2[installed]` matches the
   installed 5.42.2, `vdb-list.txt:116`) and *is not* in the 34-cp replace
   set. Every consumer cp is in the replace set, so no consumer side ever
   fires. Result: `kept=0`, 136 `BT-DROP` lines all
   `provider_off=true consumer_off=false`, `BT-RESULT abi_rebuilds 0`.
4. With no pairs, perl is not in `force_reinstall_cps` and
   `slot_operator_replaced` is empty → `[ebuild U]` without `r`; and
   `final_gates` sees `result.abi_rebuilds.is_empty()` → no block.

So the display is a *consequence* of the missing replace-set entry.
`drop_masked_pairs` is correct given its precondition (`r` only for
replace-set members); the missing entry is the bug.

### Second finding: even with the set fixed, the block is 34 rows vs 48

Portage seeds the block from the force-reinstall atom
(`dev-lang/perl:0`) and walks `_slot_operator_deps[(root, perl:0)]`
(`depgraph.py:1006-1096`). A dep counts only when the child's
slot/sub-slot actually changed (`:1069-1074`) and, for an installed
parent, a merge-bound replacement of that parent exists (`:1076-1090`);
that parent may be an upgrade or a rebuild. Portuale's scan pairs only
consumers it *schedules for rebuild*, and skips every consumer that is
`in_graph && !already` (`rebuild_scan.rs:158`). The 14 rows it would
still miss after the set is fixed are exactly the consumers that are
merge-bound for an upgrade in the same run (all 14 are `[ebuild U]` rows,
not `rR`): `virtual/perl-{Math-BigInt,File-Temp,ExtUtils-ParseXS,
Scalar-List-Utils,Module-Metadata,IO,Test-Harness,File-Spec,
ExtUtils-MakeMaker,ExtUtils-CBuilder,version,Encode,CPAN-Meta}` and
`perl-core/File-Temp`.

Deciding whether Task 2 must close that too is a scope call for the
controller; if the target is display parity on this cell, it is needed.

## Minimal fix site for Task 2

`crates/portage-repo/src/resolve/feedback/slot_conflict_abi.rs`,
`step_slot_conflict_abi` — portuale's half-port of
`_slot_conflict_backtrack_abi` (`depgraph.py:2297-2329`):

- Portage, on a conflict whose atom is a built `:=`/soname dep of an
  installed parent, calls `_slot_operator_update_probe_slot_conflict`
  and then `_slot_operator_update_backtrack(dep, new_dep=…)`
  (`depgraph.py:2415-2465`). With `new_child_slot is None`,
  `dep.parent.installed` files `_replace_installed_atom(dep.parent)` and
  `child.installed` files `_replace_installed_atom(child)` into
  `slot_operator_replace_installed`.
- portuale's `step_slot_conflict_abi` asks `slot_conflict_abi_probe`
  for `fresh` parents and stops there; it has no installed-child half.
  For the cell: parent = an installed consumer with the recorded
  `dev-lang/perl:0/5.42=` atom, child = installed `dev-lang/perl-5.42.2`,
  so both must join `slot_operator_replace_installed` **and**
  `probe_replaced` (`probe_replaced` is the display mirror; `r` and the
  `__auto_slot_operator_replace_installed__` set arg read it), then
  restart on `Config` feedback.

With perl in the replace set, `drop_masked_pairs` keeps the 34 pairs and
perl becomes the provider side of them → `r` on perl and the block both
appear. Nothing in the renderer, the display scan's reachability, or a
second seeding path needs to change for the cell's *missing* half.

If display parity with Portage's 48 rows is in scope, the pair generation
must leave `rebuild_scan.rs:158`'s `in_graph && !already` skip out of the
*display* pairs (Portage's `_slot_operator_deps` walk is independent of
the replace set), or `slot_conflict_abi_display_pairs` must cover
upgrading consumers too.

## What Task 1's fixture must reproduce

Not `p589bpull`'s shape (which is green: its pair survives because the
installed provider is not masked), but the cell's **difference** from it:

1. an installed provider `P-5.42:0/5.42` and a merge-bound `P-5.44:0/5.44`
   (the sub-slot move, as in `p589bprov`);
2. an installed consumer `C-1` with a **hard** recorded
   `P:0/5.42=` built atom (`p589bvirt`'s `:=` *without* its `||` arm —
   the `||` arm is what makes `p589b` conflict-free; the hard built atom
   is what makes `perl` conflict);
3. a second, merge-bound parent (the requested `pull`) pulling
   `=P-5.44*` and `C`, so the two `P` instances land in one slot and the
   search forms the slot conflict that masks `P-5.42` as `[installed]`;
4. assertions: `r` on `P-5.44` and the block
   `P-5.44… causes rebuilds for: C-1…` (today both absent; `C` alone
   keeps its `r` from `Reinstall{slot_operator_rebuild: true}`), plus
   the replace/pair set (not the parser's try counter).

A `p589d` family (prov 5.42/5.44, virt with the hard built atom, pull
requiring `=prov-5.44*` and virt) is the smallest shape; verify the
Portage oracle in the bed / against `3rdparty/portage` before pinning.

## Reproduce

Host controls (fixture env is just `ROOT`/`PORTAGE_CONFIGROOT` at
`pmtest/fixtures`); the three runs behind the control captures:

```
cd /home/vivo/repo/PORTUALE/pmtest
export PORTAGE_CONFIGROOT="$PWD/fixtures" ROOT="$PWD/fixtures" \
       PORTAGE_RUNNING_ROOT="$PWD/fixtures" DISTDIR="$PWD/fixtures/distfiles" \
       PORTUALE_BT_TRACE=1
E=/home/vivo/repo/PORTUALE/portuale/target/release/emerge
# 1. the deep control (the #598 argv + --debug):
$E --pretend --verbose --update --deep --debug dev-libs/p589bpull
# 2. the same without --debug (BT lines only, no BT-RESULT):
$E --pretend --verbose --update --deep dev-libs/p589bpull
# 3. the brief's plain control, without and with --debug:
$E --pretend --verbose --update dev-libs/p589bpull
$E --pretend --verbose --update --debug dev-libs/p589bpull
```

Cell (temporary wrapper because podman forwards no host env;
`PORTUALE_BT_TRACE=1` does **not** reach the container through
`podman_run_pm`'s fixed `-e` list):

```
cd /home/vivo/repo/PORTUALE/pmtest
printf '@world\n' > differential-test-bed/atomlists/l0-bt-world.txt
cat > differential-test-bed/layers/l0/bt-wrap-tmp.sh <<'EOF'
#!/bin/bash
export PORTUALE_BT_TRACE=1
exec /bin/bash /TEST/layers/l0/in-container.sh "$@"
EOF
# run/l0-bt-tmp.sh = l0-resolver.sh with that entrypoint
L0_EMERGE_OPTS="-puvD --debug" \
  bash differential-test-bed/run/l0-bt-tmp.sh differential-test-bed/atomlists/l0-bt-world.txt
```

Bed run: `l0-20261028T224843Z` (`logs/l0-20261028T224843Z/`), portuale
`d512cb05-dirty`, gentoo repo `350c0882200cebbdc04d67cace7c99a6725e1889`,
Portage 3.0.83-r1, 320 installed packages. The temporary wrapper, runner
and atom list are removed again after the run. The committed captures
hold every line the prose cites; the bed's raw `_world.txt` files (the
full 26-try trace and the full 30k-line `--debug` dump) are only in that
gitignored log directory, which the commands above regenerate.
