# 06 — declaration builtins must assignment-expand an expanded name

**Crate:** `brush-builtins` · **Files:** `src/declaration.rs` (new),
`src/export.rs`, `src/declare.rs` (+
`brush-shell/tests/cases/compat/builtins/declaration-assignment-expansion.yaml`)
· **Branch:** `fix/declaration-assignment-expansion` (slice commits S1
`dd016ba6` + S2 `38447d84`; carried on the fork's `main` as one
house-style commit `eb3b6c7b`) · **Patch:**
[`patches/06-declaration-assignment-expansion.patch`](patches/06-declaration-assignment-expansion.patch)

## Symptom

```console
$ brush -c 'var=CC; prog=( gcc ); export ${var}="${prog[*]}"; echo "CC=[${CC}]"'
CC=[]
# exit 0 -- a silent no-op

$ bash -c 'var=CC; prog=( gcc ); export ${var}="${prog[*]}"; echo "CC=[${CC}]"'
CC=[gcc]
```

`declare` / `local` / `readonly` / `typeset` fail the same operand
differently: `declare: CC=gcc: not a valid variable name` (exit 1).

The failure mode that found this: real
`toolchain-funcs.eclass:25-45` `_tc-getPROG` ends with
`export ${var}="${prog[*]}"` whenever `CC`/`BUILD_CC`/… is unset, so
under `--shell brush` `tc-getCC` echoes empty and every
`tc-check-openmp` consumer dies in `pkg_pretend`
(`emerge --shell brush media-gfx/gimp-3.2.6` →
`Your current compiler does not support OpenMP!` plus two
`command not found: -E`). The default `--shell bash` path is unaffected.

## Root cause

The parser only recognises an assignment word when the name is a literal
identifier (`brush-parser/src/word.rs`, `assigned_scalar_name`), so
`export ${var}=…` is a plain word. After expansion the builtin receives
`CommandArg::String("CC=gcc")`, and `export.rs`'s `String` branch only
marks an *existing* variable exported — a silent no-op. bash assigns
**after** expansion for declaration builtins (POSIX XCU 2.9.1).

## Fix

Option (i) from the plan: re-examine the already-expanded string inside
the builtins — split on the first `=`, honour `+=`, and route a valid
scalar name through the existing `CommandArg::Assignment` path, so
scoping (`local` / `declare` in functions), attributes and append
semantics are unchanged. Operands without `=` still just mark exported;
compound array values (`name=(…)`) keep their existing handling (bash
refuses that conversion; it must never become a scalar assignment of
the literal text — regression-guarded by the `declare +a 'arr=(3 4)'`
case). An invalid name is now bash's diagnostic instead of a silent
no-op (`export` reports `` `name=value': not a valid identifier `` with
status 1, matching `declare`'s existing status).

Option (ii) — deferring assignment detection for declaration commands so
the word is re-examined after expansion — is closer to bash's model and
to what upstream
[#1280](https://github.com/reubeno/brush/pull/1280) attempts, but #1280
is still a Draft with breaking API changes (centralized word+subscript
passes, `declare.rs` split). This fix lands entirely inside the
builtins, so it rebases onto #1280 as a deletion rather than a
conflict; align with #1280's shape when it merges instead.

## Tests

`brush-shell/tests/cases/compat/builtins/declaration-assignment-expansion.yaml`
— 8 new cases over the matrix (literal | `${var}` | `"${var}"` |
append | no-value × `export` | `declare` | `local` | `readonly` |
`typeset`), diffed against real bash. The fix additionally heals 13
previously-`known_failure` cases, unmarked in the same branch: the
quoted scalar/append `declare` forms, `command declare/export`
forwarding (4 cases), the non-reexpanding generated `export`
assignment, and the array-element `export` rejection (its stderr
still differs, so `ignore_stderr` stays).

Full compat suite on the branch: **2576 ran, 2120 succeeded,
0 unexpected failures**, 456 known-to-fail, 29 skipped (pre-fix pin
baseline: 2568 / 2099 / 0 / 469 / 29). `cargo fmt --check` and
`cargo clippy --all-targets` clean, `brush-builtins` unit tests
31 passed (5 new splitter cases).

## Notes

- `$?` after a failed `export` differs between `brush -c` (1, matching
  bash) and brush reading a script from stdin (0): pre-existing,
  unrelated to this fix (the diagnostic prints in both), left alone.
- `export 'ea[0]=1'` matches bash under the harness (message + rc 0
  there); from `-c` brush reports rc 1 where bash reports rc 0 — same
  pre-existing `$?` propagation quirk, plus bash's own
  array-element-to-`export` oddity. Not emulated.
