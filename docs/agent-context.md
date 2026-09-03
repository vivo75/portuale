# Agent context: portuale, a Rust reimplementation of Portage

This file (`docs/agent-context.md`, formerly `PROMPT-next.md`) is the
single entry point for (re)deriving where portuale stands and what to
do next, without repeating the discovery conversations that produced it.
It merges: the original porting-strategy prompt (goals, hard constraints,
architectural decisions — the standalone historic copy is
[`history/porting-strategy-prompt.md`](history/porting-strategy-prompt.md)),
the phase-execution/bash-backend investigation, the current shipped
state, and the open backlog. The session-to-session operating rhythm
lives in [`../AGENTS.md`](../AGENTS.md).

As with any settled decision below: if you disagree, say so explicitly
and re-open it — don't silently override it.

## Context

Portage (this repository) is the Gentoo package manager, written in Python.
Portuale is a Rust reimplementation of it, developed as a **friendly
fork**: a separate, cooperating codebase, not a hostile competitor. The
aim is a **real, drop-in, same-behaviour replacement** (and then some) —
reached one reviewed, contract-tested slice at a time. It is not there
yet; `scope-backlog.md` is the honest distance-to-parity.

**EAPI floor**: EAPI 0, 1, 2, 3, 4, and 6 are deprecated and removed in
this repo/fork — no ebuild uses them, and all profiles are EAPI 5 or
higher (5, 7, 8 are the live versions). Any EAPI-conditional logic being
read or ported only needs to account for EAPI 5+ as the real, live
baseline — branches that only apply to EAPI 0/1/2/3/4/6 are dead code and
can be ignored rather than faithfully ported. (Portuale's own `portage-*`
crates go further, as a deliberate simplification confirmed with the user:
no EAPI parametrization at all within the 5+ floor — every EAPI in that
range is treated identically. See `what-this-proves.md` for the many places
this precedent is invoked.)

## Team structure

- A Python team continues to own and evolve the existing Python codebase.
- A separate Rust team builds and owns the Rust implementation.
- The two teams work independently, each writing idiomatic code in their
  own language. Do not force Rust to mimic Python's structure line-for-line
  — that trades away idiomatic Rust for a cosmetic diffability that doesn't
  hold up in practice.

## Hard goals (non-negotiable)

1. **Portability of change, not of source.** After the initial port, a
   behavior change made in either codebase must be reproducible in the
   other. The mechanism for this is a **shared, jointly-owned Python
   test suite acting as an executable behavioral spec** — not structural
   mirroring of source code. A change lands with new/updated test cases;
   the other implementation is "in sync" when it passes them, regardless
   of how differently it's implemented internally.
2. **Rust must be measurably faster than Python**, not just assumed faster
   because it's Rust. This must be proven by benchmarks, tracked over time
   in CI as a regression gate (not a one-time claim).
3. **The Rust binary must run on a minimal Linux system**: statically
   linked (musl target), zero dynamic runtime dependencies, no assumption
   of glibc or a package manager being present. Prefer pure-Rust
   dependencies; avoid dynamically-linked C libraries.
4. **Tests are written in Python for both implementations.** Black-box,
   driven via CLI/subprocess against each implementation's executable(s)
   — not white-box bindings into Rust internals. This keeps the contract
   suite implementation-agnostic and neutral to whatever the long-term
   architecture turns out to be.

## Open / deliberately undecided

- **The end state is a real, complete, usable Portage** — a drop-in
  same-behaviour replacement, and then some. What's still open is whether
  it *replaces* Python Portage in Gentoo or stands permanently alongside
  it (like `uutils` vs GNU coreutils). Do not pick an architecture that
  forecloses either: subprocess/CLI-based testing keeps both open,
  in-process FFI embedding (e.g. PyO3) would not — avoid it.

## Scope of the first port

1. **Core library**: version comparison (`portage.versions`), atom/dep
   parsing and matching, config resolution, dependency graph (depgraph).
