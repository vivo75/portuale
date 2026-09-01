# `brush` pin tracking

`portuale` embeds [`brush`](https://github.com/reubeno/brush) (`brush-core`
+ `brush-builtins`) as its Rust-native bash backend for real ebuild phase
execution — see `PROMPT-next.md`'s "bash-execution-backend question" for
why brush at all, and `README.md`'s "Bash-execution backend" /
"`ebuild --shell bash|brush`" sections for how it is wired in.

`portuale/Cargo.toml` pins `brush-core` / `brush-builtins` **by exact
commit to real upstream `reubeno/brush` `main`** (not a fork, not a
crates.io release — the published `brush-core 0.5.0` predates
[#1274](https://github.com/reubeno/brush/pull/1274), which the eapi.sh
parser needs). This file records the pin and the periodic re-pin
checklist. **Keep it current whenever the pin changes** — and keep its
`[brush]` entry in **`PORTING/3rdparty/repos.toml`** (the flat,
machine-parseable registry of every third-party ref this fork tracks,
portage's own upstream base included) in sync too.

## Current pin

> **2026-09-01**: dropped the `vivo75/brush` fork entirely and moved to
> upstream `reubeno/brush` `main`. The fork had existed for two fixes;
> both are resolved now (see below).

| | |
|---|---|
| Repo | `https://github.com/reubeno/brush` (upstream) |
| Rev | `a04b09dc4a3f5beaa78899c4100734cf0f8f4472` (`main`, PR #1322) |

Frozen in `Cargo.lock` too (three `git+https://github.com/reubeno/brush?rev=a04b09dc…`
source lines: `brush-core`, `brush-builtins`, `brush-parser`).

## The two fixes the fork used to carry

### 1. Brace-less function bodies — `name() [[ … ]]`

bash's function grammar allows the body to be *any* compound command,
including an extended-test `[[ … ]]`. brush's parser only accepted
`{ … }` / `( … )` / `(( … ))`. `bin/eapi.sh` defines ~60 predicate
functions this way (`___eapi_has_pkg_pretend() [[ ${1-${EAPI-0}} != [0-3] ]]`)
and is sourced unconditionally by `isolated-functions.sh`, so this one
construct blocked brush from parsing essentially any real ebuild/eclass.

**MERGED upstream** as
[reubeno/brush#1274](https://github.com/reubeno/brush/pull/1274) (merge
commit `18851e7`, 2026-08-20) — an ancestor of the current pin. Nothing
to carry.

### 2. Pipeline function-stage deadlock

A shell function used as a *non-last* pipeline stage ran inline in brush
rather than as a concurrent task, so once it wrote more than the OS pipe
buffer (~64 KiB) to stdout before returning it deadlocked on that write
— the next stage that would drain the pipe was never spawned. Found
live-testing real `app-arch/xz-utils` / `sys-fs/fuse` once the `multilib`
eclass family was in scope.

Still **OPEN upstream** as
[reubeno/brush#1276](https://github.com/reubeno/brush/pull/1276) (no
review yet) — a real brush bug regardless. **Not load-bearing here**:
`brush strategy #2` (2026-09-01) rewrote the three places portage's own
`bin/*.sh` hit the construct — `__save_ebuild_env |
__filter_readonly_variables [| bzip2]` in `bin/phase-functions.sh` — so
the new `__save_and_filter_ebuild_env` helper stages the two functions
through a `${T}` temp file and neither is ever a pipeline stage. See
`README.md`'s "brush strategy #2" section.

**Guard**: `ebuild_phases::tests::install_does_not_deadlock_on_an_eclass_
scope_larger_than_the_pipe_buffer` (`portuale`), driven by the
`bigeclasspkg` fixture (~400 functions, ~80 KB saved environment). It
completes in ~1 s against a brush without the #1276 patch; it *hangs the
120 s deadline* if `bin/phase-functions.sh` is reverted to the pipe
form.

A real ebuild/eclass in the wild could still pipe a `pkg_*` function into
something — that path would still want #1276 upstream (or its own
strategy-#2 rewrite). Strategy #2 closed the portage-tree side only.

## Re-pin checklist (periodic — upstream `main` moves fast)

1. `cd` a `reubeno/brush` checkout, `git fetch`, pick a recent `main`
   commit; update the `rev` in `portuale/Cargo.toml` (both `brush-core`
   and `brush-builtins`).
2. `cargo update -p brush-core --precise <rev>` (or just `cargo build`
   and let it re-resolve), commit the `Cargo.lock` change.
3. Verify:
   - `cargo fmt --check`, `cargo clippy --release` (zero warnings);
   - `cargo test --release -p portuale` green — the deadlock guard above
     is the one that matters, plus every real end-to-end phase test;
   - optionally `cargo test -p brush-shell --test brush-compat-tests` in
     the brush checkout of the new rev (brush's own ~2,174-case
     bash-compat suite).
4. If a newer `main` turns out to have a regression that blocks a real
   phase run: bisect it, report upstream, and pin to the last-good
   `main` commit (still upstream, still a plain rev pin) until it's
   fixed. Only re-introduce a fork if upstream won't take the fix and
   it's genuinely blocking.

## What is *not* tracked here

This was targeted spike-and-fix work, not an exhaustive brush ↔ bash
compat sweep. Real ebuilds/eclasses almost certainly exercise brush
incompatibilities not yet tried. New ones are their own slices — fix
upstream first, or (for portage-tree `bin/*.sh`) rewrite the offending
construct, `brush strategy #2` style — and get recorded here.
