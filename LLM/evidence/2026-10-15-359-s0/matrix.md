# S0 matrix (plan 02.359 §2 S0) — Portage rows vs portuale rows

Notation: rows quoted verbatim minus the `to <root>` / `::testrepo` / `0 KiB`
framing (identical on both sides unless noted); `Total:`/`rc` always noted.
`out/<cell>.txt` = Portage, `portuale/<cell>.txt` = portuale.
Verdict vocabulary: AGREE (rows match) / DIVERGE (rows differ);
"matches prediction" = Portage output is what the plan predicted,
"CONTRADICTS" = Portage output contradicts the plan's reading (STOP sub-item,
reported in SUMMARY.md, not adapted). Owner = slice that consumes the cell.

Reused old cells (not re-run; D3-clean because their graphs never touch
`oldmovepkg`/`slotmovepkg`): `362-dtop-pvu-deep1`, `362-withdeps-pu`,
`362-needer-pu`, `362-needskeywordmasked-pu`, `362-toolchainroot-pu`
(old rows re-quoted below from `../2026-10-15-plan-359/`), plus
`361-blib-pvuD-noflags` (`-pvuD pg34-blib` → Portage `U pg34-btool-2`, the
plain control for group C). Fresh re-captures `s0-b-needer`,
`s0-b-needskeywordmasked`, `s0-b-toolchainroot` are row-identical to the old
moved-vdb captures — extra proof those were no artefact.

## (a) Debugs, masked+USE drift, range atom, depth controls

| cell | Portage | portuale | verdict / owner |
|---|---|---|---|
| `dbg-withdeps-pu` `--pretend --update --debug dev-libs/withdeps` (Portage-only) | `ebuild: upgradepkg-2.0` + `installed: upgradepkg-1.0` listed, `Child: (upgradepkg-1.0 … installed …)`; rows `[N newpkg, N withdeps]` rc 0 | n/a (`--debug` unsupported) | matches prediction (`:8445-8458` return) / S2 |
| `dbg-needskeywordmasked-pu` (Portage-only) | `ebuild: keywordmaskedpkg-1.0` + `installed: keywordmaskedpkg-2.0`, `Child: (…-2.0 … installed …)`; rows `[N needskeywordmasked]` rc 0 | n/a | matches prediction (masked-installed keep) / S2 |
| `dbg-uapp-pvN` `-pvN … --debug pg34-uapp` (Portage-only) | `Candidates: [pg34-ulib]`, ONLY `ebuild: pg34-ulib-1` (no `installed:` line — vardb skipped), digraph `uapp (no children)`; rows `[N uapp]` rc 0 | n/a | matches prediction (`reinstall=True` hides installed `:7860-7864`, then `_ignore_dependency` drops the edge); stop rule does NOT fire / S2 |
| `s0-a-maskedusedrift` `-pvN … needskeywordmaskeduse` + staged `package.use` delta `keywordmaskedusepkg -flag` | `[N needskeywordmaskeduse-1.0]` rc 0 (masked installed kept, no reinstall despite USE drift) | `[R ~ keywordmaskedusepkg-2.0 USE="flag"] [N needskeywordmaskeduse]` + `USE changes necessary: >=keywordmaskedusepkg-2.0 flag` rc 1 | DIVERGE, matches prediction direction (Portage keeps; portuale reinstalls a masked package) / S2 |
| `s0-a-gs-range` `-pvu … >=dev-libs/pg34-gs-1.0` | `[NS gs-2:2 [1.0:1]] [U gs-1.1:1 [1.0:1]]` rc 0 — same rows as bare `gs -pvu` | `[NS gs-2:2 [1.0:1]]` only (`Total: 1 new slot`) rc 0 | DIVERGE, matches D5 prediction (greedy applies to a version-range `AtomArg`; portuale misses the `U` row) / S3 |
| `@world` depth-0 control | `s0-b-world` (below): world members update at depth 0 | — | / S2 |
| `-pvu --deep=1 dtop` control | reused `362-dtop-pvu-deep1`: Portage `[U dlib-2 [1]] [N dtop]` = portuale rows | AGREE | / S2 |

## (b) Corpus adjudication (S2 bless table — bless ONLY drift equal to these captures)

