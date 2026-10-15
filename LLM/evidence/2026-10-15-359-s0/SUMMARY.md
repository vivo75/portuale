# S0 SUMMARY (plan 02.359, slice S0 only)

All S0 oracle runs from the plan are captured (37 new cells, both sides, fresh
stage per cell per D3) plus the 3 `pg359-*` fixtures (collision-checked, left
uncommitted). Per planned cell, verdict vs the plan's prediction:

## (a) Debugs + variants — all match the plan's predicted mechanisms

- `dbg-withdeps-pu`: installed `upgradepkg-1.0` picked over ebuild `2.0`
  (`:8445-8458` return). Matches. → S2.
- `dbg-needskeywordmasked-pu`: masked installed `keywordmaskedpkg-2.0`
  picked. Matches. → S2.
- `dbg-uapp-pvN`: `reinstall=True` hides the installed instance (no
  `installed:` line), edge then dropped (`_ignore_dependency`, `no
  children`). Matches; the stop rule does NOT fire. → S2.
- `s0-a-maskedusedrift` (new staged-delta cell): Portage `N` parent only
  rc 0; portuale `R ~keywordmaskedusepkg-2.0 + N` then USE-failure rc 1.
  Matches prediction direction. → S2.
- `s0-a-gs-range` (`>=gs-1.0 -pvu`): Portage `NS + U` (greedy covers range
  atoms, D5); portuale `NS` only. Matches. → S3.
- Controls reused: `362-dtop-pvu-deep1` (AGREE), `@world` depth-0 =
  `s0-b-world`.

## (b) Corpus adjudication — Portage ground truth for the S2 bless table

- `@world` / `@selected` / `@system` / `samepkg @selected` / `-isd=y` /
  `-isd=n` @world: Portage prints IDENTICAL 3-or-2 N rows everywhere, never
  the `U upgradepkg` row portuale adds. Matches the #362 reading; S2 blesses
  only drift equal to these captures. `flipinstconsumer`, `unsatuseslot`
  (both `R + N`, rc 1 `USE changes necessary`) and `needer` (`U paired + N`)
  AGREE on both sides → S2 controls. `needskeywordmasked` (no downgrade),
  `toolchainroot` (no `fakeglibc`/`libc` rows), `rebuildtrigger` ×3 (U-only,
  NO `R rebuildconsumer` — the `test_rebuild_if_star` pin encodes non-Portage
  behaviour) DIVERGE → S2 re-grounds.
- **STOP-1**: `s0-b-isd-bare` — Portage rc 2
  (`--implicit-system-deps` consumes `@world` as its value), contradicting the
  plan's bare-as-control reading. Stopped, reported; S2 must not bless that
  argv, owner decides the argparse shape.

## (c) #361 matrix — key-presence rule confirmed with two exceptions

- `--usepkg`, `-k` → `Total: 0` both sides (AGREE, S1 controls);
  `--getbinpkgonly=n` → `U btool` both sides (AGREE, S1 control);
  `FEATURES=getbinpkg --getbinpkg=n` → `U btool` both sides (AGREE: cmdline
  `=n` overrides env FEATURES — S1 control).
- `EMERGE_DEFAULT_OPTS=--usepkg=n` → Portage `Total: 0`, portuale `U`
  (DIVERGE → S1 tracks env-givenness). Plain `FEATURES=getbinpkg` →
  Portage `Total: 0`, portuale `U` (DIVERGE → S1 adds the fold; #393 not
  needed).
- **STOP-2**: `s0-c-usepkgonly-n` → Portage `Total: 0` rc 0, no error —
  the pre-declared "no-ebuilds effects" does not materialise on this shape,
  so the #396 condition is not met; discriminating probe left to S1/owner.

## (d) Moves re-verification — D3 closed

- `movedepconsumer` moves-on (both `-pv --deep` and `--pretend`, fresh
  stage): Portage rc 1 `no ebuilds "dev-libs/oldmovepkg"` — NOT a staging
  artefact; #391 confirmed (filing pending — backlog edits out of scope for
  this slice). moves-off: Portage rc 0 `N` (installed-only `oldmovepkg-1.0`
  satisfies the dep — a #360 instance); S2/S4 may move the corpus pin only
  to this capture. portuale is inverted on all four (moves-on rc 0 with a
  spurious `N newpkg` row under `--deep`; moves-off rc 1).
- `slotmovepkg` moves-on: both sides EMPTY rc 0 — old SAME verdict stands,
  no artefact. moves-off: Portage empty, portuale `NS` (S2 material).

## (e) Fixtures — Q4 answered YES, #397 not triggered

- `pg359-gbapp` (`BDEPEND=pg34-gone`, installed-only): Portage `N gextra +
  N gbapp` rc 0 with AND without `--with-bdeps=y` (note: the installed-only
  `gextra` row has no `to <root>`); portuale rc 1 both. → S4 lifts the
  `buildtime_hard` gate per Q4 default.
- `pg359-gsblk -pvu`: byte-identical `NS + uninstall + blocks b +
  Conflict: 1 block (all satisfied)`, NO `U` row → S3 control (guards the
  blocker shape; does not discriminate lookahead passes).
- `pg359-vapp -pv`: byte-identical 4 rows, `||` → `vy` → S5 control; S0
  shows no `virtual/*`-deferral divergence, so **#397 is not filed** on this
  evidence (plan condition "only if S0 shows it" not met).
- 23 files, `md5sum`-verified md5-cache entries, vdb `pg359-gsblk-1.0`
  (`CATEGORY`/`SLOT=1`/`repository`, matching house style); no `pg359-*`
  name existed before (725-dir scan + plan-block check); left uncommitted in
  pmtest alongside the parallel executor's in-flight `pg367-*` files.

## Deferred (in plan S0 but out of this slice's brief)

- Adding the #359–#369 cells to
  `differential-test-bed/atomlists/l0-fixture-oracle.txt` (edits a tracked
  bed file; brief allowed only new fixtures + runs) and filing #391/#392
  (backlog edits forbidden) — all flagged for the owning slices/close-out.
- `matrix.md` for this slice lives HERE (`matrix.md`), not in
  `../2026-10-15-plan-359/matrix.md`, per the brief's evidence-dir rule; the
  old evidence dir was reused read-only and not modified.

## Provenance / hygiene

- `cmd-provenance.txt` (binary mtime 2026-10-15T19:18:07Z, portuale
  `f221c03f` with empty `crates/` delta since the build, pmtest `f8c4bff` +
  untracked list, container `localhost/test-portuale:latest` =
  Portage 3.0.82.2, both runner corrections). Scratch `/var/tmp/s359`
  deleted; `/dev/shm` untouched; nothing committed/pushed/branched; no
  product code, tests, backlog or plan files edited.

Status: **DONE_WITH_CONCERNS** (S0 complete; STOP-1 and STOP-2 sub-parts
stopped and reported above — no improvisation).
