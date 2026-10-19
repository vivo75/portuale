# r399 — backlog #399: the 4 blk0 `skipped-updates` USE-paint rows

**Verdict: not bisectable — no code regression. Invocation artifact, always
present without `FX_HOST_RUNNING_ROOT=1`, absent with it, on every binary
since #242 (2026-10-02). Do not fix product code.**

## 1. The 4 rows

Probe argv (all rc 0 on both sides), from
`differential-test-bed/logs/l0-fx-20261019T020404Z/l0-report.txt:20-30`:

- `--backtrack=0 dev-libs/blk0a dev-libs/blk0b dev-libs/blk0c` → missed `blk0x-2`
- `--backtrack=0 dev-libs/blk0b dev-libs/blk0a dev-libs/blk0c` → missed `blk0x-2`
- `--backtrack=0 dev-libs/blk0b dev-libs/blk0c dev-libs/blk0a` → missed `blk0x-3`
- `--backtrack=0 dev-libs/blk0c dev-libs/blk0b dev-libs/blk0a` → missed `blk0x-3`

Per-row diff (identical shape in all 4; `-2`/`-3` vary as above):

- Portage missed line:
  `(dev-libs/blk0x-2:0/0::testrepo, ebuild scheduled for merge) USE="" ABI_X86="(64)" conflicts with`
- portuale missed line:
  `(dev-libs/blk0x-2:0/0::testrepo, ebuild scheduled for merge to '/tmp/l0-fixture-oracle/fixtures') USE="" ELIBC="glibc" conflicts with`

The requirer lines agree byte-for-byte on both sides
(`… required by (dev-libs/blk0b-1 …) USE="" ELIBC="glibc"` — with/without
the `to` suffix, which the comparator tolerates).

## 2. What each side prints, and why they differ

Both paints are *correct for each PM's own resolution* — the two PMs resolve
different roots because the staging pin only affects portuale:

