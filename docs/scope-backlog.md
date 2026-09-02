# Scope backlog

**Not** a Python-vs-Rust parity backlog. Every slice ships on both sides in
one commit, verified byte-identical via the shared contract suite before it
counts as done (`agent-context.md`'s "portability of change, not of source"). 690
`emerge`-contract cases pass as of this writing (951 across all suites); an
inventory scan (CLI flag tables, function-level architecture, `--json`
fields, git history) still finds zero Rust-vs-Python behavioural gaps.

This file inventories real portage behaviour not yet ported to **either**
side — deliberate, documented scope cuts and `agent-context.md` architecture
boundaries. Re-verify against `what-this-proves.md` / `git log` / the source before
trusting any entry; **`what-this-proves.md` is the
authoritative record of what has shipped.**

> **Rewritten 2026-08-31** (compaction pass). The previous version had
> accreted a shipped-item narrative for every slice — that history now
> lives in `what-this-proves.md` and `git log`. This version keeps only: a compact
> "already done" summary (Part 1), the genuinely-remaining work (Part 2),
> the standing non-goals (Part 3), and an honest distance-to-parity
> assessment (Part 4).

---

## Part 1 — already shipped (summary)

The core `emerge` / `ebuild` loop is **real and live**, verified against an
actual Gentoo tree (`app-arch/unzip`, `sys-fs/fuse`, `app-arch/xz-utils`
built and merged end to end inside `TEST/`'s container):

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

See `what-this-proves.md` for the cited-source grounding of each, and `git log
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
  - **Slice 5 shipped 2026-09-02:** the real `_show_circular_deps` block.
    Commit 1 gave every graph edge a build-time/run-time priority (the
    BFS had concatenated all five `*DEPEND` keys before flattening):
    each dep string is re-flattened over `DEPEND`+`BDEPEND` and over
    `RDEPEND`+`PDEPEND`+`IDEPEND`, an atom in the first and not the
    second is tagged `buildtime_hard` on its `QueueItem`, and
    `resolve_pretend_graph` accumulates an `EdgeKindMap`
    (`(target cp, owner cp) → (has_hard, has_soft)`).
    `topological_merge_order` now breaks a cycle at a run-time edge
    (real `_serialize_tasks`' `_ignore_runtime`) before falling back to
    discovery order. Commit 2 added `find_hard_cycles` (shortest cycle
    over the hard-edge digraph among merge-bound entries),
    `GraphResult::circular_deps`, and the `pretend.rs` renderer — real
    `_prepare_circular_dep_message`'s `<cpv> depends on` / ` <cpv>
    (buildtime)` chain + the `* Error: circular dependencies:` header +
    the generic advisory, exit 1. New `dev-libs/hardcycle{a,b}` fixture
    (mutual `DEPEND`, empty `RDEPEND`). The pure-`RDEPEND`
    `cycle-a`/`cycle-b` cycle stays exit 0. **Documented cuts:** the
    reduced cycle-only `--tree` re-display
    (`self.display(handler.merge_list)`); `_find_suggestions`'s
    ~180-line USE-flag heuristic (always the generic-advisory `else`
    branch); full elementary-cycle enumeration / `large_cycle_count`.
  - **Deferred (backlog):** "backtracking exhausted" diagnostics;
    autounmask levels tried in sequence inside the loop; autounmask
    parent-flip re-resolve feeding `extra_constraints`; the
    `resolve_graph_once` helper extraction (drop slice 1's `loop {}`
    reindent); real `get_conflict()`'s `collision_reasons` grouping /
    best-atom selection / `--verbose-conflicts` USE markers / stderr
    stream; the circular-dep cuts listed under slice 5.
  `agent-context.md` lists a backtracking resolver as out of scope for v1;
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
  remains, an `agent-context.md` non-goal.
- **`--root-deps` / multi-root, remaining edges.** *Mostly a non-gap for
  this fork.* This fork's ebuilds are all EAPI 7+, and at EAPI 7+
  (`eapi_attrs.bdepend`, `depgraph.py:4218-4238`) **`--root-deps=rdeps`
  is a complete no-op** — its `ignore_depend_deps` branch sits inside
  `else: if eapi_attrs.bdepend`. What applies at EAPI 7+: `BDEPEND` +
  `IDEPEND` always resolve against the running root (`/`), `DEPEND`
  against `ESYSROOT` (≈ target `ROOT`), and bare `--root-deps`/`=True`
  folds them into `RDEPEND` (a debugging flag). ~~The one real residual:
  the pilot routes `BDEPEND`/`IDEPEND` to `/` only *under* `--root-deps`,
  real portage does it unconditionally~~ **closed 2026-09-02** —
  `pretend.rs::resolve_root_deps_running_root` now enables running-root
  resolution whenever `--root-deps` is set **or** `running_root != target
  ROOT` (a cross-root/stage build); a strict no-op when they coincide.
  Determinism is preserved by pinning `PORTAGE_RUNNING_ROOT` to the
  fixture `ROOT` in `fixture_env`. `PDEPEND` of a running-root entry
  stays a target-`ROOT` concern (a permanent non-gap). The full
  multi-root graph (a `root` per dependency edge) stays a deliberate
  edge-by-edge approximation.
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
  *Shipped 2026-09-02:* the **`save`** module (one
  `<logdir>/elog/<cat>:<pf>:<stamp>.log` per package, `FEATURES=split-elog`
  aware) and **`save_summary`** (append to `<logdir>/elog/summary.log` —
  ON by default), both writing real `_combine_logentries` format with
  per-module `:levels` overrides; `<logdir>` is `$PORTAGE_LOGDIR` else
  `<root>/var/log/portage` (root-relative — a documented divergence from
  real `<BROOT>/var/log/portage`). **`mail` / `mail_summary`** are out of
  scope (a real SMTP client + MIME assembly is not light) — a one-line
  "unsupported" notice, then skipped. `syslog` / `custom` unported.
  Remaining: binpkg-merge / unmerge `pkg_*` elog collection.
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
  *(Shipped 2026-09-02: the **process-environment** half of the `env`
  layer — `ACCEPT_KEYWORDS=~amd64 emerge foo`, `USE="-X" emerge bar`,
  `VIDEO_CARDS=… emerge baz`, `CFLAGS=… emerge --info`. New
  `portage_profile::apply_env_layer` over a curated allowlist
  (`ENV_INCREMENTAL_VARS` / `ENV_SCALAR_VARS`), applied right after
  `make.conf`. Narrowing: env `USE` lands at the `conf` layer, not its
  real `env` position above the user-level `package.use`; env
  `USE_EXPAND` variable values are last-wins into `scalars`, not
  genuinely incremental. Test isolation: `conftest.py` strips these vars
  process-wide for the session.)*
  Still open: `/etc/portage/env` / `package.env` (the other half of the
  `env` layer); the `features` and `env.d` layers; repo `make.defaults`
  USE folded into `configdict["repo"]`; profile `package.use` interleaved
  per profile level with that level's `make.defaults` (the pilot applies
  it as one group).

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
  **Shipped 2026-09-02 (batch 11): `--regen`** (real `action_regen` →
  `MetadataRegen`) — a real write action: run every ebuild's `depend`
  phase (`ebuild_phases::run_depend_phase`, a `PORTAGE_PIPE_FD`-wired
  `bash bin/ebuild.sh depend` spawn) and (re)write
  `metadata/md5-cache/<cat>/<pf>` in real `portage.cache.flat_hash`
  format (new `regen.rs`). v1 cuts: sequential (no `--jobs` threading),
  no stale-entry pruning, `_eclasses_` from `INHERITED` md5s (this repo
  only, no masters chain). **`--metadata`** is recognized but an
  architectural no-op — real `action_metadata` transfers a repo's
  pre-generated cache into portage's own `depcachedir`, which this pilot
  doesn't model (it reads `metadata/md5-cache` directly); it prints the
  real `>>> Updating Portage cache` header and exits 0. `--regen` /
  `--metadata` reject `--pretend` with the exact real `actions.py:4106`
  message. **`--sync`** is a permanent non-goal — `emerge --sync` prints
  `Functionality has moved to \`emaint sync\`.` and exits 1 (with or
  without `--pretend`); repo syncing will never be part of portuale.
  `--read-news` stays a recognized-unimplemented option (it's a
  post-merge display toggle, not an action).
- Recognized-but-unimplemented modifier flags (user asked 2026-09-02 to
  implement the batch; shipping one coherent slice per commit).
  **Shipped 2026-09-02 (batch 1): `--fuzzy-search` / `--regex-search-auto`
  / `--search-similarity`** (`--search` now fuzzy + regex-auto by
  default, real `search.py`; new `difflib.rs` port); **`--autounmask-only`**
  (real `actions.py:456` — resolve, show only `display_problems()`,
  exit 0; new `show_merge_list` gate); **`--ask` table cleanup** (it was
  already implemented, just still listed as recognized-unimplemented).
  **Shipped 2026-09-02 (batch 2): `--reinstall-atoms ATOMS`** (real
  `WildcardPackageSet` — force-reinstall a matching already-installed
  package; scoped `--emptytree` rewrite in `resolve_pretend_graph`, new
  `reinstall_atoms` param).
  **Shipped 2026-09-02 (batch 3): `--rebuild-if-{unbuilt,new-rev,new-ver}`
  + `--rebuild-if-new-slot` + `--rebuild-exclude` / `--rebuild-ignore`**
  (real `_rebuild_config.trigger_rebuilds` — rebuild an installed
  package whose vdb build-time dep is being merged; new
  `portage-repo::rebuild_if_entries`, six new `resolve_pretend_graph`
  params, `rebuild{trigger,consumer,nochange}` fixtures).
  **Shipped 2026-09-02 (batch 4): `--dynamic-deps` / `--dynamic-deps=n`**
  (real `create_depgraph_params.py` — `=n` walks an installed package's
  vdb `*DEPEND` snapshot for its `--deep` recursion instead of the
  current ebuild; `enqueue_dependencies` gained `root`/`dynamic_deps`).
  **Shipped 2026-09-02 (batch 5): `--misspell-suggestions`** (real
  `_similar_name_search` — `difflib.get_close_matches` package-name
  suggestions for a missing `cat/pkg`; new `difflib::get_close_matches`,
  `pretend.rs::misspell_suggestion_block`, no new resolver param).
  **Shipped 2026-09-02 (batch 6): `--package-moves` / `--package-moves=n`**
  (real `actions.py:3675` — `=n` disables `profiles/updates/` move
  application; new `portage_repo::set_package_moves_enabled` process-
  global, no new resolver param).
  **Shipped 2026-09-02 (batch 7): `--complete-graph[=y|n]` +
  `--complete-graph-if-new-use` / `--complete-graph-if-new-ver`** (real
  `create_depgraph_params.py:169-175` + `depgraph.py::_complete_graph` —
  `--complete-graph` and any `--rebuild-if-{unbuilt,new-rev,new-ver}` set
  `myparams["complete"]`, which toggles a forced deep walk; the two
  `-if-new-*` triggers default ON and auto-enable it via a CLI-layer
  two-pass when a run changes an installed package. In this `--pretend`
  pilot complete mode's other facets — installed-only selection,
  `@world`/`@system` seeding — are provably inert, so the forced deep
  walk is the whole observable delta. New `resolve_pretend_graph`
  `complete` param + `portage_repo::complete_graph_auto_enable`, new
  `completegraphpkg` fixture).
  **Shipped 2026-09-02 (batch 8): `--useoldpkg-atoms ATOMS`** (real
  `main.py:713` → `WildcardPackageSet`; `depgraph.py:7936` +
  `matched_oldpkg` / `visible_matches` — for a matching package, prefer
  an existing binary package over a newer unbuilt ebuild. New
  `portage_repo::set_useoldpkg_atoms` process-global + a `matched`
  restriction in `resolve_pretend`; only bites under
  `--usepkg`/`--getbinpkg`; new `useoldpkgpkg` fixture).
  **Shipped 2026-09-02 (batch 9): `--autounmask-continue` /
  `--autounmask-backtrack`** — both recognized + validated but inert in a
  `--pretend`-only pilot (real portage gates write-and-continue on
  `"--pretend" not in myopts`, `depgraph.py:5796`, and the pilot has no
  backtracking resolver). The one real observable is the
  `actions.py:3772` `--autounmask-continue has been disabled by
  --autounmask=n` warning.
  **Shipped 2026-09-02 (batch 10): `--quickpkg-direct` /
  `--quickpkg-direct-root`** (real `actions.py:150-164` +
  `bintree._populate_additional` — when `--usepkg` + `--quickpkg-direct=y`
  + target `ROOT` ≠ source root, every package installed in the source
  root joins the binary-candidate pool for the target build, from that
  root's own vdb metadata. New `portage_repo::set_quickpkg_direct_root`
  process-global + `quickpkg_direct_index_entries`; `local_binpkg_index`
  injects the synthesized `Packages`-style records. New `quickpkgroot`
  fixture tree. Documented cut: the `_quickpkg_direct_deps_unsatisfied`
  "requires all dependencies to be merged for root" error — needs a
  running-root merge task, which this pilot's pretend model rarely
  produces).
  **The recognized-but-unimplemented modifier-flag list is now clear.**

### G. Shell backend

- ~~**brush strategy #2** — rewrite this repo's own `bin/*.sh` to avoid
  brush-hostile constructs.~~ **Done 2026-09-01** — the only
  function-as-non-last-pipeline-stage in `bin/*.sh` was the three
  `__save_ebuild_env | __filter_readonly_variables [| bzip2]` pipes in
  `bin/phase-functions.sh`; a new `__save_and_filter_ebuild_env` helper
  stages both functions through a `${T}` temp file instead. The brush
  pin dropped from `c78ea429` (fork-only deadlock fix `reubeno/brush#1276`)
  to `879d963` (just the upstream-merged #1274), full `portuale` suite
  green — incl. the `install_does_not_deadlock…` regression, which hangs
  the deadline against `879d963` *without* the script rewrite. See
  `brush-pin.md` and `what-this-proves.md`'s "brush strategy #2".
- ~~The actual bump to upstream `brush`~~ **Done 2026-09-01** — pinned
  to real `reubeno/brush` `main` (`a04b09dc`, at/after the #1274 merge
  `18851e7`); the `vivo75/brush` fork is gone. Whole workspace + pytest
  green; `install_does_not_deadlock…` completes in ~1s (no #1276 patch
  needed at all now).
- ~~**Default phase-execution backend**~~ **Flipped to `bash` 2026-09-01**
  — real-world testing found brush's `declare -f` corrupts functions with
  redirected here-docs (`toolchain-funcs.eclass`'s `_tc-has-openmp`),
  breaking `emerge <atom>` for compiled packages. `ShellBackend`'s
  default is now `Bash` (real `bash` subprocess); `brush` stays available
  via `--shell brush` on both `emerge` and `ebuild`. See `brush-pin.md`,
  "`declare -f` mangles a function with a redirected here-document".
- ~~**`PORTAGE_PYM_PATH` unset broke eclass `has_version`/`best_version`**~~
  **Fixed 2026-09-01** — `phase_env_vars` sets it to `<checkout>/lib`; the
  `bin/` `import portage` helper shims (`portageq-wrapper`,
  `ebuild-pyhelper`, `save-ebuild-env.sh`) no longer abort on their
  `cd "${PORTAGE_PYM_PATH}" || exit 1`. `emerge -v app-portage/eix` now
  completes a full real merge against a live `~amd64` tree.
- ~~**Spurious `QA Notice: Eclass '…' inherited illegally in … <phase>`**~~
  **Fixed 2026-09-01** — `compute_environment` reads the eclass list from
  the ebuild's `metadata/md5-cache` `_eclasses_` and `phase_env_vars`
  exports `INHERITED` (real `porttree.py:872`), so `bin/ebuild.sh`'s
  `__INHERITED_QA_CACHE` snapshot suppresses the notice on a phase
  re-source, exactly as real portage does.
- **Still open (shell backend):** minimize + report the `declare -f`
  heredoc bug upstream; periodic re-pin to keep up with upstream `main`
  (see `brush-pin.md`'s checklist).
  ~~`emerge --shell` does not yet reach the `prerm`/`postrm` or
  `pkg_config` paths~~ **Done 2026-09-02** — `--shell` now threads
  through `run_unmerge_pretend`/`run_depclean_pretend`/`run_prune_*`/
  `run_clean_pretend`/`execute_unmerge`/`package_options_from_env` and
  `run_config_action`, so it selects the backend for the removal hooks
  (all of `-C`/`--unmerge`/`--depclean`/`--prune`/`--clean`/
  `--rage-clean`, plus `FEATURES=unmerge-backup` `quickpkg`) and
  `emerge --config`'s `pkg_config`. See `what-this-proves.md`, "`emerge
  --shell` reaches the removal-hook and `--config` paths".

### H. Misc / cosmetic

- ~~`chown` / privilege-preserving `chmod` not reproduced; directory
  merge order sorted for determinism, not real `os.listdir()` order
  (cosmetic).~~ **Assessed — largely a deliberate design choice, one
  real touch-up applied.** `merge_tree`'s regular-file copy now mirrors
  real `movefile()`'s explicit `os.chmod(dest, sstat.st_mode)` with a
  `std::fs::set_permissions` after the copy. `os.lchown` stays out (needs
  root, which the pilot's single-user context never has — it would only
  no-op). Sorted traversal order is kept on purpose: `CONTENTS` line
  order carries no semantics portage relies on, and test determinism is
  worth more than bug-compatible arbitrariness. See `ebuild_merge.rs`'s
  module doc comment.
- ~~`profiles/updates/` package moves (`sys-libs/foo` → `sys-libs/bar`).~~
  **Done** — real `portage.update` / `_do_global_updates`: `move` +
  `slotmove` directives from every repo's `profiles/updates/<quarter>`,
  applied at read time (this pilot never syncs) to command-line /
  `@world` / `@<set>` atoms, `*DEPEND` strings, and an installed
  package's identity (with a backward `move` map so a vdb query for the
  new name still finds the pre-rename dir). Chained moves resolve. See
  README "`profiles/updates/` package moves". Cuts (all no-ops without a
  real write): on-disk vdb/world/binpkg/config rewriting, `grab_updates`'
  scandir order, and `emerge -C` bare-name resolution.
- ~~`color.map` / `PORTAGE_COLORMAP`.~~ **Done** — real
  `output.py::_parse_color_map` reads
  `<config_root>/etc/portage/color.map` and overrides the ANSI code for
  any `_styles` key / `codes` colour-name; `PORTAGE_COLORMAP` exported
  into every build phase's env. See README "`color.map` /
  `PORTAGE_COLORMAP`".
- ~~`--quiet` verbosity level (1) — the pilot models plain `-p` (2) and
  `-pv` (3) only.~~ **Done** — `-q`/`--quiet` (real `true_y_or_n`,
  bundle-compatible) drops the mask column (`include_mask_str()` =
  `verbosity > 1`), suppresses the `USE="…"` line (`print_use_string =
  verbosity != 1 or --verbose`) and the `:slot::repo` / `Total:`
  verbosity-3 output, takes `--search` terse and gates `--check-news`'s
  "no news" line. See README "`emerge --quiet` / `-q`". The
  `--columns`-only quiet line-format rewrite stays out (the pilot
  doesn't render `--columns`).

---

## Part 3 — explicit non-goals / architecture boundaries

Standing decisions, not oversights.

- **`--autounmask-write`** and any config-*writing* autounmask mode
  (conflicts with "never writes config" — the read-only suggest/resolve
  half is shipped). Note `emerge --deselect` / `-C` / `--depclean` *do*
  write `world` / `world_sets` / the vdb now — those are user state the
  package manager owns, not `/etc/portage` config.
- **A real backtracking resolver** (see Part 2.A — listed there because it
  is the substantive gap, here because `agent-context.md` scoped it out for v1).
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
- **Switching CLI option parsing to `clap`** — evaluated 2026-09-02,
  rejected. The parser (`pretend.rs`'s parse loop + `emerge_options.rs`
  tables) faithfully reproduces `emerge`'s `argparse` quirks that `clap`
  has no idiom for: optional values consumed only when they look like an
  integer (`--deep[=N]`, `--jobs[=N]`, `--backtrack[=N]`), `true_y_or_n`
  (bare / `=y` / `=n` / space `y`/`n`) vs `y_or_n` (required),
  `action:"append"` atom lists where each occurrence is itself
  space-split, `-pX requires an argument and can't be bundled`, and the
  exact real error strings. It also carries the
  recognized-but-unimplemented machinery (a real emerge option reports
  "not implemented in this pilot", not "unknown") and is kept
  structurally parallel to the Python reference so the two parsers can't
  drift. `clap` would fight every one of these; ~1500 lines across two
  languages under ~1100 contract tests, near-zero payoff.

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
   2026-09-01; the elog `save` / `save_summary` modules shipped
   2026-09-02.* **Part 2.B is now substantially complete** -- remaining:
   `resume_backup` rotation, the elog `mail*` modules (out of scope --
   real SMTP), `PORTAGE_SCHEDULING_POLICY`, killing in-flight builds on a
   hard fail.

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
   `--info` shipped 2026-09-01; the whole recognized-but-unimplemented
   modifier-flag list shipped 2026-09-02 (batches 1–10); `--regen`
   (real depend-phase md5-cache regeneration) + `--metadata`
   (architectural no-op) + the `config`/`metadata`/`regen`/`sync`
   `--pretend` rejection shipped 2026-09-02 (batch 11).* Remaining:
   `--sync` (non-goal), GLSA/`@security`.

Everything else in Part 2 is genuinely incremental — one focused slice
each, the rhythm this pilot already runs at. Items 1–4 above are the
architectural work that would turn "installs and uninstalls packages"
into "is portage."
