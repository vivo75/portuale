# L0 fixture oracle — real `emerge` on the checked-in fixture tree

Backlog #49 / `docs/second_python_copy_removal.md` §6. Runs the real
`emerge -p` on a staged copy of `fixtures/` and compares it to
portuale's own output through the same comparator the L0 bed uses.

```sh
TEST/run/l0-fixture-oracle.sh          # atom list: TEST/atomlists/l0-fixture-oracle.txt
# -> TEST/logs/l0-fx-<stamp>/l0-report.txt + l0-report.json
#    rc 0 green (every finding explained), 1 unexplained
```

## What shipped

| piece | path |
|---|---|
| staging (container-side) | `TEST/layers/l0-fixture-oracle/stage.sh` |
| runner (container-side) | `TEST/layers/l0-fixture-oracle/in-container.sh` |
| entry point (host) | `TEST/run/l0-fixture-oracle.sh` |
| cases | `TEST/atomlists/l0-fixture-oracle.txt` |
| allowlist | `TEST/compare/known-divergences-fixture-oracle.yaml` |

The staging deltas (each one a real-Portage requirement the checked-in
fixture tree does not satisfy; the checked-in tree stays untouched):

1. `repos.conf` relative `location =` -> absolute staged paths (real
   rejects the relative form portuale accepts);
2. `binrepos.conf`'s `${PORTAGE_CONFIGROOT}` resolved (portuale-only
   interpolation);
3. `etc/portage/categories` added (real masks a package whose category is
   not listed);
4. one `Manifest` per package dir with **every** ebuild's
   BLAKE2B/SHA512 (real masks a digest-less ebuild as corruption and a
   multi-version dir missing a line as "not listed in the Manifest");
5. `repo/metadata/news` dropped (portuale-format fixture news, real
   rejects them);
6. `/etc/make.local` touched in the throwaway container (fixture
   `make.conf` sources it);
