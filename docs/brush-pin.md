# `brush` pin tracking

`portuale` embeds [`brush`](https://github.com/reubeno/brush) (`brush-core`
+ `brush-builtins`) as its Rust-native bash backend for real ebuild phase
execution — see [`agent-context.md`](agent-context.md)'s "The bash-execution backend" for why brush at all, and
[`what-this-proves.md`](what-this-proves.md)'s "Bash-execution backend" /
"`ebuild --shell bash|brush`" sections for how it is wired in.

`portuale/Cargo.toml` pins `brush-core` / `brush-builtins` **by exact
commit to the thin fork `vivo75/brush`** — upstream `reubeno/brush`
`main` plus the staged [`brush-pr/`](brush-pr/) fixes, nothing else (not a
crates.io release — the published `brush-core 0.5.0` predates
[#1274](https://github.com/reubeno/brush/pull/1274), which the eapi.sh
parser needs, and carries none of the staged fixes). This file records the pin and the periodic re-pin
checklist. **Keep it current whenever the pin changes** — and keep its
`[brush]` entry in **`3rdparty/repos.toml`** (the flat,
machine-parseable registry of every third-party ref this fork tracks,
portage's own upstream base included) in sync too.

## Current pin

> **2026-09-01**: dropped the `vivo75/brush` fork and moved to upstream
> `reubeno/brush` `main` (the two fixes the fork carried were resolved).
> **2026-09-05**: back on `vivo75/brush` — as a *thin* fork this time,
> not a divergent one: its `main` is upstream `reubeno/brush` `main`
> **plus the five [`brush-pr/`](brush-pr/) commits**, nothing else.
> Merge upstream into it periodically; drop it again once the PRs
> land.

| | |
|---|---|
| Repo | `https://github.com/vivo75/brush` (thin fork of `reubeno/brush`) |
| Rev | `b9524ad51de5c8231eb5dcaf79ca982841117385` |

`b9524ad5` = `reubeno/brush@25bffd54` + the five `brush-pr/` fixes,
cherry-picked in order (`840ea40f`, `1132297d`, `10a455d8`, `8850b943`,
`b9524ad5`; the per-bug branches carry the same patches as single commits
— `bc99e6c1`, `df830c59`, `962051c9`, `dfbca97c`, `2073877d`). Frozen in
`Cargo.lock` too (`brush-core` 0.5.0 / `brush-builtins` / `brush-parser`,
three `git+https://github.com/vivo75/brush?rev=b9524ad5…` source lines).

Re-pinned 2026-09-14 (Tier 1, Track B): upstream `main` moved `812336dd`
→ `25bffd54` (reedline 0.51; MSRV 1.95 for the interactive crates only)
and every fix branch was rebased onto it. This re-pin carries three new
fixes over the previous one: 02's quoted-here-tag terminator repair (B1
of `history/backlog_tier_1_sliced.opus.md`), the `source`-parse-error status fix
(B2), and IFS-independent brace expansion (B3). Verified on the new pin:
brush's own `brush-compat-tests` 2504 ran, 2023 succeeded / 0 unexpected
failures / 481 known-fail / 29 skipped — one previously-known failure
(`echo ~/{a,b}`) now passes and was unmarked; the ad-hoc eclass sweep
round-trips 2054 functions in all 211 eclasses plus one synthetic
function per quoted here-tag form with 0 failures (same sweep on
upstream `main`: 20 round-trip failures among 1407 functions, 41
eclasses never parsed). `cargo test --release -p portuale` and the full
pytest suite are green (details in the slice notes below). The five
staged fixes are still unmerged upstream (checked each branch tip), so
the thin fork stays; the per-bug `fix/*` branches are staged for the
unopened upstream PRs.

> The gitignored **`3rdparty/brush/` working checkout** tracks the same
> `main` (`origin` = `vivo75/brush`, `upstream` = `reubeno/brush`), plus
> the five per-bug branches
> `fix/tokenizer-nested-construct-heredoc` /
> `fix/declare-f-heredoc-serialization` /
> `fix/function-pipeline-stage-deadlock` /
> `fix/dot-parse-error-status` /
> `fix/brace-expansion-ifs-independent` staged for upstream submission.

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

1. In `3rdparty/brush` (`origin` = `vivo75/brush`, `upstream` =
   `reubeno/brush`): `git fetch upstream`, merge a recent
   `upstream/main` into the thin fork's `main` (the staged fixes stay on
   top), push `origin main`; update the `rev` in `portuale/Cargo.toml`
   (both `brush-core` and `brush-builtins`) to the merge commit. (Once
   the staged PRs have landed, pin a plain `reubeno/brush` `main` commit
   instead and drop the fork.)
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

**Response (2026-09-01):** the phase-execution default flipped from the
embedded `brush` backend to a real `bash` subprocess (`ShellBackend::
Bash`; `brush` stays available via `--shell brush`). See
[`what-this-proves.md`](what-this-proves.md), "`--shell` default is now
`bash`".

**Root-caused + fixed 2026-09-05** against `reubeno/brush` `main`
(`a250b84e`) — really *four* bugs there, later **five** per-bug branches
after the 2026-09-14 Track-B pass. Each is one commit on its own branch
in the `3rdparty/brush` checkout, staged for upstream submission
(`git format-patch` exports + write-ups in [`brush-pr/`](brush-pr/),
**PRs not yet opened**; every branch was rebased onto upstream
`25bffd54` on 2026-09-14):
`fix/tokenizer-nested-construct-heredoc` (`bc99e6c1`),
`fix/declare-f-heredoc-serialization` (`df830c59`),
`fix/function-pipeline-stage-deadlock` (`962051c9`),
`fix/dot-parse-error-status` (`dfbca97c`),
`fix/brace-expansion-ifs-independent` (`2073877d`). All five are
**in the pin** (first in `vivo75/brush@5af3f6c1` / portuale `8184c11`;
carried forward into every later pin, see "Current pin" above).

