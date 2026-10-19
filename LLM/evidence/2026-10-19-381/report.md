# r381 — backlog #381: `_match_contents` narrow forms in unmerge ownership

Status: DIVERGENCE FOUND and FIXED (verify-first oracle → red tests → shared-function fix → green everywhere). No commit (not instructed). Backlog/what-this-proves untouched (per task).

## Per-variant Portage vs portuale verdict

Oracle: host Portage 3.0.82.2, scratch overlay + hermetic configroot
(`LLM/evidence/2026-10-19-381/scripts/`, same shape as the #355 S0).
All cells: replace loop (second merge unmerges the first with
`others_in_slot=[new]`), `FEATURES` with `-protect-owned` (t355 convention).

| Variant | Portage | portuale pre-fix | portuale post-fix |
|---|---|---|---|
| V1 old=`/usr/share/t381/f`, new=`/alias/f` | `--- replaced obj`, file survives | file DELETED (orphan arm), vdb dangles | `--- replaced` equivalent, file survives; tree identical modulo env noise |
| V2 old=`/alias/f`, new=`/usr/share/t381/f` (mirror) | `--- replaced obj …/alias/f` + `<<< dir …/alias`, file survives | file DELETED | file survives; identical modulo noise |
| V3 old=keepdir `dir …/sub`, new=`obj /alias/sub/f` | `<<< obj …/.keep_*`, `--- replaced dir …/sub`; tree: sub+f survive | tree: sub+f survive (listing `--- !empty dir`) | same tree; listing now `--- replaced` via the same fix |

Residual tree diffs pg-vs-pl (all three cells, pre-existing, orthogonal):
`etc/ld.so.conf` (portuale env-update), `tmp/`, `var/cache/edb/mtimedb`,
`var/lib/portage/preserved_libs_registry`, CONTENTS mtimes (merge time).
V3-pl matched V3-pg on the tree even pre-fix, proving the noise is
independent of this change.

Two more candidate shapes were analyzed and need NO cell (documented in
`SUMMARY.md` + code note): the implicit-parent exact arm and the
basename shortcut are verdict-neutral with real merges (every ancestor
is recorded explicitly); the cross-package R4b search stays
exact-or-prefix (a second-spelling same-basename live record would be
the same dentry — same spelling or a merge collision). A same-inode
*hardlink* cell is unobservable (`_match_contents` never compares file
inodes).

## Root cause

`remove_contents`' same-slot `is_owned` skip and the bug-#326685
orphan typing used exact-string `contents_text_owns` /
`contents_text_node_type`, while Portage's `isowner`/`_match_contents`
(`vartree.py:3527-3613`) also matches via live parent-dir `(dev, ino)`
— both stats follow symlinks, so either spelling of one directory
matches. When the replacer records a path under a second symlinked
path, portuale missed and deleted a file the new instance owns.

## Files changed

portuale (branch `batch/2026-10-18`; the `crates/portage-repo/*`
modifications in the tree are the #391 agent's — not touched):
- `crates/portuale/src/ebuild_unmerge.rs` — new `contents_match_contents`
  (full narrow form, exact + parent-inode rebuild); `contents_isowner`
  delegates to it (no second copy); `is_owned` + orphan typing call it
  per owner text with a per-text inode map hoisted out of the entry
  loop (Portage memoizes `_contents_inodes` the same way); 2 new
  failing-first unit tests (V1 + V2 mirror).
- `LLM/code-notes/portuale/ebuild_unmerge.md` — `## fn
  contents_match_contents` + `## tests: is_owned` extension; R4b note
  updated (why the search stays exact-or-prefix). `fact_coverage.py`
  clean, `code_notes_index.py` clean.
- `LLM/evidence/2026-10-19-381/` — `SUMMARY.md`, `scripts/`
  (setup/run/cells), `cells/` (argv/env/rc/stdout/stderr/tree/CONTENTS
  per run, pre- and post-fix `-pl`).

pmtest (branch `batch/2026-10-18`, same name; appended after the t355
block, clear of the #391 agent's uncommitted CASES work):
- `fixtures/repo-355/app-misc/t381-rep/{t381-rep-1,t381-rep-2}.ebuild`,
  `t381-k/{t381-k-1,t381-k-2}.ebuild` (+ real md5-cache entries via
  Portage `egencache` on a copy, copied back — no churn to existing
  entries). No `t381` collisions. No CASES rows (real-execution pins,
  like the t355 tests).
- 3 pins in `test_emerge_pretend_contract.py` (docstrings cite the
  Portage S0 cells): `test_t381_replace_keeps_a_file_recorded_under_a_
  second_symlinked_path`, `..._when_the_old_record_uses_the_alias_
  spelling`, `..._keeps_a_dir_claimed_through_the_alias_spelling`.

## Tests

- New Rust tests failed pre-fix (`the replacer's file must survive`),
  pass post-fix (TDD red verified in-crate).
- pmtest pins: V1+V2 FAIL on the stashed (pre-fix) binary, all 3 PASS
  on the fixed binary (red-green verified by stash/rebuild cycle).
- `cargo fmt --check` OK; `cargo clippy --release --all-targets` 0
  warnings; `cargo test --release -p portuale` 1161 passed / 0 failed.
- `pytest -k "t381 or unmerge or contents"` 21 passed.
- Two transient, fully-explained non-greens, both re-run clean: (1) a
  full-suite `fetch_src_ui…fetchcommand` ETXTBSY flake (passes alone;
  untouched module); (2) two 51-passed/4-failed module runs that were
  the stale-binary guard (`self_exe.rs:56`) firing after source edits
  without a rebuild — my own sequencing error, not a product issue.
  No merge-path gate run (no merge-path change: unmerge ownership
  only; no glibc/bash gate needed).

## Residue

- None new. #381's remaining narrow shapes (basename shortcut,
  implicit-parent exact, R4b prefix, merge-side `owns_path`) are
  analyzed as verdict-neutral/out-of-scope in the code note, not filed
  separately.
- The pmtest V3 pin asserts the tree only (the `--- replaced dir` vs
  `--- !empty dir` listing difference is invisible in `emerge` output).
