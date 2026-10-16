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

---

# 02.359 S2 (#362 `_want_update_pkg` + `_too_deep`) — slice log

Branch `backlog/359-default-solver-update` in both repos (pre-existing, not
switched). Nothing committed, pushed, added, stashed, reset or cleaned
(coordinator reviews and commits; `git add` never run). No bed scripts run
(plan S2 needs none beyond the workspace + contract gates). No `/dev/shm`
writes. Host files touched transiently for Portage captures only
(`/etc/make.local` emptied, `/etc/portage/make.profile` repointed; both
restored and checksummed after each batch — details below). Scratch
`/var/tmp/x359b` (+ `/var/tmp/x359b-delta`, `/var/tmp/x359b/st`) removed at
the end with a separate `sudo rm -rf` (plus `mkdir` for the next run);
per-run root-owned basetemp crud forced one mid-task clean before the final
full-suite passes.

## Product change (portuale repo, uncommitted)

`_want_update_pkg` + `_too_deep` for default-solver dependency selection,
ported from host Portage 3.0.82.2 (`/usr/lib/python3.14/site-packages/`).

- `crates/portage-repo/src/resolve/pretend.rs`
  - `SelectOptions` += `depth: u32`, `deep: Deep`, `complete_mode: bool`,
    `arg_atoms: &[String]`, `buildtime_hard: bool` (+ `Default`, all
    neutral). New helpers `update_too_deep` (D1 direct definition, NOT
    negated `recurses_at`), `update_recurse_satisfied`
    (`_wrapped_add_pkg_dep_string`'s flag), `installed_matches_argument`
    (+ `InstalledArgumentMatchArgs`), `installed_probe_satisfies_extra_constraints`,
    `tree_knows_use_dep_flags`, `avoid_update_outcome`.
  - Site-A (`:468`) rewrite: for a dependency atom with an installed
    match, `want = !complete && (arg || update) && !too_deep(arg ? 0 :
    depth)`; `!want` returns `avoid_update_outcome` (installed kept with
    no reinstall flip and no visibility filter; a pending reinstall is
    discarded exactly when Portage drops the edge, else kept);
    `want && !update` keeps the old flip (argument without `--update`);
    `want && update` falls through as before. `--exclude` keeps the
    pre-existing gate (no S0 cell distinguishes). New case-2: an installed
    version with no tree candidate is still kept when unwanted
    (`best_installed_matching` + extra-constraints probe), gated on
    `!buildtime_hard` (S4's `#360` gate untouched) and on the tree knowing
    every positively-required USE flag (else Portage `Missing IUSE`
    errors). Top-level atoms keep today's path.
- `resolve/pass/resolve_outcome.rs` populates the five fields per queued
  atom (`is_top_level = depth == 0` unchanged); `buildtime_hard` threaded
  through `ResolveQueueAtomArgs`, the minimize-group prediction (per
  sibling), the forced-upgrade flip, `ParentUseFlipArgs`, and the
  running-root build walk (`root_deps.rs`, marked build-time).
- `slot_operator/eliminate.rs` (`rebuild_if_entries`): Portage
  `_rebuild_config` only rebuilds parents already in the graph, so
  packages no entry reaches are never rebuilt (S0 `s0-b-rebuild-*`;
  fresh `s2-fresh-ub`). Reached parents keep the walked outcome (the
  restart-to-rebuild re-walk is a separate, unfiled gap).
- `tests_163.rs`: exhaustive literal gains the new fields (neutral).
- Unit tests: 8 new in `tests/pretend.rs` (too_deep table, arg match,
  plain/masked/USE/installed-only keeps, arg reset, deep=1, tree-USE
  gate); `tests/graph.rs`: `recursion_basic_chain_with_update…` re-grounded
  to `AlreadyInstalled`, new masked/virtual/arg-reset twins, two
  real-config uapp tests (drop without deep, keep with deep),
  `rebuild_if_unbuilt…` rewritten to absence.
- Code notes: `portage-repo/resolve/pretend.md` (S2 section),
  `…/pass/resolve_outcome.md` (plumbing), `…/slot_operator/eliminate.md`
  (gate); `python3 LLM/tools/code_notes_index.py` clean.

## pmtest change (pmtest repo, uncommitted; corpus blessed here)

- Rewritten to Portage rows: `test_update_threads…` (now keeps),
  `test_rebuild_if_star…` (trigger U-only; nochange-ub R-trigger-only),
  world/system expands+combines, onlydeps ×2, isd-n lists, libc asap,
  or_group stderr (opartlyb-only); relabeled `CASES :2569`; converted the
  `test_pg367_top_default_pvu…` strict xfail (`reason="#362"`, batch D7)
  to a normal pin (solver now prints the asserted rows).
- New default-solver pins: dtop `-pvu`/`-pvuN`, uapp `-pvN`/`-pvuN`,
  needskeywordmasked `--update`, toolchainroot `--update` (all with the
  Portage capture named in the docstring).
- `PORTUALE_CORPUS_BLESS=1` full run accepted 25 drift entries, each
  reviewed below (rows == capture rows modulo framing; `for <root>`
  suffix stays omitted per #395). Changed files:
  `test_emerge_pretend_contract.py`, `corpus/contract.json.xz`,
  `corpus/expanded.json.xz`.

### Bless table (every entry ↔ its capture)

- `s0-b-world/-selected/-system/-isd-n`, `362-withdeps-pu`,
  `362-needskeywordmasked-pu`, `362-dtop-pvu/-pvuN`,
  `362-uapp-pvN/-pvuN`, `s0-b-rebuild-unbuilt`,
  `s0-d-movedepconsumer-deep-nomoves`: S0
  (`LLM/evidence/2026-10-15-359-s0/out/`, old-plan cells via S0 matrix).
- `s2-fresh-ub` (`--rebuild-if-unbuilt rebuildnochange` → R trigger
  alone), `s2-c-onlydeps-pu`, `s2-c-samepkg-world-pu`,
  `s2-c-samepkg-system-pu`, `s2-c-toolchainroot-p/-pv/-tree`,
  `s2-c-opartly-p/-pv/-pu/-tree`, `s2-c-omasked-pu`,
  `s2-c-needsbuilt-pu`: fresh host Portage 3.0.82.2 captures taken for
  this slice (same options + staged fixture config as S0; stored here).
  `--json`/`--quiet` drifts are projections of the same rows (abort atom
  `opartlyb`; entries with `libc already_installed`); `--tree` drifts
  match the two `--tree` captures.
- `s2-probe-changeddeps.txt` (staged-only `cdparent` parent):
  `--changed-deps` without `--deep` prints the parent alone, with
  `--deep` the `R` + new dep — grounds the hide/drop boundary (never
  committed anywhere).
- Bare `--implicit-system-deps` keeps rc 0 while Portage errors (rc 2):
  pre-existing argparse divergence, S0 STOP-1, owner decision — rows
  follow `s0-b-isd-n`, rc untouched.

## Commands and results

- `cargo fmt --check` — clean. `cargo clippy --release --all-targets` —
  0 warnings (one `too_many_arguments` fixed via an args struct).
- `timeout 2400 cargo test --release -- --test-threads=2` (whole
  workspace) — all green (incl. portage-repo lib 1093 passed; the default
  thread count flakes on fixed-tempdir tests, hence the mandated flag).
- `python3 -m pytest pytests-contract-suite -q
  --basetemp=/var/tmp/x359b -p no:cacheprovider -rf` — final bless run:
  **3916 passed, 37 skipped, 3 xfailed** (baseline 3909/37/4; +6 new pins,
  +1 converted xfail; mid-task runs: 11 failed → all rewritten/converted,
  then 1 failed (args8, blessed) → 0 failed).
- `sudo rm -rf /var/tmp/x359b` — separate command, done (after the bless
  run; logs were copied out first).

## Surprises

- The rebuild `R` came from `rebuild_if_entries` (assemble synthesis),
  not the walk: the consumer is unreachable (trigger has no deps), and
  Portage's `_rebuild_config.add` only records edges for parents already
  in the graph (read at `depgraph.py:424-442/531-573`). Fixing it needed
  one fresh capture (`s2-fresh-ub`), which also corrected the
  rebuildnochange half of the pin.
- `--changed-deps` without `--deep` prints the parent alone in Portage
  too (staged `cdparent` probe): the installed `break` at `:8291-8307`
  is followed by an `_ignore_dependency` drop. The pre-existing 163
  unit test expecting the `R` without deep encoded non-Portage
  behaviour; the probe grounded the hide/drop split.
- `builtusedivergedep[divergedflag]` (vdb USE has the flag, no ebuild
  IUSE does) errors in Portage (`Missing IUSE`) while a same-shaped
  keep is correct for `keywordmaskedusepkg[flag]` (tree IUSE has it):
  told apart by `tree_knows_use_dep_flags` (fresh `s2-c-needsbuilt-pu`).
- Host captures need three host-quirk workarounds (all restored after):
  `/etc/make.local` emptied (else `EMERGE_DEFAULT_OPTS` with
  `--usepkg=n --getbinpkg=y` leaks in), `/etc/portage/make.profile`
  symlinked into the staged fixture default (else the running profile
  cannot parse with testrepo-only repos), and `PORTAGE_REPOSITORIES`
  set (else the `/usr/share` default gentoo repo at the existing
  `/var/db/repos/gentoo` leaks providers into virtual resolution, as
  seen first with `virtual/libc` → host glibc). `PORTDIR=` override is
  NOT a substitute (it unloads the main repo). Cells have no BDEPENDs,
  so the running profile does not shape rows. Two capture attempts
  with a broken stage (script wiped by a basetemp clean) produced
  config-error output and were deleted, re-staged and re-taken.
- G4 bonus inside S2's mechanism: `gapp -pv`/`-pvu` (rc 1 → 0, `N gapp`)
  and both moves-off `movedepconsumer` cells (rc 1 → 0) now match their
  captures via the installed-direct fallback. `-pvuD` shapes and
  `gone`-as-argument stay S4's.

## Left undone and why

- `s0-d-slotmovepkg-nomoves` (`--update --package-moves=n
  dev-libs/slotmovepkg`: Portage EMPTY, portuale `NS`): top-level
  same-version slot-mismatch; the plan keeps top-level atoms on today's
  path, so S2 does not touch it. No pin or corpus entry covers the argv
  (nothing fails); flagged for the owner/S3–S5.
- `--usepkgonly=n` residual: still diverges (`U` vs Portage `Total: 0`
  via the ebuild-db drop at `depgraph.py:809`); S0 STOP-2 left #396
  unfiled and S2 keeps the `excluded.is_empty()` gate — out of scope.
- In-graph installed parents never rebuild via `--rebuild-if-*`
  (portuale lacks Portage's restart-to-rebuild re-walk); no backlog
  number filed (S2 authorizes none) — owner/Z to decide.
- `LLM/what-this-proves.md`: untouched (plan assigns it to Z).
- Full-suite green depends on the bless in this slice's pmtest commit;
  the paired portuale commit carries the code + unit tests + notes.

---

# 02.359 S3 (#359 greedy slots, both Portage passes) — slice log

Branch `backlog/359-default-solver-update` in both repos (pre-existing, not
switched). Nothing committed, pushed, added, stashed, reset or cleaned
(coordinator reviews and commits; `git add` never run). No `/dev/shm`
writes. Host files touched transiently for two Portage captures only
(`/etc/make.local` emptied, then restored byte-exact: md5
`3f29e0a60206a91b570347f8d737fbd2`, `root:portage 0666`, stat recorded
before; `/etc/portage/make.profile` never touched). Scratch `/var/tmp/x359c`
(+ `/var/tmp/x359c/st`, two detached build worktrees) removed at the end
with a separate `sudo rm -rf`. Never rebuilt during the bed run.

## Product change (portuale repo, uncommitted)

`_greedy_slots` for the default solver, both passes, ported from host
Portage 3.0.82.2 (`_emerge/depgraph.py`, `select_files` + `_greedy_slots`;
`Package._dep_keys` for the lookahead):

- `crates/portage-repo/src/resolve/greedy_slots.rs` (new) —
  `greedy_slots_below_best` (the slot filter shared with the pubgrub
  bridge instead of copying it, via a `GreedySlotsBelowBestArgs` struct
  after clippy's `too_many_arguments`) and `greedy_slot_atoms` (per-pass
  expansion: non-set `AtomArg`s only — set members and `@`-sets skipped,
  `/` gate, blockers never targets; walk-identical pool of ebuilds unless
  `--usepkgonly` plus usable binaries, `is_visible` +
  `union_constraints`-filtered so a backtrack mask moves the expansion;
  installed slots from the vdb match of the ORIGINAL atom with the pinned/
  ranged atoms falling out naturally; per-slot best for the bare `cp:slot`
  atom; blocker-lookahead revision in both directions plus lower-version
  pairwise discard, with USE-dep blockers honoured against the other's
  effective USE). Installed-slot order is sorted low-first (Portage pops a
  hash set; the suite needs determinism).
- `resolve/pass/mod.rs` + `pass/seed_queue.rs` — `run_pass` recomputes the
  expansion every attempt (plan risk "re-run per attempt") and seeds it as
  depth-0 owner-less roots after the user args. `(argument)` chains and
  world writes read the request atoms, so the original text is carried and
  no `cat/pkg:1` leaks into world (existing dualslotpkg world test green).
- `solver_bridge.rs` — `target_slots` refactored onto the shared core
  (behaviour identical; `s6_target_slots_*` pins green, no lookahead added
  there — still #368 territory).
- `resolve/pretend.rs` — one-word change: `satisfies_extra_constraints`
  becomes `pub(crate)` (body untouched).
- Unit tests: `tests/greedy_slots.rs` (new, 8 tests: pure filter incl.
  higher-slot exclusion and update no-op; `gs -pvu` → `NS 2 + U 1.0→1.1`,
  `-pv` → `NS` alone, `:2`/`=gs-2` no-ops, range-atom `>=gs-1.0` → `NS+U`,
  gsblk lookahead drop, set-member skip plus a non-vacuous expansion
  assertion). Oracles cited per test.
- Code notes: `LLM/code-notes/portage-repo/resolve/greedy_slots.md` (new;
  incl. the `!`-strip bug that bit once and the gsblk discrimination
  verdict); `python3 LLM/tools/code_notes_index.py` clean.

## pmtest change (pmtest repo, uncommitted; corpus blessed here)

- Converted the strict xfail
  `test_pg34_gs_default_pvu_upgrades_installed_slot` to a normal pin
  (oracle `2026-10-15-plan-359/portage/359-gs-pvu.txt`).
- New default-solver pins: `gs -pvuD`, `gs -pvuDN` (oracles `359-gs-pvuD`,
  `359-gs-pvuDN`), `>=gs-1.0 -pvu` (oracle S0 `s0-a-gs-range.txt`),
  `pg359-gsblk -pvu` → `NS` alone (oracle S0 `s0-e-gsblk-pvu.txt`). Every
  docstring names its Portage 3.0.82.2 capture, never portuale's output.
  No new fixtures: S3 uses S0's `pg359-*` tree as the plan provides.
- `PORTUALE_CORPUS_BLESS=1` accepted exactly 2 drift entries, both
  reviewed below (rows == capture rows; rc unchanged). Changed files:
  `test_emerge_pretend_contract.py`, `corpus/expanded.json.xz`.

### Bless table (every entry ↔ its capture)

- `--pretend --update --deep dev-libs/orupgradealt` → `N marker1 + N
  marker2`, rc 0; `--pretend --update --deep --newuse ...` → same. Both
  equal fresh host Portage 3.0.82.2 captures taken for this slice (same
  options + staged fixture config as S0, fresh stage per cell per D3,
  stored here as `s3-orupgradealt-puD.txt` / `s3-orupgradealt-puDN.txt`).
  Mechanism: both orupgradealt slots are installed; greedy appends
  `orupgradealt:1`, and the `--deep` walk reaches marker1 through the
  kept 1.0 instance. The harvested `marker2`-only rows encoded
  non-Portage behaviour.

## Commands and results

- `cargo fmt --check` — clean. `cargo clippy --release --all-targets` —
  0 warnings (one `too_many_arguments` fixed via an args struct).
- `timeout 2400 cargo test --release -- --test-threads=2` (whole
  workspace) — 2695 passed, 0 failed (incl. 8 new greedy tests; the
  default thread count flakes on fixed-tempdir tests, hence the flag).
- `python3 -m pytest pytests-contract-suite -q
  --basetemp=/var/tmp/x359c -p no:cacheprovider -rf` — first run: **3921
  passed, 37 skipped, 2 xfailed** (baseline 3916/37/3; +4 new pins, +1
  converted xfail) with 2 expanded-corpus drifts (above). Bless run
  (`PORTUALE_CORPUS_BLESS=1`, fresh basetemp after a root-crud clean):
  same 3921/37/2, drift accepted into `expanded.json.xz` (verified the
  blessed blobs equal the Portage captures). Post-bless
  `test_harvested_corpus.py` alone: 1 passed, no drift. (One bless attempt
  failed with 3956 teardown errors from root-owned basetemp crud left by
  the prior run — same shape as S2's note; corpus untouched, cleaned with
  a separate `sudo rm -rf /var/tmp/x359c` and re-run.)
- `differential-test-bed/run/l0-fixture-oracle.sh` (plan §2.G) —
  explained 19, **UNEXPLAINED 4** (all `skipped-updates` USE-paint rows on
  `--backtrack=0 blk0{a,b,c}` orders: Portage `ABI_X86="(64)"` vs portuale
  `ELIBC="glibc"`). Attribution, all local reproductions, no --update
  involved: the S2 binary (`bc1a7c41`, detached worktree, since removed)
  and the pre-S1 binary (`3afc6bf0`, detached worktree, since removed)
  print byte-identical `ELIBC` paint — the 4 rows pre-date S1/S2/S3 (they
  sit in earlier stacked work or main; Oct 12–13 bed runs show
  UNEXPLAINED 0). S3 adds no new bed row: with `--update` absent the new
  code provably no-ops (early return before any pool/vdb read). Left for
  the owner — no allowlist touched (STOP rule), no backlog number filed
  (S3 authorizes none).
- `sudo rm -rf /var/tmp/x359c` — separate command, done at the very end
  (after the evidence captures were copied out).

## Surprises

- The lookahead bug that bit once: `match_from_list` strips the leading
  `!` before matching, so parsing the stripped form and testing
  `blocker != None` is always false — blocker-ness must come from the raw
  token. Pre-fix, gsblk planned `U 1.1` with an unsatisfied block
  (rc 1); post-fix the output is byte-identical to Portage.
- S0's "`pg359-gsblk` does not discriminate lookahead passes" is wrong as
  written: pass 1 alone diverges from Portage (`U` + `blocks B`
  unsatisfied, rc 1), both passes match. Residue #394 is therefore not
  filed (its condition "Portage shows no lookahead effect" is not met).
- One unreproduced single-run anomaly: an early `oranyslotalt --update`
  probe printed empty/rc 1 once, then `NS`/rc 0 on ~10 repeats (6/6 with
  exact rc capture) and in the suite. Rows agree with the Portage capture;
  noted here in case it recurs.
- `slotmovepkg --update --package-moves=n` still prints portuale `NS`
  vs Portage EMPTY (S2's flagged leftover; no pin covers the argv) —
  unchanged by S3, left for the owner/S4–S5.
- The ten `359-slots-*` controls re-checked row-for-row against the
  pre-D3 container captures (slotmovepkg against S0's re-verification):
  all agree except the known #392 `USE="-unsatuseorflag"` display on
  `unsatuseslotalt` (rows-only comparison per the plan).

## Left undone and why

- The 4 bed UNEXPLAINED rows (above): pre-existing, owner to adjudicate;
  bless/allowlist forbidden to this slice.
- `LLM/what-this-proves.md`: untouched (plan assigns it to Z).
- Full-suite green depends on the bless in this slice's pmtest commit
  (`expanded.json.xz`); the paired portuale commit carries the code +
  unit tests + notes. The two host Portage captures
  (`s3-orupgradealt-pu*.txt`) are evidence here, not fixtures.
