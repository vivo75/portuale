# Prompt: Continue the Python-to-Rust Portage pilot

This is `PORTING/PROMPT-next.md`, the single entry point for (re)deriving
where this effort stands and what to do next, without repeating the
discovery conversations that produced it. It merges: the original
porting-strategy prompt (goals, hard constraints, architectural
decisions), the phase-execution/bash-backend investigation, the current
shipped state, the open backlog, and the operating rhythm this pilot has
settled into session over session.

As with any settled decision below: if you disagree, say so explicitly
and re-open it — don't silently override it.

## Context

Portage (this repository) is the Gentoo package manager, written in Python.
The goal is to create a Rust implementation as a **friendly fork**: a
separate, cooperating codebase, not a hostile competitor and not (yet)
committed to being a full replacement.

**EAPI floor**: EAPI 0, 1, 2, 3, 4, and 6 are deprecated and removed in
this repo/fork — no ebuild uses them, and all profiles are EAPI 5 or
higher (5, 7, 8 are the live versions). Any EAPI-conditional logic being
read or ported only needs to account for EAPI 5+ as the real, live
baseline — branches that only apply to EAPI 0/1/2/3/4/6 are dead code and
can be ignored rather than faithfully ported. (This pilot's own `portage-*`
crates go further, as a deliberate simplification confirmed with the user:
no EAPI parametrization at all within the 5+ floor — every EAPI in that
range is treated identically. See `PORTING/README.md` for the many places
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

- **End state is undecided**: this may become two permanent sibling
  implementations (like `uutils` vs GNU coreutils) or a strangler-fig
  migration where Rust eventually replaces Python. Do not pick an
  architecture that forecloses either option. Subprocess/CLI-based testing
  satisfies this; in-process FFI embedding (e.g. PyO3) would not, so avoid
  it for now.

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
invokes `emerge`/`ebuild` by name directly. **Shipped**: `PORTING/rust/portuale`.

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
  atoms/deps. `PORTING/bench/extract_snapshot.py` refreshes
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

## Current state (read `PORTING/README.md` for the authoritative, living detail)

As of commit `f1ae9287d` (branch `rust`), both major phases of this pilot
are live — re-verify against `git log`/`README.md` before trusting this
paragraph for long, since it decays fast.

**Dry-run (`emerge --pretend`)**: full recursive DEPEND/RDEPEND/BDEPEND/
PDEPEND/IDEPEND resolution; profile/make.conf-derived USE/ACCEPT_KEYWORDS
with the full real `USE_ORDER` precedence; every `package.*` file
(`.mask`/`.unmask`/`.accept_keywords`/`.use`/`.use.mask`/`.use.force`/
`.use.stable.mask`/`.use.stable.force`), repo-scoped across main **and**
overlay repos; `package.provided` (a listed CPV satisfies a dependency
atom silently / triggers the real `WARNING: … package.provided:` block
for a direct target); explicit `repos.conf` `masters =` parsing (not just
the implicit main-repo default); cross-repo profile parents; `USE_EXPAND`;
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
real 8-phase chain via an embedded `brush` (Rust-native bash) driving
unmodified `bin/*.sh`; `ebuild <file> merge` really copies `${D}` into
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
`inherit()` support; `ebuild --shell bash|brush` picks the execution
backend explicitly.

`PORTING/README.md`'s "What this proves" section is the incrementally-
updated record of every shipped slice, each grounded in cited real Python
source — read that, not this list, for current detail, and `git log` for
how work has actually been landing (one small, fully-shipped, documented-
and-tested slice at a time — see "How this pilot actually runs" below).

### Open backlog (re-derived 2026-08-25, preserve-libs control-flow wiring, CONFIG_PROTECT "confmem rejected", preserve-libs NEEDED.ELF.2 pruning, fifo/device node CONTENTS, and --root-deps recursive-build-entry (first increment) items updated 2026-08-26 — treat any earlier version of this list as void)

Every item this list previously carried (`--usepkg-exclude`/
`--rebuilt-binaries`, `opt=`/`opt?` USE-deps, real `--tree`, the `--json`
state-change trace, explicit `repos.conf` `masters =`, `--root-deps`
satisfiability/disjunctive-branch-selection, preserve-libs control-flow
wiring, CONFIG_PROTECT's "confmem rejected this update" skip-copy,
preserve-libs `NEEDED.ELF.2` pruning on `remove_from_contents`, fifo/
device node `CONTENTS` support) has since shipped, and `--root-deps`'s
own recursive-build-against-the-running-root gap has its first real
increment shipped too (still not fully recursive — see the backlog
entry below for exactly what's left). Re-verify against
`README.md`/`git log` before trusting even *this* version — a "scope the
next slice" round should always re-ground candidates in current code,
not just read this list. What's actually left, grouped by area:

> See also `PORTING/SCOPE_BACKLOG.md` — a wider inventory of real portage
> behavior not yet ported to *either* side (config-resolution `USE_ORDER`
> depth, the actual remote-binpkg *download*/`gpkg`, fetch resume/
> `RESTRICT=primaryuri`, brush strategy #2 (rewrite brush-hostile
> `bin/*.sh`), …).
> Re-derived 2026-08-27; keep it in sync alongside this file when a
> slice closes one of its entries. Recently closed from it:
> the **remote binpkg download + merge** (2026-08-31): `emerge
> --getbinpkgonly <atom>` non-`--pretend` — live `Packages` refresh
> (`bintree._populate_remote`, `wget`), binary-only resolve, download
> into `$PKGDIR` + `SIZE`-check, then merge (`binpkg::extract_binpkg`,
> `ebuild_merge::merge_binpkg` reusing `merge_tree`/`env_update`,
> `emerge_getbinpkg.rs`; `write_vdb_entry_from_dir` refactor).
> **Replacing an installed version shipped 2026-08-31**: `merge_binpkg`
> does the real merge-then-unmerge (new binpkg + vdb entry first, then
> every same-slot installed version's files unmerged with the new `PF`
> folded into `others_in_slot`, then the old vdb dir deleted);
> `run_unmerge` split into `pub(crate)` `unmerge_pkgfiles` +
> `delete_vdb_dir` so the replace reuses them phase-free;
> `run_getbinpkgonly` takes `Upgrade`/`Downgrade`/`Reinstall` too.
> **All four binpkg install/remove `pkg_*` hooks shipped 2026-08-31**:
> `extract_binpkg` keeps `environment.bz2` + `<pf>.ebuild` verbatim;
> `ebuild_phases::run_phase_from_saved_env` = `run_single_phase` + real
> `BinpkgEnvExtractor` (`bunzip2 environment.bz2 > ${T}/environment` +
> `.raw` marker, `EMERGE_FROM=binary`); `merge_binpkg` peeks metadata
> first, runs `setup`→`preinst` before the copy, then per replaced
> same-slot version `prerm`→remove→`postrm` (from that version's own vdb
> `environment.bz2`), then `postinst`, all gated on `DEFINED_PHASES`;
> new `binpkgphasepkg-1.0` + `binpkgrmpkg-{1.0,2.0}` fixtures. v1 cuts:
> no preserve-libs on replace, `SIZE`-only digest, no
> `layout.conf`/`Packages.gz`/resume. Rust-unit-tested end to end over
> loopback HTTP;
> the `[ebuild …]` **bracket + merge-order** two-part real-tree finding
> (2026-08-30, container run against a real Gentoo tree). **Increment 1
> — mask column**: the pilot gated the 7th `PkgAttrDisplay` column on
> `-v`, but real `include_mask_str()` = `verbosity > 1` and real default
> `emerge -p` verbosity is 2, so it shows at plain `-p` too, absent only
> under `--quiet` (not modelled). `attr_display_field` /
> `format_blocker_lines` render it unconditionally now; keyword/hard-mask
> markers (`~`/`*`/`#`) now visible without `-v`. **Increment 2 — merge
> order** (Model A, confirmed via AskUserQuestion): the pilot's BFS
> built `entries` parent-first; real portage's `mylist` is a topological
> merge schedule. `resolve_pretend_graph` (+ Python mirror) now re-sorts
> `entries` dependency-first as its last step
> (`topological_merge_order`, stable, cycles kept in discovery order).
> `entries` canonically merge-ordered for the flat list, `emerge
> --buildpkgonly`, and `--json` (which also stamps an explicit
> `"merge_order"` int); `--tree` re-derives from `required_by`,
> unaffected. Both increments: ~240 pinned assertions each, both
> implementations byte-identical throughout. **Follow-on slice —
> `USE="…"` at plain `-p`, DONE 2026-08-30** (real-tree finding): real
> `print_use_string = verbosity != 1` (not `-v`-gated); `-v` changes
> `all_flags` = *which* flags render. Inc 1: a `New` entry's USE line
> shows at `-p` (identical to `-pv` bar `::repo` + counters). Inc 2:
> `build_use_expand_display`/`_build_use_expand_display` grew
> `all_flags: bool`, `render_flag` returns `Option` and omits unchanged
> flags (+ the `(-flag%)` removed list) when off — so a
> `Reinstall`/`Upgrade`/`Downgrade` shows only its changed flags at
> `-p`. New `GraphEntry::use_expand_display_p` (Python re-renders at
> display time). `-pv` unchanged. ~30 pinned `-p` assertions total.
> Inc 3 (2026-08-31): `reinst_flags` — `build_use_expand_display` gained
> a `reinst_flags` set (`Reinstall::changed_flags` / real
> `_reinstall_for_flags`); a trigger flag is force-shown even when
> unchanged. Only visible at `-p`: a flag dropped from IUSE that still
> triggered a `--newuse`/`--changed-use` reinstall now shows in the
> `(-flag%)` removed list (`dev-libs/reinstdropiusepkg` fixture).
> `_create_use_string` now fully modelled bar ANSI colour);
> `license_groups` read location (**real-tree finding**, 2026-08-30 —
> the first time portuale was run against a real Gentoo tree, inside the
> new `localhost/test-portuale` container, `emerge --pretend <anything>`
> failed for every package: `resolve_config` read `license_groups`
> per-profile-chain-level, but real `LicenseManager._read_license_groups`
> (`LicenseManager.py:47`) iterates `LocationsManager.profile_locations`
> (`LocationsManager.py:432` = `[<main_repo>/profiles] + [<overlay>/
> profiles …]`) — the repo `profiles/` *bases*. `@FREE` expanded to
> nothing → every ebuild failed the license check. `resolve_config` now
> reads `<repo>/profiles/license_groups` for the main repo + every
> overlay; 2 fixture files moved out of their chain-level subdirs; after
> the fix portuale's `--pretend` package selection matches real `emerge
> --pretend` on a real tree); `RESTRICT=fetch` in the fetch path (real `fetch.py:1167` — a plain
> `SRC_URI` URI + the public mirrors are barred from the wget candidate
> list; a fetch-restricted package fetches only from an already-verified
> `DISTDIR` copy; `fetch+`/`mirror+` re-permits it; `pkg_nofetch` phase
> = a documented cut; new `dev-libs/fetchrestrictpkg` fixture,
> 2026-08-30); `emerge -pc
> <atoms> --deselect=n` (real `action_depclean`'s `deselect` — keeps
> `world` as a protection root in args mode so a named world member is
> kept; `depclean_cleanlist` `deselect` param; fixed `--deselect`
> wrongly triggering the standalone action alongside `--depclean`/
> `--prune`/`--unmerge`, 2026-08-30); the `-pC`/
> `-pP` higher-slot set-protection refinement (real `unmerge.py:421-441`
> `higher_slot` — the "still listed in package sets" warning is
> suppressed when a newer, different-slot install also matches the set
> atom; shared `still_listed_parents`, new `dev-libs/dualslotpkg`
> fixture, 2026-08-30); real
> `--autounmask` **keyword + USE** resolution (2026-08-30, two
> increments — the pilot now applies the implicit `=cpv ~arch` /
> `package.use` flip, resolves the graph, and prints real
> `_display_autounmask`'s `The following <keyword|USE> changes are
> necessary to proceed:` block, exit 0; `-pv`'s `USE=` line reflects the
> USE flip; `resolve_pretend` `autounmask_keywords`/`autounmask_use`
> params, `GraphResult::autounmask_keyword_changes`/`autounmask_use_
> changes`, `--json` arrays; `--autounmask-use=n` restores strict
> matching. **Increment 3 — the `opt=` parent flip — done 2026-08-30**:
> when a dep's `opt?`/`opt=` use-dep is unsatisfiable AND the child's
> flag is `use.mask`'d (no child flip), `resolve_pretend_graph` flips
> the *requesting parent's* flag, re-resolves the freed dep, records
> `>=<parent-cpv> -flag` (new `dev-libs/parentflip{childpkg,eqpkg}`
> fixtures). Cuts: re-resolves only the freed dep, one-level parent dep
> chain, non-`New` parent re-rendered as `New`. `--autounmask-license`
> still open);
> `mirror+`/
> `fetch+` SRC_URI prefixes (`portage_fetch` strips + records
> `override_mirror`/`override_fetch`; `mirror+` re-permits the
> `GENTOO_MIRRORS` fallback under `RESTRICT=mirror`; `override_fetch`
> inert until `RESTRICT=fetch` is modelled, 2026-08-30); `ebuild …
> package` / `emerge --buildpkgonly` emitting `gpkg` (`BINPKG_FORMAT=
> gpkg` → real, unmodified `bin/gpkg-helper.py compress`; round-trips
> through this pilot's own `read_gpkg_metadata`; signing cut, 2026-08-30);
> the `-pv`
> `USE=` flag list's natural (`_alnum_sort_key`) sort (2026-08-30);
> `alias:path` profile parents (real `get_location_for_name`, 2026-08-30 — note the
> `::alias` *atom* case was never a gap: real `match_from_list` does a
> straight name comparison, the pilot matched it already); the
> `$PKGDIR` directory-scan fallback (real `bintree._populate_local` —
> `binpkg.rs` gpkg+xpak readers + `scan_pkgdir` + `BinaryIndex`
> refactor, 3 increments, done 2026-08-30) and the `build-info`
> dependency-metadata generation it surfaced (real
> `_post_src_install_write_metadata` + whole-`build-info`-into-vdb,
> 2026-08-30); real
> `RESTRICT=mirror` (the public `GENTOO_MIRRORS` flat-layout fallback is
> skipped — `FetchOptions::restrict_mirror` — 2026-08-29); the blocker
> line's own real `output.py::_blockers` layout + `PKG_BLOCKER` colour +
> deferred ordering (2026-08-29); an installed dependency's USE-dep
> checked against its *built* `USE` (real `_iuse_implicit_built`, bug
> 640318 — 2026-08-29); the `--pretend` half of remote binpkgs
> (`--getbinpkg`/`--getbinpkgonly` — `binrepos.conf`/`PORTAGE_BINHOST`,
> cached-`Packages`-index candidates, the `g` bracket column; the actual
> download is still open); `USE_EXPAND`
> corners in full (including `emerge -pv`'s own `USE_EXPAND` grouping +
> `USE_EXPAND_HIDDEN` + installed-vs-new `*`/`%` markers + the `( … )`
> forced/masked wrap + the `[ebuild N ~]` bracket-mask column), the
> `--root-deps` recursion + `IDEPEND`
> follow-ups (including a top-level package's own
> `IDEPEND`-vs-running-root), `--changed-deps` in full (per-key +
> `strip_slots` + the structured `flat=False` comparison via new
> `portage_use_reduce::use_reduce_structured`), and
> `metadata/layout.conf` in full
> (`masters =` middle tier, `repo-name`, `profile-formats` gate) +
> `profiles/repo_name` canonical name + `aliases` + the section-name
> mismatch drop.

**Merge/unmerge subsystem** (`ebuild_merge.rs`/`ebuild_unmerge.rs`):
- **preserve-libs *registration* side** — the full real *computation*
  (`_find_libs_to_preserve`/`LinkageMap.rebuild`/`findConsumers`, ELF
  `NEEDED`/soname introspection via `NEEDED.ELF.2` plus a graph-based
  reachability algorithm) shipped across five narrow, individually-
  confirmed slices (2026-08-25/26) in new `needed_elf.rs` — see
  `README.md`'s own "`preserve-libs` registration: the full `LinkageMap`/
  `findConsumers`/decision computation" for the full grounding of all
  five. In order: (1) real `NEEDED.ELF.2` generation via real,
  unmodified `bin/misc-functions.sh install_qa_check` (real `scanelf`,
  no ELF-parsing code at all) copied into the real vdb entry on merge —
  this also surfaced and fixed a real, broader gap (`_post_phase_cmds
  ["install"]` wasn't running at all before, affecting every install,
  not just preserve-libs) and a real fixture bug (EAPI 8's own
  `strict-keepdir` unconditionally strips a genuinely empty `dodir`'d
  directory; `envupdatepkg`/`collisionpkg-a` fixed to use `keepdir`);
  (2) `NeededEntry`, the data model for one parsed line; (3)
  `read_all_needed_entries`, real `rebuild()`'s own initial per-package
  vdb-read loop; (4) `rebuild` itself, the real soname providers/
  consumers map (multilib categorization, `$ORIGIN` runpath expansion,
  implicit-runpath inference for bundled libraries, real inode-based
  `ObjKey` dedup); (5) `getlibpaths`/`find_consumers`/`find_libs_to_
  preserve`, real `findConsumers()`'s own consumer-satisfaction logic
  and `_find_libs_to_preserve()`'s own graph-reachability decision (a
  minimal `LibGraph`, narrowed to exactly the three real `digraph`
  operations needed).
  ~~the real control-flow integration that actually wires `find_libs_to_
  preserve`'s own output into a real merge/unmerge~~ — **shipped
  2026-08-26**: `preserve_libs_on_unmerge` (new, `ebuild_merge.rs`) ports
  real `dblink._prune_plib_registry(unmerge=True, ...)`
  (`vartree.py:2228-2314`, called right before `_unmerge_pkgfiles()` at
  `vartree.py:2493`/`2529`), narrowed to the one real shape this pilot's
  own always-separate `merge`/`unmerge` CLI invocations reach
  (`unmerge_with_replacement` always `False`). `register_preserved_libs`
  ports real `PreservedLibsRegistry.register`/`.unregister` as one
  function (real `unregister` **is** `register(..., [])`).
  `ebuild_unmerge::run_unmerge` calls it right before `remove_contents`,
  threading the preserved-path set into a new `remove_contents` parameter
  that filters them out of `CONTENTS` before the per-file removal loop —
  see `README.md`'s own "`preserve-libs` registration: wired into a real
  unmerge's control flow" for the full grounding, including the real,
  `gcc`-compiled dual-package (`libpreservetest`/`consumepreservetest`)
  end-to-end fixture proof and live-CLI verification. `needed_elf.rs`
  now has a real caller.
  ~~real `NEEDED`/`LinkageMap` bookkeeping in `unregister_preserved_libs`
  (real `removeFromContents` also strips matching `NEEDED` lines so
  stale linkage data doesn't corrupt a future preserve-libs decision) —
  moot without the registration side ever writing `NEEDED` data~~ —
  **shipped 2026-08-26, no longer moot**: `remove_from_contents`
  (`vartree.py:1244-1310`) now prunes a package's own `NEEDED.ELF.2` too
  whenever a `CONTENTS` line was actually removed, dropping any entry
  whose `filename` no longer appears among the surviving paths — see
  `README.md`'s own "preserve-libs: real `NEEDED.ELF.2` pruning on
  `remove_from_contents`". New `NeededEntry::to_needed_line`
  (`needed_elf.rs`) ports real `NeededEntry.__str__`, the rewrite-side
  sibling of the existing `parse`/`parse_file` read side. This was the
  last documented preserve-libs gap in this immediate area.
  Deliberately still missing, confirmed with the user each time: the one
  non-`NEEDED.ELF.2`-driven branch inside real `rebuild()` itself (live
  `scanelf` for orphaned preserved libs, `LinkageMapELF.py:233-324` —
  the one real spot a raw ELF header read would matter).
- CONFIG_PROTECT/`_protect()`: no remaining gaps in this immediate area.
  Symlink protection, `--noconfmem`, `new_protect_filename`'s last-file
  reuse, the type-independent `dest_md5`/`dest_link` comparison, and now
  `_installed_instance`/`FEATURES=config-protect-if-modified` have all
  shipped — see `README.md`'s own "CONFIG_PROTECT for symlinks,
  `--noconfmem`, and `new_protect_filename`'s own file-reuse logic",
  "CONFIG_PROTECT: a type-changing update is real-protected too", and
  "CONFIG_PROTECT: `_installed_instance`/`FEATURES=config-protect-if-
  modified`" (the latter also surfacing a real, previously-undiscovered
  default-`FEATURES` mismatch: `config-protect-if-modified` *is* one of
  `make.globals`'s own default tokens, `cnf/make.globals:79`, the same
  category the `protect-owned`/`unmerge-orphans` fix found earlier).
  `installed_instance_pf`/`owned_node_value_pf`, new, reuse the real
  per-package `COUNTER` file this pilot already writes on every merge
  rather than needing new persistence.
  ~~One remaining, deliberate v1 simplification: an already-offered,
  unmodified-since update is applied directly instead of real portage's
  own "leave the destination untouched, still record CONTENTS"~~ —
  **shipped 2026-08-26**: `protect_decision` now returns `(write_dest,
  moveme)` instead of just a path, and `merge_tree`'s own `obj`/`sym`
  branches only perform the actual copy/symlink-write `if moveme` —
  matching real `mergeme()`'s own `if moveme:` gate around `movefile()`
  (`vartree.py:5547`/`5749`), traced against the real `move_me = protected
  = bool(cfgfiledict["IGNORE"])` gate in `_protect()`
  (`vartree.py:5831-5901`) line by line. See `README.md`'s own "CONFIG_
  PROTECT: \"confmem rejected this update\"" for the full grounding and a
  live-verified example. This closes the last documented CONFIG_PROTECT
  gap in this area.
- `unmerge` remaining cuts: none left in this immediate area.
  ~~`bsd_chflags` handling~~ — **removed 2026-08-25, was never a real
  gap**: `lib/portage/__init__.py:311` sets `bsd_chflags = None`
  unconditionally on non-BSD, and this pilot's own hard goal is
  Linux-only/musl-static — same category of mis-scoping as the earlier
  `FEATURES=verify-sig` removal below.
  ~~`INFOPATH`/`INFODIR` env-var-driven inode-match half of real
  `INFOPATH` cleanup~~ — **shipped 2026-08-25**: `env_update::
  info_dirs_inodes` collates real `INFOPATH`/`INFODIR` values from
  `/etc/env.d/*` the same way `env_update::run_env_update` collates
  every other real `COLON_SEPARATED` key, and `run_unmerge` threads the
  resulting inode set down through `remove_contents`/`remove_dirs` into
  `cleanup_info_dir` — see `README.md`'s own "`unmerge`: real `INFOPATH`
  cleanup". (Both `FEATURES=unmerge-orphans` and the "symlink orphan"
  refinement, bug #326685 + `_unmerge_dirs()`'s own bug #640058
  recursive-parent-revisit, have also shipped — see `README.md`'s own
  "`unmerge`'s own \"symlink orphan\" refinement (bug #326685)"` and
  "`unmerge`: real `FEATURES=unmerge-orphans`", the latter including a
  real, confirmed finding that real `_unmerge_protected_symlinks()`'s
  own delete-or-warn logic is unreachable dead code in current portage.
  `run_unmerge` also gained an `UnmergeOptions` struct in the process,
  mirroring `MergeOptions`'s own shape now that it has five fields
  instead of two loose parameters.)
  ~~per-file failure tolerance~~ — **removed 2026-08-25, was never a real
  gap**: re-reading real `_unmerge_pkgfiles()` directly
  (`vartree.py:3033-3059`) shows it `raise`s on an unexpected `errno`,
  exactly like this pilot's own current hard-fail behavior; the
  "failure counter" this item used to cite belongs to a different
  function (`dblink.unmerge()`'s own prerm/postrm phase-failure count,
  not per-file removal).
- No remaining standalone-command gaps in this area (`qmerge`,
  standalone `config`/`info`, and standalone `prerm`/`postrm` all
  shipped — see `README.md`'s own "Real `ebuild <file> qmerge`",
  "Standalone `ebuild <file> config`/`info`", and "Standalone `ebuild
  <file> prerm`/`postrm`"). `preinst`/`postinst` deliberately stay
  internal-only (real ordering constraint tying them to `merge`'s own
  file-copy step, `dblink.treewalk()` invokes them directly around it,
  a constraint `prerm`/`postrm` don't share with `unmerge`).
- Minor: no `chown`/permission-preserving `chmod` reproduction (this
  pilot's own single-user dev/test context has no privilege-dropping
  concept anywhere else either); directory merge order is sorted for
  determinism, not real `os.listdir()`'s own arbitrary order (cosmetic,
  `CONTENTS` line order has no real semantic meaning).
  ~~fifo/device nodes: real `mergeme()` handles these too, but no
  fixture this pilot has needs them~~ — **shipped 2026-08-26**: real
  `mergeme()`'s own fifo/device `else:` branch (`vartree.py:5787-5811`)
  is real now — new `create_special_node` recreates a fresh node at the
  destination via real `mkfifo(3)`/`mknod(3)` (matching the source's own
  type/permissions/major-minor), only when nothing's there yet, with the
  real `fif`/`dev` `CONTENTS` line always written regardless. The
  unmerge side needed no functional change — real `_unmerge_pkgfiles()`
  never unlinks either node type in the first place, and this pilot's
  own catch-all already happened to match by coincidence, doc comment
  now corrected to cite the real reason. See `README.md`'s own
  "`ebuild_merge.rs`/`ebuild_unmerge.rs`: real FIFO/device node
  `CONTENTS` support (`fif`/`dev`)". Real device-node creation is only
  unit-tested (genuinely requires root/`CAP_MKNOD`, not reproducible
  live in this dev/test environment); the real FIFO case is proven live
  end to end with a new `dev-libs/fifopkg` fixture.

**Binary packages / fetch**:
- `--getbinpkg`/`--getbinpkgonly`: the **`--pretend` half shipped
  2026-08-29** (`binrepos.conf`/`PORTAGE_BINHOST` parsing, remote binhost
  candidates from each binhost's *cached* `Packages` index, the `g`
  bracket column, download `SIZE` -> `Size of downloads:`). The **actual
  remote download + merge shipped 2026-08-31**: `emerge --getbinpkgonly
  <atom>` (non-`--pretend`) refreshes each `http(s)` binhost's live
  `Packages` (real `bintree._populate_remote`, `wget`), resolves
  binary-only, downloads each remote binpkg into `$PKGDIR`, size-checks
  it, and merges it (new `binpkg::extract_binpkg`, `ebuild_merge::merge_
  binpkg`, `emerge_getbinpkg.rs`; `write_vdb_entry_from_dir` refactor).
  **Replacing a same-slot installed version shipped 2026-08-31**:
  `merge_binpkg` does the real merge-then-unmerge (new binpkg + vdb
  entry written first, then every same-slot installed version's files
  unmerged — the new `PF` folded into `others_in_slot` so a shared path
  survives with the new content — then the old vdb dir deleted, then
  `env_update`); `run_unmerge` split into `pub(crate)` `unmerge_pkgfiles`
  (env-free file-removal core) + `delete_vdb_dir` for phase-free reuse;
  `run_getbinpkgonly` accepts `Upgrade`/`Downgrade`/`Reinstall` outcomes.
  **All four binpkg install/remove `pkg_*` hooks shipped 2026-08-31**:
  `extract_binpkg` keeps `environment.bz2` + `<pf>.ebuild` verbatim;
  `ebuild_phases::run_phase_from_saved_env` = `run_single_phase` + real
  `BinpkgEnvExtractor` (`bunzip2 environment.bz2 > ${T}/environment` +
  `.raw` marker + `EMERGE_FROM=binary` → `bin/ebuild.sh` sources the
  saved env, skips re-sourcing the ebuild, for `pkg_setup` too);
  `merge_binpkg` peeks metadata first (image → real
  `${PORTAGE_BUILDDIR}/image`), runs `setup`→`preinst` before the copy,
  then per replaced same-slot version `prerm`→remove→`postrm` (from that
  version's own vdb `environment.bz2`), then `postinst`, all gated on
  `DEFINED_PHASES`; new `binpkgphasepkg-1.0` + `binpkgrmpkg-{1.0,2.0}`
  fixtures.
  **`--getbinpkg` mixed source+binary merge shipped 2026-08-31**:
  `run_getbinpkgonly` → `run_merge_plan`, dispatching per resolved entry
  on `entry.source` (`Binary` → `merge_one_binary_entry`, else →
  `emerge_build::merge_one_source_entry`). "Prefer binary, fall back to
  source" stays the resolver's job.
  **`merge_binpkg` collision-protect / blocker exclusion / preserve-libs
  parity shipped 2026-08-31**: the same `find_collisions` +
  `collision-protect`/`protect-owned` abort + `unregister_preserved_libs`
  as `merge_after_install`; `blockers_from_flat_deps` (factored out)
  reads the binpkg's already-USE-reduced `*DEPEND` build-info for the
  `mypkglist` blocker term.
  Still open: live `layout.conf` negotiation, `Packages.gz`,
  resume, real `SHA*` digest verification.
- **`$PKGDIR` directory-scan fallback** (real `bintree._populate_local`
  — scan `$PKGDIR` for binpkg *files* when there's no trusted index; a
  `gpkg` already resolves fine when it's *in* an index, which is
  format-agnostic). **DONE — 3 increments, 2026-08-29/30**:
  `portuale/src/binpkg.rs` `read_gpkg_metadata` (real `gpkg.get_metadata`,
  `tar` + seven decompressors) + `read_xpak_metadata` (real
  `xpak.tbz2.scan`, pure Rust) + `scan_pkgdir` wired into `pretend.rs`
  (`Config::scanned_binpkgs`, only when `Packages` absent, never written
  back). `portage_repo::BinaryIndex` refactor
  (`from_pkgdir`/`from_entries` through every binary-candidate fn). Both
  sides; contract-tested. Remaining: `Manifest`/`.sig` verification (cut
  — no crypto), bare `.xpak` multi-instance, mtime-staleness index
  revalidation. See [[porting_pkgdir_scan_gpkg_buildout]].
- **`build-info` metadata generation** (found during the binpkg-scan
  buildout) **shipped 2026-08-30**: `ebuild_phases::write_post_install_
  metadata` (real `doebuild.py::_post_src_install_write_metadata`) writes
  the `DEPEND`/`RDEPEND`/`LICENSE`/… build-info files
  `bin/phase-functions.sh` doesn't, and `write_vdb_entry` now copies the
  whole `build-info` dir into the vdb (real `treewalk()`) — so a
  pilot-merged/built package carries its real dependency metadata.
  **`:=` slot-operator binding shipped 2026-08-31**: `ebuild_phases::
  bind_slot_operator` (real `_slot_operator._eval_deps`) rewrites each
  `*DEPEND` `:=` atom to `:<slot>/<sub-slot>=` from the highest installed
  match in `<root>/var/db/pkg`, bare if unresolvable — so a pilot-merged
  package's stored deps carry the sub-slot info a rebuild check needs.
  **Slot-operator REBUILD edges shipped 2026-08-31 (v1)**:
  `resolve_pretend_graph`'s post-pass `slot_operator_rebuild_entries`
  (real `_slot_operator_trigger_reinstalls` / the
  `@__auto_slot_operator_replace_installed__` set) — an installed
  consumer whose built `cat/pkg:S/SS=` dep no longer matches how the run
  leaves `cat/pkg` in that slot becomes a
  `Reinstall { slot_operator_rebuild: true }` (`[ebuild R]`, new sixth
  Reinstall trigger field, `--json` bool), ordered after the dep.
  **Increment 2 (2026-08-31)**: `_show_abi_rebuild_info`'s "The following
  packages are causing rebuilds:" block (`GraphResult::abi_rebuilds`;
  `--verbose-slot-rebuilds[=y|n]` wired, default on, NOT `--verbose`;
  `--json` `abi_rebuilds` array).
  **Increment 3 (2026-08-31)**: `--ignore-built-slot-operator-deps`
  (real `main.py:470`, `y_or_n`, debug-only) — `resolve_pretend_graph`
  gained an `ignore_built_slot_operator_deps` param that skips the
  `slot_operator_rebuild_entries` post-pass entirely (real portage
  strips the built `:=` parts via `FakeVartree`; same net effect).
  v1 cuts: single-pass (no backtracking), consumer's own `:=` not
  re-bound, no `--changed-slot` interaction. New
  `dev-libs/slotbind{target,consumer,fresh}` fixtures.
  Cuts still: `IUSE_EFFECTIVE`; reading the binpkg `Packages` *index*
  entry from build-info instead of md5-cache (cosmetic — `--pretend`
  reads the index); real `_eval_deps`'s RDEPEND/PDEPEND-vs-DEPEND/BDEPEND
  target-vs-running-vdb split (pilot binds all against target `ROOT`).
- `BINPKG_FORMAT=gpkg` on the *write* side **shipped 2026-08-30**
  (`PackageOptions::binpkg_format` → real, unmodified
  `bin/gpkg-helper.py compress` via `__dyn_package`'s own gpkg branch; a
  `<cat>/<pf>.gpkg.tar` this pilot's own `read_gpkg_metadata`
  round-trips; `Packages` gets a `PATH` field). Cuts: gpkg signing
  (`FEATURES=binpkg-signing` — no crypto), `BUILD_ID` in the basename.
  Still open: `BUILD_ID`/packdebug/splitdebug/RPM;
  `PKGDIR`-index locking. (Real `PORTAGE_COMPRESSION_
  COMMAND` resolution shipped — see `README.md`'s own "Real
  `PORTAGE_COMPRESSION_COMMAND` resolution": all six real compressors,
  `BINPKG_COMPRESS` now defaults to real `"zstd"` — this pilot's own
  previous hardcoded `"bzip2 -c"` predated noticing real portage's own
  default had changed from `bzip2`.)
- Fetch: resume support, live `layout.conf` negotiation, `RESTRICT=
  primaryuri` (doesn't port cleanly — candidate ordering already
  deviates), and running the ebuild's own `pkg_nofetch` phase for a
  missing `RESTRICT=fetch` distfile (real `spawn_nofetch`). (`mirror+`/
  `fetch+` SRC_URI prefixes + `RESTRICT=fetch` shipped 2026-08-30 —
  `portage_fetch` strips the prefixes + records `override_mirror`/
  `override_fetch`; `FetchOptions::restrict_fetch` bars a plain
  `SRC_URI` URI + the public mirrors so a fetch-restricted package
  fetches only from an already-verified `DISTDIR` copy; `fetch+`/`mirror+`
  re-permits the URI; new `dev-libs/fetchrestrictpkg` fixture.) (Real
  `RESTRICT=mirror`
  shipped 2026-08-29 — `FetchOptions::restrict_mirror` gates the
  `GENTOO_MIRRORS` flat-layout fallback, new `dev-libs/restrictmirrorpkg`
  fixture. Real `custommirrors` shipped — see
  `README.md`'s own "Real `custommirrors`: an admin-configured
  `/etc/portage/mirrors` file"; `FetchOptions` gained a `config_root`
  field mirroring `ebuild_merge::MergeOptions`'s own. Real
  `FEATURES=distlocks` shipped too — see `README.md`'s own
  "`FEATURES=distlocks`, and a real default-`FEATURES` correction to
  `protect-owned`/`unmerge-orphans`", which also fixed a real,
  previously-undiscovered mistake: `protect-owned` and `unmerge-orphans`
  had shipped defaulting to `false`, but real `cnf/make.globals` (lines
  77-84) actually defaults both of them **and** `distlocks` to `true` —
  both now correctly default `true`, confirmed live. ~~`FEATURES=
  verify-sig` (GPG)~~ — **removed 2026-08-25, was mis-scoped from the
  start**: real signature verification is a `gpkg`/repo-sync concept,
  not `SRC_URI`/distfile-fetch at all — zero hits grepping `fetch.py`
  directly for either term.)
- No remaining gaps in this area. `--keep-going` is real for all three
  non-`--pretend` merge paths: `--buildpkgonly` (see `README.md`'s own
  "`emerge --buildpkgonly --keep-going`" — its depgraph gate guarantees
  no entry depends on another) and, as of 2026-08-31, `emerge <atom>` /
  `emerge --getbinpkg` (`emerge_build::run_merge_loop` — the general
  version: BFS-drop the failed entry's transitive dependents via
  `GraphEntry.required_by`, real `Scheduler._calc_resume_list`).

**Depgraph / dry-run**:
- **`--root-deps` fuller fidelity, remaining half.**
  ~~doesn't feed running-root satisfiability into disjunctive (`||`)
  branch selection~~ — **shipped 2026-08-25**: both real dep-walk sites
  (the main New/Upgrade/Reinstall flatten and `enqueue_dependencies`'s
  own `--deep`/`AlreadyInstalled` recursion) now accept a `||` branch
  that's running-root-satisfied even when it's invisible in the tree —
  see `README.md`'s own "`emerge --pretend --root-deps`" section, mirrored
  in `emerge_pretend_reference.py` with a new pytest contract test
  (`test_root_deps_disjunctive_branch_selection_matches_between_
  implementations`).
  ~~Still open: real portage can recursively pull in and build a *new*
  package against the running root when it's not already there, never
  attempted here~~ — **first increment shipped 2026-08-26, confirmed
  with the user to build toward the full real multi-root shape one
  increment at a time** (matching the five-part preserve-libs
  registration precedent) rather than the full architectural rewrite in
  one slice: new `unsatisfied_root_deps_atoms`/`resolve_root_deps_
  build_entry` resolve an unsatisfied `DEPEND`/`BDEPEND` atom against the
  running root directly (reusing `resolve_pretend` wholesale) and add it
  as its own real `GraphEntry` (`targets_running_root: true`, new field).
  Also fixed in the same slice: a real, previously-invisible bug where
  such an atom used to *also* fall through and resolve a second time
  against `ROOT` via the ordinary `flat_deps` queue — invisible until the
  new `dev-libs/rootdepsbuildpkg` fixture (deliberately tree-visible,
  unlike `rootdepsprovider`/`rootdepsnonexistent`) exposed it. See
  `README.md`'s own "`emerge --pretend --root-deps`: recursively
  building a new package against the running root" for the full
  grounding, the real `BDEPEND`-always-targets-running-root/`DEPEND`-
  EAPI-conditional finding that motivated confirming scope with the user
  before writing any code, and why this increment is deliberately **not**
  recursive (a real cycle-safety hazard — two packages `BDEPEND`ing each
  other, neither satisfied by the running root, would recurse with no
  cross-call cycle memory).
  ~~deciding whether/how `--pretend` output should visually distinguish a
  `targets_running_root` entry from an ordinary one, left as its own
  separately-scoped follow-up~~ — **shipped 2026-08-27**: new
  `root_suffix` (`pretend.rs`, mirrored in `emerge_pretend_reference.py`)
  ports real `output.py:841-862`'s own `darkgreen("to " + pkg.root)`
  suffix, narrowed to annotate *only* the running-root build entries
  (not every non-`/` `ROOT` entry — that would break contract-suite
  determinism). `--json` grew a `"builds_against_running_root"` field;
  `--tree` carries the marker through the indent. Also fixed a real,
  pre-existing Rust/Python `required_by` divergence the new `--json`/
  `--tree` parity assertions exposed (Python's post-pass unconditionally
  overwrote `required_by`, wiping the build entry's own `[owner]`; Rust
  already guarded it). See `README.md`'s own "`emerge --pretend
  --root-deps`: the running-root build entry is marked in `--pretend`/
  `--json`/`--tree` output".
  ~~walking the new entry's *own* further dependencies (the recursion,
  with cross-call cycle memory)~~ — **shipped 2026-08-27**:
  `resolve_root_deps_build_entries` (replacing the non-recursive
  `resolve_root_deps_build_entry`) walks a running-root build entry's
  own `DEPEND` + `BDEPEND` + **`RDEPEND`** against the running root
  recursively (RDEPEND is the deliberately-broader half, confirmed with
  the user — real `_add_pkg_deps` resolves all three against `pkg.root`
  when `pkg.root` is the running root). Cycle safety via the existing
  `root_deps_build_seen` set (insert-before-recurse); `required_by` now
  names the *immediate* requester so `--tree` nests correctly. Also
  confirmed with the user: an unbuildable, not-installed build dep is
  now surfaced as its own `NoVisibleCandidate` entry (the renderer's
  `!!! no visible ebuild` note) instead of being silently swallowed.
  New `dev-libs/rdr*` fixtures. `unsatisfied_root_deps_atoms` grew a
  `dep_keys` param; `root_deps_unsatisfied` is now a `Vec` not a
  `HashSet` for deterministic entry order. See `README.md`'s own
  "`emerge --pretend --root-deps`: the running-root build walk is
  recursive".
  ~~`IDEPEND` of a running-root build entry~~ — **shipped 2026-08-27**:
  the recursive dep-key set is `["DEPEND", "BDEPEND", "RDEPEND",
  "IDEPEND"]` now (real `depgraph.py:4247-4252` — `IDEPEND` *always*
  targets `_running_root.root`). New `dev-libs/rdri*` fixtures, +1 Rust
  unit test, +1 contract test. `PDEPEND` deliberately still out (real
  portage keeps it a target-`ROOT` concern).
  ~~a *top-level* package's own `IDEPEND` still resolves against `ROOT`
  under `--root-deps`~~ — **shipped 2026-08-27**: `root_deps_satisfied_
  atoms` gained the same `dep_keys` param its complement already had, and
  both ordinary dep-walk sites now pass `["DEPEND", "BDEPEND", "IDEPEND"]`
  to *both* the satisfied and unsatisfied helpers (they must agree, or an
  atom in neither falls through to `ROOT`). New `dev-libs/topidepapp`
  fixture, +1 Rust unit test, +1 contract test. See `README.md`'s own
  "`emerge --pretend --root-deps`: a top-level package's own `IDEPEND`
  vs the running root".
  **Scope note (2026-09-01, confirmed with the user)**: this fork's
  ebuilds are all EAPI 7+ (profiles stay EAPI 5, but `--root-deps` is
  never a profile concern). At EAPI 7+ (`eapi_attrs.bdepend`),
  `depgraph.py:4218-4238` puts the `--root-deps == "rdeps"`
  `ignore_depend_deps` branch inside the `else` of `if
  eapi_attrs.bdepend`, so **`--root-deps=rdeps` is a complete no-op for
  this fork**. What *does* apply at EAPI 7+: `BDEPEND` and `IDEPEND`
  always resolve against the running root (`_running_root.root`,
  genuinely `/`), `DEPEND` against `ESYSROOT` (≈ target `ROOT` when not
  cross-compiling), and bare `--root-deps`/`=True` folds
  `DEPEND`/`BDEPEND`/`IDEPEND` into `RDEPEND` (a debugging flag).
  **Remaining gap, narrowed**: real portage routes `BDEPEND`/`IDEPEND`
  to `/` *unconditionally*, the pilot only under `--root-deps` — but
  this is observable only when `ROOT != /` (building a stage/chroot),
  itself outside the pilot's practical scope, and keeping the
  running-root lookup `--root-deps`-gated is a deliberate testability
  choice (an unconditional lookup would consult the real host's
  `/var/db/pkg` in every contract test). The full multi-root graph
  architecture (a `root` per dependency edge) stays a deliberate
  edge-by-edge approximation.

**Eclass/shell backend**:
- ~~`eclass_locations_value` is single-repo only — no masters-chain
  eclass resolution~~ — **shipped 2026-08-25**: `eclass_locations_value`
  now resolves the real masters chain (`RepoConfig::masters`, already
  resolved elsewhere for profile/USE config stacking) via `portage_repo::
  find_repos(config_root)`, exported in real declared order (own repo
  first, masters after) — see `README.md`'s own "Real eclass
  `inherit()` support: `PORTAGE_ECLASS_LOCATIONS`". Required threading
  `config_root: &Path` through the whole phase-execution call chain
  (`run_commands`/`run_single_phase` down to `phase_env_vars`) and a new
  `config_root` field on `UnmergeOptions`/`PackageOptions` (mirroring
  `MergeOptions::config_root`'s own established shape) — `ebuild_merge.rs`
  already had one. Proven with a real, end-to-end fixture: an overlay
  ebuild inheriting an eclass that only exists in its own master repo,
  never redeclared locally — the exact real case the single-repo v1
  couldn't reach.
- brush strategy #3 (formalize the forked `brush` dependency, track
  fork-vs-upstream-merged fixes) — **done 2026-08-27**: see
  `PORTING/BRUSH_FORK.md` (#1274 merged upstream, #1276 open and the sole
  fork-only fix, ancestry of the pin, re-pin checklist). brush strategy
  #2 (rewrite this repo's own `bin/*.sh` to avoid brush-hostile
  constructs) still open.

### `PORTING/helpers/` reference material

- `PORTING/helpers/devmanual/` — a full local checkout of the Gentoo
  devmanual (`function-reference/`, `tools-reference/`, and per-phase
  `ebuild-writing/functions/*/text.xml` docs). Useful any time real
  ebuild-helper (`doins`, `dodir`, `insinto`, etc.) or phase-ordering
  semantics need grounding.
- `PORTING/helpers/emerge_-1v_--debug_--getbinpkgonly__sys-fs--fuse.log`
  — a real `emerge --getbinpkgonly` debug trace. The remote binpkg
  download + merge (and the `--getbinpkg` mixed source+binary merge)
  shipped 2026-08-31 (see the "Binary packages / fetch" backlog entry
  above); this trace stays useful for the still-open pieces
  (`layout.conf` negotiation, resume, `Packages.gz`).

## Real ebuild phase execution + filesystem merge (shipped; ongoing refinement)

This used to be "the next major phase after dry-run" — it's fully live
now. `ebuild <file> install` runs the real `pretend → setup → unpack →
prepare → configure → compile → test → install` chain via an embedded
`brush` shell driving real, unmodified `bin/*.sh`
(`PORTING/rust/portuale/src/ebuild_phases.rs`). `ebuild <file> merge`
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

Each of these has its own dedicated, cited-source "What this proves"
section in `PORTING/README.md` — read that for the real Python source
grounding, the v1 scope cuts, and a runnable example per feature; this
file only tracks what's still *missing* (see "Open backlog" above).

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
   function in `PORTING/helpers/devmanual/ebuild-writing/functions/`.
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
`PORTING/BRUSH_FORK.md`.**

### Candidate strategies (complementary, not mutually exclusive)

1. **Default to brush, fall back to system bash** on a parse failure —
   not implemented as automatic fallback. What *did* ship instead:
   `ebuild --shell bash|brush` (default `brush`), an explicit, real
   second backend (`_doebuild_spawn()`-shaped `bash <bin_dir>/ebuild.sh
   <phase>` subprocess) a caller can pick directly. See README's own
   "`ebuild --shell bash|brush`" section.
2. **Fix our own `bin/*.sh` to avoid brush-hostile constructs** — still
   open (see "Open backlog" above). Low-risk, immediately effective for
   this repo's own tree, doesn't preempt real-world ebuilds/eclasses.
3. **Maintain a local brush fork with our fixes until upstream merges** —
   the tracking doc now exists: **`PORTING/BRUSH_FORK.md`** (which fixes
   are upstream-merged vs fork-only, the ancestry of the pinned rev, and
   a re-pin checklist). `vivo75/brush` is still pinned by exact commit in
   `portuale/Cargo.toml`; #1274 merged upstream, #1276 (the deadlock fix)
   is open and is the sole reason the pin isn't plain upstream `main`.
   Still to do: bump to upstream once #1276 merges (or rebase the fork
   branch onto current upstream `main` in the meantime).

## How this pilot actually runs, session to session

This is operational knowledge, not written anywhere else in the repo —
worth preserving explicitly since a fresh session has no way to
rediscover it except by trial and error.

The user drives this pilot by literally saying **"next slice"** (or
"scope the next slice") and expects the same rhythm every time:

1. **Ground candidates in real code, not guesses.** Grep for the actual
   restriction/TODO/scope-cut comment (this repo's own doc comments are
   meticulous about citing real portage source — file names, line
   numbers — for wording/behavior instead of inventing it) or read the
   corresponding real `lib/portage`/`lib/_emerge` source directly. A
   surprising number of real slices have come from spotting a doc
   comment's own "deliberately deferred"/"main repo only"/"still out of
   scope" language and checking whether it's still accurate — and from
   catching this file's own backlog claiming something is open when
   README/`git log` shows it already shipped.
2. **Present 2–4 concrete candidate slices via `AskUserQuestion`**, with
   one clearly marked "Recommended" and a short, source-grounded
   rationale for each. Let the user pick — don't just proceed on the
   most obvious option.
3. **Re-open judgment calls that surface during implementation**, rather
   than silently picking a default. Mirrors this pilot's own repeatedly-
   documented norm ("confirmed with the user directly" appears throughout
   `README.md`) — e.g. whether a sub-feature should be included in v1 or
   deferred, or being willing to *correct* an earlier claim once deeper
   investigation shows it was wrong, rather than quietly shipping the
   original framing. If a slice turns out to conflict with an existing
   hard constraint (e.g. test-suite determinism), stop and surface the
   conflict explicitly rather than picking a shortcut — see the
   `--root-deps` slice for a worked example (real `ESYSROOT` defaults to
   the host's own `/`, which would have made every fixture test
   host-dependent; resolved by scoping the real behavior as new,
   strictly opt-in machinery instead).
4. **Implement in both language sides in lockstep**: `PORTING/rust/...`
   and `PORTING/python/emerge_pretend_reference.py` must stay
   behaviorally identical, verified *empirically* — run both directly
   against `PORTING/fixtures` and diff the output — not just by trusting
   the pytest suite to catch a divergence. (Real-execution-only features
   — `merge`/`unmerge`/`package`/`fetch`/real phase execution — have no
   Python mirror at all, since they're real filesystem mutation, not
   part of the dry-run/pretend contract; only their CLI-recognition
   surface needs mirroring for the contract suite to still match.)
5. **Add fixtures by hand** under `PORTING/fixtures/repo/...` (or
   `PORTING/fixtures/overlay/...`, or `PORTING/fixtures/var/db/pkg/...`
   for vdb-only entries) plus matching `metadata/md5-cache/...` entries
   when a slice needs new test data. No generator script for the
   ebuild-tree fixtures. Before spending fixture-authoring effort on a
   scenario, sanity-check that it actually *distinguishes* the new
   behavior from any pre-existing mechanism that might produce the same
   observable result for an unrelated reason — a fixture that "passes"
   without truly isolating the change under test is worse than no
   fixture at all. **Check for naming collisions with existing tracked
   fixtures before creating new ones** (`git status`/`grep` the fixtures
   tree first) — a silent overwrite of a pre-existing fixture has broken
   an unrelated test before.
6. **Add both a parametrized `CASES` entry** in
   `PORTING/tests/test_emerge_pretend_contract.py` **and at least one
   dedicated pinned-output test function** (exact stdout/stderr/exit-code
   assertions) for dry-run features, plus a matching Rust unit test in
   the relevant crate's own `#[cfg(test)]` module. Real-execution
   features get Rust unit tests (often fixture-driven, real end-to-end
   merge/unmerge/etc.) instead, since they have no Python side to
   contract-test against.
7. **Update `PORTING/README.md`**: append a new paragraph to the "What
   this proves" narrative (never rewrite prior slices' own paragraphs —
   they're a historical record) and add a runnable example to "Running
   it", *live-verified* against the actual built binaries, not written
   from memory. When a slice reveals that an *earlier* paragraph's own
   claim has gone stale, fix that paragraph in place rather than leaving
   two contradictory claims in the file. (This file, `PROMPT-next.md`,
   gets the same treatment for its own "Current state"/"Open backlog"
   sections, though it decays faster and is worth a full re-derivation
   pass now and then rather than only incremental patching.)
8. **Run the full verification pass** before considering a slice done:
   `cargo fmt --check`, `cargo clippy --release --all-targets` (zero
   warnings, not just zero errors), `cargo test --release` (whole
   workspace), and `python3 -m pytest PORTING/tests -q` (whole suite,
   not just the new cases).
9. **Only `git commit`/`git push` when explicitly asked** — these are
   separate, later requests each time, never implied by finishing a
   slice. Commit messages follow a `<what changed>: <short description>`
   title, wrapped body explaining the *why* and the real-source grounding,
   `Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>` trailer.
10. **Track slices as tasks** (`TaskCreate`/`TaskUpdate`) — one task per
    shipped slice, marked `completed` only once the full verification
    pass in step 8 is green and `README.md` is updated.

## How to use this prompt

Treat "Context" through "Ownership" above, and the phase-execution
investigation's "bash-execution-backend question"/"Candidate strategies"
sections, as settled, citation-backed decisions/findings — not things to
re-derive from scratch. "Current state" and "Open backlog" were freshly
re-derived on 2026-08-25 (grounded directly against `README.md`, `git
log`, and the actual source, not copied from an earlier version of this
file) but are still just a snapshot — re-verify against current
`README.md`/`git log`/the task list before assuming any of it still
holds, and for the bash-backend investigation specifically, check the
live `reubeno/brush` PR/crate state against `PORTING/BRUSH_FORK.md` (as
of 2026-08-27: #1274 merged, #1276 open; a new release may exist, new
incompatibilities may have been found). If
something here conflicts with current reality, or a genuinely open
decision isn't covered above, ask before proceeding rather than assuming.
