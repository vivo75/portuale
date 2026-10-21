# Report — slug f459 (backlog #459: binary-merge `pkg_postinst` failure parity)

Branch (both repos): `backlog/458-462-followups-459`. No commits made (not instructed);
no `PORTUALE_CORPUS_BLESS` used. Stray untracked `opencode.json` (MCP config
accidentally created in the repo root) deleted.

## Starting point

A previous worker left uncommitted edits in both worktrees plus
`LLM/evidence/2026-10-21-f459/` (oracle: `setup.sh`, `cells.sh`, `portage.txt`
from host `emerge` 3.0.82.2, `portuale.txt` from the implementation), then
stopped mid-`cargo test`. Preamble2 (`2026-10-21-444-445-455-456/preamble2.md`)
does not exist in this worktree (only `.superpowers/sdd/batch-2026-09-29/`
does) — worked from the backlog entry, the evidence, and the diff instead.

## Review of the inherited diff (kept / fixed)

**Kept — oracle-grounded.** The four oracle cells (lone / plus-good /
keep-going / foreground, all `--usepkgonly` on real-built binpkgs) overrule
the filed #459 claim exactly the way #444's oracle did: Portage does NOT
record `world` and does NOT print a `(postinst failed)` suffix for a binary
postinst failure — `>>> Completed` prints, then `>>> Failed to install <cpv>
to <root>, Log file:`, the die log replays with `FAILED postinst: 1`, one
end-of-run `Messages` block, builddir kept, dependents merge in a resumed
pass. The worker's endgame (`MergeOutcome::postinst_failed` through
`merge_binpkg`/`merge_binpkg_finish`, shared `record_postinst_failure`,
`postinst_failed_error` skipping the world record, `Ledger` postinst arm +
`Binary` coverage in `source_paths_for`/`failed_pkg_label`/`label_cpv_slot`,
background-only per-entry log capture) matches all four cells, and the pmtest
test passes against the current binary. Fixtures verified by hand: md5-cache
`_md5_` values equal `md5sum` of the ebuilds; `Packages` MD5/SHA1/SIZE equal
the actual tbz2 files; tbz2s are valid XPAK binpkgs whose `repository`
member is `testrepo`.

**Fixed — one real gap the worker left unpinned.** Both oracle captures show
real printing `ERROR: dev-util/f459-postinstfail-1::fetchrepo failed (postinst
phase)` while portuale printed `::` (empty repo). Root cause, confirmed by
inspection (not guessed): binary hooks run from the binpkg's saved
`environment.bz2`, which never contains `PORTAGE_REPO_NAME` (verified by
decompressing the member from both the fixture and the oracle binpkgs), and
nothing on portuale's binary path re-supplies it. Real always does:
`doebuild_environment` sets `configdict["pkg"]["PORTAGE_REPO_NAME"] =
repo.name` (`doebuild.py:483`) for the whole binary merge. Fix: `repo_name:
Option<&str>` on `RunPhaseFromSavedEnvArgs` and `RunInstprepArgs`
(pushed into `extra_env` next to `MERGE_TYPE`), threaded from the xpak
`repository`/`REPO` record through `HookCtx` (`None` for the `__unknown__`
fallback and for unmerge hooks — no oracle there, behavior unchanged);
source-merge `instprep` passes `None` (its `build_env` already carries the
var via `entry_identity_env`). New pin: `"::testrepo failed (postinst
phase)" in r.stderr` in the contract test. Verified live: the die line now
reads `::testrepo`.

**Deliberately not touched (standing gaps, out of scope).** `environment,
line 185` vs real's 186 (the source path is off by two, shipped in #444 —
pre-existing phase-runner gap); the digest `xpak … [ ok ]` line is unprinted;
`--resume` after a lone postinst failure is #460's item, untouched.

## What changed (this turn)

- `crates/portuale/src/ebuild_phases/single_phase.rs`: `repo_name` field +
  `extra_env` push in `run_phase_from_saved_env` and `run_instprep`.
- `crates/portuale/src/ebuild_merge/from_binpkg.rs`: `HookCtx.repo_name`,
  xpak-record sourcing, `instprep` call-site threading.
- `crates/portuale/src/ebuild_merge/after_install.rs`: `repo_name: None` at
  the source-merge `instprep` and unmerge call sites (+ one-line comments).
- `crates/portuale/src/ebuild_phases/tests/commands.rs`: `repo_name: None`
  in the existing `run_instprep` test call.
- `LLM/code-notes/.../from_binpkg.md`, `.../single_phase.md`: bullets for
  the new field (no new files → no index regen needed).
- `LLM/backlog-tasks.md`: #459 → DONE (SHAs pending commit); `LLM/what-this-proves.md`:
  paragraph appended. No `scope-backlog.md` hits for 459.
- pmtest `test_portuale.py`: `::testrepo` die-line pin.

## Verification (all foreground)

- `cargo fmt --check` — clean.
- `cargo clippy --release --all-targets` — zero warnings (one interim
  `unused_variables` from my own edit was caught and fixed: a `replaced_by_version`
  passthrough I had mistyped as `None`; restored).
- `cargo test --release --no-fail-fast` — 2908 passed, 0 failed.
- `python3 -m pytest ...test_emerge_failed_binpkg_postinst_matches_portage` — passes.
- `JOBS=4 scripts/pytest-parallel.sh -q` — exit 0; 4266 progress dots, 0 F,
  0 E, no corpus drift. Note: the shared basetemp
  (`/var/tmp/pytest-parallel`) accumulates root-owned dirs from
  `test_merge_skips_an_identical_0` (binpkg payload preserves root
  ownership), which makes the *next* run die in startup cleanup with
  INTERNALERROR; `sudo rm -rf` of the basetemp fixes it. Pre-existing
  environmental quirk, unrelated to this slice (that test is untouched).
- L0/L1 bed + glibc+bash merge gate: NOT run (coordinator does the L1 gate
  per the brief).

## Merge-path code touched (for the L1 gate)

`merge_binpkg` (return type + finish endgame — inherited), `merge_binpkg_finish`
(postinst endgame, elog skip, builddir keep — inherited),
`merge_one_binary_entry` (Completed/error routing, per-entry log — inherited),
`run_phase_from_saved_env` + `run_instprep` (hook `extra_env` gains
`PORTAGE_REPO_NAME` — this turn), `Ledger` failure tail (Binary arms —
inherited), `record_postinst_failure` signature (inherited). No `merge_tree` /
`movefile` / publish / vdb-commit changes: the atomic-write property from
#96/#97 is untouched.
