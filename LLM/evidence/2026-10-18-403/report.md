# #403 report — S10: the merge/unmerge listing (2026-10-18)

Branch `backlog/403-merge-listing` in both repos, uncommitted, unpushed
(per the brief: no commit, no push). The #403 backlog entry is NOT flipped
(coordinator's).

## What was built

One listing printer (`crates/portuale/src/ebuild_merge/listing.rs`,
`MergeListing`) shared by the source merge, the binpkg merge and the
replace-unmerge, printing every line Portage's `dblink` prints on the
foreground path, on stdout. Gated on a new `MergeOptions.direct` flag
(`ebuild.rs` sets it for `merge`/`qmerge`/`unmerge`; the emerge-scheduler
paths never do) and `UnmergeOptions.direct` (set from it in the replace
loop, set for direct `ebuild unmerge`, unset for `emerge -C`). All
conditions, formats and order from Portage 3.0.82.2
`lib/portage/dbapi/vartree.py` (vendored checkout verified identical to
the host install).

Marker → source line: collision line `:4052-4055` (N = files + non-dir
symlinks, dirs excluded); `>>> Merging` `:5049-5052`; per-entry
`>>>`/`---`/`===`/`->` `:5654-5945` (`_needs_move` `:6363-6393`,
confmem skip via `_protect:6038-6042`); fifo/dev `>>>`/`!!!` `:5962-5994`;
`>>> Safely unmerging` `:5195`; `_show_unmerge` format `:2866-2867`;
`--- replaced` `:3090`, `--- cfgpro` `:3047-3048`, `<<<` `:3111/:3194/:3223`,
`--- !mtime/:3118 !dir/:3123 !sym/:3128 !found/:3032 !md5/:3215
!obj/:3202,3209 !fif/:3226 !prefix/:3017`, empty-desc `---` for kept
fifo/dev `:3228/:3230`; `_unmerge_dirs` `:3370-3493`; `>>> Original`
`:5222`; `>>> merged.` `:5348` (only after successful postinst).

## Oracle (new evidence `LLM/evidence/2026-10-18-403/`)

Recipe `scripts/l.sh` (+ `lib.sh`, S0-derived), 11 hermetic cells × both
PMs, all rc-parity: l01 fresh merge, l02 emerge-new, l05 qmerge-over-7,
l06 symlinks, l07 config-divert (`._cfgNNNN_`), l07b confmem-skip (`---`),
l08 collision abort (rc 1 both, protect-owned on), l08b merge-through,
l09 UNINSTALL_IGNORE `--- cfgpro` + `--- !empty`, l10 NOCOLOR plain,
l12 direct standalone unmerge. Synthetic packages live in the scratch
overlay only; no pmtest fixtures were added for the oracle.
`matrix.md` (verdicts + full marker inventory + order analysis),
`PROVENANCE.md`. Root-normalized stdout diff with the new binary:
l07/l07b/l09/l12 **identical**; l01/l05/l06/l10 differ in 4 lines only
(merge-side sibling order, by design — see below); l08b adds Portage's
24-line collision report (pre-existing portuale gap, out of scope).

## Findings the coordinator should know

1. **Merge-side order differs by design.** Portage's order is
   readdir-derived (pop-from-end; on tmpfs the image creation order,
   stable across the S0 and S403 runs). portuale traverses sorted
   (`mod.rs` determinism rule). No sorted traversal can reproduce a
   readdir-derived order (l01 needs reverse-sorted siblings at one level
   and sorted at the next), so portuale prints inline in its own order:
   same line set, sibling-subtree order differs. Pins compare the entry
   lines as a root-normalized multiset with verbatim anchors; the replace
   side (reverse-sorted CONTENTS) is byte-identical.
2. **Brief corrections (oracle beats the brief).** There is **no**
   `!owner` marker in 3.0.82.2 (tree-wide grep; l08b merges straight
   through with a plain `>>>`). `--- cfgpro` is the UNINSTALL_IGNORE
   match (`:3047`), not CONFIG_PROTECT (`/etc` files divert on the merge
   side instead). Both pinned by oracle cells.
3. **Emerge-path sink: nowhere.** The listing reaches neither the terminal
   (l02 both sides) nor `PORT_LOGDIR` nor the noclean `${T}/build.log`
   (both end after install) — the merge-child stdout is discarded. The
   printer is therefore a no-op there and portuale's build log needs no
   change, as the brief anticipated.
4. **Color rule** (`bin/ebuild:128-134`, `_MergeProcess._target:248`):
   always colorized except `NO_COLOR` set or `NOCOLOR=yes|true`; piped and
   `TERM=dumb` stay colored (all probed). The printer follows it.
5. **No STOP condition fired.** No existing pin moved (full suite green),
   every output is explained from Portage's source above, the emerge path
   needed no change.

## Files changed (portuale)

- `crates/portuale/src/ebuild_merge/listing.rs` (new): printer + formats.
- `tree.rs`: per-entry emits; `MergeTreeArgs.listing`.
- `after_install.rs`: collision line (before the abort check), `>>> Merging`,
  `>>> merged.` (in finish, after successful postinst), Safely/Original
  frame around the replace loop.
- `from_binpkg.rs`: same frame (silent today — binpkg merges are
  emerge-driven — but wired for the shared printer).
- `collisions.rs`: also returns file/symlink counts for the `N`.
- `options.rs` (`MergeOptions.direct`), `ebuild_unmerge.rs`
  (`UnmergeOptions.direct`, full `_unmerge_pkgfiles`/`_unmerge_dirs`/R4b
  display), `ebuild.rs` (sets `direct` on the three direct commands).
- Tests: `tests/listing.rs` (new, 4 tests incl. the small-tree listing),
  `tests/tree.rs` + `tests/collisions.rs` (silent-listing updates + count
  assertion).
- Docs: `LLM/what-this-proves.md` (appended paragraph),
  `LLM/code-notes/portuale/ebuild_merge/listing.md` (new) + 5 notes
  appends, index regenerated. Backlog entry untouched.

## Files changed (pmtest)

- `fixtures/repo/dev-libs/pg403-listpkg/{1.0,2.0}.ebuild` (new, tiny,
  fast: keep/old/new files + retargeted symlink) +
  `fixtures/repo/metadata/md5-cache/dev-libs/pg403-listpkg-{1.0,2.0}`
  (new, `_md5_`-correct, guard-passing). No other fixtures touched;
  pg359* files left alone.
- `pytests-contract-suite/test_portuale.py`: two pinned-output tests
  (fresh-merge multiset pin, qmerge verbatim replace pin; docstrings name
  the S403 capture files + Portage argv). No CASES rows: pretend output
  is untouched by construction (printer no-op off the direct path), so
  new pretend rows would be noise and the corpus stays clean.

## Gates (in order, sequential, fresh `cargo build --release` first)

- `cargo fmt --check`: clean.
- `cargo clippy --release --all-targets`: zero warnings.
- `cargo test --release` (whole workspace, TMPDIR=/tmp/t): **2807
  passed, 0 failed.**
- pmtest `python3 -m pytest pytests-contract-suite -q
  --basetemp=/var/tmp/pt403 -p no:cacheprovider`: **4047 passed,
  37 skipped, 2 xfailed** (includes the 2 new pins). Basetemp removed
  after (`sudo rm -rf`); `/var/tmp` and `/tmp` scratch cleaned.
- Corpus drift: none (suite green, no bless run).
- NOT run per the brief: container bed, glibc/bash merge gate.

## Test-environment note (pre-existing, not this slice)

`mrg::tests::redb_merge_phases_query_the_parent_over_the_pipe` (and once
its `vdb_ipc` sibling) fails when TMPDIR makes the test socket dir longer
than 55 chars: the test then expects the `/tmp` fallback, but the
implementation's try-and-fall-back still fits the path in `sun_path` and
uses it. Pure TMPDIR-length threshold behavior in `mrg.rs`/`vdb_ipc.rs`,
untouched by this diff; the suite is green with a short TMPDIR
(`/tmp/t`). The brief's example TMPDIR (`/var/tmp/t403`) lands in the
failing zone.