2. **`emerge` and `ebuild` executables.** The first slice was deliberately
   restricted to **dry-run / read-only** behavior (dependency resolution,
   `--pretend` output, parsing and validation) to limit blast radius while
   the parity test suite was still young. **That restriction no longer
   holds**: real ebuild phase execution and filesystem-mutating
   merge/unmerge have since shipped too (see "Real ebuild phase execution
   + filesystem merge" below) — it's live, exercised by real fixtures, and
   has its own ongoing backlog, not a deferred future phase anymore.

### `emerge`/`ebuild` binary shape

Ship `emerge` and `ebuild` as **one multicall binary** (busybox-style),
dispatching behavior based on `argv[0]` via symlinks/hardlinks pointing at
a single executable. This is both a good minimal-Linux fit (one static
binary, no duplicated code) and drop-in compatible with tooling that
invokes `emerge`/`ebuild` by name directly. **Shipped**: `rust/portuale`.
A bare `portuale` (or `portuale --help`/`-h`) lists the applets with a
one-line description and exits 0; an unrecognized applet name still
errors. `emerge --help` is a grouped tour of every action/option portuale
implements (`pretend.rs`'s `HELP_TEXT`, mirrored in
`emerge_pretend_reference.py` and pinned by the contract suite).

## Test/benchmark harness architecture

- For pure-library-level parity (versions, atom parsing, etc.), define a
  neutral **CLI test-harness binary** on each side (not the real product
  CLI) exposing the library surface as subcommands, with an identical
  argv/output contract between the Python and Rust harnesses.
- For `emerge`/`ebuild`, black-box test against the **real CLIs directly**
  (with symlinks set up in the test `PATH` so multicall dispatch is
  exercised as in real usage), since they're in scope as actual products,
  not just internal library surface.
- The harness needs **two modes**:
  - *Correctness mode*: one operation per process invocation, pytest-driven,
    exhaustive edge cases.
  - *Benchmark mode*: batch input (many operations per single process
    invocation) to avoid fork/exec overhead dominating the measurement.
- Benchmark data: a **real, vendored Gentoo tree snapshot** (not purely
  synthetic stress data) — realistic scale and distribution of versions/
  atoms/deps. `bench/extract_snapshot.py` refreshes
  `gentoo_snapshot.json` against a live tree using real
  `portage.versions.pkgsplit` as the authority.
- CI gates on both: correctness suite must pass on both implementations;
  benchmark suite must show Rust ahead of Python and must not regress
  over time.
- Rust CI also gates on a **musl static build** smoke-tested inside a
  minimal (`scratch`/busybox-level) container.

## Ownership

- Python team: `pym/portage` core + the Python-side test harness.
- Rust team: the Rust crate + the Rust-side test harness.
- The **shared pytest contract suite is jointly owned** (separate repo or
  shared submodule) — neither team may unilaterally narrow it to make
  their side pass.

## Current state (read `what-this-proves.md` for the authoritative, living detail)

Both major phases of this pilot are live — re-verify against `git
log`/`what-this-proves.md` before trusting this paragraph for long, since
it decays fast.

**Dry-run (`emerge --pretend`)**: full recursive DEPEND/RDEPEND/BDEPEND/
PDEPEND/IDEPEND resolution; profile/make.conf-derived USE/ACCEPT_KEYWORDS
with the real `USE_ORDER` precedence for the full
`env.d`/`repo`/`features`/`pkginternal`/`defaults`/`conf`/`pkg`/`env`
layer chain (`env.d` from `/etc/profile.env`, the lowest tier, added
2026-09-03 — Part 2.C is now complete); every `package.*` file
(`.mask`/`.unmask`/`.accept_keywords`/`.use`/`.use.mask`/`.use.force`/
`.use.stable.mask`/`.use.stable.force`), repo-scoped across main **and**
overlay repos; `package.provided` (a listed CPV satisfies a dependency
atom silently / triggers the real `WARNING: … package.provided:` block
for a direct target); explicit `repos.conf` `masters =` parsing (not just
the implicit main-repo default); cross-repo profile parents;
bare command-line names (`emerge eix` → `app-portage/eix`, real
`dep_expand`/`cpv_expand`, ambiguity → the real `!!! ... ambiguous`
block, added 2026-09-03); `USE_EXPAND`;
REQUIRED_USE; blockers; slot conflicts (sub-slots, slot operators);
slot-aware installed matching with the `[ebuild NS]` new-slot marker;
the `[ebuild I..]` interactive (`PROPERTIES=interactive`) bracket column;
the `-pv` `Total: N packages (…)` / `Conflict:` counters summary line;
the `[ebuild ..f]`/`[ebuild ..F]` `RESTRICT=fetch` bracket column, and
`-pv`'s `Size of downloads` / `Fetch Restriction:` counters lines
(completing `_PackageCounters`); the `-pv` `USE=` line's enabled-first
order + `--alphabetical` + `all_flags` (always on for `-pv`: the diff
shows every flag, plain for unchanged, `(-flag%)` for one dropped from
IUSE); the real `PkgAttrDisplay` fixed-width bracket field
(`[I][N/r][S/R][f/F/g][U][D]` + a 7th mask column at `-v`) and the
`[old-ver]` column replacing the `(upgrade from X)` / `(reinstall for …)`
prose (increment 1 of the `-pv` real-`output.py` layout + ANSI-colour
buildout — see README's own two `-pv layout + colour` bullets; increment 2
(the `\x1b[` colour primitive + `--color=y|n` gating via new
`portuale/src/color.rs` + the coloured bracket line, `pkgprint`
world/system palette), increment 3 (USE-flag colours), and increment 4
(counters-line `interactive`/fetch colour + `-pC`/`-pc`/`-pP`
cleanup-action colour), and increment 5 (blocker line: real
`output.py::_blockers` `[blocks B     ]` layout + `PKG_BLOCKER` red
colour + deferred "print after every package line" ordering, new
`dev-libs/blockerorderpkg` fixture) all shipped 2026-08-29 — the `-pv`
layout + colour buildout is complete bar `--autounmask` message colour,
its own future slice); verbosity-3
`:slot`/`::repo` decoration of the bracket cpv + every `[old-ver]` (real
`_append_slot`/`_append_repository`/`convert_myoldbest`,
`GraphEntry::sub_slot`/`repo_name`/`oldbest`) shipped 2026-08-29 too; the
`g` remote-binpkg bracket column shipped 2026-08-29 with the `--pretend`
half of `--getbinpkg`/`--getbinpkgonly` (`binrepos.conf` +
`PORTAGE_BINHOST` parsing via new `portage-profile` `BinRepo`/
`parse_binrepos`, remote binhost `Packages`-index candidates via
`portage-repo` `list_remote_binary_candidates`, `Size of downloads:` from
the index `SIZE`), completing the `-pv` output arc: the real
`--autounmask` block shipped for both the keyword and USE kinds
2026-08-30 (`emerge --pretend [--autounmask] <blocked-pkg>` resolves the
graph with the implicit `=cpv ~arch` / `package.use` flip applied +
prints real `_display_autounmask`'s `The following <X> changes are
necessary to proceed:` block, exit 0; `-pv`'s `USE=` line reflects the
USE flip);
multiple/versioned/slotted atoms; USE-deps including the `opt=`/`opt?`
conditional forms; `--update`/`--deep`/`--emptytree` (`-e`: forces deep,
clears selective, every installed atom in the tree -> a bare
`[ebuild R]` reinstall -- for comparison with real portage +
debugging)/`--newuse`/`--changed-use`/
`--changed-deps`/`--changed-slot`/`--changed-deps-report`/`--with-bdeps`/
`--with-test-deps`/`--selective`/`--noreplace`/`--onlydeps`/`--nodeps`/
`--exclude`/`--deselect`/`--unmerge`/`-C` (full `_unmerge_display`:
selected/omitted/protected, sys-apps/portage self-skip, system-profile +
still-listed-in-sets warnings, a literal
`/var/db/pkg/cat/pkg-ver[/pf.ebuild]` path arg — **and, without
`--pretend`, a REAL removal now**: `pretend.rs::execute_unmerge` →
`ebuild_merge::unmerge_one_installed` per selected version
(`pkg_prerm` from vdb-saved env → files → `pkg_postrm` → vdb dir),
`>>> Unmerging (N of M)` lines, then `deselect_from_world` (real
`WorldSelectedPackagesSet.cleanPackage`); the `requires --pretend` gate
is gone)/`--depclean`/
`-c` (no-args full form AND the `--depclean <atoms>`
narrowing — the RDEPEND/PDEPEND/DEPEND/BDEPEND reachability closure
from `@world`+`@system` (build-time deps kept, real bdeps="auto" for
remove mode), the cleanlist in real topological removal order
(`topological_removal_order`, incl. the `runtime_slot_op` edge-priority
bump and the cycle-breaking single-node pop as of 2026-08-31), the stats
block; `--verbose` reverse-dep display
(`show_parents`: `<cpv> pulled in by: <parent> requires <atom>`);
`--depclean-lib-check` (the `NEEDED.ELF.2` soname-consumer scan — a
cleanlist pkg a surviving binary still links against is kept, via a
second cleanlist pass; the `* …will not be removed` WARNING; `=n`
skips it + shows the `Depclean may break link level dependencies`
advisory; wires up the previously-dead `needed_elf` module); the
"dependencies could not be completely resolved" safety halt (real
`unresolved_deps()` — a kept pkg's unsatisfiable hard runtime dep
(`RDEPEND`/`PDEPEND`) prints the `bad(" * ")` block and exits 1 without
removing anything; `||`-group + libc-provider atoms narrowed out);
**without `--pretend`: real removal** via `execute_unmerge`, stats line
`Number removed:`
)/`--prune`/`-P` (`prune_cleanlist` — non-highest
versions of multi-version cps, kept if still needed; no advisory/stats
block; `--verbose` `show_parents` display; `--depclean-lib-check` too;
`--nodeps` = the `_unmerge_display` prune branch, no dep check at all
(`prune_nodeps_selection`); **without `--pretend`: real removal** too)/`--config`
(real `action_config` — one atom, vdb-matched; `Configuring pkg...` +
real `pkg_config` from the vdb-saved env via
`ebuild_merge::run_vdb_saved_env_phase`; ignores `--pretend`; `--ask`
picker cut)/`--alphabetical` (the `-pv` `USE=`
line is enabled-first by default now, real `_create_use_string`;
`--alphabetical` gives the one interleaved list)/`--autounmask`/`--autounmask-use`/
`--autounmask-keep-keywords`/`--newrepo`/`--rebuilt-binaries`/
`--buildpkgonly`/`--usepkg-exclude`/real `--tree` nested display/
`--root-deps` (v1 scope: running-root existence check only, see backlog);
binary package support (`--usepkg`/`--usepkgonly`/`--binpkg-respect-use`);
a `--json` mode with a per-entry mask/unmask/keyword *provenance*
state-change trace; full CLI-surface recognition for both `emerge` and
`ebuild`.

**Real execution (filesystem-mutating)**: `ebuild <file> install` runs the
real 8-phase chain driving unmodified `bin/*.sh` — by default via a real
`bash` subprocess, optionally via the embedded `brush` (`--shell brush`;
the default flipped from `brush` to `bash` on 2026-09-01 after brush's
`declare -f` was found to corrupt real eclass functions — see
`what-this-proves.md`, "`--shell` default is now `bash`", and
`brush-pin.md`); `ebuild <file> merge` really copies `${D}` into
`${ROOT}` and writes a real vdb entry, with real `CONFIG_PROTECT`
(`obj`/`sym` entries, `NOCONFMEM`, `new_protect_filename` file reuse),
`FEATURES=collision-protect`/`protect-owned`, preserve-libs collision
exclusion, real blocker exclusion, and `env_update()`/`ldconfig`
triggering; `ebuild <file> qmerge` does the same minus a redundant
`install` re-run, gated on the real `${PORTAGE_BUILDDIR}/.installed`
marker; `ebuild <file> unmerge` really removes a package, including
the `others_in_slot` reverse-dependency check, its own "symlink
orphan" refinement (bug #326685/#640058), real
`FEATURES=unmerge-orphans`, real `INFOPATH` cleanup, real
`stale_confmem` cleanup, and real preserve-libs registration
(a still-needed shared library survives unmerge and the real
`preserved_libs_registry` is updated, not just the earlier merge-side
collision exclusion); standalone
`ebuild <file> config`/`info`/`prerm`/`postrm` really run the real
`pkg_config`/`pkg_info`/`pkg_prerm`/`pkg_postrm` phase functions, no
merge/unmerge/vdb step involved; `ebuild <file> package` builds a real
binpkg, real `PORTAGE_COMPRESSION_COMMAND` resolution (all six real
compressors, `BINPKG_COMPRESS` defaulting to real `"zstd"`); `emerge
--buildpkgonly` (without `--pretend`) really builds; a plain `emerge
<atom>` (no `--pretend`) really builds **and merges** from source
(`emerge_build::run_source_merge`; `New` + `Upgrade`/`Downgrade`/
`Reinstall` — an in-place same-slot replace unmerges the old version via
`ebuild_merge::unmerge_replaced_same_slot`, shared with `merge_binpkg`
and `ebuild <file> merge`); and `emerge --getbinpkg`/`--getbinpkgonly`
merges a mix of binary and source entries per the resolver's plan
(`emerge_getbinpkg::run_merge_plan`); real `SRC_URI`
fetch (including `mirror://`/`custommirrors` resolution and real
`FEATURES=distlocks` file locking) via real `wget`; real eclass
`inherit()` support; `ebuild --shell bash|brush` and `emerge --shell
bash|brush` (pilot-only flags) pick the execution backend explicitly —
`emerge`'s covers every real phase chain it can drive as of 2026-09-02
(source/binpkg merge, the `pkg_prerm`/`pkg_postrm` removal hooks under
`-C`/`--unmerge`/`--depclean`/`--prune`/`--clean`/`--rage-clean`, and
`emerge --config`'s `pkg_config`).
As of 2026-09-01 `emerge -v app-portage/eix` completes a full real merge
against a live `~amd64` tree (real `eautoreconf`/`./configure`/`make`/
`make install` → vdb entry; `qlist -I` agrees) — see `what-this-proves.md`,
"`PORTAGE_PYM_PATH` is now set".

**Backtracking (resolver retry loop)** — the `--autounmask*` family is
fully shipped, and as of 2026-09-01 **slice 1 of real backtracking**:
`resolve_pretend_graph` is now a `'backtrack` retry loop (real
`_emerge/resolver/backtracking.py` shape) — each pass rebuilds the graph
from scratch, and a **solvable slot conflict** (one version of the
conflicted `cat/pkg` satisfies every parent atom that landed on the slot)
folds those atoms into `slot_constraints`, fed to `resolve_pretend`'s new
`extra_constraints` param, and the whole walk re-runs (up to
`MAX_BACKTRACK = 10`). Unsolvable conflicts, and anything still
conflicting after 10 passes, fall through and are reported as before.
**Slice 2 (2026-09-01)** added the real `--backtrack=COUNT` flag
(`backtrack_max` param, default 10, `--backtrack=0` disables). **Slice 3
(2026-09-01)** added the real `runtime_pkg_mask`: `extra_constraints`
gained a `!`-negation form, `resolve_pretend_graph` tracks `slot_pullers`
and runs a trial-and-revert state machine — on an unsolvable slot
conflict it masks the conflicted `cpv` + every puller-parent version with
a lower alternative, re-runs, and keeps the masks only if every conflict
clears with no new `NoVisibleCandidate`. **Slice 4 (2026-09-01)** replaced the compact `[slot conflict]` line with
a simplified transcription of real `_show_slot_collision_notice` →
`slot_conflict_handler.get_conflict()`: the `!!! Multiple package
instances …` block (`SlotConflict.instances` = every conflicting version
+ its `(parent_cpv, atom)` pullers, via `build_slot_conflict`) + the
advisory paragraph with the `--backtrack=30` hint gated the real way.
Deferred (see `scope-backlog.md` Part 2.A): "backtracking
exhausted" / "circular dependencies" diagnostics, autounmask levels tried
in sequence inside the loop, the `resolve_graph_once` helper extraction
(drop slice 1's `loop {}` reindent), and real `get_conflict()`'s
`collision_reasons` grouping / `--verbose-conflicts` markers / stderr.

`what-this-proves.md` is the incrementally-
updated record of every shipped slice, each grounded in cited real Python
source — read that, not this list, for current detail, and `git log` for
how work has actually been landing (one small, fully-shipped, documented-
and-tested slice at a time — see "How this pilot actually runs" below).

### Open backlog

This section previously carried a per-slice "recently closed" / shipped
narrative that had grown to ~700 lines — the same drift `scope-backlog.md`
fought, and the same fix: the narrative is gone. It is snapshotted at
[`history/agent-context-open-backlog-2026-09-03.md`](history/agent-context-open-backlog-2026-09-03.md).

- **What has shipped** — `what-this-proves.md` (the living, cited-source
  per-slice record) and `git log` (one small, fully-shipped,
  documented-and-tested slice at a time).
- **What is genuinely still open** — real portage behaviour not ported to
  either side, the deliberate cuts, the standing non-goals, and the
  honest distance to a drop-in replacement: **`scope-backlog.md`** (Part
  2 = remaining work, Part 3 = non-goals, Part 4 = distance-to-parity).
  Kept lean on purpose; keep it current when a slice closes one of its
  entries.

The single large architectural item left is a **real backtracking
resolver** (`scope-backlog.md` Part 2.A / Part 4) — the shipped
`'backtrack` loop reconciles solvable slot conflicts, masks unsolvable
ones, and renders the real notices, but does not try autounmask levels
*inside* the loop or let `||`-preference / slot-operator-rebuild feedback
drive a retry. Everything else in Part 2 is one focused slice each.

When scoping the next slice, re-ground candidates in current code
(`what-this-proves.md` / `git log` / the source), never in a stale list.

### `helpers/` reference material

- `helpers/devmanual/` — a full local checkout of the Gentoo
  devmanual (`function-reference/`, `tools-reference/`, and per-phase
  `ebuild-writing/functions/*/text.xml` docs). Useful any time real
  ebuild-helper (`doins`, `dodir`, `insinto`, etc.) or phase-ordering
  semantics need grounding.
- `helpers/emerge_-1v_--debug_--getbinpkgonly__sys-fs--fuse.log`
  — a real `emerge --getbinpkgonly` debug trace. The remote binpkg
  download + merge is shipped; this trace stays useful for the still-open
  pieces (live `layout.conf` negotiation, `Packages.bz2`/`.lz4` — see
  `scope-backlog.md` Part 2.E).

## Real ebuild phase execution + filesystem merge (shipped; ongoing refinement)

This used to be "the next major phase after dry-run" — it's fully live
now. `ebuild <file> install` runs the real `pretend → setup → unpack →
prepare → configure → compile → test → install` chain via an embedded
`brush` shell driving real, unmodified `bin/*.sh`
(`rust/portuale/src/ebuild_phases.rs`). `ebuild <file> merge`
copies `${D}` into `${ROOT}`, writes a real vdb entry, and runs real
`pkg_preinst`/`pkg_postinst` (`ebuild_merge.rs`). `ebuild <file> unmerge`
is `merge`'s natural complement (`ebuild_unmerge.rs`), without which
`merge` alone could never be exercised through a real install/reinstall/
removal cycle. `ebuild <file> package` builds a real binpkg
(`ebuild_package.rs`). `emerge` itself has real, non-`--pretend`
merge actions now (all in `emerge_build.rs` / `emerge_getbinpkg.rs`):
`--buildpkgonly` (build a binpkg per resolved entry, never merge);
**`FEATURES=buildpkg` / `--buildpkg`/`-b`** (2026-08-31 — a binpkg of
each source entry written to `$PKGDIR` before the vdb merge, real
`_emerge/EbuildBinpkg`: `ebuild_package::package_after_install` split
from `run_package`, new `run_merge`/`merge_one_source_entry` `buildpkg`
param; `--buildpkg=n` beats `FEATURES=buildpkg`; `--buildpkg` is a
`--pretend` no-op; `--buildpkg-exclude <atoms>` shipped 2026-08-31 —
`emerge_build::entry_matches_any` filters `buildpkg` to `None` per
matching entry, still merged); a
plain **`emerge <atom>`** (real source build + merge via
`run_source_merge` → `ebuild_merge::run_merge`; `New` +
`Upgrade`/`Downgrade`/`Reinstall`, an in-place same-slot replace
unmerging the old version via `ebuild_merge::unmerge_replaced_same_slot`);
and **`emerge --getbinpkg`/`--getbinpkgonly`** (`run_merge_plan`,
dispatching per resolved entry — `Binary` → download + `merge_binpkg`
(all four `pkg_*` hooks, same-slot replace), else →
`merge_one_source_entry`; `--getbinpkgonly` is just the case where the
binary-only resolve never yields a source entry). Both merge paths take
`--keep-going` now (`emerge_build::run_merge_loop`: on a failure BFS-drop
the failed entry's transitive dependents via `GraphEntry.required_by`,
merge the rest, exit non-zero with a combined failed+skipped report —
real `Scheduler._calc_resume_list`). v1 cut left: no preserve-libs on
the replace.

**`emerge -jN` / `--jobs=N` parallel build scheduler shipped 2026-09-01**
(real `_emerge/Scheduler.py`): for a plain source `emerge <atom>`,
`run_source_merge` routes `jobs > 1` to `run_build_scheduler`.
`merge_one_source_entry` is split into `build_one_source_entry` (real
`EbuildBuild`+`EbuildBinpkg` — `install` phase + `--buildpkg` binpkg,
no vdb write) and `merge_one_built_entry` (real `EbuildMerge` — reuses
`run_qmerge`). The scheduler builds the forward-dep DAG from
`GraphEntry.required_by`, dispatches a build (`std::thread::scope`
worker) only when all its deps are merged, runs up to `jobs` concurrently
(bare `--jobs`/`-j` = `usize::MAX`, capped), and **serializes the vdb
merge on the main thread** (real portage merges one at a time).
`--keep-going` preserved (`scheduler_skip_dependents`). `--jobs[=N]` /
`-j[N]` parses like `--deep`; `--load-average=LA` / `-l LA` (real
`type=float`) holds off *additional* jobs while the 1-min system load
(`system_loadavg_1min`, `/proc/loadavg`) exceeds LA, never the first.
Rust-only — no contract-suite mirror (never executes builds). Each
parallel build's phase output is captured to `${T}/build.log`
(`run_commands_logged`, real `PORTAGE_LOG_FILE`; captured builds forced
onto the `bash` backend for a complete OS-level redirect) instead of
interleaving on stdout; the scheduler prints `>>> Jobs: X of Y complete`
after each merge and folds the failed build's log tail into a
`--keep-going` report. Cuts: the serialized merge step's `pkg_*` hooks
still run uncaptured through brush (residual stderr noise, pre-existing);
`--quiet-build` isn't a flag yet; one tokio runtime per `run_commands`;
in-flight builds finish (not killed) on a hard fail.
`unmerge_replaced_same_slot` (factored out of `merge_binpkg`) is also
wired into `merge_after_install`, so `ebuild <file> merge` of v2 over v1
no longer orphans v1's files.
**`emerge -C <atom>` / `--unmerge` without `--pretend` is a real removal
now** (2026-08-31): `pretend.rs::execute_unmerge` walks the
`_unmerge_display` selection and calls
`ebuild_merge::unmerge_one_installed` (factored out of
`unmerge_replaced_same_slot`) per version — `pkg_prerm` from the vdb's
own saved env → `unmerge_pkgfiles` → `pkg_postrm` → `delete_vdb_dir` —
then `deselect_from_world` (real `WorldSelectedPackagesSet.cleanPackage`).
**`emerge --depclean` / `--prune` / `--prune --nodeps` without `--pretend`
remove for real too** (2026-08-31): `run_depclean_pretend` /
`run_prune_pretend` / `run_prune_nodeps_pretend` each gained a
`pretend: bool` and route their computed cleanlist through the same
`execute_unmerge` (real `action_depclean` feeds its cleanlist to the
identical `unmerge()`); the `unresolved_deps()` safety halt +
`--depclean-lib-check` still gate removal, and depclean's stats line
reads `Number removed:`. **Every `requires --pretend` gate is gone now**
— `emerge --deselect` (no `--pretend`) rewrites `var/lib/portage/world`
+ `world_sets` for real (2026-08-31, real `action_deselect`'s
`world_set.replace(remaining)`; `Removing` vs `Would remove` verb; both
files sorted, comments dropped).
**`emerge --config <atom>`** (2026-08-31, real `action_config`):
`pretend.rs::run_config_action` — one vdb-matched atom → `Configuring
pkg...` → real `pkg_config` from the vdb-saved env via
`ebuild_merge::run_vdb_saved_env_phase` (factored out of
`unmerge_one_installed`, sans the `DEFINED_PHASES` gate) + a best-effort
builddir clean. Ignores `--pretend`; `--ask` picker/prompt cut. New
`dev-libs/emergeconfigpkg` fixture.
**`FEATURES=unmerge-backup`** (2026-08-31, real `dblink._pre_unmerge_
backup`): before `pkg_prerm` in `unmerge_one_installed` (for the
standalone `-C`/`--depclean`/`--prune` paths, `backup:
Option<&PackageOptions>`), `ebuild_package::quickpkg_from_vdb` builds a
binpkg of the still-installed package into `$PKGDIR` from its vdb
`CONTENTS` files (`image/` staged from `${ROOT}`, `build-info/` = a copy
of the vdb dir, then the same `bin/misc-functions.sh __dyn_package` via
the new `invoke_dyn_package`). A quickpkg failure aborts that unmerge.
Cuts: `treewalk()` replace-loop `_pre_merge_backup`/`downgrade-backup`,
`BUILD_TIME` idempotency (narrowed to file existence), `fif`/`dev` nodes.
`MergeOptions::from_env` factors the env-var config reads shared by
`emerge <atom>` and `ebuild <file> merge`. A successful `emerge <atom>`
(not `--buildpkgonly`) records each requested target as `cat/pkg` in
`var/lib/portage/world` (`pretend.rs::update_world_file`, real
`Scheduler._world_atom`); `--oneshot`/`-1` (now implemented) suppresses
that, and at `--pretend` drops a favorite from `PKG_MERGE_WORLD` to plain
`PKG_MERGE` colour. A `cat/pkg:slot` arg for a genuinely slotted cp
(2026-08-31, real `create_world_atom`: >1 SLOT in the repo or a lone
non-`"0"` SLOT) is recorded slot-qualified. World-file v1 cuts still:
version-pinned args that identify one slot, the vdb-only multislot
fallback. `emerge @set` build support shipped 2026-08-31 (any non-built-in
`@name` → recursive `resolve_custom_set`, same as the unmerge/depclean
paths), and with it `saveNomergeFavorites`'s `@name` → `world_sets`
recording (`update_world_sets_file`); `@selected` (= the pilot's `@world`
expansion, shared `expand_selected`) and `@installed` (`installed_set_atoms`,
real `EverythingSet` — a `cat/pkg:slot` atom per vdb package, always
slot-qualified) landed alongside. Real `SRC_URI` fetch (`portage-fetch` crate +
`portuale/src/fetch.rs`) downloads via real `wget`, verifies real
`Manifest` digests, and resolves `mirror://` against real
`profiles/thirdpartymirrors` + `GENTOO_MIRRORS`. Real eclass `inherit()`
support (`eclass_locations_value`) unblocked running real, unmodified
ebuilds/eclasses from the actual `gentoo` tree — `app-arch/unzip`,
`sys-fs/fuse`, `app-arch/xz-utils` have all been live-verified end to
end.

Each of these has its own dedicated, cited-source section in
`what-this-proves.md` — read that for the real Python source
grounding, the v1 scope cuts, and a runnable example per feature; for
what's still *missing* see `scope-backlog.md` Part 2.

### What "install a package into the filesystem" actually is

Splits into two separable pieces, grounded in real Python source (still
useful orientation even though both are now shipped):

1. **Phase execution** — shelling into a bash interpreter to run the real
   ebuild phase functions in order: `pkg_pretend` (always) → `pkg_nofetch`
   (restricted packages only) → `pkg_setup` (always) → `src_unpack` →
   `src_prepare` → `src_configure` → `src_compile` → `src_test` (source
   builds + tests enabled) → `src_install` (source builds) →
   `pkg_preinst`/`pkg_postinst` (if defined) → `pkg_prerm`/`pkg_postrm`
   (already-installed only) → `pkg_config` (user-requested only) →
   `pkg_info` (always). This calling-order table is documented per-
   function in `helpers/devmanual/ebuild-writing/functions/`.
   The devmanual also has `function-reference/` and `tools-reference/`
   covering the ebuild-helper commands (`doins`, `dodir`, `insinto`,
   etc.) used inside those phases.

2. **The vdb merge** — copying `${D}` into `${ROOT}` and recording
   `CONTENTS` under `/var/db/pkg/...`. Real implementation:
   `dblink.merge()`/`treewalk()`/`mergeme()` in
   `lib/portage/dbapi/vartree.py` (~6500 lines). Module-level `merge()`
   (`vartree.py:6231`) forks a `MergeProcess` that calls `dblink.merge()`
   (`:5958`) → `treewalk()` (`:4191`) → `mergeme()` (`:5323`, the actual
   copy loop, via `portage.util.movefile.movefile`).

   The Python orchestrator tying both together is `doebuild()`
   (`lib/portage/package/ebuild/doebuild.py:768`): for `mydo == "merge"`
   it runs the `install` phase via `spawnebuild()` (`:1592-1623`) and
   only calls `merge()` if that succeeds; `qmerge` skips straight to
   `merge()` assuming `install` already ran; a bare `install` just runs
   the phase without merging. Env setup (`D`, `S`, `WORKDIR`, `T`,
   `FILESDIR`, `EBUILD_PHASE`, `PORTAGE_BUILDDIR`, etc.) happens in
   `doebuild_environment()` (`:381`).

### The bash-execution-backend question (resolved)

The original plan was: "the Rust executor shells out to the system bash —
a deliberate, accepted dynamic dependency, in tension with the minimal-
Linux goal." Revisited because Rust-native bash implementations exist
(`reubeno/brush`, `shellgei/rusty_bash`).

**`rusty_bash` was ruled out**: early-stage at investigation time (27/84
real bash test scripts passing), not designed as an embeddable library.

**`brush` (`brush-core`/`brush-builtins`) is real and mostly works**: pure
Rust, MIT-licensed, embeddable via `brush_core::Shell`, tested against
real bash with ~1700 compatibility tests. A spike confirmed the hard
parts work: standard builtins, and real subprocess spawning (`doins`,
an actual bash script under `bin/ebuild-helpers/`, resolved via `PATH`
and executed as a child process).

**A real, high-blast-radius parser gap was found and is now fixed
upstream**: brush's grammar rejected bash's brace-less function-
definition form (`name() single-compound-command`, without `{ }`) —
valid in real bash, and used **60 times** in `bin/eapi.sh` alone (e.g.
`___eapi_has_pkg_pretend() [[ ${1-${EAPI-0}} != [0-3] ]]`), which is
unconditionally sourced by `isolated-functions.sh` — so this single
construct blocked brush from parsing essentially any real ebuild/eclass
pipeline. Fixed (new `CompoundCommand::ExtendedTest` AST variant,
`function_body` taught to accept it) and **merged upstream** as
**[reubeno/brush#1274](https://github.com/reubeno/brush/pull/1274)**
(merge commit `18851e7`, 2026-08-20). Verified against real unmodified
`bin/eapi.sh`/`isolated-functions.sh`/`phase-helpers.sh` end-to-end,
full `brush-parser`+`brush-core` suites green before/after.

A second, separate bug (a real runtime deadlock in `brush-core`'s own
pipeline-function-stage handling, hit once enough eclasses — the
`multilib` family — were in scope) was found live-testing real
`sys-fs/fuse`/`xz-utils`, root-caused, fixed in `brush-core/src/
commands.rs` (fork commit `c78ea429`, branch
`fix/pipeline-function-stage-deadlock` — the current pin), and submitted
upstream as **[reubeno/brush#1276](https://github.com/reubeno/brush/pull/1276)**
(open, no review yet). This is the one fork-only fix keeping the pin off
upstream `main`. Only these two constructs have been proven fixed — real
ebuilds/eclasses in the wild almost certainly exercise other bash
constructs not yet tried against brush; this was targeted spike-and-fix
work, not an exhaustive compatibility sweep. **Full fork-tracking record:
`brush-pin.md`.**

### Candidate strategies (complementary, not mutually exclusive)

1. **Default to brush, fall back to system bash** on a parse failure —
   not implemented as automatic fallback. What shipped instead:
   `ebuild --shell bash|brush` / `emerge --shell bash|brush`, explicit
   backend selection (`_doebuild_spawn()`-shaped `bash <bin_dir>/
   ebuild.sh <phase>` subprocess vs the embedded brush). **The default
   flipped from `brush` to `bash` on 2026-09-01** — brush's `declare -f`
   corrupts real eclass functions with redirected here-docs
   (`toolchain-funcs`'s `_tc-has-openmp`), breaking `emerge <atom>` for
   compiled packages. So in practice this pilot now does the *reverse*:
   default to bash, opt into brush. See `what-this-proves.md`'s
   "`--shell` default is now `bash`" and `brush-pin.md`.
2. **Fix our own `bin/*.sh` to avoid brush-hostile constructs** — **done
   2026-09-01**: the only function-as-non-last-pipeline-stage in
   `bin/*.sh` (the `__save_ebuild_env | __filter_readonly_variables`
   pipes in `bin/phase-functions.sh`) now stages through a `${T}` temp
   file via `__save_and_filter_ebuild_env`. See `brush-pin.md` and
   `what-this-proves.md`'s "brush strategy #2".
3. **Maintain a local brush fork with our fixes until upstream merges** —
   no longer needed: **`portuale/Cargo.toml` now pins real upstream
   `reubeno/brush` `main`** (the `vivo75/brush` fork is gone — #1274
   merged upstream, and #1276's deadlock is designed around by strategy
   #2 above, not patched). `brush-pin.md` tracks the pin and its
   periodic-re-pin checklist.

## How this pilot actually runs, session to session

The session-to-session operating rhythm — the "next slice" workflow, the
lockstep/fixture/test rules, the full verification pass, and the
commit/push rules — lives in **[`../AGENTS.md`](../AGENTS.md)**. Read it
before scoping or implementing a slice.

## How to use this prompt

Treat "Context" through "Ownership" above, and the phase-execution
investigation's "bash-execution-backend question"/"Candidate strategies"
sections, as settled, citation-backed decisions/findings — not things to
re-derive from scratch. "Current state" is a decaying snapshot —
re-verify against current `what-this-proves.md`/`git log`/the task list
before assuming any of it still holds. "Open backlog" is now just a
pointer to `scope-backlog.md`; that file is the one to keep current.
For the bash-backend investigation specifically, check the live
`reubeno/brush` crate state against `brush-pin.md`. If
something here conflicts with current reality, or a genuinely open
decision isn't covered above, ask before proceeding rather than assuming.
