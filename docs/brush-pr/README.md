# brush upstream fixes — staged for submission

Three independent fixes for [`reubeno/brush`](https://github.com/reubeno/brush),
found while running real Gentoo `bin/*.sh` / eclasses through brush as portuale's
bash backend. **Not yet submitted upstream** — review, then open PRs.

In the local `3rdparty/brush` checkout (base `a250b84e`, `brush-v0.4.0-136`):

| # | branch | commit | Area | One-liner |
|---|--------|--------|------|-----------|
| [01](01-tokenizer-nested-construct-heredoc.md) | `fix/tokenizer-nested-construct-heredoc` | `bd4793ab` | `brush-parser` tokenizer | A `${…}` / `$(…)` / `$((…))` on a here-tag line has its sub-tokens stolen by the pending here-document, corrupting the enclosing word (and, for `<<${VAR}`, the tag). |
| [02](02-ast-heredoc-serialization.md) | `fix/declare-f-heredoc-serialization` | `3d2bde47` | `brush-parser` AST `Display` | `declare -f` of a function with a here-document produces output that neither bash nor brush can re-parse (redirect ordering, body indentation), + smaller mismatches that make `declare -f` non-idempotent. Also drops the now-unused `indenter` dep. |
| [03](03-pipeline-function-deadlock.md) | `fix/function-pipeline-stage-deadlock` | `ca11d652` | `brush-core` command exec | A function used as a non-last pipeline stage runs to completion inline before the next stage spawns → deadlocks past one pipe buffer. (Re-do of the never-merged #1276.) |

Each branch is one commit off `a250b84e`. **`main` = `a250b84e` + the three
commits cherry-picked linearly** (`95959d30` → `e9157f0a` → `de451b39`, tip
`de451b39`) — the "working" branch for portuale to build against once it is
pushed and `portuale/Cargo.toml` re-pinned (that bump also pulls ~130 commits of
upstream drift, so treat it as its own slice).

`patches/*.patch` are `git format-patch` exports of the branch commits (carry the
full messages; `git am` them, or submit each branch as its own PR).

## Verification (all three applied)

- `cargo test -p brush-parser` — 236 passed (was 234; +2 new tokenizer snapshot tests)
- `cargo test -p brush-core` — green
- `cargo clippy -p brush-parser -p brush-core --all-targets` — clean
- `cargo test --release --test brush-compat-tests` — **2447 ran, 1976 ok, 0 failed**,
  471 known-to-fail, 29 skipped. Without the fixes: 2442 ran, 1971 ok, 0 failed
  (no existing compat case exercises any of these four bugs) — i.e. **zero
  regressions**, and the 5-case delta is the new regression tests added here.
  During development an intermediate state (patch 02 partial) briefly showed
  7 failures — a `{ : }` body rendered at column 0 — fixed by the "first line of
  a verbatim span is still indented" rule; the final suite is clean.
- Ad-hoc: every function in all **211** Gentoo eclasses (1843 functions) now
  round-trips through `declare -f` → `eval` → `declare -f` with **0** parse
  failures, **0** eval failures, **0** non-idempotent results. Before: dozens of
  parse failures (01), and the ones that did parse drifted on every round-trip (02).

## Before / after (pristine `a250b84e` worktree vs. `main` with all three)

Real portage phase-boundary flow — `source` a stack of eclasses
(`multilib` / `toolchain-funcs` / `flag-o-matic`; ~170 functions),
`declare -f > env`, `source env` (what `__save_ebuild_env` does at every
phase boundary):

| | pristine `a250b84e` | patched `main` |
|---|---|---|
| `source env` | `error: unterminated here document sequence` | rc 0, byte-idempotent across 3 phases |
| `declare -f _tc-has-openmp` → `eval` | `error: unterminated here document sequence` | byte-identical to bash |
| `big \| wc -l` (function stage, 20k lines) | hangs (SIGKILL after timeout) | `20000` |
| per-function sweep over all 211 eclasses | dozens of parse-fails + unbounded drift | 1843/1843 clean |

## Why portuale cares

`bin/save-ebuild-env.sh`'s `__save_ebuild_env` runs `declare -f` on every
in-scope function and writes the result to `${T}/environment`; the next build
phase does `source "${T}/environment" || die`. Bugs 01 and 02 make that file
unparseable (or make it grow without bound across phases). Bug 03 hangs any
`pkg_*` function piped into a filter. Together they are why portuale's phase
backend currently defaults to a real `bash` subprocess instead of the embedded
brush (`ShellBackend::Bash`; see `docs/brush-pin.md`).
