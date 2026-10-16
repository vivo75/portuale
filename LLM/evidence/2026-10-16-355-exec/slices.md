# S1 slices — plan `LLM/02.355-reinstall-unmerge-parity.opus.md`, slice S1 (#358)

Branch (both repos): `backlog/355-358-unmerge-parity`.
Oracle rule: expected values from Portage 3.0.82.2 captures, never from portuale output.

## What changed

pmtest (bed side, no product code):

- `differential-test-bed/layers/l1/consume.sh`
  1. Pre-merge CONTENTS snapshot: right after `installed-before.txt`, every
     `/var/db/pkg/*/*/CONTENTS` is copied to `$OUT.pre-contents/<cat>/<pf>/CONTENTS`
     (before the vdb convert/mount blocks rewrite `/var/db/pkg`).
  2. Path list: for every cpv in `installed-before` that is gone in
     `installed-after` (replaced) or listed in `merged-cpvs` (reinstalled),
     the pre-merge CONTENTS paths are emitted too. Always on (observation only).
  3. Both extractions use a new `contents_paths()` shell function following
     Portage's `_contents_re` (`obj` strips ` <md5> <mtime>` from the right,
     `sym` cuts at the last ` -> `, `dir`/`dev`/`fif` take the rest; bad lines
     skipped). This is #357's rule on the bed side; replaces both
     `awk '{print $2}'` splits.
  4. New opt-in `L1_CONSUME_INSTALL_MASK`: when set, appended to
     `/etc/portage/make.conf` as `INSTALL_MASK="${INSTALL_MASK} <value>"`
     before the merge, in both consumers.
- `differential-test-bed/run/l1-merge-from-binpkg.sh`: forwards
  `L1_CONSUME_INSTALL_MASK` into both consumers; header documents it.
- `differential-test-bed/README.md`, `USAGE.AGENTS.md`: the new variable and
  the pre-merge CONTENTS paths.
- `differential-test-bed/findings/l1.md`: appended (see §Proof).

portuale (docs only, no code):

- `LLM/agent-context.md` §"Merge-path safety gate": third invocation
  (masked reinstall) documented as required for any change to the unmerge
  half of the merge path.

`bash -n` on both scripts: OK.

## Proof runs (G4)

All from `../pmtest`, one at a time, no cargo build during any bed run.
`GENTOO_MIRRORS=http://10.48.0.229:8080` exported (nothing needed fetching;
`L1_SKIP_BUILD=1` reused the warm pkgcache). Expected values from Portage
3.0.82.2 in the same runs, never from portuale output.

1. **Masked gate at HEAD** (`a530fbdc`, binary built from the slice tree):
   `L1_SKIP_BUILD=1 L1_CONSUME_REINSTALL=1
   L1_CONSUME_INSTALL_MASK="/usr/share/doc /usr/share/info /usr/share/man"`
   → `differential-test-bed/logs/l1-20261016T215952Z`: **0 hard /
   0 unexplained** (3934 files snapshotted, merge_rc=0 both PMs). The mask
   bit: pre-merge CONTENTS lists e.g.
   `obj /usr/share/doc/glibc-2.43-r2/ChangeLog.old/ChangeLog.1.bz2`;
   post-merge both PMs deleted the whole masked trees (only the emptied
   parent dirs and install-info's regenerated `dir` index remain).
2. **Masked gate on the pre-#354 tree** (parent of `8772601e` =
   `5e4b36b5`, via `git worktree add /var/tmp/355-s1-pre354` +
   `cargo build --release -p portuale` there — 1m17s, no fallback needed —
   and a temporary `portuale-pre354` entry in `managers/managers.yaml`,
   run with `PMTEST_PM=portuale-pre354 PMTEST_NO_BUILD=1`):
   → `logs/l1-20261016T220455Z`: **75 hard findings, all
   `[MISSING] … present for portuale, absent for portage`**, every one
   under `/usr/share/doc/{glibc-2.43-r2,bash-5.3_p15}`,
   `/usr/share/info`, `/usr/share/man` (rc=1, as expected for the buggy
   tree). The cell sees exactly the stale-file bug class. Entry reverted
   and worktree removed afterwards (`git worktree list` shows only the
   main checkout; `managers.yaml` has no diff).
3. **Plain-gate smoke at HEAD** (no mask):
   `L1_SKIP_BUILD=1 L1_CONSUME_REINSTALL=1`
   → `logs/l1-20261016T220225Z`: **0 hard / 0 unexplained** (4010 paths,
   4007 files). Pre-S1 like-for-like baseline `l1-20261014T091928Z`
   (gate reinstall): 4006 files. The path list grew and the verdict did
   not change. The +1 file is real coverage: the old post-merge-only list
   never looked at `obj /usr/share/info/bash.info.bz2` /
   `sym /usr/share/info/bashref.info.bz2` (present in pre-merge CONTENTS,
   dropped by the reinstall image); both PMs deleted both and agree.

No pytest CASES (bed shell only, per plan). Full pmtest suite not re-run:
no Python files touched (`bash -n` OK on both scripts instead).

## Anything surprising / left undone

- Nothing blocking. The smoke run independently confirmed the #354 fix
  holds on identical-image reinstalls for `/usr/share/info` files dropped
  between the image build and the binpkg build (both PMs delete, 0/0).
- Left undone (per plan, not this slice): S2–S5 product fixes; the sqlite/redb
  reinstall gate lines (come after S3); `l1-porttest.txt` runs (after S4/S5).
- No commits made (coordinator reviews and commits). No allowlist entries
  added. No hard/unexplained rows at HEAD to report.

## Continuation verification (2026-10-16, second worker)

The first worker's turn ended while a background build was pending, so a
second worker re-entered to finish S1 and found all three proof runs
already on disk with the findings note already appended to
`differential-test-bed/findings/l1.md`. Per the brief ("do NOT redo what
is already done") the bed runs were **verified, not repeated**:

- `bash -n` re-run on `consume.sh` and `l1-merge-from-binpkg.sh`: OK.
- `logs/l1-20261016T215952Z/l1-report.txt`: 0 hard / 0 unexplained,
  3934 files, merge_rc=0 both PMs. Pre-merge `CONTENTS` under
  `<pm>.pre-contents/` list e.g.
  `obj /usr/share/doc/glibc-2.43-r2/ChangeLog.old/ChangeLog.1.bz2`;
  both `paths.txt` hold 70 `/usr/share/doc` entries.
- `logs/l1-20261016T220455Z/l1-report.txt`: 75 hard, all
  `[MISSING] … present for portuale, absent for portage` (69 under
  `/usr/share/doc`, 2 `/usr/share/info`, 4 `/usr/share/man`); its
  `pm.json` stamps version `5e4b36b5`, which is `8772601e^` (checked with
  `git log` in portuale), so the buggy tree is genuine.
- `logs/l1-20261016T220225Z/l1-report.txt`: 0/0; both `paths.txt` are
  4010 lines vs 4007 in baseline `l1-20261014T091928Z` (4006 files there);
  `/usr/share/info/bash.info.bz2` is in the new paths and in neither
  baseline paths file.
- Revert state: `git worktree list` shows only the main checkout,
  `/var/tmp/355-s1-pre354` and `/var/tmp/t355*` do not exist, and
  `managers/managers.yaml` has no diff and no `pre354` reference.
- No commits made; no bed run was active while checking, and no cargo
  build was started. `/var/tmp` holds no S1 scratch.
