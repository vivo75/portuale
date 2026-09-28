# G215 report — backlog #215 (v2 #24f): the in-walk unsatisfied probe (2026-09-28)

Worktree `/home/vivo/repo/PORTUALE/wt-215-unsatisfied-probe/{portuale,pmtest}`,
branch `backlog/215-unsatisfied-probe`.
Real source: vendored portage 3.0.82.2 (`3rdparty/portage/lib/_emerge/depgraph.py`).

## Status: DONE (BED-PENDING — coordinator runs the beds)

## Commits (paired, pmtest first)

- pmtest `345e079` — `fixtures: #215 S0 unsatisfied-probe oracle shape + g215 atomlist`
  (S0: ebuilds + md5-cache + `sousat-unsat` stage fragment + new
  `l0-fixture-oracle-g215.txt` wired into `l0-fixture-oracle-all.sh`;
  names the portuale branch + slice).
- pmtest `af49e43` — `contract: #215 pin for the unsatisfied-probe parent reinstall`
  (S1 pin; names the portuale branch + slice).
- portuale `c0665bae` — `resolver: #215 port the in-walk unsatisfied slot-operator
  probe (pmtest af49e43)` (S1 product change; quotes the pmtest short sha).

`READY-FOR-BEDS c0665bae` written as the last line of `g215-progress.md`.
No `BEDS c0665bae: …` reply was present at report time; per common
rule 13 the beds below are coordinator-only and DONE is reported with
BED-PENDING. Entry #215 not flipped (brief).

## §1. S0 — the shape, real's answer, and a version artifact disclosed

