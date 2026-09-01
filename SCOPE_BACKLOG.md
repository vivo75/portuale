# Scope backlog

**Not** a Python-vs-Rust parity backlog. Every slice ships on both sides in
one commit, verified byte-identical via the shared contract suite before it
counts as done (`PROMPT.md`'s "portability of change, not of source"). 690
`emerge`-contract cases pass as of this writing (951 across all suites); an
inventory scan (CLI flag tables, function-level architecture, `--json`
fields, git history) still finds zero Rust-vs-Python behavioural gaps.

This file inventories real portage behaviour not yet ported to **either**
side — deliberate, documented scope cuts and `PROMPT.md` architecture
boundaries. Re-verify against `README.md` / `git log` / the source before
trusting any entry; **`README.md`'s "What this proves" section is the
authoritative record of what has shipped.**

> **Rewritten 2026-08-31** (compaction pass). The previous version had
> accreted a shipped-item narrative for every slice — that history now
> lives in `README.md` and `git log`. This version keeps only: a compact
> "already done" summary (Part 1), the genuinely-remaining work (Part 2),
> the standing non-goals (Part 3), and an honest distance-to-parity
> assessment (Part 4).

---

## Part 1 — already shipped (summary)

The core `emerge` / `ebuild` loop is **real and live**, verified against an
actual Gentoo tree (`app-arch/unzip`, `sys-fs/fuse`, `app-arch/xz-utils`
built and merged end to end inside `PORTING/TEST/`'s container):

- **Dependency resolution / `--pretend`**: full atom + slot + sub-slot +
  USE-dep grammar; `||` groups; `DEPEND`/`RDEPEND`/`BDEPEND`/`PDEPEND`/
  `IDEPEND`; `--update`/`--deep`/`--newuse`/`--changed-use`/`--changed-deps`
  (structured `use_reduce` comparison)/`--changed-slot`/`--with-bdeps`/
  `--with-test-deps`/`--emptytree`/`--noreplace`/`--selective`/`--onlydeps`/
  `--nodeps`/`--exclude`/`--newrepo`; `package.mask`/`.unmask`/`.use`/
  `.accept_keywords`/`.license`/`.provided`; `USE_EXPAND` (+ `_IMPLICIT` /
  `_HIDDEN` / `_UNPREFIXED` / `IUSE_EFFECTIVE`); `REQUIRED_USE`; keyword +
  license + PROPERTIES + RESTRICT masking; slot-operator (`:=`) rebuild
  edges + `--ignore-built-slot-operator-deps`; blockers; slot-conflict
  *detection*; `--root-deps` (recursive running-root build walk); the full
  real `resolver/output.py` bracket layout + `[old-ver]` column + ANSI
  colour + counters line + `--tree`/`--columns`; `--autounmask` *keyword*
  and *USE* **resolution** (implicit flip, re-resolve, print the block,
  exit 0) incl. the `opt=` parent flip; `--json` with a per-entry
  mask/keyword/USE provenance trace; `--getbinpkg`/`--getbinpkgonly`
  `--pretend` (binrepos.conf / PORTAGE_BINHOST / cached `Packages` index).

- **Real ebuild phase execution**: the full `pkg_pretend → setup → unpack
  → prepare → configure → compile → test → install` chain via an embedded
  `brush` (Rust-native bash) driving unmodified `bin/*.sh`; real eclass
  `inherit()` across the masters chain; real `SRC_URI` fetch via `wget`
  (Manifest digests, `mirror://` + `custommirrors` + `thirdpartymirrors`,
  `FEATURES=distlocks`, `RESTRICT=mirror`/`fetch`, `mirror+`/`fetch+`).

- **Real filesystem mutation**: `ebuild <file> merge`/`unmerge`/`qmerge`/
  `package`/`config`/`info`/`prerm`/`postrm`; real `CONFIG_PROTECT` (obj +
  sym, `NOCONFMEM`, `new_protect_filename` reuse, `_installed_instance`),
  `FEATURES=collision-protect`/`protect-owned`, preserve-libs (full
  `LinkageMap`/`findConsumers`/`_find_libs_to_preserve` computation +
  registration wired into both merge and unmerge), `env_update()`/
  `ldconfig`, fifo/device `CONTENTS` nodes, real `INFOPATH` cleanup,
  `FEATURES=unmerge-orphans`, the `others_in_slot` reverse-dep check.

- **`emerge` itself, non-`--pretend`** (every `requires --pretend` gate is
  gone): `emerge <atom>` real source build + merge (`New` / `Upgrade` /
  `Downgrade` / `Reinstall`, in-place same-slot replace); `--getbinpkg`/
  `--getbinpkgonly` remote download + merge (all four `pkg_*` hooks from
  `environment.bz2`, same-slot replace, collision/blocker/preserve-libs
  parity); `--buildpkgonly`; `FEATURES=buildpkg` / `--buildpkg`/`-b` /
  `--buildpkg-exclude`; `--keep-going` (BFS-drop failed entries' dependents);
  world-file recording (real `create_world_atom` incl. slot atoms);
  `emerge @set` build support + `world_sets` recording; `@world` / `@selected`
  / `@system` / `@installed` / `@<custom>` set expansion; `--oneshot`/`-1`;
  `emerge -C`/`--unmerge` real removal + world deselect; `emerge --depclean`
  / `--prune` / `--prune --nodeps` real removal (reachability closure,
  `bdeps="auto"`, topological removal order incl. slot-op priority +
  cycle-break single-node pop, `--depclean-lib-check` soname scan,
  `unresolved_deps()` safety halt, `--verbose` reverse-dep display);
  `emerge --config <atom>` real `pkg_config`; `FEATURES=unmerge-backup`;
  `emerge --deselect` real `world` / `world_sets` rewrite.

- **Binary packages**: xpak + gpkg readers/writers, `$PKGDIR` directory
  scan, `--usepkg`/`--usepkgonly`/`--binpkg-respect-use`/`--usepkg-exclude`/
  `-include`/`--rebuilt-binaries`, real `PORTAGE_COMPRESSION_COMMAND` (all
  six compressors), `build-info`-into-vdb metadata + `:=` binding.

See `README.md` for the cited-source grounding of each, and `git log
--grep portuale` / `--grep portage-repo` for the slice-by-slice history.

---

## Part 2 — genuinely still open

### A. Resolver

Every *forward-pass* resolver feature is shipped (the `--autounmask*`
family completed 2026-08-31). What remains is architectural — a
single-pass BFS can't grow into these incrementally:

- **Backtracking.** *Slices 1–4 shipped 2026-09-01.*
  `resolve_pretend_graph` is now a `'backtrack` retry loop (real
  `_emerge/resolver/backtracking.py` shape): each pass rebuilds `entries`
  from scratch, only `slot_constraints` (keyed by `cat/pkg`) + the
  iteration counter survive. A **solvable slot conflict** — one version
  satisfies every atom that landed on the slot — folds those atoms into
  `slot_constraints` (fed to `resolve_pretend`'s `extra_constraints`
  param) and re-runs the whole walk, up to `backtrack_max`. The slices:
  - **Slice 1:** the retry loop + solvable slot-conflict reconciliation.
  - **Slice 2 shipped 2026-09-01:** real `--backtrack=COUNT` flag
    (`backtrack_max` param, default 10, `=0` disables — replaces slice 1's
    `MAX_BACKTRACK` constant).
  - **Slice 3 shipped 2026-09-01:** unsolvable conflict → real
    `runtime_pkg_mask` (`extra_constraints` gained a `!`-negation form,
    `slot_pullers` tracking, a trial-and-revert state machine — mask the
    conflicted version + puller-parent versions with a lower alternative,
    keep only if every conflict clears with no new `NoVisibleCandidate`).
  - **Slice 4 shipped 2026-09-01:** the real `_show_slot_collision_notice`
    block (`SlotConflict.instances` — every conflicting version + its
    `(parent_cpv, atom)` pullers, via `build_slot_conflict`), the real
    advisory paragraph + `--backtrack=30` hint gated the real way.
  - **Deferred (backlog):** "backtracking exhausted" / "circular
    dependencies prevent backtracking" diagnostics; autounmask levels
    tried in sequence inside the loop; autounmask parent-flip re-resolve
    feeding `extra_constraints`; the `resolve_graph_once` helper
    extraction (drop slice 1's `loop {}` reindent); real
    `get_conflict()`'s `collision_reasons` grouping / best-atom
    selection / `--verbose-conflicts` USE markers / stderr stream.
  `PROMPT.md` lists a backtracking resolver as out of scope for v1;
  slices 1–4 nonetheless take it from "detects and reports conflicts" to
  "reconciles solvable conflicts, masks unsolvable ones, and reports the
  rest with real portage's own notice". The deferred items above are
  refinements, not the core mechanism.
- ~~**`--autounmask-license` / `--autounmask-keep-masks=n`.**~~ **Shipped
  2026-08-31** (autounmask buildout increments 4 + 5): `license_masked_only`
  / `mask_masked_only` (+ `missing_licenses` = real `_getMaskedLicenses`
  list form), `autounmask_suggest_license` / `autounmask_suggest_masks`
  gates (license off unless `--autounmask` explicit or `=y`; masks off
  unless `--autounmask-keep-masks=n` — real KEEPS masks by default),
  `GraphResult::autounmask_license_changes` / `autounmask_mask_changes`,
  the `The following {license,mask} changes are necessary to proceed:`
  blocks (real `_writemsg` order: keyword, mask, USE, license), `--json`
  arrays. New `dev-libs/licensemasked{pkg,consumer}` +
  `dev-libs/maskmaskedconsumer` fixtures. **The whole `--autounmask*`
  family is shipped now** — only `--autounmask-write` (file-writing)
  remains, a `PROMPT.md` non-goal.
- **`--root-deps` / multi-root, remaining edges.** *Mostly a non-gap for
  this fork.* This fork's ebuilds are all EAPI 7+, and at EAPI 7+
  (`eapi_attrs.bdepend`, `depgraph.py:4218-4238`) **`--root-deps=rdeps`
  is a complete no-op** — its `ignore_depend_deps` branch sits inside
  `else: if eapi_attrs.bdepend`. What applies at EAPI 7+: `BDEPEND` +
  `IDEPEND` always resolve against the running root (`/`), `DEPEND`
  against `ESYSROOT` (≈ target `ROOT`), and bare `--root-deps`/`=True`
  folds them into `RDEPEND` (a debugging flag). The one real residual:
  the pilot routes `BDEPEND`/`IDEPEND` to `/` only *under* `--root-deps`,
  real portage does it unconditionally — observable only when `ROOT != /`
  (a stage/chroot build), which is outside the pilot's practical scope,
  and the `--root-deps` gate is a deliberate testability choice (an
  unconditional running-root lookup would hit the real host's vdb in
  every contract test). `PDEPEND` of a running-root entry stays a
  target-`ROOT` concern (a permanent non-gap). The full multi-root graph
  (a `root` per dependency edge) stays a deliberate edge-by-edge
  approximation.
- **Slot-operator rebuild v1 cuts**: single-pass (no backtracking for a
  rebuild that itself shifts another sub-slot), the rebuilt consumer's own
  `:=` deps not re-bound in the pretend graph, no `--changed-slot`
  interaction, `IUSE_EFFECTIVE` in the built-dep domain.
- **`package.provided` depclean corner**: a provided CPV as a depclean
  root, and the `-pc` advisory's "will be removed by depclean even if in
  world" wording.
- Minor: `dependency_avoid_update_candidate` version-only cross-slot match
  — **fixed 2026-08-31**, listed here only so a future re-derivation
  doesn't re-flag it.

### B. Scheduler / build orchestration

- **Parallel builds.** *Shipped 2026-09-01:* `emerge -jN` / `--jobs=N`
  (`run_build_scheduler`, DAG-aware dispatch, `std::thread::scope`
  workers, serialized vdb merge, `--keep-going` preserved) + real
  `--load-average`/`-l` throttle + per-package build-log capture
  (`run_commands_logged` → `${T}/build.log`, real `PORTAGE_LOG_FILE`;
  captured builds forced onto the `bash` backend) + the `>>> Jobs: X of
  Y complete` status line + build-log tail folded into a failure report.
  Remaining: the merge step's `pkg_*` hooks still run uncaptured through
  brush (residual stderr noise); `--quiet-build[=y|n]` isn't a flag yet
  (capture is `-j >1`-only); one tokio runtime per `run_commands`;
  killing in-flight builds on a hard failure; `PORTAGE_LOGDIR` /
  `split-log`.
- **`--resume` / `--skipfirst`.** *Shipped 2026-09-01:* a failed source
  `emerge <atoms>` writes `mtimedb["resume"]` (`mtimedb.rs`);
  `emerge --resume [--skipfirst]` replays the saved mergelist. Remaining:
  `resume_backup` rotation; `--resume --pretend` list display; carrying
  the original `myopts`; binary-entry replay.
- **`--ask` / interactive prompts.** *Shipped 2026-09-01:* `--ask`/`-a`
  prompts before a real `emerge <atom>` merge and before `-C` /
  `--depclean` / `--prune` removal (`ask_confirm`, exit 130 on No), and
  the `CLEAN_DELAY` countdown runs before every real removal
  (`clean_delay_countdown`). Remaining: `--ask` for `--config` /
  `--deselect`; TTY gating; prompt colour; re-prompt on a bad answer.
- **`elog` / `PORTAGE_ELOG_*`.** *Shipped 2026-09-01:* the `echo` module
  (`elog::echo_summary` — the `* Messages for package <cpv>:` block, real
  `mod_echo`; default-on, filtered by `PORTAGE_ELOG_CLASSES`);
  `create_directories` makes `${T}/logging` so `elog`/`ewarn` reach it.
  Remaining: `save` / `save_summary` (file-writing, `PORTAGE_LOGDIR`);
  `mail*`; binpkg-merge / unmerge `pkg_*` elog collection.
- **`PORTAGE_NICENESS` / `PORTAGE_IONICE_COMMAND`.** *Shipped
  2026-09-01:* `apply_portage_scheduling_policy` (real
  `actions.py::apply_priorities`) renices/ionices this process at startup.
  Remaining: `PORTAGE_SCHEDULING_POLICY` (`chrt`); `shlex` quoting for
  the ionice command.

### C. Config resolution depth

- **`package.use` per-level `USE_ORDER` layering.** *(Shipped
  2026-09-01: the three `package.use` sources now land in their own
  `Config` fields at their own real positions — `package_use_repo`
  before the IUSE `pkginternal` seed, `package_use` (profile) in the
  `defaults` layer before `make.conf`, `package_use_user` in the `pkg`
  layer after it; `use_tokens` split into profile `make.defaults` +
  `conf_use_tokens`. `effective_use_flags` does the real reversed-
  `USE_ORDER` walk `repo → pkginternal → defaults → conf → pkg`.)*
  Still open: the `env` (`$USE` / `/etc/portage/env` / `package.env`),
  `features`, and `env.d` layers; repo `make.defaults` USE folded into
  `configdict["repo"]`; profile `package.use` interleaved per profile
  level with that level's `make.defaults` (the pilot applies it as one
  group).

### D. Sandbox / build isolation — **substantially complete (2026-09-01)**

The whole `FEATURES` isolation set is modelled: for the six real `src_*`
phases (`SANDBOXED_SRC_PHASES`), `run_one_phase` builds a wrapped bash
subprocess — `unshare <flags> --map-root-user -- sh -c '<config>; exec
"$@"' _ [sandbox] bash bin/ebuild.sh <phase>` (`Isolation` /
`phase_isolation` / `sandbox_wrapped_command`), forcing the `Bash`
backend. All wrappers compose; the `unshare` combo is validated once,
degrading with one warning if unprivileged userns is unavailable.

- **`FEATURES=sandbox` / `usersandbox`**: `sandbox bash …` (real
  `spawn_sandbox`, `/usr/bin/sandbox`); `SANDBOX_LOG=${T}/sandbox.log` +
  `SANDBOX_DISABLED=0` so `bin/ebuild.sh` does its own `SANDBOX_ON=1` /
  `addwrite` setup; the binary logs + non-zero-exits on a write outside
  the build tree. The `bin/misc-functions.sh` calls (`install_qa_check`
  post-`install`, `__dyn_package`) are wrapped too, with a separate
  `${T}/sandbox-misc.log` (real `MiscFunctionsProcess._spawn`). Missing
  binary → unsandboxed + warning (real `free = True`).
- **`FEATURES=network-sandbox`**: `unshare --net` + `ip link set lo up`.
- **`FEATURES=ipc-sandbox`**: `unshare --ipc`.
- **`FEATURES=mount-sandbox`**: `unshare --mount` + `mount --make-rslave /`.
- **`FEATURES=pid-sandbox`**: `unshare --pid --fork --mount-proc`.

Remaining (deliberate cuts): `RESTRICT=network-sandbox` /
`PROPERTIES=live`/`test_network` exemptions (no USE-reduced
`RESTRICT`/`PROPERTIES` in the phase env); the `AI_ADDRCONFIG` loopback
addresses; SELinux sandbox; `userpriv` / `fakeroot` (single-user
dev/test context — see also the `chown` note below).

- Various *non-isolation* `FEATURES` unmodelled (the pilot forces
  `FEATURES=""` into the phase env): `ccache`, `distcc`, `splitdebug`,
  `installsources`, `nostrip`/`strip`, `compressdebug`, `test` gating
  beyond `src_test` running, `preserve-libs` live-`scanelf` orphan
  branch. A scoped real-`FEATURES` passthrough is a separate slice.

### E. Binary packages / fetch

- **Remote binhost**: live `layout.conf` negotiation,
  `RESTRICT=primaryuri` interleave, `Packages.bz2`/`.lz4`, binpkg `SHA1`
  (no sha1 crate). *(Shipped 2026-09-01: `Packages.gz`/`.zst` compressed
  index; binpkg `MD5` verification; SRC_URI fetch **resume**
  (`RESUMECOMMAND` `-c`). SRC_URI `SHA512`/`BLAKE2B` verification was
  already real.)*
- **gpkg**: `.sig` verification + signing (`FEATURES=binpkg-signing` —
  cut, the pilot has no crypto), bare `.xpak` multi-instance,
  `binpkg-multi-instance`, mtime-staleness index revalidation, `BUILD_ID`
  in the basename. *(Shipped 2026-09-01: the internal `Manifest` `DATA`
  digest check (size + BLAKE2B/SHA512, member↔record set match) at merge
  time — `binpkg::verify_gpkg_manifest`, real `gpkg._verify_binpkg`'s
  checksum layer, wired into `extract_binpkg`. `gpkgreadpkg-1.0.gpkg.tar`
  rebuilt with a real Manifest.)*
- **`BUILD_ID` / `splitdebug` / `packdebug` / RPM**, PKGDIR-index locking,
  `FEATURES=buildpkg-live`, real `EbuildBinpkg` failure semantics under
  `--keep-going`.
- Fetch: real candidate ordering / shuffling. *(Running the ebuild's own
  `pkg_nofetch` phase for a missing distfile shipped 2026-09-01 —
  `ebuild_phases::fetch_sources` runs `run_one_phase(env, "nofetch")` on
  any fetch failure.)*

### F. Whole `emerge` actions not implemented

- Standalone actions buildout (2026-09-01). *Shipped: `--list-sets`
  (real `actions.py:3839`); `--search` / `-s` / `--searchdesc` / `-S`
  (real `action_search` → `search.py` output shape); `--check-news`
  (real `count_unread_news` / `display_news_notifications` — v1 cuts:
  no `.unread`/`.skip` persistence, `Display-If-Installed` only);
  `--clean` (real `unmerge` `unmerge_action="clean"` — keep newest per
  slot), `--rage-clean` (fast `--unmerge`), `--info` (real `action_info`
  — narrowed to the deterministic `Repositories:` + `VAR="value"` block;
  the host-state half — version header, uname/mem, tool-version probes,
  `info_pkgs`, timestamps — is a documented cut).*
  Remaining: `--regen` (metadata cache), `--metadata`.
  `--read-news` stays a recognized-unimplemented option (it's a
  post-merge display toggle, not an action). `--sync` (repo network
  syncing) stays a non-goal.
- Recognized-but-unimplemented modifier flags: `--complete-graph[-if-*]`,
  `--rebuild-if-new-{slot,rev,ver}` / `--rebuild-if-unbuilt`,
  `--reinstall-atoms` / `--useoldpkg-atoms` / `--rebuild-exclude` /
  `--rebuild-ignore`, `--dynamic-deps`, `--fuzzy-search`, `--misspell-
  suggestions`, `--package-moves` (`profiles/updates/`), `--quickpkg-direct`,
  `--autounmask-continue` / `--autounmask-backtrack` / `--autounmask-only`.

### G. Shell backend

- **brush strategy #2** — rewrite this repo's own `bin/*.sh` to avoid
  brush-hostile constructs (low-risk, immediately effective for this tree).
- The actual bump to upstream `brush` (blocked on
  [reubeno/brush#1276](https://github.com/reubeno/brush/pull/1276)) +
  periodic rebase. See `BRUSH_FORK.md`.

### H. Misc / cosmetic

- `chown` / privilege-preserving `chmod` not reproduced; directory merge
  order sorted for determinism, not real `os.listdir()` order (cosmetic).
- `profiles/updates/` package moves (`sys-libs/foo` → `sys-libs/bar`).
- `color.map` / `PORTAGE_COLORMAP`.
- `--quiet` verbosity level (1) — the pilot models plain `-p` (2) and
  `-pv` (3) only.

---

## Part 3 — explicit non-goals / architecture boundaries

Standing decisions, not oversights.

- **`--autounmask-write`** and any config-*writing* autounmask mode
  (conflicts with "never writes config" — the read-only suggest/resolve
  half is shipped). Note `emerge --deselect` / `-C` / `--depclean` *do*
  write `world` / `world_sets` / the vdb now — those are user state the
  package manager owns, not `/etc/portage` config.
- **A real backtracking resolver** (see Part 2.A — listed there because it
  is the substantive gap, here because `PROMPT.md` scoped it out for v1).
- **PyO3 / in-process FFI embedding** — would foreclose the
  two-sibling-implementations end state.
- **EAPI 0/1/2/3/4/6** — dead in this repo; the `portage-*` crates have no
  EAPI parametrization at all within the 5+ floor.
- **`bsd_chflags`** — `None` on non-BSD; the pilot is Linux-only/musl-static.
- **RPM binary packages, repo syncing (`emerge --sync`), news items,
  GLSA/`@security`, GPG signing/verification, Prefix/cross-`ROOT` beyond
  the `ESYSROOT` distinction** — not in scope.
- **`equery` / `portageq` / `etc-update` / `dispatch-conf`** — separate
  tools, separate binaries.

---

## Part 4 — how far is this from a "perfect clone that installs and uninstalls"?

**Short answer: the pilot already installs and uninstalls packages for
real** — `emerge <atom>` (source and binary), `emerge -C`, `--depclean`,
`--prune`, `--config`, `--deselect` all perform real filesystem mutation,
with real ebuild-phase execution, real vdb bookkeeping, real
`CONFIG_PROTECT` / `collision-protect` / preserve-libs / `env_update`, and
it has built + merged + unmerged real Gentoo packages end to end. For the
**happy path of operating on one package (or a small dependency closure)
at a time**, it is close.

It is **not** a drop-in replacement, and the distance to one is dominated
by a few large items rather than a long tail of small ones:

1. **Backtracking resolver (Part 2.A).** Real portage re-tries the graph
   through slot / USE / mask / autounmask conflicts. *Slices 1–4
   (2026-09-01)* turned `resolve_pretend_graph` into a real retry loop:
   it reconciles a **solvable slot conflict** (one version satisfies every
   parent atom), masks a version to resolve an **unsolvable** one via real
   `runtime_pkg_mask` (trial-and-revert), exposes the real `--backtrack`
   flag, and reports the rest with real portage's own `!!! Multiple
   package instances …` notice. What's still not covered: USE/keyword
   autounmask levels tried *inside* the loop, and `||`-preference /
   slot-operator-rebuild feedback driving a retry. An upgrade that needs
   portage to juggle all of those together still exceeds the pilot.

2. **The Scheduler (Part 2.B).** *`emerge -jN` parallel builds +
   `--load-average` + build-log capture + the `>>> Jobs:` line + `--ask`
   / `CLEAN_DELAY` + `PORTAGE_NICENESS` / `PORTAGE_IONICE_COMMAND` shipped
   + `elog` `echo` module + `--resume`/`--skipfirst` (mtimedb) shipped
   2026-09-01.* **Part 2.B is now substantially complete** -- remaining:
   `resume_backup` rotation, the elog `save`/`mail` modules,
   `PORTAGE_SCHEDULING_POLICY`, killing in-flight builds on a hard fail.

3. **Config-resolution depth (Part 2.C).** *The `package.use` per-level
   `USE_ORDER` layering shipped 2026-09-01 (`repo`/`pkginternal`/
   `defaults`/`conf`/`pkg` all modeled).* Remaining: the `env`
   (`$USE` / `package.env`), `features`, and `env.d` layers — a config
   that leans on those still diverges.

4. **Sandbox enforcement (Part 2.D).** *Substantially complete
   2026-09-01: `sandbox`/`usersandbox` + `network`/`ipc`/`mount`/`pid`-
   sandbox all wrap the `src_*` phases (and the `misc-functions.sh`
   calls for `sandbox`). Remaining are deliberate cuts: `userpriv` /
   `fakeroot` (single-user dev context), SELinux, the
   `RESTRICT`/`PROPERTIES` exemptions.*

5. **Breadth of actions and flags (Parts 2.E/F).** *`--list-sets`,
   `--search`/`-s`/`-S`, `--check-news`, `--clean`, `--rage-clean`,
   `--info` shipped 2026-09-01.* Remaining: `--regen` / `--metadata`
   (needs a new depend-phase md5-cache-generation capability, Rust-only),
   `--sync` (non-goal), GLSA/`@security`, and the ~dozen
   recognized-but-unimplemented modifier flags.

Everything else in Part 2 is genuinely incremental — one focused slice
each, the rhythm this pilot already runs at. Items 1–4 above are the
architectural work that would turn "installs and uninstalls packages"
into "is portage."
