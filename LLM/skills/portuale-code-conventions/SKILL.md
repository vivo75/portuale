---
name: portuale-code-conventions
description: Use when writing, splitting or refactoring Rust in this workspace — sizing a function or module, extracting code out of a giant function, moving tests, deciding what belongs in a comment versus a code note, or checking that a refactor really changed nothing.
---

# Code conventions

The rules here are mechanical on purpose: a pure move must be provably pure,
and a comment must be short enough that the context lives somewhere findable.

## Size limits

- **A production function is at most 200 code lines.** `LLM/02.339-giant-functions.opus.md`
  states the goal for every production function in `crates/`, with no change in
  behaviour or output. At that time 44 production and 5 test functions
  exceeded it; `run` in `pretend.rs` was the longest at 6,070 lines.
- **A production file is around 500 lines where natural**
  (`LLM/plans/2026-10-10-split-map.md`, "production file <= ~500 lines where
  natural"). Modules are split by responsibility, not by line count target.
- **More than five parameters → an `Args` struct.** `LLM/tools/args_struct.py`
  generates it, matching the clippy configuration.

## A pure move is a pure move

- Statements move **verbatim and in order** into a named function next to the
  host. No renames, no signature changes, no reordering of `impl` blocks
  (`LLM/plans/2026-10-10-split-map.md`).
- **Closures become private functions** whose parameters are the captured
  locals, or one context struct — never a 50-parameter function.
  Convert closures **before** splitting the surrounding logic, so the extracted
  arms can take `&Ctx`.
- **Cut the straight-line, side-effect-free code first.** Leave the function's
  core decision inline: in one case the right cut was the
  `let prune_unresolved = …` binding, and the backtrack `match` — the actual
  subject of the function — stayed put (`LLM/02.339-giant-functions.review.md`).
- **One function per arm group** once arms exceed ~30 lines; small arms stay in
  the `match`.
- **A context struct whose fields serve two unrelated statement groups means
  the cut is in the wrong place.** Split it, do not widen it.

## What a refactor must prove

- **No output change.** For resolver refactors, capture merge-order traces
  before and after and diff them:
  `diff -r <(cat $T-pre/*.trace) <(cat $T-post/*.trace)` must be empty
  (`LLM/02.339-giant-functions.opus.md`).
- **L0 identical, no corpus drift** for anything that touches output.
- **No new clippy warnings.** `cargo clippy --release --all-targets` is
  zero-warning; the count is recorded before and after.
- **Tests move without changing behaviour**: a `tests_16x` module moves as-is
  first (`#[cfg(test)]`, sibling `tests/`), and only then is it split into
  `<module>/tests.rs`. Shared helpers go to `src/test_support.rs`; large
  integration fixtures to `resolve/tests/`.
- **Merge-path moves go last and get the gate.** Any step that moves
  `run_merge_loop`/`merge_one_*` or touches `binpkg/extract.rs` /
  `local_index.rs` needs the differential bed and the glibc+bash merge gate.

## Comments and code notes

Comments stay short: `///` summary plus `# Errors`/`# Panics`, and `//` only
for a non-obvious *why*, one `// Portage: <file>, <function>` reference where
needed. Everything else — context, Portage `file:line` citations, backlog
numbers, history — goes to `LLM/code-notes/<crate>/<path without src/ and .rs>.md`
under `## <kind> <symbol>`.

- After adding a notes file: `python3 LLM/tools/code_notes_index.py`.
- After shortening a comment: `python3 LLM/tools/fact_coverage.py BASE FILE...`
  proves no fact (backticked span, `file:line`, `#NNN`, quoted string, date,
  version) disappeared.
- **Do not reword vendored text.** `bin/`, `cnf/` and the bash runtime are
  re-synced from upstream; a rewording breaks the re-sync contract and changes
  the embedded runtime.
- **Do not rename external API names** — another crate's enum variants, Python
  stdlib names. They are quoted, not owned.

## The tooling that makes this cheap

| Tool | What it does |
|---|---|
| `LLM/tools/fn_lengths.py`, `fn_outline.py` | list functions over the limit, and their statement outline |
| `LLM/tools/rust_items.py` | the definition surface of a file, without reading it whole |
| `LLM/tools/extract_block.py` | cut a statement range into a new function (refuses `?`/`return` it cannot handle) |
| `LLM/tools/flatten_flow.py` | flatten nested control flow before extraction |
| `LLM/tools/args_struct.py` | generate an `Args` struct for a many-parameter function |
| `LLM/tools/split_module.py`, `split_tests.py` | module and test-module splitting |
| `LLM/tools/dupes.py` | `cargo dupes` summary over `crates/` |

The extraction tool is a **helper, not an authority**: it refuses ranges with
`?` and `return Err(..)`, asserts on non-tail calls, and in `--loop` mode can
lose an assignment to a `Copy` local declared outside the range. Read its
output; do not trust its silence.

## Traps

- **A lint test that scans zero files passes vacuously.** After moving a
  directory, add an assertion that the file list is non-empty, or the guard is
  silently off.
- **An extracted `tests.rs` is scanned as production code** unless it stays
  under `#[cfg(test)]`; that flips VDB lint results.
- **`cargo` invoked inside a `3rdparty/` crate fails**: those crates have no
  workspace manifest and believe they are in a workspace. Run cargo from the
  workspace root.
- **A parallel worktree cannot run the pmtest gate**: it builds and tests the
  main checkout, and untracked reports are invisible to worktrees.
- **Two writers on one crate** is not a merge conflict, it is a corrupt tree.
- **`grep` with BRE in a check script** treats `{3}` literally; a comment-only
  diff check built that way can never pass.

## Where to look

| Question | Source |
|---|---|
| The comment rule and the notes layout | `AGENTS.md`, "Code comments and code notes" |
| Giant functions: method and gates | `LLM/02.339-giant-functions.opus.md` |
| Module split map, per crate | `LLM/plans/2026-10-10-split-map.md` |
| Readability refactor decisions | `LLM/plans/2026-10-10-readability-refactor.review.md` |
| Per-symbol context and Portage citations | `LLM/code-notes/` |

## Provenance

The documents this skill was distilled from, and how many verified facts each
contributed: [`references/sources.md`](references/sources.md).
