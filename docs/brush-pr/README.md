# brush upstream fixes — staged for submission

Six independent fixes for [`reubeno/brush`](https://github.com/reubeno/brush),
found while running real Gentoo `bin/*.sh` / eclasses through brush as portuale's
bash backend. **Three of them have landed upstream** (01 as #1416, 03 as
#1276, 06's `export` half as #1564); **the rest are not yet submitted** —
review, then open PRs. (A fifth fix, 05, was superseded by upstream's own
IFS rework, #988; see its row.)

In the local `3rdparty/brush` checkout (fixes rebased 2026-11-15 onto
upstream `main` `6a688f19`):

| # | branch | commit | Area | One-liner |
|---|--------|--------|------|-----------|
| [01](01-tokenizer-nested-construct-heredoc.md) | ~~`fix/tokenizer-nested-construct-heredoc`~~ | `995cfbf4` | `brush-parser` tokenizer | A `${…}` / `$(…)` / `$((…))` on a here-tag line has its sub-tokens stolen by the pending here-document, corrupting the enclosing word (and, for `<<${VAR}`, the tag). **MERGED upstream as [#1416](https://github.com/reubeno/brush/pull/1416) (`7dc43800`); no PR needed.** |
| [02](02-ast-heredoc-serialization.md) | `fix/declare-f-heredoc-serialization` | `f3bf115c` | `brush-parser` AST `Display` | `declare -f` of a function with a here-document produces output that neither bash nor brush can re-parse (redirect ordering, body indentation, quoted terminators), + smaller mismatches that make `declare -f` non-idempotent. Also drops the now-unused `indenter` dep. Upstream's #1580 is still open. |
| [03](03-pipeline-function-deadlock.md) | ~~`fix/function-pipeline-stage-deadlock`~~ | `2173b730` | `brush-core` command exec | A function used as a non-last pipeline stage runs to completion inline before the next stage spawns → deadlocks past one pipe buffer. **MERGED upstream as [#1276](https://github.com/reubeno/brush/pull/1276) (`144f2938`); no PR needed.** |
| [04](04-dot-parse-error-status.md) | `fix/dot-parse-error-status` | `07a3723a` | `brush-core` `source` | A parse error in a *sourced* file was fatal to the calling shell (and its `source … \|\| …` guard never ran). bash returns 2 and keeps going (POSIX mode still ends the shell). |
| [05](05-brace-expansion-ifs.md) | `fix/brace-expansion-ifs-independent` | `868ff957` | `brush-core` expansion | **Superseded by upstream [#988](https://github.com/reubeno/brush/pull/988) "improve IFS support" (`411b9a32`), which made brace expansion produce its fields independently of IFS.** No implementation PR needed; only the `IFS=`/`IFS=:` regression case that rework missed remains on the fork (`868ff957`, test-only). |
| [06](06-declaration-assignment-expansion.md) | `fix/declaration-assignment-expansion` | `eb3b6c7b` (squashed from S1 `dd016ba6` + S2 `38447d84`) | `brush-builtins` declaration builtins | Declaration builtins (`export` / `declare` / `local` / `readonly` / `typeset`) never assignment-expanded an expanded name (`export ${var}=value` was a silent no-op). **`export`'s half landed upstream as [#1564](https://github.com/reubeno/brush/pull/1564) (`e59919d5`); the `declare`/`local`/`readonly`/`typeset` half stays on the fork.** |

`vivo75/brush`'s `main` now carries fix 02, fix 04 and fix 05's regression test
on top of upstream `6a688f19`; **portuale is pinned to that main, `59e9b380`**
(re-pinned 2026-11-15), so it already builds against these fixes.
See [`../brush-pin.md`](../brush-pin.md) "Current pin".

The three still-carried `fix/*` branches on `vivo75/brush` are rebased onto
current `upstream/main` (`f3bf115c`, `07a3723a`, `868ff957`); the branches
for the superseded fixes still point at their old commits (`bc99e6c1`,
`962051c9`, `eb3b6c7b`) and need no PR.

`patches/*.patch` are `git format-patch` exports of the branch commits (carry the
full messages; `git am` them, or submit each branch as its own PR — still to do).
02, 04 and 05 were regenerated on 2026-11-15 from the rebased branches; 01, 03
and 06 are the original exports of the now-superseded branches (01 and 03 landed
upstream, 06's `export` half landed as #1564) and are kept as history.

## Opening the PRs (user-owned; B6)

Two PRs are ready — fix 02 and fix 04. Fixes 01, 03 and 06's `export` half
landed upstream (#1416, #1276, #1564) and need none; fix 05's implementation
is superseded by #988 and needs none (its regression test could ride with 02
or as its own tiny PR). The branches are already rebased onto current
`upstream/main`, so run from this repo root (or adjust `--body-file` paths):

```sh
gh pr create -R reubeno/brush --head vivo75:fix/declare-f-heredoc-serialization \
  --title 'fix(parser): make `declare -f` of a here-document parseable and idempotent' \
  --body-file docs/brush-pr/02-ast-heredoc-serialization.md
gh pr create -R reubeno/brush --head vivo75:fix/dot-parse-error-status \
  --title "fix(core): don't exit the calling shell on a \`source\` parse error" \
  --body-file docs/brush-pr/04-dot-parse-error-status.md
```

Fix 06's `declare`/`local`/`readonly`/`typeset` half has no rebased branch
yet: build one from the current `main`'s `brush-builtins/src/declaration.rs`
+ `declare.rs` change (the `export.rs` half is upstream's now) and reuse
[`06-declaration-assignment-expansion.md`](06-declaration-assignment-expansion.md)
for the body.

Record the resulting PR URLs here and in [`../brush-pin.md`](../brush-pin.md).

## Verification (2026-09-20 re-pin, fixes 01–04 + 05's regression test)

- `cargo test -p brush-parser` / `-p brush-core` — green.
- `cargo clippy -p brush-parser -p brush-core --all-targets` — clean.
- `cargo test --test brush-compat-tests` — **2568 ran, 2099 succeeded,
  0 unexpected failures**, 469 known-to-fail, 29 skipped. The delta over the
  2026-09-14 pin (2504 / 2023 / 0 / 481 / 29) is upstream's new cases and
  its #988 IFS rework (which unmarked more known failures).
- Ad-hoc (2026-09-14 pin): every function in all **211** Gentoo eclasses
  round-trips through `declare -f` → `eval` → `declare -f` with **0** parse
  failures, **0** eval failures, **0** non-idempotent results, plus one
  synthetic function per quoted here-tag form. Sweep on pristine upstream
  `25bffd54`: 41 eclasses never parsed, 20 round-trip failures among the
  1407 functions that did.
- `cargo test --release -p portuale` — 542 passed / 0 failed, incl. the
  `install_does_not_deadlock…` guard.

## Before / after (pristine `25bffd54` worktree vs. `main` with the fixes)

Portage phase-boundary flow — `source` a stack of eclasses
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