- Portage ignores `PORTAGE_RUNNING_ROOT` (portuale-only variable). With the
  bed's default staging (`PORTAGE_CONFIGROOT=$FX`, `ROOT=$FX`, running root
  `/`), the missed `blk0x` rides blk0b's EAPI-8 `DEPEND="…blk0x…"` edge into
  the **running root** `/` (Portage `_emerge/depgraph.py` EAPI≥7
  `DEPEND`→`ESYSROOT` rule; #242 XB). Its USE paint therefore uses the
  **host profile**: `profiles/arch/amd64/make.defaults:47` sets
  `ABI_X86="64"` (enabled) and `profiles/arch/amd64/use.force:10` forces
  `abi_x86_64` (the `( )` parens — Portage
  `_emerge/UseFlagDisplay.py:60,103-106`). `ELIBC` stays hidden under the
  host's `USE_EXPAND_HIDDEN`, so no `ELIBC` group renders. Display call:
  `depgraph.py:1668-1677` (`_show_missed_update_slot_conflicts`) with
  `modified_use=self._pkg_use_enabled(pkg)`; grouping in
  `UseFlagDisplay.pkg_use_display` (`UseFlagDisplay.py:55-112`).
- portuale with the default pin (`PORTAGE_RUNNING_ROOT=$FX`,
  `in-container.sh:49-53` else-branch) resolves **single-root**
  (`portage-repo/src/env_roots.rs:75-79` `running_root_from_env`). The
  missed node is target-root, so `skipped_update_roots`
  (`portage-repo/src/resolve/entry.rs:166-213`, "Both are `false` on a
  single-root run") returns `missed_running=false`, and
  `pretend/problem_reports.rs:204-216` falls back to `header.skipped_use`
  — painted at resolve time with the **fixture profile**
  (`direct_solve.rs:541`, `withhold.rs:398` via
  `conflict_use_display.rs:20-58` `skipped_update_use_display_for`).
  The fixture profile sets `ELIBC="glibc"`
  (`fixtures/repo/profiles/base/make.defaults:36`) and no `ABI_X86`, so
  the paint is `ELIBC="glibc"` with the `to '<root>'` suffix. Correct
  single-root output — but single-root is not what Portage does here.

With `FX_HOST_RUNNING_ROOT=1` (the pin dropped) portuale also resolves
cross-root: the missed node recomputes with `running_config` (host profile)
and paints `ABI_X86="(64)"` bare — byte-identical to Portage.

## 3. Bisect evidence (no first bad commit)

Experiment matrix, 4-probe atomlist
`differential-test-bed/logs/blk0-399.txt` (gitignored scratch file, left
in place for re-runs), `PMTEST_PM=portuale-bed PMTEST_NO_BUILD=1`:

| run | binary | env | UNEXPLAINED |
|---|---|---|---|
| E1 `l0-fx-20261019T031923Z` | bedbin (≈HEAD `3dd86926`) | no knob | **4** (exact rows of §1) |
| E2 `l0-fx-20261019T031937Z` | same | `FX_HOST_RUNNING_ROOT=1` | **0** (4/4 clean) |
| E3 `l0-fx-20261019T032235Z` | `3e2a18a2` (Oct-13, = binary of the green Oct-13 run) rebuilt in `/var/tmp/s10/bis/e3`, `CARGO_TARGET_DIR=/var/tmp/s10/bis/target` | no knob | **4** (exact rows of §1) |

Further:

- E1 vs E3 portuale outputs are **byte-identical** on all 4 probes
  (diffed `…/031923Z/portuale/*.txt` vs `…/032235Z/portuale/*.txt`): the
  paint behavior is unchanged between Oct 13 and HEAD.
- E2's portuale missed block is byte-identical to Portage's (`ABI_X86`
  bare); the residual file diffs are only comparator-tolerated preamble
  and `to`-suffix/trailing-slash noise.
- The Oct 12–13 green runs (e.g. `l0-fx-20261013T133139Z`, 48 probes / 0
  UNEXPLAINED, binary `3e2a18a2`) show portuale painting `ABI_X86="(64)"`
  bare — i.e. they ran cross-root, i.e. **with** the knob (via
  `l0-fixture-oracle-all.sh`, which passes `FX_HOST_RUNNING_ROOT=1` for
  the main list). The Oct 16+ red runs (02.359 slice work, Oct 19 bed)
  invoked `l0-fixture-oracle.sh` directly without the knob
  (`pm: portuale-bed` / `portuale`, single-root portuale output).
- Window review (`git log --since=2026-10-12 --until=2026-10-16`, 70
  commits): the only touches of the paint/root files are mechanical #339
  splits (`4bc31d58`, `2f66f197`, `cda2f98f` et al.) and an unrelated
  message tweak (`5e4b36b5`, `show_skipped_missing_deps` binary/ebuild
  wording + a `binary_only` field init in `skipped.rs`). No change to root
  assignment or USE-paint logic.
- pmtest side unchanged too (only wording #337 touched the
  runner/atomlist/allowlist since Oct 10); the retired allowlist entry
  `skipped-updates-cross-root-missed-line`
  (`compare/known-divergences-fixture-oracle.yaml:91-96`) already
  documents this exact shape and says the main list "now runs with
  `FX_HOST_RUNNING_ROOT=1`".

So: any post-#242 binary is red without the knob and green with it. There
is no first bad commit to bisect to.

## 4. Is Portage's `ABI_X86="(64)"` a portuale bug?

No. Portage names `ABI_X86` because the missed instance genuinely sits in
the running root, whose profile forces `abi_x86_64`. portuale names
`ELIBC` because, under the pin, the missed instance genuinely sits in the
target root, whose profile defaults `ELIBC=glibc` and knows no `ABI_X86`.
The paint correctly names the flag *of the root the package resolved in*
on each side. The bug, if any, is running the main list without its
documented knob — `atomlists/l0-fixture-oracle.txt:7-12` says "Run with
`FX_HOST_RUNNING_ROOT=1`" and `run/l0-fixture-oracle-all.sh:30` does so.

## 5. Proposed fix (harness, pmtest — not implemented)

No product-code change. Make a direct `l0-fixture-oracle.sh` invocation of
the main list behave like `all.sh`, in
`differential-test-bed/run/l0-fixture-oracle.sh` right after the `ATOMLIST`
resolution (`:19-23`), so the default is forwarded into the container by
the existing `-e "FX_HOST_RUNNING_ROOT=${FX_HOST_RUNNING_ROOT:-}"` at `:40`
and honored by `in-container.sh:49-53`:

```bash
# The main list's documented invocation resolves the running root against
# the container host (backlog #242 Slice D); default it so a direct run
# matches l0-fixture-oracle-all.sh. Explicit env still wins; other lists
# keep default staging.
case $ATOMLIST in
  */l0-fixture-oracle.txt) FX_HOST_RUNNING_ROOT=${FX_HOST_RUNNING_ROOT:-1} ;;
esac
```

Safe: other lists' filenames don't match, and an explicit
`FX_HOST_RUNNING_ROOT=` still overrides. Alternative (discouraged):
re-add a narrowed allowlist entry with the no-knob bytes per the comment
at `known-divergences-fixture-oracle.yaml:91-96` — it would green-light
direct runs but also bless the single-root-vs-cross-root mismatch shape,
hiding future real drift in it.

## 6. State / reproducibility notes

- No commits, no pushes, no tracked-file edits in either repo
  (`git status` clean in both; only gitignored `logs/blk0-399.txt` added).
- `/var/tmp/bedbin/portuale` restored byte-exact (`cp -p` from
  `/var/tmp/s10/bedbin-orig-portuale`, md5 `5905012f…` both sides).
  NOT refreshed to HEAD: the main tree's `target/release/portuale`
  (mtime 01:37, same md5) predates the current main HEAD `719c7f31`
  (03:22, moved by the #385 agent mid-task), so per instructions I rebuilt
  nothing — re-run `cp -p ../portuale/target/release/portuale
  /var/tmp/bedbin/portuale` after the main tree rebuilds.
- Worktree `/var/tmp/s10/bis/e3` removed (`git worktree list` clean);
  `/var/tmp/s10/bis/target` (build cache) left for follow-ups.
- Full evidence: `/var/tmp/s10/e1b-nopin.txt`, `e2-knob.txt`,
  `e3-nopin-oldbin.txt`, `e3-build.txt`; reports `l0-fx-20261019T031923Z`
  (E1), `…031937Z` (E2), `…032235Z` (E3).