| cell | Portage | portuale | verdict |
|---|---|---|---|
| `s0-b-world` `--pretend --update @world` | `[N newpkg] [N innernestedsetpkg] [N withdeps]` rc 0 (NO `U upgradepkg`) | adds `[U upgradepkg-2.0 [1.0]]` second | DIVERGE → S2 |
| `s0-b-selected` `--pretend --update @selected` | same 3 N rows rc 0 | same +U shape as world | DIVERGE → S2 |
| `s0-b-system` `--pretend --update @system` | `[N newpkg] [N withdeps]` rc 0 | adds `[U upgradepkg]` | DIVERGE → S2 |
| `s0-b-samepkg-selected` `--pretend --update dev-libs/samepkg @selected` | same 3 N rows rc 0 (no `samepkg` row: already installed) | same +U shape | DIVERGE → S2 |
| `s0-b-isd-y` `… --implicit-system-deps=y @world` | same 3 N rows rc 0 (bias invisible: nothing merge-ordered beyond N rows) | biased +U `[N newpkg, U upgradepkg, N innernested, N withdeps]` | DIVERGE → S2 (pins collapse: all `@world` variants print identically) |
| `s0-b-isd-n` `… --implicit-system-deps=n @world` | same 3 N rows rc 0 | unbiased +U `[N newpkg, N innernested, U upgradepkg, N withdeps]` | DIVERGE → S2 |
| `s0-b-isd-bare` `… --implicit-system-deps @world` | `emerge: error: argument --implicit-system-deps: invalid choice: '@world' (choose from 'y', 'n')` rc 2 (bare form CONSUMES `@world` as its value) | rc 0 biased +U rows | CONTRADICTS plan reading — STOP-1 (below) / S2 must not bless; owner decision |
| `s0-b-flipinstconsumer` `--pretend --update dev-libs/flipinstconsumer` | `[R flipinstdep-1.0 [1.0] USE="wantflag*"] [N flipinstconsumer]` + `USE changes necessary: >=flipinstdep-1.0 wantflag` rc 1 | IDENTICAL rows + message rc 1 | AGREE → S2 control (preserve) |
| `s0-b-needer` | `[U paired-2.0 [1.0]] [N needer]` rc 0 (= old `362-needer-pu`) | identical | AGREE → S2 control |
| `s0-b-needskeywordmasked` | `[N needskeywordmasked]` rc 0 (= old cell) | `[UD keywordmaskedpkg-1.0 [2.0]] [N needskeywordmasked]` | DIVERGE → S2 (no downgrade without `--deep`) |
| `s0-b-toolchainroot` | `[N plainleaf] [N toolchainroot]` rc 0 (= old cell) | `[N fakeglibc-2.0] [U virtual/libc-2.0 [1.0]] [N plainleaf] [N toolchainroot]` | DIVERGE → S2 |
| `s0-b-unsatuseslot` | `[R unsatuseslotother-1.0 USE="unsatuseorflag*"] [N unsatuseslot]` + `USE changes necessary` rc 1 | IDENTICAL rc 1 | AGREE → S2 control |
| `s0-b-rebuild-unbuilt` `-u --rebuild-if-unbuilt rebuildtrigger` | `[U rebuildtrigger-2.0 [1.0]]` rc 0 (NO `R rebuildconsumer`) | adds `[R rebuildconsumer-1.0]` | DIVERGE → S2 re-grounds `test_rebuild_if_star…` |
| `s0-b-rebuild-newver` | same U-only rc 0 | same +R shape | DIVERGE → S2 |
| `s0-b-rebuild-newrev` `--rebuild-if-new-rev=y` | same U-only rc 0 | same +R shape | DIVERGE → S2 |
| (reused) `362-withdeps-pu` | `[N newpkg] [N withdeps]` rc 0 | (old capture: adds `U upgradepkg-2.0`) | DIVERGE → S2 |

## (c) #361 option matrix on `pg34-blib -pvuD` (S1)

| cell | Portage | portuale | verdict |
|---|---|---|---|
| `s0-c-usepkg` `--usepkg` (bare=True) | `Total: 0` rc 0 | `Total: 0` rc 0 | AGREE (key-presence rule; portuale already Total 0 via value `true`) → S1 control |
| `s0-c-k` `-k` | `Total: 0` rc 0 | `Total: 0` rc 0 | AGREE → S1 control |
| `s0-c-usepkgonly-n` `--usepkgonly=n` | `Total: 0` rc 0 — NO error, NO distinguishing output | `[U btool-2 [1]] Total: 1` rc 0 | DIVERGE but CONTRADICTS the pre-declared "no-ebuilds effects" shape — STOP-2 (below). #396 condition ("only if S0 (c) shows it") NOT met by this cell |
| `s0-c-getbinpkgonly-n` `--getbinpkgonly=n` | `[U btool-2 [1]] Total: 1` rc 0 (bdeps-auto stays ON; ebuilds kept) | identical | AGREE → S1 control |
| `s0-c-env-usepkgn` `EMERGE_DEFAULT_OPTS=--usepkg=n` + `-pvuD --getbinpkg=n` | `Total: 0` rc 0 (env opt sets the key: same parse) | `[U btool-2]` | DIVERGE → S1 (`usepkg_given` must cover the env parse) |
| `s0-c-features-getbinpkg-off` `FEATURES=getbinpkg` + `-pvuD --getbinpkg=n` | `[U btool-2 [1]]` rc 0 (cmdline `=n` overrides env FEATURES: no usepkg key) | identical | AGREE → S1 control (fold must respect the override) |
| `s0-c-features-getbinpkg` `FEATURES=getbinpkg` + `-pvuD` | `Total: 0` rc 0 (+ hermetic `Unsupported binary package format from ''` ×13 from the `file://` fixture binhost) | `[U btool-2]` | DIVERGE → S1 adds the fold; #393 NOT needed (Portage folds; portuale just lacks it) |
| (reused) `361-blib-pvuD-noflags` | `[U btool-2]` (bdeps-auto ON) | (old: SAME) | AGREE → S1 control |

