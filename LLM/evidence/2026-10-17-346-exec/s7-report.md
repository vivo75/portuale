# S7 (351-ABORT) execution report — 2026-10-18 — STOP: two pins must move

Branch `backlog/346-rest` (both repos). No commit, no push (per hard rules).
Oracle: Portage 3.0.82.2 captures in `LLM/evidence/2026-10-15-346-s0/a/`
(A cells a01–a12) for the new pins, plus `LLM/evidence/2026-10-12-348-s0/d/`
cells `d-eapi99-oneshot`, `d-eapi7a-buildpkg`, `d-binpkg-eapi99` for the two
moved pins below. Say "Portage", never "real". Container bed not run
(not required for this slice).

## Status: STOP per the brief's trigger — two existing pins must move

Only the non-pretend half was implemented (plan Q6 answered by the owner:
the `--pretend` all-masked arm stays exactly as it is; the resulting
incoherence is noted in the S7 code note under
`## fn top_level_abort_notice_shown`). The full pmtest suite moves exactly
two pins, both non-pretend top-level all-masked aborts asserting the old
`stdout == ""`. Their new stdout is Portage-grounded (see §4), but the
brief says a pin that must move is a STOP: the two asserts below are left
untouched for reviewer approval.

## Files changed (portuale repo, tracked only)

Mechanism (the top-level all-masked arm exits via resolver `Err` before
`show_resolution_header` runs; the other abort arms exit via
`ResolveOutcome::Aborted` after it):

- `crates/portage-repo/src/masked_deps.rs` — new `pub fn
  is_all_masked_abort_report(msg)`: matches the top-level "all masked"
  block (`all_masked_report`/`all_binary_masked_report`) by its
  `!!! All ebuilds that could satisfy` prefix. The only resolver `Err`
  raised after resolution started, so the only one whose exit path prints
  the notice. Portage's `for <root>.` header suffix sits after the matched
  prefix, so both shapes match.
- `crates/portuale/src/pretend/merge_list.rs` — `show_resolution_header`
  body extracted into `show_resolution_header_with_counts(opts,
  backtrack_restarts, backtrack_max)` (same gates, same bytes);
  `show_resolution_header` is a thin wrapper passing the result's counts.
- `crates/portuale/src/pretend/gates.rs` — new `pub(crate) fn
  top_level_abort_notice_shown(pretend, msg)`: `!pretend &&
  is_all_masked_abort_report(msg)`.
- `crates/portuale/src/pretend/resolve_phases.rs` — the resolve-error
  handler prints `show_resolution_header_with_counts(opts, 0,
  opts.backtrack_max)` to stdout before the stderr report when the gate
  fires. Zero restarts: the argument dies on first dequeue, before any
  backtrack (A cells show `0/20`); the max is the run's own
  `opts.backtrack_max`, the same value a `GraphResult` would carry.
- Docs (append-only): `LLM/what-this-proves.md` (+1 paragraph);
  `LLM/code-notes/portage-repo/masked_deps.md` (new `## fn
  is_all_masked_abort_report`), `LLM/code-notes/portuale/pretend/{gates
  (new ## fn, incl. the Q6 incoherence note),merge_list,resolve_phases}.md`
  (+ `code_notes_index.py` re-run; INDEX.md unchanged).

## Tests added

- Rust (failing first — compiled against the missing helper, then passed):
  `pretend::tests::gates::top_level_all_masked_abort_prints_the_notice_only_off_pretend`
  (non-pretend + masked report → true; pretend + masked → false;
  non-pretend + plain-miss / USE-miss → false), grounded in A cells a01/a02.
- pmtest CASES row (Portage rc 1):
  `#351 S7: non-pretend top-level all-masked abort earns Portage's rc 1`
  (`["--oneshot", "dev-libs/keywordrevokedpkg"]`, 1).
