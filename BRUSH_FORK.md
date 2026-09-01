# `vivo75/brush` fork tracking

`portuale` embeds [`brush`](https://github.com/reubeno/brush) (`brush-core` +
`brush-builtins`) as its Rust-native bash backend for real ebuild phase
execution — see `PROMPT-next.md`'s own "bash-execution-backend question"
for why brush at all, and `README.md`'s "Bash-execution backend" /
"`ebuild --shell bash|brush`" sections for how it is wired in.

`portuale/Cargo.toml` pins `brush-core`/`brush-builtins` **by exact
commit** to the fork `https://github.com/vivo75/brush`, not to upstream
or a crates.io release. This file records why, what the fork carries, and
what to check before bumping the pin. **Keep it current whenever the pin
in `portuale/Cargo.toml` changes or an upstream PR moves.**

> Last reviewed: **2026-08-27**.

## Current pin

> Re-pinned **2026-09-01** from `c78ea429` (which also carried the
> fork-only pipeline deadlock fix, `reubeno/brush#1276`) down to
> `879d963` — the deadlock fix is no longer load-bearing for this tree
> after **brush strategy #2** rewrote `bin/phase-functions.sh` to keep
> `__save_ebuild_env`/`__filter_readonly_variables` out of any pipeline
> (see "Fix 2" below).

| | |
|---|---|
| Repo | `https://github.com/vivo75/brush` |
| Rev | `879d963458a3ee84124d839f922e19552881ae2c` |
| Fork branch it lives on | `fix/function-body-extended-test` (also an ancestor of `fix/pipeline-function-stage-deadlock`) |
| Ancestry | `879d963` (brace-less function bodies, fork copy of #1274) → `ec6fcb2` (upstream `main` at fork time) |

The pin is also frozen in `Cargo.lock` (three `git+https://github.com/vivo75/brush?rev=879d963…` source lines: `brush-core`, `brush-builtins`, `brush-parser`).

## Fixes carried, and their upstream status

### 1. Brace-less function bodies — `name() [[ … ]]`

- **What**: bash's function grammar allows the body to be *any* compound
  command, including an extended-test `[[ … ]]`. brush's parser only
  accepted `{ … }` / `( … )` / `(( … ))`. `bin/eapi.sh` defines ~60
  predicate functions this way (e.g.
  `___eapi_has_pkg_pretend() [[ ${1-${EAPI-0}} != [0-3] ]]`), and it is
  sourced unconditionally by `isolated-functions.sh` — so this single
  construct blocked brush from parsing essentially any real ebuild/eclass
  pipeline. Fix: new `CompoundCommand::ExtendedTest` AST variant,
  `compound_command()` grammar rule accepts it, interpreter handles it.
- **Upstream**: **MERGED** as
  [reubeno/brush#1274](https://github.com/reubeno/brush/pull/1274),
  merge commit **`18851e7`**, 2026-08-20, on `reubeno:main`.
- **Fork status**: the fork carries its own pre-merge copy of this change
  (`879d963`, on branch `fix/function-body-extended-test`) — this is now
  **the pin itself**. It is **functionally identical to post-`18851e7`
  upstream `main`**; a rebase of the pin onto that upstream would drop
  `879d963` and leave a plain upstream tracking pin (see "Rebase / bump
  checklist").

### 2. Pipeline function-stage deadlock — *no longer carried*

- **What it was**: a shell function used as a *non-last* pipeline stage
  ran inline in brush instead of as a concurrent task, so if it wrote
  more than the OS pipe buffer (~64 KiB on Linux) to stdout before
  returning, it deadlocked on that write forever — the next stage had
  not been spawned to drain the pipe. Hit live-testing real
  `app-arch/xz-utils` / `sys-fs/fuse` once the `multilib` eclass family
  was in scope.
- **Upstream**: still **OPEN** as
  [reubeno/brush#1276](https://github.com/reubeno/brush/pull/1276)
  ("fix(interp): run function pipeline stages as background tasks, not
  inline"), **no review yet**. brush's own bug regardless.
- **Why the pin no longer needs it (brush strategy #2, 2026-09-01)**:
  the only place *this repo's own* `bin/*.sh` hit the construct was
  three `__save_ebuild_env | __filter_readonly_variables [| bzip2]`
  pipes in `bin/phase-functions.sh` (both stages shell functions whose
  combined output routinely tops 64 KiB once a few eclasses are in
  scope). A new helper `__save_and_filter_ebuild_env` stages the two
  through a regular file in `${T}` — neither is a pipeline stage any
  more, so the construct is gone. `bash -n` clean, behaviourally
  identical for bash. **Verified**: the whole `portuale` test suite —
  including `install_does_not_deadlock_on_an_eclass_scope_larger_than_
  the_pipe_buffer` (the `bigeclasspkg` fixture, ~400 functions) — is
  green against `879d963`, which does **not** carry the #1276 patch
  (it hangs the 120 s deadline without the script rewrite).
- Real ebuilds/eclasses in the wild could still exercise the construct
  (a `pkg_*` function piped into something in an eclass, say) — that
  would want #1276 (or its own strategy-#2 rewrite). This closed only
  the portage-tree side.

## Rebase / bump checklist

The pin no longer carries a fork-only *behaviour* fix (see Fix 2) — it
carries only `879d963`, the fork's own copy of the upstream-merged
#1274. So the remaining move is a plain "track upstream" bump, do it
periodically (upstream `main` moves fast — it was at `#1298` by
2026-08-24, four days after #1274 merged).

1. **Preferred**: switch the `portuale/Cargo.toml` pin to **upstream**
   `reubeno/brush` at a commit at/after `18851e7` (the #1274 merge),
   drop `879d963` and the fork, and reduce this file to "we track
   upstream `main` at rev X" (or delete it).
2. If newer upstream turns out to have its own regressions, carry a
   fresh fork branch rebased onto current upstream `main` with only the
   needed fix, re-pin `portuale/Cargo.toml` + `Cargo.lock`, and record
   it here.
3. **Either way**, after re-pinning:
   - `cargo test -p portuale -- --test-threads=6` green;
   - a real end-to-end phase run still works — the pipeline-deadlock
     regression test (`install_does_not_deadlock_on_an_eclass_scope_
     larger_than_the_pipe_buffer`, `portuale`) is the guard;
   - `cargo test -p brush-shell --test brush-compat-tests` in a brush
     checkout of the new rev still passes its ~2,174 cases (brush's own
     bash-compat suite), if you have one handy.

## What is *not* a fork concern

Only two brush constructs were ever proven to matter here: brace-less
function bodies (Fix 1, upstream-merged, this pin's sole content) and
the pipeline function-stage deadlock (Fix 2, worked around in
`bin/phase-functions.sh` rather than in brush). Real ebuilds/eclasses
in the wild almost certainly exercise other bash constructs not yet
tried — this was targeted spike-and-fix work, not an exhaustive compat
sweep. New brush incompatibilities found later are their own slices
(fix upstream first, or rewrite the offending `bin/*.sh` — brush
strategy #2 — if it's portage-tree code), tracked here as they arise.