## (d) Moves staging re-verification (D3)

| cell | Portage (fresh stage) | portuale | verdict |
|---|---|---|---|
| `s0-d-movedepconsumer-deep` `-pv --deep` (moves on) | rc 1 (backtrack 1/20) `no ebuilds to satisfy "dev-libs/oldmovepkg" for <root>` (+ `required by` chain, each line doubled) | rc 0 `[N newpkg] [N movedepconsumer]` | DIVERGE, matches prediction → #391 (to file; pending — no backlog edits in this slice) |
| `s0-d-movedepconsumer-deep-nomoves` `-pv --package-moves=n --deep` | rc 0 `[N movedepconsumer]` | rc 1 `no ebuilds "dev-libs/oldmovepkg"` | DIVERGE (inverted), matches §0.4.3 prediction → S2/S4 may move the corpus pin (recorded rc 1) ONLY here (rc 0 + N row). Mechanism: moves-off keeps vdb `oldmovepkg-1.0` installed; RDEPEND satisfied installed-only with no `--update` — a #360 instance, which is why portuale (narrow fallback) fails it |
| `s0-d-movedepconsumer-p` `--pretend` (moves on) | rc 1, same error text | rc 0 `[N movedepconsumer]` | DIVERGE → re-verifies old `360-movedepconsumer-p`: NOT a staging artefact (fresh-stage agrees with old capture) |
| `s0-d-movedepconsumer-p-nomoves` | rc 0 `[N movedepconsumer]` | rc 1 `no ebuilds` | DIVERGE (inverted, same mechanism) |
| `s0-d-slotmovepkg` `--pretend --update` (moves on) | EMPTY plan rc 0 (no rows) | EMPTY rc 0 | AGREE → D3 re-verify clean (old SAME stands) |
| `s0-d-slotmovepkg-nomoves` | EMPTY rc 0 | `[NS slotmovepkg-1.0 [1.0]]` rc 0 | DIVERGE → S2 material |

## (e) New fixtures (23 files, uncommitted in pmtest)

| cell | Portage | portuale | verdict |
|---|---|---|---|
| `s0-e-gbapp-pvuD` `-pvuD … pg359-gbapp` (BDEPEND on installed-only `pg34-gone`) | `[N pg34-gextra-1::testrepo  0 KiB]` (NOTE: no `to <root>`) `[N pg359-gbapp-1]` rc 0 | rc 1 `no ebuilds to satisfy "dev-libs/pg34-gone"` | DIVERGE → Q4 answered: Portage ACCEPTS the installed-only BDEPEND (walks vdb RDEPEND) → S4 lifts the `buildtime_hard` gate |
| `s0-e-gbapp-pvuD-bdy` `… --with-bdeps=y …` | identical rows rc 0 | rc 1 same error | DIVERGE → S4 (gate is in the BDEPEND walk with and without the explicit flag) |
| `s0-e-gsblk-pvu` `-pvu … pg359-gsblk` | `[NS gsblk-2:2 [1.0:1]] [uninstall gsblk-1.0:1] [blocks b gsblk:1 (soft blocking gsblk-2)] Total: 1 (1 new slot, 1 uninstall) Conflict: 1 block (all satisfied)` rc 0 — NO `U gsblk-1.1` row | byte-identical incl. order | AGREE → S3 control (both PMs already agree on the blocker shape; fixture guards it but does not discriminate pass 1 vs pass 2) |
| `s0-e-vapp-pv` `-pv … pg359-vapp` | `[N vy] [N vb] [N virtual/pg359-v] [N vapp]` rc 0 (`||` resolves to `vy`, pulled by `vb`) | byte-identical incl. order | AGREE → #397 NOT triggered (S0 shows NO `virtual/*`-deferral divergence); `vapp` becomes an S5 control pin |

## STOP sub-items (Portage contradicts the plan's reading — stopped, reported, not improvised)

- **STOP-1** (`s0-b-isd-bare`): bare `--implicit-system-deps` takes `@world` as its VALUE (`invalid choice: '@world' (choose from 'y', 'n')`, rc 2). The plan treated bare as a default-matching control; the corpus/CASES argv `["--pretend","--update","--implicit-system-deps","@world"]` errors in Portage while portuale prints rc-0 biased rows. S2 must not bless that argv's rows; the argparse shape is a separate decision for the owner.
- **STOP-2** (`s0-c-usepkgonly-n`): `--usepkgonly=n` on the installed top level prints `Total: 0` rc 0 — no error, no output distinct from bdeps-off. The plan's pre-declared "no-ebuilds effects" expectation does not materialise on this shape, so the #396 filing condition is not met by S0 (c); a discriminating probe (e.g. a NEW package under `--usepkgonly=n`) is S1/owner business, not run here.