7. the committed `common-1.0` entry is staled (wrong `_md5_`, plus a
   resolution-visible `KEYWORDS=~amd64` against the ebuild's `amd64`) so
   both PMs must regenerate it from the ebuild (#46 S4). This replaced
   the old backquote-rewrite delta, moot once #46 S1 fixed the two
   backquoted ebuilds: staging no longer touches ebuild bytes, so the
   S1-valid committed entries keep validating;
8. `masters = testrepo` appended to the overlay repos' `layout.conf`
   (real warns otherwise; `repnamerepo` has a `layout.conf` without it);
9. comment lines stripped from staged `profiles/updates/*` (real's parser
   errors on them);
10. `*/<pkg>` profile atoms rewritten to the package's real category
    (real reports "Invalid atom" otherwise);
11. `profiles/repo_name` added where missing (real warns otherwise);
12. after the portage pin upgrade, the container's `/var/db/pkg` is
    replaced with the fixture vdb, so the running root's installed set is
    the fixture set -- otherwise real prints "The following installed
    packages are masked" (the image's gentoo set is not in the staged
    fixture repos) on every probe. Portuale's `fixture_env` pins
    `PORTAGE_RUNNING_ROOT` to fixtures for the same reason.

Portage is pinned/upgraded to `3.0.82.2` before the cases run, like the
L0 bed.

## Result (2026-09-15, `TEST/logs/l0-fx-20260915T160454Z`)

(`l0-fx-20260915T101508Z` is the pre-#46 S4 baseline.)

13 cases, **6 clean** (no findings at all), 20 findings explained, 0
unexplained. The added `dev-libs/common` case exercises staging delta 7
(stale `_md5_` + `KEYWORDS=~amd64`): both PMs regenerate it from the
ebuild and resolve identically (`#46 S4`, `TEST/findings/l2.md`).

- clean: `dev-libs/diamond`, `dev-libs/anyof` (`||` group),
  `dev-libs/iusedefaultpkg` (REQUIRED_USE), `dev-libs/dualslotpkg`
  (multi-slot), `dev-libs/blockusedeptarget`, `dev-libs/common` (the
  stale-entry case);
- explanation-only (wording/staging path, see the allowlist):
  `dev-libs/autounmaskkeywordpkg`, `dev-libs/kwneedpkg`,
  `dev-libs/requiredusebadpkg`;
- **genuine resolver differences, filed as backlog #54** (suppressed by
  two `owner: portuale-bug` allowlist entries so the layer can gate other
  changes): `--update dev-libs/paired`, `dev-libs/needer`,
  `=dev-libs/paired-2.0`, and the `needer + othermod` triangle's block
  content/exit.

## #54 — the installed-consumer pin over-approximation

Real 3.0.82.2 in the staged (hermetic) environment:

| case | real | portuale |
|---|---|---|
| `--update dev-libs/paired` | `[ebuild U] paired-2.0 [1.0]`, rc 0 | silent, keeps 1.0 |
| `dev-libs/needer` | `U 2.0` + `N needer`, **no block**, rc 0 | same list **+ block** |
| `=dev-libs/paired-2.0` | `U 2.0`, **no block**, rc 0 | `U 2.0` **+ block** |
| `needer dev-libs/othermod` | `U 2.0` + `N needer` + `N othermod` + block, rc 1 | same list + block, rc 0 |

Mechanism, from real's source: complete mode is only enabled when the
package tracker already holds a **slot conflict**
(`depgraph.py:9446-9453` `_resolve_conflicts`), and the end-of-walk loop
that pulls an installed satisfier into the graph only sees deps of nodes
already in the graph (`depgraph.py:8562+`, the `_unsatisfied_deps` loop).
`keeper-1.0`'s `=paired-1.0` pin is outside every target closure, so it
is only walked when a competing constraint (`othermod`'s `<paired-2.0`)
keeps the installed `paired-1.0` in the graph — the triangle. In the
single-sided shapes real replaces `paired-1.0` and never sees keeper.
Portuale applies the pin (and renders the residual block) unconditionally.

The three differences are pinned the wrong way in the contract suite
(`test_satisfiable_installed_pin_still_holds_the_upgrade`,
`test_explicitly_pinned_upgrade_breaks_an_installed_pin_and_reports_it`,
`test_hard_dependency_requirement_breaks_an_installed_pin_and_reports_it`)
with docstrings claiming live verification; those pins must move with the
#54 fix, and the triangle's expected parents include keeper where real
names only `othermod`.

## #54 S0 — oracle matrix: world state × case × complete-graph gate

`docs/05.054-residual_conflict.opus.md` §S0's hypothesis H: the 511e659
docstrings ("verified live") came from a probe where `keeper` was
reachable from `@world`; both oracles are right under different world
states, and the fix is to restrict the consumer scan to reachable
consumers.

Bed: `TEST/layers/l0-fixture-oracle/stage.sh` now honours
`FX_WORLD_EXTRA` (space-separated atoms appended to the staged
`var/lib/portage/world`); a throwaway vdb-only fixture
`fixtures/var/db/pkg/dev-libs/keeperroot-1.0` (`RDEPEND=dev-libs/keeper`,
no matching ebuild — the same "vdb orphan" pattern as `oldmovepkg` etc.)
gives a transitive-reachability cell. Real: `emerge -p --pretend` inside
`localhost/test-portuale:latest`, staged via `in-container.sh` directly
(not through the atomlist-comparator wrapper — this is an oracle-only
exploration, not a pinned case set), one throwaway run dir per cell.

| world state | `--update paired` | `needer` | `needer othermod` | `=paired-2.0` |
|---|---|---|---|---|
| hermetic | `[U] 2.0`, silent, rc 0 | `U`+`N needer`, no block, rc 0 | 3 merges + block (`othermod` only), rc 1 | `U`, no block, rc 0 |
| keeper in `world` | **new shape**: no merge, `WARNING: One or more updates/rebuilds have been skipped due to a dependency conflict`, rc 1 | `U`+`N needer`+block (parent: keeper), rc 1 | 3 merges + block (parents `othermod` **and** keeper), rc 1 | `U`+block (parent: keeper), rc 1 |
| `keeperroot` (RDEPEND keeper) in `world` | (not re-run: same reachability closure as the row above) | byte-identical to "keeper in world" (only `Dependency resolution took` differs) | byte-identical | byte-identical |

Run dirs: `TEST/logs/l0-fx-054-hermetic`, `l0-fx-054-keeperworld`,
`l0-fx-054-keeperroot` (all four cases each).

Extra cells:

- hermetic, `--complete-graph-if-new-ver=n` (`l0-fx-054-noauto`):
  `--update dev-libs/paired` merges `[U] 2.0` with **no** block (auto-enable
  off means complete mode never triggers, so the pin -- reachable or not --
  is never even scanned); `dev-libs/needer` the same, `U`+`N needer`, no
  block. Confirms the flag fully bypasses the mechanism, independent of H.
- keeper in `world`, `--nodeps` (`l0-fx-054-nodeps`): `=dev-libs/paired-2.0`
  merges `[U] 2.0` with no block -- `--nodeps` disables the dependency
  walk entirely, so complete mode's consumer scan never runs even with a
  reachable pin present.

**Verdict: H holds for three of the four cases** (`needer`, `=paired-2.0`,
the `needer`+`othermod` triangle) — with `keeper` reachable from `@world`
(directly or transitively through `keeperroot`), real reproduces
511e659's exact shapes byte-for-byte (module the volatile "Dependency
resolution took" line), confirming the fix is reachability gating, not a
mechanism real doesn't have. The `--update dev-libs/paired` case is a
**new third shape** under keeper-in-world that 511e659 never observed: no
merge at all plus a `WARNING: ... skipped due to a dependency conflict`
notice (portage's "the entry never enters the graph, so it reports a
skip instead of a slot collision" branch — a *different* real code path
than the `!!! Multiple package instances...` block the other three
cases render, since there is no competing hard requirement forcing the
upgrade in this shape). `test_satisfiable_installed_pin_still_holds_the_
upgrade` only pins the hermetic (silent) shape and needs no change; the
keeper-in-world shape for this one case is new coverage, not a
correction, if S1 adds it.

Commit: bed switch + throwaway fixture + this section (no code change).

## #54 S1 — surfaced regression: the triangle's block disappears entirely

S1 (`b27bdb4`) gated `reverse_dependency_constraints`'s consumer scan on
`ResolveCtx::slot_op_reachable`, as S0 confirmed. Fixed 3 of the 4 cases
byte-identical to real hermetically (`--update paired`, `needer`,
`=paired-2.0` all merge cleanly, no block) and reproduced 511e659's exact
keeper-reachable shapes on a copied configroot (K2).

**New finding, not anticipated by the S1 plan:** the needer+othermod
triangle's residual-conflict block, which S1 §3 expected to "list only
othermod" once gated, instead **disappears entirely** — portuale merges
all three packages silently, no block, exit 0. Root cause: the block
(`build_residual_slot_conflicts`) is driven exclusively by
`reverse_dependency_constraints`'s *dropped* pins; with keeper correctly
excluded (unreachable), `dropped` is empty and the function never fires.
Real's mechanism for this specific case is different: needer's `>=2.0`
and othermod's `<2.0` are both **directly-requested** hard atoms (no
installed vdb consumer at all), so real's ordinary two-hard-atom
slot-conflict detection reports it independent of `_complete_graph`.
Portuale has no equivalent path — the vdb reverse scan is the *only*
producer of this block shape, and it doesn't fire when both conflicting
atoms come from the requested-atom tree itself.

Surfaced via `AskUserQuestion` mid-S1 (the fix as specced was already a
clean win for 3/4 cases; building the missing mechanism inline risked
guessing at a central resolver path). **Decision: ship S1 as specced,
file the gap separately as backlog #57.** Contract coverage:
`test_needer_othermod_triangle_merges_cleanly_when_the_pin_is_unreachable`
documents the new (narrower) divergence with a docstring explaining why.

## #54 S2 — fixture oracle + L0 verification

Fixture oracle (`TEST/run/l0-fixture-oracle.sh`, run
`l0-fx-20260915T173233Z`): **0 unexplained**, 14 probes / 10 clean /
parity 0.714 / 17 explained. The two stale `portuale-bug` #54 entries
were deleted (S1); `triangle-residual-conflict-exit`'s reason text was
updated to describe the new gap, and two new entries
(`triangle-residual-conflict-missing-block-header`/`-second-line`)
explain the triangle's now-missing block lines, both referencing backlog
#57 (filed in S3).

Full L0 (`TEST/run/l0-resolver.sh`, run `l0-20260915T173332Z`, same
120-probe atomlist as the last recorded snapshot): **100/120 clean,
parity 0.833** (up from the prior 96/120 = 0.800 — a net improvement,
not a regression: the delta is other work landed since that snapshot,
not #54). 0 portuale invariant violations, 0 control violations. None of
the 32 unexplained findings mention `paired`/`needer`/`othermod`/
`keeper`/`revdeptarget`/`revdepconsumer`/`residual`/`libdisplay-info` —
every one is a pre-existing, already-triaged cluster (merge-order #,
`gcr[gtk]`/`xwayland[libei]` masked-during-backtracking, `plasma-meta`
truncation). `media-libs/libdisplay-info` does not appear at all,
confirming the reverse-dep-atoms fix (memory
`complete-graph-reverse-dep-atoms`) stays intact — real's `@world`
reaches almost every installed package on the live tree, so the
reachability gate has (as predicted) no visible effect there; the gap it
closes is specific to a consumer *outside* every reachable set, which
the live tree's `@world` essentially never leaves anyone in.

Commit: allowlist fixes + this section + S1's surfaced-regression
writeup above (no further code change).

## #57 S0 — oracle matrix: backtrack trace + argv order + solvable control

`docs/06.057-directly_requested_hard_atom_conflict.opus.md` §S0. Same
fixture as #54 (`dev-libs/{needer,othermod,paired}`, `paired-1.0`
installed), plus a new throwaway solvable-control fixture
`dev-libs/plainuser-1.0` (`RDEPEND="dev-libs/paired"`, bare/unversioned)
and a new `stage.sh` knob `FX_DROP_VDB` (space-separated `cat/pf`
entries removed from the staged vdb before either PM runs — the
merge-vs-merge control needs `paired-1.0` *not* installed without
touching the shared fixture).

Real: `emerge -p --pretend` (cell a also `--debug`) inside
`localhost/test-portuale:latest`, staged via `in-container.sh` directly
(oracle-only exploration, not the atomlist-comparator wrapper), two run
dirs (cells a/b/c/e/f share the default staging; cell d uses
`FX_DROP_VDB=dev-libs/paired-1.0`).

| # | args | real | portuale |
|---|---|---|---|
| a | `needer othermod --debug` | block (installed 1.0 pulled by othermod, merge 2.0 pulled by needer), rc 1, `backtrack: 4/20` | no block, rc 0 |
| b | `othermod needer` | same block, rc 1 | no block, rc 0 |
| c | `needer othermod --backtrack=0` | **byte-identical block** to cell a (`backtrack: 0/0`) | no block, rc 0 |
| d | `needer othermod`, `paired-1.0` dropped from vdb (`FX_DROP_VDB`) | block (**both** sides `ebuild scheduled for merge`), rc 1 | **same block already renders** (existing `resolved_slots` path), rc 0 |
| e | `plainuser needer` / `needer plainuser` | `U 2.0` + both new, silent, rc 0 (both orders) | identical, silent, rc 0 (both orders) |
| f | `othermod` / `needer` alone | matches #54 S0 hermetic baselines | identical |

Run dirs: `TEST/logs/l0-fx-057-main-20260915T195546Z` (a/b/c/e/f),
`TEST/logs/l0-fx-057-notinstalled-20260915T195546Z` (d).

**§2 open question, answered by cell a's `--debug` trace and confirmed
by cell c:** real's final reported state is exactly "the tracker holds
installed 1.0 plus merge 2.0, unsolvable, choices exhausted" — **none**
of the 4 backtrack tries change what gets reported. Trace detail:
`runtime_pkg_mask` grows across tries 1-4 (try 1 masks *both* the
`paired-1.0` ebuild and its installed instance for the slot conflict
against needer; try 2 masks the `paired-2.0` ebuild, which then makes
othermod's own `<2.0` dep unsatisfiable — "All ebuilds that could
satisfy othermod have been masked"; try 3 and 4 keep widening the same
way, ending with needer itself unsatisfiable), and after
"backtracking aborted after 4 tries" `get_best_run` falls back to the
**original, first-discovered** conflict shape — byte-identical to the
`--backtrack=0` output (cell c). There is no masked-`paired-1.0`
fallback shape in the final render; the "installed instance in the
block" is simply the untouched initial state, not a side effect of a
backtrack step. **Verdict: "tracker collision + slot-conflict
backtracking exhausted" — proceed to S1 as specced (no K4 trigger).**

**Portuale's own `--debug` trace (cell a)** confirms §2's outcome-model
diagnosis directly: the digraph shows `needer -> (paired-2.0, ebuild
scheduled for merge)` (an `Upgrade` outcome, indexed in
`resolved_slots`) and `othermod -> (paired-1.0, installed)` (an
`AlreadyInstalled` outcome, invisible to slot tracking) — two different
outcomes for the same slot, confirmed by the earlier static reading of
`lib.rs`, not merely inferred. It also reproduces the exact `Parent Dep`
misfiling §2 flagged: **both** `dev-libs/paired required by (needer)`
and `... required by (othermod)` print under the single `paired-2.0`
child, even though the digraph correctly routes othermod's edge to
`paired-1.0` — the trace's per-child parent listing reads a different,
coarser map than `build_slot_conflict` will need. S1 step 1's
`state.slot_pullers` change is expected to fix this as a side effect
(verify, per the doc's "check, don't assume").

**Cell d confirms the K2 scope note ("mirrors `_process_slot_conflicts`
... nothing broader") from the opposite direction**: with `paired-1.0`
not installed, both instances are ordinary merge candidates and
portuale's *existing* `resolved_slots` slot check already renders the
same block real does (module path-string formatting the comparator
already normalizes) — confirming the gap S1 needs to close is
installed-instance-specific, not a defect in the slot-conflict
mechanism itself.

**Cell e confirms the K2 regression guard**: a bare, unversioned
`dev-libs/paired` dependency (satisfied by whichever instance is
already graphed) merges silently in both argv orders, both PMs,
byte-identical merge-list order — the solvable shape S1 step 3 must
keep silent stays silent today, giving S1 a concrete before/after
control.

Commit: `stage.sh` `FX_DROP_VDB` knob + `plainuser` fixture (ebuild +
md5-cache entry) + this section (no resolver code change).

## #57 S1 — installed-instance slot tracking

`docs/06.057-directly_requested_hard_atom_conflict.opus.md` §S1, built on
the S0 verdict above (tracker collision + slot-conflict backtracking
exhausted; no K4 trigger). The walker now indexes `AlreadyInstalled`
graph nodes by `(cat, pkg, slot)` in a new `PassState::installed_slots`
and checks the slot from **both** sides — the `AlreadyInstalled` early
branch against `resolved_slots`, and the merge-outcome slot check
against `installed_slots` — so an installed instance is a slot-conflict
party whichever argv order graphs it first. Neither check `continue`s:
real keeps the installed node and still merges the other instance.

Solvable vs unsolvable is **not** a second solver: the record is fed to
the existing backtracker, whose `collect_feedback` `slot_want`
solvability check is portuale's `_solve_non_slot_operator_slot_conflicts`
(the merge-vs-merge machinery behind
`fixture_solvable_slot_conflict_is_reconciled_by_backtracking`). A
jointly-satisfiable slot reconciles on the retry and the record
disappears with it; an unsatisfiable one exhausts the mask trials and
`get_best_run` reports the original shape, matching real's
`backtrack: 4/20` run byte-for-byte modulo the two residues below.

| cell | before S1 | after S1 |
|---|---|---|
| a `needer othermod` | no block, rc 0 | block (installed 1.0 ← othermod, merge 2.0 ← needer), rc 0 |
| b `othermod needer` | no block, rc 0 | same block, rc 0 |
| c `--backtrack=0` | no block | same block as cell a |
| e `plainuser needer` / reverse | silent | **still silent** (K2 guard holds) |
| f singles | unchanged | unchanged |

Residues, both deliberate and both pre-existing:

* **rc** stays 0 (K1). Allowlist entry `triangle-residual-conflict-exit`
  reworded to cite the convention, owner `portuale-bug` →
  `claude-opus-5`; `-suppressed-merge-list` likewise; the two
  `-missing-block-*` entries are deleted.
* **instance order** inside the block is inverted vs real in every case:
  portuale's queue is FIFO and lists the first-graphed instance first,
  real's `_dep_stack` is LIFO and lists the other one first. Cell d
  (merge-vs-merge, neither instance installed) shows the same inversion
  *without* any S1 code, so this is not new. The comparator compares
  `!!!` lines as a set, so it is invisible to the oracle.

Three side findings:

1. **The `--debug` `Parent Dep` misfiling is NOT fixed** by step 1's
   `slot_pullers` change (the doc said "check, don't assume" — checked,
   and it is not). `slot_pullers` already carried both parents with
   their atoms, which is why `build_slot_conflict` files them correctly;
   the trace reads `GraphEntry::required_by` instead — a `(cat, pkg)`
   map with no atom text and no per-instance attribution — and prints
   the bare `cat/pkg`, not the pulling atom. The installed `paired-1.0`
   node is not even in the entry list `dump_resolution_walk` iterates
   (the cluster-I `mergebound_cp_slots` retain drops an
   `AlreadyInstalled` entry whose cp/slot is also merge-bound), so there
   is no second child to file anything under. `--debug` trace fidelity,
   not the block: belongs with backlog #59.
2. **`test_oracle_slotop_rebuild_order` pinned real's merge list against
   the wrong fixture.** Upstream re-uses the name `app-misc/C` for two
   different ebuilds — `test_slot_operator_rebuild.py` case 1's
   `|| ( app-misc/X app-misc/A:= )` and `test_slot_conflict_rebuild.py`'s
   `<app-misc/A-2` cap — and portuale's tree has one ebuild per name,
   carrying the cap. The rebuild-order case therefore rebuilt its
   consumer against the *cap*, whose `<app-misc/A-2` only installed
   `A-1` satisfies; before S1 nothing looked at the installed instance's
   slot, so the cap was silently ignored and the case passed for the
   wrong reason. New fixture `app-misc/Cor` carries upstream's ebuild,
   the case points at it, and real's `[A-2, (B-0, Cor-0)]` is
   reproduced exactly.
3. **`build_residual_slot_conflicts` needed two plumbing fixes** for the
   #54 interplay (step 5): its records now go through
   `record_slot_conflict` instead of `extend` (the walker and the
   residual builder can produce the same `(package, merge version,
   installed version)` triple — the keeper-reachable triangle — and real
   renders one block carrying both parents, which is the residual
   record); and the dropped reverse-dep pins are collected *before* a
   slot-conflict mask feedback can return, with the current node
   adopting the pass's merged accumulators the way `DeadEnd` already
   does, or `get_best_run` came back to a node that had never seen them
   and keeper's `=dev-libs/paired-1.0` parent line vanished.

One more S1-only resolver fix fell out of the K2 guard: the two
`avoid_update` shortcuts in `resolve_pretend` returned
`AlreadyInstalled` without consulting `extra_constraints`, so the
solvable retry re-derived the same collision forever (the plainuser
control looped and then reported a block). Both are now gated on the
constraint set, which is what real's `_select_pkg_highest_available`
weighing the whole atom set means.

Verification: workspace-root `cargo build/test/clippy --release` clean
(378 portage-repo unit tests incl. three new ones next to
`reverse_dependency_constraints_skips_an_unreachable_installed_consumer`);
full `pytest tests -q` 1693 passed / 0 failed / 5 xfailed. Corpus drift
was exactly two contract rows, both reviewed:
`test_oracle_slotop_complete` (`socc-2` → `socc-1`, i.e. **K3
alternative A**: the bug-614390 strict xfail XPASSed, the full merge
list now equals real's, the marker is removed and the test is a
permanent pin) and `test_oracle_slotop_rebuild_order` (`C-0` → `Cor-0`,
side finding 2). `tests/corpus/expanded.json.xz` — the whole fixture
atom × option grid — did not drift at all.

Live regression check on this host's real tree: `emerge -pu --deep
@world` (71 lines, blockers and slot-op rebuilds) is **byte-identical**
before and after. It is also ~37 % slower (30.7 s → 42.4 s); an ablation
build that keeps every new vdb read but skips the two
`record_slot_conflict` calls times 31.0 s, so the whole delta is extra
resolver passes — a solvable installed-vs-merge collision somewhere in
`@world` now takes the same reconcile-and-restart route real takes.
Correct work, not overhead; flagged here for S2 to confirm against the
120-probe L0 bed.

## What a fixture addition must not break

A new fixture that real will read needs: a digest for **every** ebuild in
its package dir, a category in `etc/portage/categories`, and no `*/pkg`
atoms or `profiles/updates` comments. A new fixture that trips one of
these shows up as a real-only staging diagnostic, not as a portuale
difference — check `stage.sh`'s header before filing a finding.
