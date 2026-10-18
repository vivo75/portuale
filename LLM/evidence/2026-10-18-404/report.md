# #404 report — S11: auto-clean (351-CLEAN, 2026-10-18)

Branch `backlog/404-autoclean` in both repos, uncommitted, unpushed
(per the brief: no commit, no push). The #404 backlog entry is NOT
flipped (coordinator's).

## What was built

After a successful non-`--pretend`/non-`--buildpkgonly` merge with a
user-set `AUTOCLEAN=yes`, portuale prints `>>> Auto-cleaning
packages...` and runs the `--clean` selection system-wide, which
either reports no outdated packages or unmerges them, with no
`CLEAN_DELAY` countdown. make.globals default `no`; unset/`=no`,
`--pretend`, and failed merges print nothing.

Portage source lines (3.0.82.2, "Portage"), each verified against the
host install (and the vendored checkout where #403 checked it):

- Gate + trigger: `_emerge/actions.py:694-704` (`retval ==
  os.EX_OK and not (buildpkgonly or fetchonly or pretend)` →
  `settings.get("AUTOCLEAN") == "yes"` → `writemsg_stdout(">>>
  Auto-cleaning packages...\n")` → `unmerge(..., "clean", [],
  autoclean=1)`). Default `AUTOCLEAN="no"`: `make.globals:103`.
- No countdown: `_emerge/unmerge.py:638` (`if not unmerge_action ==
  "rage-clean" and clean_delay and not autoclean`).
- Headers: `_unmerge_display` prints `>>> Using system located in ROOT
  tree <root>/` only for non-`/` ROOT, then the per-cp `selected:` /
  `protected:` / `omitted:` blocks, `All selected packages: …`, the
  two `>>> … slated …` lines, `>>> Unmerging (N of M) …`, or (empty,
  system-wide) `\n>>> No outdated packages were found on your
  system.\n` (`unmerge.py:316-319`).
- Outdated = same-CP+slot, counter-ordered protection:
  `unmerge.py:274-314` (per-slot `counterkeys.sort()`, last-merged
  protected, rest selected). `--clean` is not `--depclean`.
- Out of scope, deliberately not modelled: forced-yes after applied
  global updates (`actions.py:3671-3691`, bug #792195) and after
  depclean (`:809-813`); every cell runs with `--package-moves=n`.

## Oracle (new evidence `LLM/evidence/2026-10-18-404/`)

Recipe `scripts/l.sh` (+ `lib.sh`, S0/S403-derived; no `CLEAN_DELAY`
anywhere by design, so the 5-second default applies and any countdown
line is a finding), 8 cells × both PMs, all rc-parity:
cl01 outdated-consumed-by-replace, cl01c reconstructed leftover
unmerged, cl02 fresh, cl03 AUTOCLEAN unset, cl04 `=no`, cl05
different-slot leftover protected, cl06 `--pretend`, cl07 failed
merge. `matrix.md` (verdicts), `PROVENANCE.md`. Post-fix binary:
every auto-clean block is byte-identical to Portage modulo the one
known blank-line delta below.

## Findings the coordinator should know

1. **No STOP condition fired.** No existing pin moved (full suite
   green, corpus clean), every new output is explained from the
   Portage lines above, the shared unmerge code changes behavior only
   when the new `autoclean` flags are set (manual paths pass `false`).
2. **consolidated-`metadata` stamp caveat (oracle-side).** Portage
   serves `<entry>/metadata` while its stamp is valid, so a
   same-second `SLOT` edit alone is invisible (first cl05 run kept
   selecting 7). The cell deletes the stale `metadata` file after the
   edit (documented fallback, `_explode_metadata_file`), and then
   Portage protects both slots (`No outdated…`). Same trap applies to
   any future test that hand-edits a Portage-merged vdb entry.
3. **Blank-line delta (left as is).** Portage ends the
   slated/protected pair with `\n\n` (blank before the first `>>>
   Unmerging`); portuale ends it with one `\n` on every removal path
   already (manual `--clean`/`-C` included). Fixing it would move the
   verbatim `--pretend` pins, so the auto-clean block carries the same
   delta; pins assert per-line presence. Pre-existing, not
   auto-clean-specific.
4. **S0 finding l04 is already fixed.** With `CLEAN_DELAY=0` in
   `make.conf` only (no env), a manual `emerge -C` prints no countdown
   line and finishes in 0.08 s — the S4 resolved-config plumbing
   (`clean_delay_countdown(Some(config))`) honors it. Live-verified on
   the final binary; no fix needed, per the brief's one-liner rule.
5. **Info-line ordering.** The auto-clean runs before the merge's own
   post-merge notices, so ` * GNU info directory index is
   up-to-date.` prints once, after the unmerge — matching the oracle
   tail. The removal-side notices are skipped for the auto-clean only
   (elog batch, world cleanup and the removal loop are unchanged).
6. **Narrowings, both Portage-faithful.** `clean_selection` protects
   by version order where Portage protects by COUNTER order (highest
   counter = most recently installed); identical whenever install
   order follows version order (all oracle cells). The auto-clean
   return value is discarded (a nothing-outdated clean exits 1 while
   the merge already succeeded). An empty-plan merge still triggers
   the run (Portage: `mergecount == 0` → `retval == EX_OK` → clean
   runs); a `--resume`-driven merge does not go through this hook
   (separate path, out of scope).

## Files changed (portuale)

- `crates/portuale/src/pretend/env_config.rs`: `autoclean_enabled`
  (resolved `AUTOCLEAN == "yes"`, calling env over config).
- `crates/portuale/src/pretend/removal/prune_clean.rs`:
  `RunPruneNodepsOrCleanArgs.autoclean` (skips the countdown; manual
  call sites pass `false`).
- `crates/portuale/src/pretend/removal/unmerge.rs`:
  `ExecuteUnmergeArgs.autoclean` (skips the info regen,
  preserved-libs and news notices the merge step runs right after;
  manual call sites + existing tests pass `false`).
- `crates/portuale/src/pretend/post_merge.rs`:
  `RunAutocleanAfterMergeArgs` + `run_autoclean_after_merge`
  (headers + shared clean run, exit code discarded).
- `crates/portuale/src/pretend/mod.rs`: the hook in
  `run_after_display` (after `record_world_after_merge`, before
  `show_post_merge_notices`; skipped for `--buildpkgonly`).
- Tests: `crates/portuale/src/pretend/tests/autoclean.rs` (new: gate
  truth table, unmerge arm, no-outdated arm) + `tests/mod.rs`
  registration.
- Docs: `LLM/what-this-proves.md` (appended paragraph),
  `LLM/code-notes/portuale/pretend/{post_merge,removal/prune_clean,removal/unmerge}.md`
  (appends), index regenerated. Backlog entry untouched.

## Files changed (pmtest)

- `pytests-contract-suite/test_portuale.py`: five `test_autoclean_*`
  pins (leftover unmerge, fresh no-outdated, unset/`=no` silence,
  pretend + failed-merge silence, different-slot protection) reusing
  `_s5_configroot` with `AUTOCLEAN="yes"` in `make.conf`,
  `CLEAN_DELAY` scrubbed from the child env, `NOCOLOR=yes`; per-line
  assertions against the S404 Portage captures named in each
  docstring. No new fixtures (repo-348 packages + one inline scratch
  repo); pg359* files untouched.
- `pytests-contract-suite/test_emerge_pretend_contract.py`: one CASES
  row at the END of CASES (`--clean --pretend` rc 0, refactor guard
  for the shared code; inert for invariants/corpus — non-resolver
  commands are skipped there).

## Gates (in order, sequential, fresh `cargo build --release` first)

- `cargo fmt --check`: clean.
- `cargo clippy --release --all-targets`: zero warnings.
- `cargo test --release` (whole workspace, TMPDIR=/tmp/t): **2810
  passed, 0 failed** (2807 pre-existing + 3 new).
- pmtest `python3 -m pytest pytests-contract-suite -q
  --basetemp=/var/tmp/pt404 -p no:cacheprovider`: **4053 passed,
  37 skipped, 2 xfailed** (4047 pre-existing incl. the #403 pins + 5
  new pins + 1 new CASES row; exact accounting, no other delta).
  Basetemp removed afterwards (`sudo rm -rf`); `/var/tmp` and `/tmp`
  scratch cleaned (only other agents' leftovers remain).
- Corpus drift: none (`test_harvested_corpus` green, no bless run).
- NOT run per the brief: container bed, glibc/bash merge gate.