1. **tokenizer** — a `${…}` / `$(…)` / `$((…))` on a here-tag line has its
   sub-tokens stolen by the pending here-doc, so `"${base}.c"` tokenizes as
   `base` + `"${}.c"` (and `<<${VAR}`'s tag becomes `VAR`). An *execution*
   bug, not just serialization.
2. **AST `Display`** — the here-doc body is emitted inline (indented by the
   enclosing block, and before any later redirect on the same command)
   instead of deferred to column 0 after the line. Plus: multi-line words
   (`local x='…\n…'`) get re-indented every round-trip; `>(list)` renders
   with doubled parens; `|` / `>&` spacing. **Found 2026-09-14 (B1):** the
   deferred terminator was the *raw* tag word, quotes included
   (`<<'EOF'` → a terminator line `'EOF'`), and the command-line tag was
   not re-quoted the way a shell prints it (`<<"EOF"` / `<<\EOF` stay as
   written); both fixed in the same commit, with compat cases per quoting
   form.
3. **command exec** — a function used as a non-last pipeline stage runs
   inline to completion before the next stage is spawned → deadlocks past
   one pipe buffer (re-do of the never-merged #1276).
4. **`source` status** — a parse error inside a *sourced* file was marked
   fatal, so the `ExecutionResult` asked for `ExitShell` and the whole
   calling script stopped there (an embedded caller saw exit code 2 and no
   further execution). bash's `source`/`.` returns 2 and execution
   continues, so real `source "${T}/environment" || die` fires; brush now
   clears the fatal parse error's control flow at the `source` boundary
   only (a top-level/`-c` parse error stays fatal; `eval` is still a
   known separate divergence).
5. **brace expansion vs IFS** — brace expansion built one space-joined
   string and relied on field splitting to separate its alternatives, so
   under `IFS=`/`IFS=:` (e.g. after `local IFS`) `{A..C}` stayed a single
   word. Real `__filter_readonly_variables` builds bash's special-variable
   list with `printf '${!%s*} ' {A..Z} {a..z} _` *after* `local IFS`, so
   the list came back malformed and nothing was filtered — `BASHOPTS`,
   `EUID`, `PPID`, `SHELLOPTS`, `UID` were saved into `${T}/environment`
   and every later `source` printed `cannot mutate readonly variable`.
   `basic_expand` now expands each alternative separately, giving each its
   own field(s), IFS-independent as in bash (this also fixed the known
   failure `echo ~/{a,b}`).

Verified on the 2026-09-14 pin: brush's own `brush-compat-tests`
0 unexpected failures (one previously-known failure now passes and was
unmarked); the ad-hoc sweep over all 211 Gentoo eclasses round-trips 2054
functions + 5 synthetic quoted-tag functions with 0 parse-fail /
0 eval-fail / 0 non-idempotent (upstream `main` baseline: 20 round-trip
failures among 1407 functions, 41 eclasses never parsed); `cargo test
--release -p portuale` green against the new pin (incl. the
`install_does_not_deadlock…` guard and the new B2/B3 regressions).

**Still to do:** open the five upstream PRs; once merged, re-pin to
`reubeno/brush` directly (dropping the thin fork) and reconsider flipping
the `--shell` default back to `brush`.

## What is *not* tracked here

This was targeted spike-and-fix work, not an exhaustive brush ↔ bash
compat sweep. Real ebuilds/eclasses almost certainly exercise brush
incompatibilities not yet tried. New ones are their own slices — fix
upstream first, or (for portage-tree `bin/*.sh`) rewrite the offending
construct, `brush strategy #2` style — and get recorded here.

- **2026-09-13 — a compiled ebuild's `src_compile` no-ops under brush
  (#38 G3 smoke). FIXED 2026-09-14 (B1–B4).** `TEST/images/overlay/porttest/
  porttest/splitdebug` (a `src_compile` whose heredoc pattern is
  `cat > pt-sd.c <<-'EOF'` plus `tc-getCC`) under `emerge --shell brush
  --buildpkgonly` — or `ebuild --shell brush <fixture> install` — returned
  0 but produced an **empty image**: `work/` stayed empty, so `src_install`
  had nothing to `dobin`, alongside `error: declare: cannot mutate readonly
  variable` and `env: '': No such file or directory` noise. Root causes:
  the quoted-tag terminator defect in fix 02 (B1), the silent
  `source`-abort (B2) and the missing `$BASH` + IFS-dependent brace
  expansion (B3). Re-run on the 2026-09-14 pin: `emerge --shell brush
  --buildpkgonly porttest/splitdebug` exits 0 and the archive's
  `image.tar.zst` carries `usr/bin/pt-splitdebug`, `usr/lib64/libptsd.so*`
  and the splitdebug `.debug`/`.build-id` trees (12 KiB installed tree, not
  the old 1 KiB empty image); the same shape is pinned fixture-side by
  `dev-libs/heredocpkg` (Bash/Brush image-set equality test) and by the
  corrupt-saved-environment regression test. See `TEST/findings/l2.md`
  "#38 S2" and `docs/what-this-proves.md`'s Track-B slice note.

## References

- [`reubeno/brush`](https://github.com/reubeno/brush) — the embedded
  bash interpreter.
- [`shellgei/rusty_bash`](https://github.com/shellgei/rusty_bash) — an
  alternative Rust bash implementation, evaluated as a backend candidate
  (see [`agent-context.md`](agent-context.md), "The bash-execution backend").
