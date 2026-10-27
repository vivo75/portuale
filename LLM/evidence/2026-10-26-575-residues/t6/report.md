# Task 6 report: remote-binary `pkg_pretend` in the pretend pass (#579)

Date: 2026-10-27. Branch `fix/2026-10-26-575-residues` in both repos.
No commits, no pushes (manager's job); full gate / merge-path gate not
run here (manager runs them).

## What changed

`portuale` (product):
- `crates/portuale/src/emerge_getbinpkg.rs`
  - `binary_entry_needs_pretend` now takes `BinaryEntryNeedsPretendArgs`
    and gates a remote entry with no local file on its **remote index
    record** (EAPI/`DEFINED_PHASES`), via the director's `BinpkgIndex`
    seam -- as Portage's `_pkg_needs_pretend` reads `x.metadata`
    (`lib/_emerge/Scheduler.py:938-944`). Shared gate factored into
    `index_record_needs_pretend`.
  - New `remote_binpkg_target` (+ `RemoteBinpkgLocationArgs` /
    `RemoteBinpkgTarget`): one shared lookup (owning binrepo, record,
    download dir + dest) for the pass fetch and the merge download, with
    the merge's own missing-file texts.
  - New `fetch_binpkg_bytes` (+ args): the fetch half split out of
    `download_and_verify` (copy/wget to `<dest>.partial`); verification
    and rename stay with each caller. `download_and_verify` behaviour
    unchanged.
  - New `fetch_remote_binpkg_for_pretend` (+ args): fetch + verify in the
    pass, reusing an already-verified destination file. Success appends
    the verifier's `;-) ... [ ok ]` line to the staging capture (before
    any hook output); a missing-digests refusal or a mismatch appends
    Portage's failure block + the `>>> Failed to emerge` tail (no
    `, Log file:`, as in S0(c)) and returns the silent
    `binpkg_digest_failure` sentinel.
  - `pretend_one_mixed_entry`: remote arm fetches via the above, then
    pretends from the fetched file (`pretend_binary_entry_from_path`,
    split out of the local tail, behavior unchanged).
  - `merge_one_binary_entry` remote arm: looks the target up once, reuses
    the destination file when it exists and verifies (else downloads as
    before). `PORTAGE_QUIET` check factored into `portage_quiet_enabled`,
    used by both.
  - `MixedPretendEntry` / `PretendOneMixedEntryArgs` gained `config`.
- `crates/portuale/src/emerge_build/pretend_pass.rs`: two stale doc
  lines updated (remote binaries are fetched here now).
- `crates/portuale/src/fetch.rs`: `print_verified_names` split into
  `verified_names_line` (string for the staged log) + the print wrapper;
  output bytes unchanged.
- `LLM/code-notes/portuale/emerge_getbinpkg.md`: the #579 "gap" section
  became a "ported" note (order, both failure shapes, buffering rule,
  `fetchonly` reuse difference); new sections for the new symbols.
  `python3 LLM/tools/code_notes_index.py` run (no new files, index
  unchanged). `fact_coverage.py HEAD` on the three touched sources:
  0 facts lost.

`pmtest` (1 file): `test_getbinpkg_pretends_a_remote_binary_before_any_merge`
end-to-end pin (`-k --getbinpkg` of a `file://` binhost serving
`binpkgpretendpkg`): rc 0, pass order
(status < verify `[ ok ]` < `>>> Emerging binary`), hook ran first in
`var/lib` log, vdb live.

## What each test pins

All grounded in the S0 captures (`t6/s0.md`, host Portage 3.0.83) and
the cited Portage source.

- `remote_binary_pretends_in_the_pass_before_any_merge` (S0 a,
  `Scheduler.py:1083-1086,1147-1180,1250-1263`): pass replay holds the
  status line before the `MD5 SHA1 size ;-)` verify line (`[ ok ]`
  present), binary before source in merge-list order; the hook ran up
  front from fetched metadata; fetched file in the download dir; no vdb.
- `remote_binary_pretend_failure_aborts_before_anything_merges` (S0 b,
  `Scheduler.py:1310-1315`): pass fails with `pkg_pretend failed`, the
  hook's death replays; `run_merge_plan` errors, no vdb, no hook log.
- `remote_binary_is_fetched_once` (S0 a, `Scheduler.py:1154-1180`): the
  pass fetches; with the binhost file deleted, `merge_one_binary_entry`
  still merges from the pass's file; `$PKGDIR` stays empty.
- `remote_binary_verify_failure_aborts_before_anything_merges` (S0 c,
  `Scheduler.py:1206-1212`, `BinpkgVerifier.py:139-166`): pass fails with
  the silent sentinel; replay holds status, `!!! Digest verification
  failed:`, `Got: 3394`, `Expected: 12345`, the `>>> Failed to emerge`
  tail with no `Log file:`; plan aborts; no vdb/payload; exactly one
  `._checksum_failure_.` leftover, no `.partial`; the exact-count HTTP
  stub (5 = refresh 3 + explicit pass 1 + plan pass 1) proves no merge
  download.
- pmtest `test_getbinpkg_pretends_a_remote_binary_before_any_merge`:
  black-box version of the first pin.

## Commands and results

- `cargo build --release -p portuale` — clean.
- `cargo fmt --check` — clean (after `cargo fmt`).
- `cargo clippy --release --all-targets` (workspace) — 0 warnings.
- `cargo test --release -p portuale --bin portuale pretend` RED run:
  only the 3 new file:// tests failed (behavioral asserts), everything
  else green; after the fix all pass.
- `cargo test --release -p portuale --bin portuale` (full crate) — 1343
  passed, 0 failed, 3 ignored.
- `cargo test --release -p portuale --bin portuale emerge_getbinpkg` —
  70 passed. `emerge_build` — 100 passed. `fetch`/`digest`/`merge_engines`
  — green (see flake note).
- pmtest: new pin passes; `-k 'getbinpkg or usepkgonly or mrg_remote'`
  68 passed; pretend-contract getbinpkg/remotebin 43 passed;
  `test_output_invariants.py` 22 passed.
- `python3 LLM/tools/fact_coverage.py HEAD -- <3 touched sources>` —
  0 facts not found.

Flake note: one `fetch`-filter run failed once while the new tests were
being added (parallel stub-port race signature), then passed 5
consecutive runs plus the full suite. Not attributed to this change, but
flagging for the gate run.

## Open doubts

- None that block. Two accepted narrowings are recorded in the
  code-notes ported section: multi-job buffering replays in merge-list
  order (follows #577; Portage prints verifier output live), and the
  pass failure path writes no elog `* Messages for package` block (the
  ledger has no elog item for the sentinel; matches portuale's existing
  pass architecture).
- No OWNER QUESTION: every behavior decision is settled by Portage
  source or the live S0 captures. The host-probe merge-phase hang (a
  `_mtime_` lookup artifact of the hand-written index) is documented in
  `t6/s0.md` and does not affect the pass-order findings.
