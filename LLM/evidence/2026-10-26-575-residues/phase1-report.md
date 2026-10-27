# Plan 02.589 — phase 1 report (2026-10-27)

Phase 1 ran with Claude Code as the manager and opencode workers
(`muse-spark-1.3-contributor`), one writer at a time. Each worker had a
brief under the gitignored `.superpowers/sdd/fix-2026-10-26-575-residues/`.
The manager reviewed each diff against the task and the Global
Constraints, then ran the full gate (`gate.sh`) and, for Tasks 2, 3, 4
and 6, the merge-path gate, then committed. Branch
`fix/2026-10-26-575-residues` in both repos; nothing pushed.

| Task | Item | Result | Commits (pmtest → portuale) | Gates |
|---|---|---|---|---|
| 1 | #590 version 300.83.0 | DONE | — → `e5771955` | full |
| 2 | #585 mtimedb full shape | DONE | `8705f8f` → `26ddf226` | full; L1 `l1-20261026T232346Z` |
| 3 | #591 one `digest` (0.11) | DONE | — → `684c19d3` | full; L1 `l1-20261026T233746Z` |
| 4 | #582 RMD160/WHIRLPOOL | DONE | — → `2c743d31` | full; L1 `l1-20261026T235140Z` |
| 5 | #588 temp-dir leaks | DONE | — → `280e5c5d` | full; workspace leak check empty |
| 6 | #579 remote-binary pretend | DONE | `43f5205` → `1fe54fb9` | full; L1 `l1-20261027T012438Z` |
| 7 | #581 system runtime-deps gate | **held: owner question** | not committed (`t7/port.patch`) | full + L1 `l1-20261027T014327Z` on the patch |

Last full gate (Task 7 patch applied): 3165 Rust tests, 4552 pmtest
passed (2 skipped, 2 xfailed), clippy 0 warnings, no corpus drift. On
the committed tip without the patch the last gate is Task 6's (3162 /
4552). Merge-gate details: `LLM/evidence/merge-gate-fix-2026-10-26-575-residues.md`.
Host `/var/cache/edb/mtimedb` checked unchanged (`1792756554 2601`)
throughout Task 2.

## Manager changes on top of the workers

- Task 4: `bytes_hex_digest` uses the shared `portage_util::hex_lower`
  from Task 3 instead of its own closure, and a code comment that named a
  plan step is reworded.
- Task 5: `build_signed_gpkg` hid the same leak behind `std::mem::forget`;
  it now returns its guard. Not caught by the `.keep()` scan, so the
  worker had listed it as a residue candidate.

## Open owner questions

- `OWNER-QUESTION-t7.md`: port #581 with unit tests only (recommended),
  build the bed cell first, or park.

## Accepted differences (recorded in code-notes or quirks)

- #585: `mtimedb` `"version"` is `portuale-300.83.0` (portage-quirks Q18).
  The warnings are sorted, while Portage prints them in set order.
  `emerge moo` / `status` load the file and so warn, where Portage exits
  before loading; this predates the change (those actions are
  unimplemented in portuale).
- #585: the two-root warning count comes from the source
  (`actions.py:3378-3391`), not a live probe, because the host's
  running-root file must stay untouched.
- #579: with more than one job the pass output is buffered and replayed
  in merge-list order (as #577), where Portage prints the verifier
  output live. The pass failure path writes no elog
  `* Messages for package` block.
- #588: `load_vdb_shadow` keeps its remote scratch directory on purpose
  ("left for forensics" at the call site); it is on the scan allowlist.

## Residues filed in phase 1

- #595 (Tier 3): the `vdb_ipc` tests fail under a `TMPDIR` longer than
  about 13 characters (Unix socket path limit). Test-only, predates the
  change. Next free number: #596.
- Not filed, for phase-2 triage: the Task 7 patch reads the vdb at fill
  time to decide whether a runtime edge is satisfied, where Portage reads
  the resolve-time `satisfied` bit (`DepPriority.py`,
  `Scheduler.py:1707-1717`). Same outcome today.

## Phase 2

Nothing in phase 2 has started. The phase-2 R-tasks filed so far: R1 #592,
R2 #593, R3 #594 (plan §R). Per the plan the owner starts phase 2 ("execute
plan 02.589 phase 2"). Tasks 9–10 as rewritten were reviewed by
plan-reviewer (`LLM/02.589-575-residues.review-t9-t10.md`, applied in
`5ab1a6fc`). Task 9 must write its rules back into Task 10, and plan-reviewer
must review them, before any resolver code.