Shape: installed `app-misc/sousatpar-1` with an unsatisfied built `:=`
dep (`RDEPEND=">=app-misc/sousatprov-1:0/1="`, EAPI-bearing vdb so
real's `FakeVartree` overlay registers it; old provider slot abandoned
AND uninstalled) + available replacement parent (here the parent's own
live ebuild, `RDEPEND="app-misc/sousatprov:="` resolving to
`app-misc/sousatprov-2` at `SLOT="2/2"`). EAPI-8 ebuilds, profiles
untouched (EAPI gate); RDEPEND-only both sides (the g212 precedent,
dodging the staged-ROOT BDEPEND cross-root split, #242).

Real's answer, two independent oracles (both 3.0.82.2):

1. **Vendored ResolverPlayground, host-local, version-consistent
   shape** (`/tmp/opencode/g215/pgsousat-debug.py`, GPG home created
   under `/tmp` and stopped+removed per rule 17): mergelist
   `[sousatprov-2, sousatpar-1]`, and `--debug` fires verbatim
   ```
   slot_operator_unsatisfied_probe:
      existing parent package: (...sousatpar-1..., installed)
      existing parent atom: >=app-misc/sousatprov-1:0/1=
      new parent package: (...sousatpar-1..., ebuild scheduled for merge)
      new child package:  (...sousatprov-2..., ebuild scheduled for merge)
   backtracking due to unsatisfied built slot-operator dep: ...
   ```
   then `backtracking try 1` with
   `forced reinstall atoms: ... atom: app-misc/sousatpar:0`
   (`@__auto_slot_operator_replace_installed__` seed). At
   `--backtrack 0` the same Playground run FAILS:
   `emerge: there are no ebuilds to satisfy
   ">=app-misc/sousatprov-1:0/1="`, success=False (the probe is
   `_allow_backtracking`-gated, `:3447`). Without `--update` the run
   still merges both (no probe lines -- the satisfied path heals).
   A veto parent (`RDEPEND="<app-misc/sousatprov-2"`, live+installed
   +world) does NOT stop real: mergelist unchanged (the unsatisfied
   probe has no `check_reverse_dependencies` refusal, unlike #211).
   Upstream's #522652 shape
   (`test_slot_operator_rebuild.py::testSlotOperatorRebuild`) does NOT
   fire the probe (0 debug matches) -- different machinery.
2. **Live container sessions** (`localhost/test-portuale:latest`,
   fixture-oracle `stage.sh` staging, host-exact `ROOT=/`):
   - Session 1 (image portage 3.0.81.3): `N prov-2 + UD par-1`, rc 0.
   - Session 2 (upgraded to the pin 3.0.82.2 first, per
     `in-container.sh`'s own PIN step): byte-identical `N prov-2 +
     UD par-1`, rc 0.
   - DISCLOSED ARTIFACT: both sessions staged the vdb as
     `sousatpar-1.0` (version `1.0`) against ebuild `sousatpar-1`
     (version `1`) -- a phantom in-slot DOWNGRADE (`UD`), not the
     probe shape. The version-consistent reruns (vdb `sousatpar-1`)
     are what §2/§3 verify. The sessions also never captured probe
     `--debug` lines live (the probe fires in the Playground's
     split-root/synthetic-profile shape; live-single-root heals
     without counted restart -- same merge set either way).

Pre-fix portuale on the version-consistent shape: `-uD` aborts rc 1
(`emerge: there are no ebuilds to satisfy
">=app-misc/sousatprov-1:0/1="`, required-by `[installed]` +
`(argument)` rows already real-shaped); `--backtrack=0` aborts rc 1
(agreement); plain arg merges rc 0 (`N` + reasonless `R`, agreement).
So the divergence is exactly the `-uD` abort-vs-heal -- the probe's rule.

## §2. S1 — what landed

`slot_operator_unsatisfied_probe` (`rust/portage-repo/src/lib.rs`,
next to the #211/#212 probes) + a dead-end-pass trigger in
`collect_feedback` (before the `DeadEnd` return, gated
`ctx.backtrack_max > 0`, returning the scan's own
`BacktrackFeedback::Config` restart on set growth -- monotone, so the
search terminates; no hit stays a dead end). Per-NVC-entry walk over
`slot_want` atoms: built-form (`is_built_slot_op`) + installed owner
+ owner unmasked (else restart-loop) + owner non-excluded (real
`:2818-2824`) + same-slot visible non-excluded ebuild replacement
whose live `=`-operator non-blocker atom on the dep cp resolves
(`without_use` + visible-tree-or-installed, real `:2835-2845`).
Runtime keys always, `DEPEND`/`BDEPEND` iff `with_bdeps` (the scan's
key discipline; real's `validated_atoms` spans all keys -- documented).
USE-reduced against the replacement's effective USE (real
`matchall=True` -- documented fork). No refusal gate (real has none
here). Same-version-first seeding is cp-level like the scan's (with a
single available parent version the two coincide; newer-version
coexistence is a filed cut -- see §4).

Post-fix worktree binary on the S0 staging: `-uD` rc 0 with
`[ebuild N] sousatprov-2` + `[ebuild rR] sousatpar-1` (the `rR`
matches the Playground debug display path); `--backtrack=0` still
rc 1 with the same missing-dep line; plain arg unchanged (`N`+`R`).

## §3. Verification (all green, release profile)

- `cargo fmt --check`: clean (after one `cargo fmt` pass).
- `cargo clippy --release --all-targets`: zero warnings.
- `cargo test --release` (whole workspace): **1894 passed / 0 failed**
  (log `/tmp/opencode/g215/cargo-test.log`), incl. the new
  `slot_operator_unsatisfied_probe_seeds_a_replacement_parent`
  (positive + unbuilt-atom / masked-owner / excluded-owner guards)
  and every pre-existing slot-op test unchanged.
- Focused contract pin
  `test_oracle_slotop_unsatisfied_probe_heals_through_parent_reinstall`:
  passes (log: pmtest run `pmtest-g215-pin2`, 1 passed). Set-based
  (`_slotop_cpv` set equality, the conflict-mass precedent -- robust
  to display-letter questions the bed owns); `-uD` arg, `-uD @world`
  and plain arg heal, bt0 aborts rc 1 with real's line (stderr).
  RED basis: pre-fix binary aborts the `-uD` shapes rc 1 (verified
  manually on the true shape; the pin's `-uD` arms assert rc 0).
- Full pmtest suite: **2179 passed / 37 skipped / 4 xfailed / 0 failed**
  (log `/tmp/opencode/g215/pmtest-full.log`, basetemp
  `/var/tmp/pmtest-g215-full`). Corpus drift: NONE (do NOT bless --
  nothing to bless). `pmtest/3rdparty/portage` present. No GnuPG home
  left behind (rule 17: Playground homes stopped + removed; Bedford
  probes create none).
- Private basetemaps for every pytest run.

## BED-PENDING (coordinator only, from the pmtest worktree)

1. `FX_SOUSAT_UNSAT=1 FX_HOST_ROOTS=1 differential-test-bed/run/l0-fixture-oracle.sh differential-test-bed/atomlists/l0-fixture-oracle-g215.txt`
   -- expect the two `-uD` cells green post-fix with live-agreeing
   display (esp. the `rR` vs `UD` letter on the reinstall row and the
   `backtrack:` count), the bt0 cell rc-1-abort agreement, the plain
   cell agreement. NOTE: the S0 live probes ran the heal shape at
   3.0.81.3 (mismatched versions, disclosed above) and at pinned
   3.0.82.2 (same mismatch); the version-consistent live display is
   bed-determined -- if live prints anything but `[ebuild rR]
   sousatpar-1` (+ `N prov-2`), the pin's set-equality still holds
   but the cell will show it.
2. `differential-test-bed/run/l0-fixture-oracle-all.sh` -- expect
   10/10 lists green (the probe only fires on dead-end passes, so
   green shapes are unreachable by construction; the suite above
   confirms no abort pin moved).
3. `differential-test-bed/run/l0-resolver.sh` -- expect
   identical-or-better vs the last green L0 row (Track G guard).

## §4. Concerns / disclosures

1. **#213 overlap** (`prune_rebuilds`, parallel branch
   `backlog/213-prune-rebuilds`): that branch has no code diff vs
   `main` at report time, so nothing collided. Overlap surface if it
   lands: both write `slot_operator_replace_installed` and ride
   `BacktrackFeedback::Config` restarts; #213's prune restart
   (`:5763-5780`) consumes the replace set on re-resolve, which now
   can additionally contain unsatisfied-probe seeds. No logic
   conflict, but its S0 should re-run the sousat cells.
2. **Newer-version coexistence cut**: with same-version AND newer
   parent ebuilds available, real reinstalls same-version
   (`_replace_installed_atom`'s `=cpv` half) while this port seeds
   cp-level (highest wins). Single-version shapes coincide; the mixed
   shape is untested on both sides.
3. **Conditional-`:=` fork**: replacement atoms reduce against
   effective USE; real uses `matchall`. Only matters with
   USE-conditional `:=` on the replacement -- no pin covers it.
4. **Playground-vs-live walk difference** (§1, disclosed not
    resolved): the probe's debug lines fire in the Playground shape
    but no probe lines were captured in either live session; merge
    sets agree regardless. If the bed shows a live display the pin
    didn't predict, the pin needs a letters update, not a logic one.
5. Did not flip backlog entry #215 (brief).

## Round 2 — review fixes (2026-09-28, portuale `a454e150`, pmtest `70b18f7`)

Review of `c0665bae` (`g215-review.md`, REQUEST_CHANGES); every fix
cites real 3.0.82.2 `lib/_emerge/depgraph.py` file:line.

**Important 1 — masked replacement screen.** The candidate filter now
skips a replacement ebuild hidden by `runtime_pkg_mask`, per package
instance like `_iter_similar_available` (`:3036-3040`, which skips
`pkg in _runtime_pkg_mask` before the excluded check): a `!=cp-ver`
negative screens only that version (recovered with the same
`split_cpv` idiom C2 uses), and a `[binary]`-marked negative never
screens the same-version ebuild (real keys by package object, so the
ebuild stays yielded; see `is_binary_only_mask`). The owner-side
check narrowed the same way: real `:3447-3448` tests the installed
`dep.parent` instance, so only a masked installed version withholds
its own slot instead of the old cp-level `contains_key` refusal.
Unit assertions: masking one of two same-slot candidates still seeds
through the other (per-version precision); masking the only
same-slot candidate yields no seed while the owner instance stays
unmasked.

**Important 2 — owner↔atom pairing.** The probe seeds only owners
whose own recorded want is the built atom. The pairing was
recoverable without new plumbing: every queued owner atom is
recorded at queue time in the pass `slot_pullers` triples
(`(parent_cat, parent_pkg, parent_version, atom_text)` per dep cp --
merge-bound owners at the flat-deps record site, installed owners at
`enqueue_dependencies`' A4 site, top-level ownerless), so the probe
takes `&pass.slot_pullers` (threaded through `collect_feedback`)
and requires a triple matching `(owner, built-form, dep cp)` --
real probes a single dep edge (parent -> the built atom,
`:3447-3458` into `:2817`), never the cp-level `required_by` x
`slot_want` cross product. Cost of threading: one new probe
argument; no walk changes. (A USE-conditional built want never
parses at the pairing check -- the puller text is pre-evaluation --
so that corner rides the already-filed conditional-`:=` fork, §4.3.)
Unit assertion: owner A (installed, plain atom, own live
replacement) + owner B (installed, built atom) seeds only B -- the
old cross product seeded both. The S0 contract pin still passes
unchanged, proving the real walk records the paired triple.

**Minor 4** -- unit assertions for no replacement (stays a dead
end), uninstalled owner (`installed.is_empty()` arm), and
different-slot replacement (the `c.slot == *parent_slot` gate).

**Minor 5 -- residue for the coordinator (file a number).** The
non-installed-parent `slot_operator_mask_built` arm (`:2901-2903`)
is a cut of its own, not covered by #212, and deliberately not
ported. Shape sketch: a non-installed ebuild parent (e.g. a new
`app-misc/nipar-1` ebuild, nothing installed) whose dep string
carries an unsatisfied built `:=` (e.g. bound to an abandoned
provider slot with no visible provider) -- real masks that parent
instance (`slot_operator_mask_built`) and restarts; this port
`continue`s past parentless owners, so the pass stays a dead end
and the run aborts where real steers. Doc comment fixed to say so.

**Minor 6** -- one line each in "Gates and cuts" for
`dep.atom.package` (`:3449`, vacuous -- parsed atoms always carry a
package) and `onlydeps` (`:2844-2845`, ignored -- the re-resolve
passes `&[]`). The `:3036-3040` citation corrected (mask skip +
excluded check, previously attributed to excluded only).

**Minor 7** -- `stage.sh` fragment comments renumbered into file
order (pmtest `70b18f7`): sousat #215 -> §12, slotop-bdeps #65 ->
§13, prune #213 -> §14 (with its "as in §13" cross-reference
fixed). The pmtest merge also resolved the `l0-fixture-oracle-all.sh`
conflict to eleven lists (g213 + g215 entries both kept).

**#213 overlap resolved (§4.1):** #213 landed as `0a51fa6f` /
pmtest `2aef78e` and merged cleanly both sides (portuale merge
`dcfea15b`, pmtest merge `70b18f7`; the pmtest
`l0-fixture-oracle-all.sh` conflict above was the only one). #213's
prune restart consumes the replace set the probe seeds into -- no
logic conflict (rounds 1+2 gates green on the merged tree), but the
beds should still re-run the sousat cells alongside the g213 cells.

**Round-2 verification (merged tree):** `cargo fmt --check` clean,
`cargo clippy --release --all-targets` zero warnings,
`cargo test --release` whole workspace **1894 passed / 0 failed**,
full pmtest suite **2180 passed / 37 skipped / 4 xfailed /
0 failed** on fresh basetemp `/var/tmp/pmtest-g215b-full` (log
`/tmp/opencode/g215b/pmtest-full.log`). Corpus drift: NONE (do NOT
bless -- nothing to bless). Focused pin passes (see above).
`READY-FOR-BEDS a454e150` written as the last line of
`g215-progress.md`; beds below are coordinator-only.

## BED-PENDING round 2 (coordinator only, from the pmtest worktree)

1. `FX_SOUSAT_UNSAT=1 FX_HOST_ROOTS=1 differential-test-bed/run/l0-fixture-oracle.sh differential-test-bed/atomlists/l0-fixture-oracle-g215.txt`
   -- expect the two `-uD` cells green with live-agreeing display,
   the bt0 cell rc-1-abort agreement, the plain cell agreement.
2. `differential-test-bed/run/l0-fixture-oracle-all.sh` -- expect
   11/11 lists green (g213 + g215; the probe only fires on dead-end
   passes, so green shapes are unreachable by construction).
3. `differential-test-bed/run/l0-resolver.sh` -- expect
   identical-or-better vs the last green L0 row (Track G guard).
