# Plan 02.575 — final report (2026-10-26)

Branch `fix/2026-10-25-review` in both repos, committed per task, not
pushed and not merged to `main`.

## What shipped

| Item | What | portuale | pmtest |
|---|---|---|---|
| Task 0 | backlog entries, gate baseline | `58a1b8b9` | — |
| Task 1 (#562, #576) | stop the two mtimedb writes that damaged Portage's record | `6ed0d5fe` | `09fcd87` |
| Task 2 | merge-wait queues every gated merge, drains in finish order | `523bfd7e` | — |
| Task 3 (#577) | `pkg_pretend` pass: safe log file, `--jobs`, one runner | `13148bb5` | `d96b8bb` |
| Task 4 (#578) | remote binpkg verification checks every supported digest | `27e56b36` | — |
| Tasks 5–11 (#576) | global updates ported: vdb, world and `package.*`, dep strings, binpkgs, output, record, AUTOCLEAN force | `dd518948`, `1882fb8b`, `f328574d`, `48c45732`, `3f499070`, `0202665d`, `c99fc5eb` | `ff2cf61`, `51491f2` |
| Task 12 (#564) | each repos.conf warning once, in file order | `0cb3ba0f` | `82b0a06` |
| Task 13 (#571) | categories gate: Portage's path, one record per run | `84b24333` | `ccbe58d` |
| Task 14 | repos.conf filter documented | — | `5bd5cbc` |
| Task 15 | one dependency-tree parser in the resolver | `65ec636c` | — |
| Task 16 (#491) | slot-operator probe skips `||` branches Portage would not pick | `5a2fedb7` | `6bdb765` |
| Task 17 | merge-wait keeps two slots of one package apart | `418a6935` | — |
| Task 18 | dead code, silencers, per-call work removed | `cdd8a2dc` | — |
| Task 19 | `*Args` structs instead of `too_many_arguments` | `d95a5544` | — |
| Task 20 | comment sweep, one commit per crate | `4d6da1b2`, `6ff7519f`, `f6d8d3d9`, `70e544a4`, `492e4eaa` | — |
| Task 21 | misdated stamps, plan status headers, brief template | `64268e88` | — |
| #583 | versioned `slotmove` applied to matching versions only | `af8494bf` | — |
| #584 | global updates before `--deselect`, `--list-sets`, `--regen`, `--metadata` | `2b52ead2` | `e687e6d` |
| #586 | suite refuses root, host sentinel on mtimedb and vdb | — | `19ecaea` |
| #587 | signature check no longer leaks a temp directory | close-out commit | — |

Docs-only commits: `22fef466` (Portage naming), `6a968ff1` and
`6b95cf73` (owner questions and rulings), `bfa28387` (merge-gate
evidence), `6387e171` (#586 filed).

## Verification

- Full gate before every code commit: `cargo fmt --check`, clippy with
  0 warnings, `cargo test --release` (final: 3143 passed), pmtest
  `scripts/pytest-parallel.sh` (final: 4548 passed, 2 skipped,
  2 xfailed). No corpus bless was needed after Task 12's one-case bless
  in `82b0a06`.
- Merge gate (glibc-2.43-r4 and bash-5.3_p20 merged by both Portage and
  portuale, 0 hard findings, 0 unexplained, 0 payload diffs): see
  `LLM/evidence/merge-gate-fix-2026-10-25-review.md`, last run
  `l1-20261026T190753Z` after Task 19.
- L0 resolver bed: `l0-20261026T171004Z` (Task 15) and
  `l0-20261026T174920Z` (Task 16), zero moved probes against the earlier
  runs.
- No stop condition triggered.

## Owner questions

`OWNER-QUESTION-t11.md` — all answered 2026-10-26:
1. bed updates fixture: not needed;
2. Portage's full mtimedb shape: filed #585;
3. suite as root: guards A + B, shipped as #586;
4. `G0`/`G1`/`G2` comment labels: stay.

Exit codes (from #586): the owner keeps two codes, 4 for the root refusal and 1 for a host
sentinel trip (ruling 2026-10-26).

## Open residues

#579 (remote-binary `pkg_pretend`), #580 (`runtime_pkg_mask` port),
#581 (`_unsatisfied_system_deps` gate), #582 (`RMD160`/`WHIRLPOOL`),
#585 (full mtimedb shape), #588 (test modules leaking temp dirs).

## Notes

- Flaky tests seen once under full load, green on rerun:
  a `merge_wait` test (8/8 green on rerun) and
  `binpkg::tests::manifest::verify_gpkg_manifest_rejects_a_manifest_only_file`
  and `fetch::tests::fetch_src_uri_runs_a_configured_fetchcommand` (5/5 green
  alone, full workspace rerun 3143 passed).
- pmtest `6bdb765` has no backlog number in its title (#491); left as
  is, since it is committed history.
- `LLM/code-notes/portuale/pretend/tests/autoclean.md` still points at
  "the task report" for the live `AUTOCLEAN=yes` merge proof; that
  pointer is probably stale.
