# `brush` pin tracking

`portuale` embeds [`brush`](https://github.com/reubeno/brush) (`brush-core`
+ `brush-builtins`) as its Rust-native bash backend for real ebuild phase
execution — see [`agent-context.md`](agent-context.md)'s "bash-execution-backend
question" for why brush at all, and
[`what-this-proves.md`](what-this-proves.md)'s "Bash-execution backend" /
"`ebuild --shell bash|brush`" sections for how it is wired in.

`portuale/Cargo.toml` pins `brush-core` / `brush-builtins` **by exact
commit to real upstream `reubeno/brush` `main`** (not a fork, not a
crates.io release — the published `brush-core 0.5.0` predates
[#1274](https://github.com/reubeno/brush/pull/1274), which the eapi.sh
parser needs). This file records the pin and the periodic re-pin
checklist. **Keep it current whenever the pin changes** — and keep its
`[brush]` entry in **`3rdparty/repos.toml`** (the flat,
machine-parseable registry of every third-party ref this fork tracks,
portage's own upstream base included) in sync too.

## Current pin

> **2026-09-01**: dropped the `vivo75/brush` fork entirely and moved to
> upstream `reubeno/brush` `main`. The fork had existed for two fixes;
> both are resolved now (see below).

| | |
|---|---|
| Repo | `https://github.com/vivo75/brush` (upstream) |
| Rev | `5af3f6c1869550389a9254be43b0448667a90365` (`main`, PR #1322) |

Frozen in `Cargo.lock` too (three `git+https://github.com/vivo75/brush?rev=5af3f6c1…`
source lines: `brush-core`, `brush-builtins`, `brush-parser`).

> The **`3rdparty/brush/` working checkout** (gitignored, cloned per
> `3rdparty/repos.toml`) is deliberately *ahead* of this pin — as of
> 2026-09-05 it sits at upstream `main` `a250b84e` (`brush-v0.4.0-136`),
> with `origin` = `vivo75/brush`, `upstream` = `reubeno/brush`. It is
> where the [`brush-pr/`](brush-pr/) fixes are being developed; the pin
> only moves once they land upstream.

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
__filter_readonly_variables [| bzip2]` in `phase-functions.sh` — so the
new `__save_and_filter_ebuild_env` helper stages the two functions
through a `${T}` temp file and neither is ever a pipeline stage. The
change lives in the **vendored** `bin/phase-functions.sh`
(`ebuild_phases::bin_dir()` overlays `bin/` over the checkout's
`bin/`; the upstream file stays pristine — see
`3rdparty/repos.toml`'s `vendored_paths`). See
[`what-this-proves.md`](what-this-proves.md)'s "brush strategy #2" section.

**Guard**: `ebuild_phases::tests::install_does_not_deadlock_on_an_eclass_
scope_larger_than_the_pipe_buffer` (`portuale`), driven by the
`bigeclasspkg` fixture (~400 functions, ~80 KB saved environment). It
completes in ~1 s against a brush without the #1276 patch; it *hangs the
120 s deadline* if `bin/phase-functions.sh` is reverted to the
pipe form.

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

## Open brush bugs found in real-world testing

### `declare -f` mangles a function with a redirected here-document

**Found 2026-09-01** running `emerge -v app-portage/eix` against a real
Gentoo tree. brush's `declare -f` (function serializer) corrupts any
function body containing `cmd <<-EOF > file`:

```bash
f() { cat <<-EOF > "${base}.c"
	#include <omp.h>
	EOF
}
```

bash's `declare -f f` round-trips this exactly. brush emits the redirect
**after** the heredoc body as `> base "${}.c"` — the `${base}` parameter
name dropped entirely — and re-indents the `<<-` body/terminator with
spaces, so `EOF` no longer terminates the heredoc.

**Impact:** `__save_ebuild_env` runs `declare -f` on every in-scope
function between phases. `toolchain-funcs.eclass`'s `_tc-has-openmp`
(and others) trips this → the written `${T}/environment` is unparseable
→ the next phase's `source "${T}/environment" || die` aborts. Breaks a
real `emerge <atom>` for essentially every compiled package.

**Response:** the phase-execution default flipped from the embedded
`brush` backend to a real `bash` subprocess (`ShellBackend::Bash`;
`brush` stays available via `--shell brush` / `--shell=brush`). See
[`what-this-proves.md`](what-this-proves.md), "`--shell` default is now
`bash`".

**Root-caused + fixed 2026-09-05** against `reubeno/brush` `main`
(`a250b84e`) — it is really *four* bugs, staged as PRs in
[`brush-pr/`](brush-pr/) (write-ups + `git format-patch` exports, **not yet
submitted**). In the `3rdparty/brush` checkout: one commit per bug on
`fix/tokenizer-nested-construct-heredoc` (`bd4793ab`) /
`fix/declare-f-heredoc-serialization` (`3d2bde47`) /
`fix/function-pipeline-stage-deadlock` (`ca11d652`), and **`main` = `a250b84e`
+ all three cherry-picked** (tip `de451b39`) — a working combined branch (still
needs a push + `Cargo.toml` re-pin before portuale builds against it; that also
pulls ~130 commits of upstream drift, so do it as its own slice).

1. **tokenizer** — a `${…}` / `$(…)` / `$((…))` on a here-tag line has its
   sub-tokens stolen by the pending here-doc, so `"${base}.c"` tokenizes as
   `base` + `"${}.c"` (and `<<${VAR}`'s tag becomes `VAR`). An *execution*
   bug, not just serialization.
2. **AST `Display`** — the here-doc body is emitted inline (indented by the
   enclosing block, and before any later redirect on the same command)
   instead of deferred to column 0 after the line. Plus: multi-line words
   (`local x='…\n…'`) get re-indented every round-trip; `>(list)` renders
   with doubled parens; `|` / `>&` spacing.
3. **command exec** — a function used as a non-last pipeline stage runs
   inline to completion before the next stage is spawned → deadlocks past
   one pipe buffer (re-do of the never-merged #1276).

Verified: full brush-compat-tests suite 0-regressions (+5 new cases);
every function in all 211 Gentoo eclasses (1843 fns) now round-trips
`declare -f` → `eval` → `declare -f` with 0 parse-fail / 0 eval-fail /
0 non-idempotent.

**Still to do:** submit the three PRs; once merged, re-pin
`portuale/Cargo.toml` and (optionally) flip the `--shell` default back.

## What is *not* tracked here

This was targeted spike-and-fix work, not an exhaustive brush ↔ bash
compat sweep. Real ebuilds/eclasses almost certainly exercise brush
incompatibilities not yet tried. New ones are their own slices — fix
upstream first, or (for portage-tree `bin/*.sh`) rewrite the offending
construct, `brush strategy #2` style — and get recorded here.

## References

- [`reubeno/brush`](https://github.com/reubeno/brush) — the embedded
  bash interpreter.
- [`shellgei/rusty_bash`](https://github.com/shellgei/rusty_bash) — an
  alternative Rust bash implementation, evaluated as a backend candidate
  (see [`agent-context.md`](agent-context.md), "bash-execution-backend
  question").
