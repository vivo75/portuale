# brush upstream fixes — staged for submission

Five independent fixes for [`reubeno/brush`](https://github.com/reubeno/brush),
found while running real Gentoo `bin/*.sh` / eclasses through brush as portuale's
bash backend. **Not yet submitted upstream** — review, then open PRs.

In the local `3rdparty/brush` checkout (rebased 2026-09-14 onto upstream
`main` `25bffd54`, `brush-v0.4.0-*`):

| # | branch | commit | Area | One-liner |
|---|--------|--------|------|-----------|
| [01](01-tokenizer-nested-construct-heredoc.md) | `fix/tokenizer-nested-construct-heredoc` | `bc99e6c1` | `brush-parser` tokenizer | A `${…}` / `$(…)` / `$((…))` on a here-tag line has its sub-tokens stolen by the pending here-document, corrupting the enclosing word (and, for `<<${VAR}`, the tag). |
| [02](02-ast-heredoc-serialization.md) | `fix/declare-f-heredoc-serialization` | `df830c59` | `brush-parser` AST `Display` | `declare -f` of a function with a here-document produces output that neither bash nor brush can re-parse (redirect ordering, body indentation, quoted terminators), + smaller mismatches that make `declare -f` non-idempotent. Also drops the now-unused `indenter` dep. |
| [03](03-pipeline-function-deadlock.md) | `fix/function-pipeline-stage-deadlock` | `962051c9` | `brush-core` command exec | A function used as a non-last pipeline stage runs to completion inline before the next stage spawns → deadlocks past one pipe buffer. (Re-do of the never-merged #1276.) |
| [04](04-dot-parse-error-status.md) | `fix/dot-parse-error-status` | `dfbca97c` | `brush-core` `source` | A parse error in a *sourced* file was fatal to the calling shell (and its `source … \|\| …` guard never ran). bash returns 2 and keeps going. |
| [05](05-brace-expansion-ifs.md) | `fix/brace-expansion-ifs-independent` | `2073877d` | `brush-core` expansion | Brace expansion joined its alternatives with spaces and relied on IFS field splitting, so `{A..C}` collapsed to one word under `IFS=`/`IFS=:`. |

Each branch is exactly one commit on current upstream `main`. `vivo75/brush`'s
`main` carries all five cherry-picked (`840ea40f` → `1132297d` → `10a455d8` →
`8850b943` → `b9524ad5`); **portuale is pinned to that main, `b9524ad5`**
(re-pinned 2026-09-14), so it already builds against these fixes. See
[`../brush-pin.md`](../brush-pin.md) "Current pin".

`patches/*.patch` are `git format-patch` exports of the branch commits (carry the
full messages; `git am` them, or submit each branch as its own PR — still to do).

## Opening the PRs (user-owned; B6)

One PR per branch, from the already-pushed `vivo75/brush` refs. Run from this
repo root (or adjust `--body-file` paths):

```sh
gh pr create -R reubeno/brush --head vivo75:fix/tokenizer-nested-construct-heredoc \
  --title "fix(parser): don't let a nested \${…}/\$(…) on a here-tag line steal the here-doc's tokens" \
  --body-file docs/brush-pr/01-tokenizer-nested-construct-heredoc.md
gh pr create -R reubeno/brush --head vivo75:fix/declare-f-heredoc-serialization \
  --title 'fix(parser): make `declare -f` of a here-document parseable and idempotent' \
  --body-file docs/brush-pr/02-ast-heredoc-serialization.md
gh pr create -R reubeno/brush --head vivo75:fix/function-pipeline-stage-deadlock \
  --title "fix(commands): run a function pipeline stage as a background task, not inline" \
  --body-file docs/brush-pr/03-pipeline-function-deadlock.md
gh pr create -R reubeno/brush --head vivo75:fix/dot-parse-error-status \
  --title "fix(core): don't exit the calling shell on a \`source\` parse error" \
  --body-file docs/brush-pr/04-dot-parse-error-status.md
gh pr create -R reubeno/brush --head vivo75:fix/brace-expansion-ifs-independent \
  --title "fix(expansion): make brace expansion produce fields independently of IFS" \
  --body-file docs/brush-pr/05-brace-expansion-ifs.md
```

Also decide [#1276](https://github.com/reubeno/brush/pull/1276) (the old,
never-reviewed deadlock fix): the plan's recommendation (D1) is to close it as
superseded by fix 03; the branch `fix/pipeline-function-stage-deadlock2` is
still on `vivo75/brush` for reference. Record the resulting PR URLs here and
in [`../brush-pin.md`](../brush-pin.md).

## Verification (all five applied)

- `cargo test -p brush-parser` / `-p brush-core` — green.
- `cargo clippy -p brush-parser -p brush-core --all-targets` — clean.
- `cargo test --test brush-compat-tests` — **2504 ran, 2023 succeeded,
  0 unexpected failures**, 481 known-to-fail, 29 skipped. The delta over the
  previous pin is the new regression cases (quoted here-tags, `source` parse
  errors, brace expansion under empty IFS) plus one previously-known failure
  (`echo ~/{a,b}`) that now passes and was unmarked.
- Ad-hoc: every function in all **211** Gentoo eclasses round-trips through
  `declare -f` → `eval` → `declare -f` with **0** parse failures, **0** eval
  failures, **0** non-idempotent results, plus one synthetic function per
  quoted here-tag form. Sweep on pristine upstream `25bffd54`: 41 eclasses
  never parsed, 20 round-trip failures among the 1407 functions that did.

## Before / after (pristine `25bffd54` worktree vs. `main` with all five)

Real portage phase-boundary flow — `source` a stack of eclasses
(`multilib` / `toolchain-funcs` / `flag-o-matic`; ~170 functions),
`declare -f > env`, `source env` (what `__save_ebuild_env` does at every
phase boundary):

| | pristine `25bffd54` | patched `main` |
|---|---|---|
| `source env` | `error: unterminated here document sequence` | rc 0, byte-idempotent across 3 phases |
| `declare -f _tc-has-openmp` → `eval` | `error: unterminated here document sequence` | byte-identical to bash |
| `declare -f` of `<<'EOF'` / `<<"EOF"` / `<<\EOF` / `<<-'EOF'` | terminator keeps its quotes (`'EOF'`); command-line tag not canonicalized | byte-identical to bash, idempotent |
| `source bad.sh` (parse error) | aborts the whole script; `source bad.sh \|\| echo caught` never runs the guard | returns 2, guard runs, caller continues |
| `printf '<%s>' {A..C}` with `IFS=` | `<A B C>` (one word) | `<A><B><C>` |
| `big \| wc -l` (function stage, 20k lines) | hangs (SIGKILL after timeout) | `20000` |
| per-function sweep over all 211 eclasses | 41 eclasses unparsed, 20 round-trip failures | 211/211 eclasses, 0 failures |

## Why portuale cares

`bin/save-ebuild-env.sh`'s `__save_ebuild_env` runs `declare -f` on every
in-scope function and writes the result to `${T}/environment`; the next build
phase does `source "${T}/environment" || die`. Bugs 01/02 make that file
unparseable (or make it grow without bound); bug 04 stops the `|| die` from
firing at all; bug 05 keeps `__filter_readonly_variables` from filtering bash's
special variables (the empty `$BASH` half of that is handled portuale-side, see
`docs/brush-pin.md`). Bug 03 hangs any `pkg_*` function piped into a filter.
Together they are why portuale's phase backend currently defaults to a real
`bash` subprocess instead of the embedded brush (`ShellBackend::Bash`; see
`docs/brush-pin.md`).
