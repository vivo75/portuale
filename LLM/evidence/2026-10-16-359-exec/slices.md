# 02.359 S1 (#361 `--usepkg` option presence) — slice log

Branch `backlog/359-default-solver-update` in both repos (pre-existing, not
switched). Nothing committed, pushed, added, stashed or reset (coordinator
reviews and commits). No bed scripts run (plan S1 needs none). No
`/dev/shm` writes. No `/var/tmp` scratch left (`/var/tmp/x359a` used for
`--basetemp`, removed afterwards with a separate `sudo rm -rf`).

## S0 backlog item handled here

The plan files #391/#392 at S0 ("filed at S0, not Z"), and S0 deferred them
(backlog edits were out of its brief). Neither number existed in any tier
(checked `LLM/backlog-tasks-2026-10.md` + `LLM/backlog-tasks.md`; the
390–399 block was free — the only `390` hit was a "3906 passed" count).
Registered both in `LLM/backlog-tasks-2026-10.md` Tier 2, after #362, in the
file's entry style with Portage citations and S0 evidence paths:
- #391 `movedepconsumer` moves-on (Portage rc 1, portuale rc 0; fresh-stage
  re-verified, not an artefact; corpus pin may move only to the moves-off
  capture in S2/S4).
- #392 `359-slots-unsatuseslotalt` `USE="-unsatuseorflag"` display gap.

## S1 product change (portuale repo, uncommitted)

