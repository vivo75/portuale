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

| | |
|---|---|
| Repo | `https://github.com/vivo75/brush` |
| Rev | `c78ea42965023fe0c1b2b708939c44228f262f03` |
| Fork branch it lives on | `fix/pipeline-function-stage-deadlock` |
| Ancestry | `c78ea429` (deadlock fix) → `879d963` (brace-less function bodies, fork copy of #1274) → `ec6fcb2` (upstream `main` at fork time) |

The pin is also frozen in `Cargo.lock` (three `git+https://github.com/vivo75/brush?rev=c78ea429…` source lines: `brush-core`, `brush-builtins`, `brush-parser`).

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
  (`879d963`, on branch `fix/function-body-extended-test`) as an ancestor
  of the pin. It is **functionally redundant with upstream `main`** now —
  a rebase of the pin onto post-`18851e7` upstream would drop `879d963`
  entirely and keep only fix #2. Not yet done (see "Rebase / bump
  checklist").

### 2. Pipeline function-stage deadlock

- **What**: a shell function used as a *non-last* pipeline stage ran
  inline in brush instead of as a concurrent task, so if it wrote more
  than the OS pipe buffer (~64 KiB on Linux) to stdout before returning,
  it deadlocked on that write forever — the next stage had not been
  spawned to drain the pipe. Hit live-testing real
  `app-arch/xz-utils` / `sys-fs/fuse` once the `multilib` eclass family
  was in scope (after the eclass `inherit()` support landed). Fix
  (`brush-core/src/commands.rs`, +72): the owned-shell path spawns the
  function via `tokio::task::spawn_blocking`, mirroring
  `execute_via_builtin_in_owned_shell`; the parent-shell path (only the
  pipeline's final stage) keeps running inline. Regression case in
  `brush-shell/tests/cases/compat/pipeline.yaml`.
  See `README.md`'s own "Root-caused down to a real bug in the pinned
  `brush` fork" writeup for the full grounding.
- **Upstream**: **OPEN** as
  [reubeno/brush#1276](https://github.com/reubeno/brush/pull/1276)
  ("fix(interp): run function pipeline stages as background tasks, not
  inline"), opened 2026-08-18, 1 commit (`8e8be9a2`), **no review yet**.
  The PR commit is a separately-authored version of the same fix, not
  literally the pinned `c78ea429`.
- **Fork status**: **fork-only** — this is the reason the pin exists.
  The fork also has a `fix/pipeline-function-stage-deadlock2` branch
  (an alternative take on the same fix); the pin is on the non-`2` one.

## Rebase / bump checklist

Do this when #1276 merges, or periodically (upstream `main` moves fast —
e.g. it was at `#1298` by 2026-08-24, four days after #1274 merged).

1. **If #1276 merged** and no other fork-only fix is needed: switch the
   `portuale/Cargo.toml` pin to **upstream** `reubeno/brush` at a commit
   at/after both `18851e7` and the #1276 merge, delete this section's
   "fork-only" note, and consider dropping this file (or reducing it to
   "we track upstream `main` at rev X").
2. **If #1276 is still open** but you want newer upstream: on the fork,
   rebase `fix/pipeline-function-stage-deadlock` onto current upstream
   `main` (this drops the now-merged `879d963`), force-push, re-pin
   `portuale/Cargo.toml` + `Cargo.lock` to the new fork head, update
   the "Current pin" table above.
3. **Either way**, after re-pinning:
   - `cargo test -p portuale -- --test-threads=6` green;
   - a real end-to-end phase run still works — the pipeline-deadlock
     regression test (`install_does_not_deadlock_on_an_eclass_scope_
     larger_than_the_pipe_buffer`, `portuale`) is the guard;
   - `cargo test -p brush-shell --test brush-compat-tests` in a brush
     checkout of the new rev still passes its ~2,174 cases (brush's own
     bash-compat suite), if you have one handy.

## What is *not* a fork concern

Only these two constructs have been proven fixed against brush. Real
ebuilds/eclasses in the wild almost certainly exercise other bash
constructs not yet tried — this was targeted spike-and-fix work, not an
exhaustive compat sweep. New brush incompatibilities found later are
their own slices (fix upstream first, carry on the fork only if
blocking), tracked here as they arise.
