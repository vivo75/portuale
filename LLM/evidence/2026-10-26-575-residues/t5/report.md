# Task 5 report: stop tests from leaking temp directories (#588)

Date: 2026-10-27. Branch `fix/2026-10-26-575-residues` (both repos, not
committed — manager commits). Reference Portage 3.0.83.

## S0 — `t5/sites.md`

`grep -rn '\.keep()' crates --include=*.rs`, excluding
`portage-util/src/temp_dir.rs` (its own keep self-test): **304 calls**.
Each call is labeled in `t5/sites.md` (file:line, label, code):

- **298 test**: 256 under `tests/` directories, 42 inside `#[cfg(test)]`
  modules in product files (verified with a brace-tracking classifier;
  spot-checked `mrg.rs`, `solver_bridge.rs`, `categories.rs`, `elog.rs`,
  `portageq.rs`, `needed_elf.rs`, `regen.rs`,
  `emerge_getbinpkg.rs::mixed_pretend_tests` by hand).
- **6 product-region**, all legitimate, no leak:
  - `pretend_pass.rs:159,200` — `tempfile::NamedTempFile::keep`, **not**
    `TempDir` (staging capture removed after read in `run_staged_pretend`;
    composed log removed by `replay_pretend_log`).
  - `remote/binpkg_flow.rs:136` (`load_vdb_shadow`) — deliberate, call-site
    comment says the pulled tree is left for forensics.
  - `remote/binpkg_flow.rs:211` (`run_binpkg_flow`) — `remove_dir_all` on
    every exit path.
  - `remote/config_place.rs:203` (`place_config_root`) — owned by
    `PlacedConfig.tmp`, removed on `Drop` (guard-equivalent).
  - `remote/stages.rs:327` (`install_file_atomic`, ssh path) — removed
    after send.

No product leak found, so no separate product commit was needed.

## RED — the scan (was failing, now green)

`crates/portage-util/tests/temp_dir_keep_scan.rs`,
`only_allowlisted_calls_may_keep_a_scratch_guard`: scans every `.rs`
file under `crates/` and fails on any `.keep()` outside 7 allowlisted
(file, enclosing-function) pairs (the guard's own self-test, the two
`tempfile` staging keeps, the four remote-path keeps). The needle is
built by string concatenation so the scan's own source never matches.
It failed with 298 offenders before the fix and passes now. (A source
scan was chosen over an `LLM/tools/` script so it runs in the cargo
gate. No Portage citation applies — this is repo hygiene, not ported
behaviour.)

## Fix — mechanical sweep, crate by crate

Rule applied: the helper returns the `TempDir` guard (or a tuple with
the derived path); each test binds it and uses `dir.path()` (deref
covers `.join()`/`&dir`/`display()`). 101 files, +676/−594. Per-crate
isolated leak checks (`TMPDIR=$T cargo test --release -p <crate>`;
`find $T -maxdepth 1 -name 'portuale-td-*'`) all came back empty:

- mrg-director, portage-fetch, portage-util, portage-profile (135 tests),
  portage-repo (1239 tests): bulk `s/\.keep();//` + helper return-type
  changes (`tempdir`, `synthetic_root`, `populated_root`,
  `user_profile_test_root`, `mo_tmp_vdb`, `s6_temp_vdb`,
  `s6_temp_repo`, `tmp_vdb`, `abi_probe_harness`, `dir_162/163/165/166/184`,
  `clean_root_162`, `mini_571`, `rebuild_if_394_vdb`, `install_dev_libs_root`,
  `mbabilib_vdb`, `p491_vdb`, `unsat_base`); `dir.as_path()` →
  `dir.path()`; 13× `req.root = root.path().to_path_buf()`.
- portuale: same, larger. Fixes beyond the pure-mechanical that the
  compiler or the leak check forced (all behaviour-preserving, tests
  prove it):
  - Helpers that returned a path into a dropped guard changed to
    return `(TempDir, PathBuf)`: `build_gpkg`, `build_gpkg_entries`,
    `repack_gpkg_with_mutation`, `build_gpkg_with_tar_bytes`,
    `build_gpkg_with_inner_tars`, `build_gpkg_with_inner_entries`,
    `updates_test_repo`, `test_gpg_home` (×2), `envs`/`db_envs`
    (vdb_view), `image` (line_endings). Callers destructure
    `let (_guard, g) = …` (precedent: `p491or_vdb`).
  - 4 dangling `&tempdir()` temporaries bound to named guards
    (`source_merge.rs`, `buildpkgonly.rs` ×3).
  - `portageq` sqlite seed: `OnceLock<&'static Path>` (leaked by design)
    replaced by a per-`q()` `SqliteSeed` guard (23 ms per build, ~1 s
    total); `Backend::Sqlite` now carries `&Path`.
  - One behaviour fix: the second `fetch_src_uri` call in the
    ro-distdirs test got its own fresh empty `distdir` (reusing the
    first dir would have made the closed-port fetch succeed).
  - `S4Root.tmp` / `S8Tree.tmp` fields became `TempDir`.
  - One stale doc comment (`tests_p491.rs`, mentioned `.keep()`) reworded
    so the scan stays exact.

## Verification

- `cargo fmt --check`: pass. `cargo clippy --release --all-targets`:
  0 warnings.
- `cargo test --release` (whole workspace): **3158 passed, 0 failed**
  (baseline 3143 + sibling-task tests; compare by name — no failures).
- Scan test passes; `grep` confirms the only remaining `.keep()` calls
  are the 7 allowlisted ones.
- Isolated-TMPDIR leak checks per crate: all empty (see above).

## Deviations and open points

- The plan's leak-check command uses `mktemp -d /var/tmp/leak-XXXX`,
  but `vdb_ipc` fails under any TMPDIR longer than 13 chars: the test
  nests `portuale-vdb-ipc-<pid>-<nanos>` inside a `portuale-td-…` dir and
  the socket path exceeds 107 bytes (`vdb_ipc.rs:90-100`). Both files are
  byte-identical to HEAD (confirmed: my diff touches neither), so this is
  **pre-existing** and unrelated to the sweep. Checks ran under
  `/tmp/l-XXXX` instead. OWNER QUESTION: fix the test (shorter tags or
  `new_in`-style single dir) in a follow-up, or leave as-is?
- One `vdb_ipc` failure in a full-suite run passed on retry and in
  isolation (timing flake, same pre-existing area). Worth a glance if it
  recurs.
- `build_signed_gpkg` still leaks via `std::mem::forget(ScratchDir)`
  (`binpkg/tests/mod.rs:347-349`). Out of this task's scope (not a
  `.keep()` call; invisible to the scan and to the `portuale-td-*`
  check since the prefix is `portuale-gpg-*`). Suggested residue: port
  it to a guard return like the other builders.
- `load_vdb_shadow`'s forensics keep (see S0) is deliberate and
  documented; leaving it was a judgment call — confirm.
- `LLM/code-notes/portage-util/temp_dir.md` updated with the new rule;
  `code_notes_index.py` run (no index change — existing file).
- Temp files: `/var/tmp/t5-keep.txt` (S0 grep dump) and
  `/var/tmp/t5-tmpdir.txt` removed/leftover-roots removed; verify
  `/tmp/l-*` are gone (each run removed its own).