`bdeps=auto` is now a `--usepkg` key-presence test on the post-implication
options, matching Portage (`create_depgraph_params.py:97-103`,
`_emerge/main.py:1109-1121`, `_emerge/actions.py:3709-3723`, all re-read on
the host's Portage 3.0.82.2 for this slice):

- `crates/portuale/src/pretend/args.rs` — new `EmergeOpts::usepkg_given`
  (set at every `--usepkg`/`-k` spelling incl. the `-k` bundle member) and
  `getbinpkg_given` (every `--getbinpkg`/`-g` spelling; needed to tell an
  explicit `--getbinpkg=n` from the default, since only the former vetoes
  the `FEATURES=getbinpkg` fold). `EMERGE_DEFAULT_OPTS` needed no separate
  handling: verified (`env_config.rs:153-165`, `mod.rs:195`) it prepends to
  argv and parses through the same function, so env-givenness is covered.
- `crates/portuale/src/pretend/resolve_plan.rs` — new pure helper
  `plan_with_bdeps(opts, features)`: explicit `--with-bdeps` wins,
  otherwise `with_bdeps_auto && !(usepkg_given || usepkgonly ||
  getbinpkg_eff || getbinpkgonly)` where `getbinpkg_eff` carries the
  `FEATURES=getbinpkg` fold unless cmdline said `=n`. The fold is local to
  the rule (deliberate: a global fold would also flip remote binary
  loading; noted in code notes). `plan_resolve` calls it before the
  options destructure and keeps the existing `opts.with_bdeps` writeback.
- `crates/portuale/src/pretend/tests/bdeps_presence.rs` (new, 14 tests)
  + `tests/mod.rs` registration — parse-level given-ness for every
  spelling, env-prepend path, and the full S0 (c) matrix through
  `plan_with_bdeps`, each docstring citing the S0 cell and the Portage
  source. The `--usepkgonly=n` test pins bdeps-auto ON with the reason
  spelled out (rule tests only the `--usepkg` key; Portage's `Total: 0`
  there comes from the ebuild-db drop at `depgraph.py:809`, unmodelled).
- Code notes: revised the stale `--with-bdeps` bullet in
  `LLM/code-notes/portuale/pretend/resolve_plan.md` (it stated the old
  value-based reading) and added the given-ness bullet in
  `.../pretend/args.md`; ran `python3 LLM/tools/code_notes_index.py`.
  Left for Z per the plan: the pubgrub-bridge `solver_bridge.rs:4663`
  "stays cut" comment + its code note (#390 territory, behavior untouched).

## S1 pmtest change (pmtest repo, uncommitted)

- `CASES`: relabeled `:451` (old label "plans nothing" is false for its
  flag-less argv — Portage plans `U pg34-btool-2`) and added one entry for
  `-pvuD --getbinpkg=n --usepkg=n pg34-blib` (rc 0).
- Two new default-solver pins: `..._binflags_plans_nothing` (`[]`, oracle
  `../2026-10-15-plan-359/portage/361-blib-pvuD.txt`) and
  `..._pvuD_upgrades_btool` (`[("U", "dev-libs/pg34-btool-2")]`, oracle
  `.../361-blib-pvuD-noflags.txt`); every docstring says the rows are
  Portage 3.0.82.2's, never portuale's.
- Fixed the two pubgrub blib docstrings (the "installed parent not merged"
  rationale is false; the `[]` holds because of the `--usepkg=n` key,
  #361 S1; the no-flags pubgrub gap is now named as residue #390).
- `pg34-blib-1.ebuild` DESCRIPTION corrected (old text claimed Portage
  `-pvuD` "plans nothing") + md5-cache entry regenerated (`md5sum` of the
  edited ebuild in `_md5_`); verified the metadata still reads (rows
  unchanged) and no test/doc references the old string.

## Commands and results

- `cargo fmt --check` — clean (after `cargo fmt` once for the new file).
- `cargo clippy --release --all-targets` — 0 warnings (3
  `field_reassign_with_default` in the new tests fixed with struct-update
  syntax).
- `cargo test --release -- --test-threads=2` (whole workspace,
  `timeout 2400`, foreground) — all suites ok, 0 failures; portuale bin
  suite 1070 passed (1056 pre-existing + 14 new), no hangs.
- Live binary vs S0 Portage matrix (fixture env, `target/release`):
  7 of 8 group-C cells now row-identical (`--usepkg`, `-k`,
  `--getbinpkg=n --usepkg=n`, `EMERGE_DEFAULT_OPTS=--usepkg=n`,
  `FEATURES=getbinpkg` ± `--getbinpkg=n` override,
  `--getbinpkgonly=n`, no-flags). Before the fix the three key cells
  (`--usepkg=n` shapes, env, `FEATURES`) wrongly printed `U pg34-btool-2`.
- `python3 -m pytest pytests-contract-suite -q -k blib
  --basetemp=/var/tmp/x359a` (pmtest) — 7 passed, 3943 deselected
  (2 new pins + new/relabeled CASES + both pubgrub blib pins + the
  `--with-bdeps=n` CASES). Full pmtest suite NOT run (coordinator's);
  corpus not blessed (no drift seen in the selection).
- `sudo rm -rf /var/tmp/x359a` — separate command, done.

## Surprises

- Almost none: every S0 (c) cell behaved exactly as the plan + source
  reading predicted, including the `--usepkgonly=n` residual (Portage
  `Total: 0` via the ebuild-db drop, portuale `U btool-2` with bdeps on).
  The `FEATURES` fold and the `=n` veto both worked first try.
- One self-caught test bug: the env test first asserted bdeps stays ON
  under `EMERGE_DEFAULT_OPTS=--usepkg=n` (inverted — Portage is `Total: 0`
  there); fixed to `!plan_with_bdeps`.

## Left undone and why

- `--usepkgonly=n` still diverges (`U` vs Portage `Total: 0`). This is the
  ebuild-db-drop quirk (`depgraph.py:809`), not the bdeps rule — the plan
  scopes it out of S1 ("does not measure the bdeps rule", D4) and S0 left
  its #396 filing condition unmet (cell does not discriminate bdeps-off
  from ebuild-drop on an installed top level). No new number filed (brief
  authorizes only #391/#392). Discriminating probe for the owner: a NEW
  (not installed) package under `--usepkgonly=n` — ebuild-drop predicts
  rc 1 "no ebuilds", bdeps-off predicts a normal merge.
- #393 correctly not filed (fold was trivial and is in).
- #390 (pubgrub no-flags BDEPEND fold) untouched — separate residue, Z/S5
  business; only named in the fixed docstrings.
- `LLM/what-this-proves.md` untouched (plan assigns it to Z).
- Full pmtest suite + corpus bless: coordinator's gate, per the brief.