- pmtest `test_s7_abort_masked_top_level_prints_resolution_notice`
  (2 params, `_t341_configroot` + `dev-libs/bareacceptkeywordspkg`;
  docstring names both capture files and the full Portage argv):
  `oneshot` pins `Calculating dependencies ... done!` +
  `Dependency resolution took (backtrack: 0/20).` + blank (cell
  `a01-masked-top-oneshot-portage.stdout`, modulo the cut wall-clock
  seconds); `ask-verbose` pins the leading blank + `These are the
  packages that would be merged, in order:` + blank + the same two lines
  (cell `a04-masked-top-ask-portage.stdout`). Both pin rc 1 and the
  unchanged `_all_masked` stderr. Non-pretend is safe here: the abort
  exits before any merge (throwaway ROOT).

## Gates (in order, never concurrently)

- `cargo fmt --check`: clean. `cargo clippy --release --all-targets`:
  0 warnings. `cargo build --release`: ok.
- `cargo test --release` (workspace): 2798 passed, 0 failed
  (S8 baseline 2797 + 1 new).
- pmtest `python3 -m pytest pytests-contract-suite -q
  --basetemp=/var/tmp/pt346e -p no:cacheprovider`: **4029 passed,
  37 skipped, 2 xfailed, 2 failed** — the two moved pins below, nothing
  else. Basetemp removed afterwards. Live-verified before pinning in a
  `/var/tmp` scratch replicating `scripts/a.sh` (repos.conf + t341 +
  default-profile symlink, scratch removed): `--oneshot` and `--ask=n -v`
  print the A-cell shapes, `-p` stays empty, `--quiet` stays empty.
- Corpus drift: none. The full run emitted no drift warnings, and the two
  failing tests run in isolation emit none either (no harvested entries
  for their calls). NOT blessed (nothing to bless).

## Moved pins (left untouched — the STOP question)

Both fail with the new, Portage-grounded stdout where they assert `""`:

1. `test_t348_s5_unsupported_eapi_oneshot_and_buildpkgonly_mask`
   (`test_emerge_pretend_contract.py:27992`): `["--oneshot",
   "=app-misc/t348-d-eapi99-1"]` and `["--buildpkgonly",
   "=app-misc/t348-d-eapi7a-1"]` now print `Calculating dependencies ...
   done!\nDependency resolution took (backtrack: 0/20).\n\n` on stdout.
   Grounding: `LLM/evidence/2026-10-12-348-s0/d/d-eapi99-oneshot.portage.stdout`
   and `d-eapi7a-buildpkg.portage.stdout` both show exactly the notice +
   timing (`backtrack: 0/20`, rc 1); their `Performing Global Updates`
   blocks are the 351-UPD cut (S0 cells here run `--package-moves=n`-less
   on the Portage side; portuale never prints them).
2. `test_t348_s5_binpkg_with_unsupported_eapi_is_masked` (`:28102`):
   `["-K", "--usepkgonly", "=app-misc/t348-s5-bineapi-1"]` (the
   `all_binary_masked_report` arm, covered by the same predicate) now
   prints the same two notice lines. Grounding:
   `d/d-binpkg-eapi99.portage.stdout` shows the notice + timing (rc 1);
   Global Updates + news blocks are the 351-UPD / 351-NEWS cuts.

Proposed resolution (for the reviewer, not applied): replace each
`assert result.stdout == ""` with the notice-lines assert used by the new
S7 test (`splitlines() == ["Calculating dependencies ... done!",
"Dependency resolution took (backtrack: 0/20).", ""]`), keeping the
`_all_masked_eapi` stderr asserts unchanged.

## Left as-is (adjacent shapes, verified unchanged)

- `-p`/`-pv` top-level all-masked: stdout still empty (Q6; existing
  `stdout == ""` pins all pass).
- `--quiet` non-pretend top-level masked: stdout still empty (shared gate).
- Other top-level abort shapes (plain miss, USE miss, REQUIRED_USE):
  unchanged — verified byte-identical pre/post fix for
  `=dev-libs/t341-dep-1[x]` via stash-rebuild-compare (that arm already
  printed the notice through the autounmask path before this slice).
- Untracked `pmtest/fixtures/*pg359*` files left alone. No tracked files
  outside this slice touched (portuale: 5 code/test + 4 notes + paragraph;
  pmtest: the one contract file).
