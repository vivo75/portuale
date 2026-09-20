# HL comparison: Portage vs Portuale

Source grounding: `docs/cleanroom/portage/01-entry-and-cli.md` through
`docs/cleanroom/portage/12-data-structures-invariants.md` (behavior contract
over `bin/emerge`, `lib/_emerge/*.py`, consumed `lib/portage/*`) and
`docs/cleanroom/portuale/01-overview.md` through
`docs/cleanroom/portuale/06-support-and-function-index.md` (observable contract
over `rust/portuale/src/*.rs` plus `portage-repo`, `portage-profile`,
`portage-fetch`, `portage-dep`, `portage-versions`, `mrg-director`).
Conformance vocab differs: Portage spec uses `MUST / MUST NOT / SHOULD`;
Portuale spec uses `MUST` (contract-suite pinned) / `SHOULD` (loosely checked) /
`MAY` (documented cut). All `path:line` cites below point at the analyzed
snapshots named in those specs, not at live code.

## 0. One-paragraph verdicts

**Portage** is the complete, battle-hardened reference: total version order,
EAPI-gated atom/USE/REQUIRED_USE semantics, deterministic highest-visible-wins
selection with lazy visibility and disjunctive deferral, layered backtracking
with best-run diagnostics, NFS-safe locks, thin/thick manifests with GPG,
preserved-libs / CONFIG_PROTECT / collision handling, and a 5-queue async
scheduler with load/space/jobserver/merge-wait gates plus keep-going resume.
Its cost is architectural: a ~4300-line `actions.py` god-file, ~86 engine
modules, a frozen/dynamic/tracker/frontier resolver state machine, two-pass CLI
parsing with pre-argparse string rewriting, FIFO/pickle IPC with a bash
handshake, and many interacting gates, caches, sentinels, and tunables.

**Portuale** is the cleaner, statically-linked reimplementation: one binary with
a multicall dispatch, table-driven CLI parity, a single sorted-`readdir` seam
for determinism, validated 3-tier metadata fallback, dual SAT-style solvers
(PubGrub/Resolvo) behind one outcome type, real-`bash` phases with opt-in
`brush` and unshare sandboxing, Manifest-verified fetch, VDB-provenance merges,
ELF/soname tracking with depclean lib-check, GLEP-42 news, resumable DB with
backup, and dual-format (xpak/gpkg) binpkgs with GPG policy. Its cost is
explicit narrowing: EAPI 5+ floor, dropped/legacy/inert flags, strict aborts
where Portage is lenient, a sibling-only lock module, and a simpler scheduler
without jobserver/cgroup observability.

**Neither** is modern where it counts: no CDCL solver with explanations, no
content-addressed/atomic store with rollback, no structured IPC/event stream, no
TUF-style trust, no transactional VDB, no hermetic builds, no typed config.
See §10.

## 1. Verdict table

| Dimension | Portage strong | Portage weak | Portuale strong | Portuale weak / cut |
|---|---|---|---|---|
| Entry / CLI | UTF-8 re-exec, FD sanitize, explicit signal→exit map (`portage/01`); profile gate refuses broken-profile builds | Two-pass parse + `insert_optional_args` rewrite + installed-vs-not `sys.path` branch (`portage/01:99,38-43,1197`) | Early `--help`, prepend `EMERGE_DEFAULT_OPTS` via `shell_split`, table `lookup` parity (`portuale/02:2.1`) | `--config-root` inert; `--ask`-under-`--pretend` ignored; `--moo/--status/--rage-clean` stubs (`portuale/02:2.2`) |
| Config / repos | Central `load_emerge_config`, `adjust_configs`, privilege/logging/resume normalization (`portage/02`) | 4300-line god-file, implicit alias chains, global side effects (`portage/02:4300`) | Layered `resolve_config:2490`, `RepoConfig:886`, exactly-one-main-repo rule (`portuale/01:1.3,04:4.1-4.2`) | Env-only config root, no CLI flag (`portuale/01:1.3`) |
| Resolver | Highest-visible-wins, disjunctive deferral, best-run backtracking (`portage/03`) | Frozen/dynamic/tracker/frontier machine, `2^n` USE search, manual slot fixes (`portage/03:Step 7-8`) | Dual solver, `PretendOutcome`, caret/JSON/tree output (`portuale/04:4.4-4.5`) | Unparsed-token drop (must be zero), narrow pregen cache, `@system` bias flag (`portuale/04:4.2,4.4`) |
| Scheduler | 5 queues, load/space/jobserver/merge-wait gates, keep-going resume (`portage/05`) | Gate interaction, fifo/loadavg/`SIGCONT/WINCH/USR2`, dropped in-flight success (`portage/05`) | Leaves-first `-j`, loadavg gate, hard-fail kills children, resume DB (`portuale/03:3.6`) | No jobserver/cgroup equivalent; simpler ordering |
| Fetch | Already-fetched fast path, resume `.partial`, failure-evidence rename (`portage/06`) | Per-proto commands, pty/pipe fork, asyncio-in-child + suicide (`portage/06`) | Manifest verify, mirror/resume/`RESTRICT`, per-file locks (`portuale/05:5.2`) | External `wget`/`FETCHCOMMAND`; primary-only cap after checksum fails |
| Build / phases | Fine queues, skippable phases, EAPI gating, die/fail-clean, soname QA (`portage/07`) | Unlock-on-every-path, sentinels (`.ed/.tested/.raw`), fakeroot/sandbox/SELinux matrix (`portage/07`) | Real `bash` + opt-in `brush`, unshare isolation, pumps/gzip, phase order contract (`portuale/05:5.1`) | `brush` shaping; `noclean/test`/signing gates |
| Merge / unmerge | `MergeProcess` unification, `chpathtool` EPREFIX, retrying regen, full `post_emerge` (`portage/08`) | xpak/tar `SIGPIPE(141)` tolerance, self-protection exactness, countdown guard, pickle+mtimedb coupling (`portage/08`) | `merge_tree:1552`, `protect-owned` + `collision_message:2867`, `protect_decision:694`, `PlibRegistry:806`, `write_vdb_entry:2115` (`portuale/05:5.4-5.5`) | Symlink-over-dir always aborts (strict); flat-file VDB retained |
| IPC / locks | `HUP`/`EIO` hardening (bugs 339976/401919), `NoGlobalsUnpickler`, atomic rate-limited observability (`portage/09`) | FIFO/pickle raciness, fifo+exit-file handshake, dual socket+JSON channel (`portage/09`) | `PortageLockfile:21` simplicity, `is_privileged:26/deny_superuser:68` (`portuale/06:6.9`) | 55-line sibling-only locks vs NFS-safe `locks.py`; `mrg` remote flags parsed-but-dropped (`portuale/06:6.11`) |
| Lib / data model | Total `vercmp`, ~37 EAPI predicates, USE SAT, slot-op/DNF/soname, locks, checksums, manifests (`portage/10,12`) | No purpose headers, `sets/` vs `_sets/` trap, legacy shims, cache flush discipline, EAPI explosion (`portage/10:10.1`) | Determinism seam, 3-root split, static binary, EAPI 5+ floor (`portuale/01:1.3-1.4`) | Older EAPIs out of scope by design |

## 2. Entry and CLI

**Portage strengths** (`portage/01-entry-and-cli.md`): forces UTF-8 with re-exec
(`bin/emerge:9-14`); maps `PermissionDenied/IsADirectory/ParseError` to
`exit(errno)/exit(1)` and finalizes `mod_echo` on unexpected exceptions
(`:62-87`); `SIGPIPE→DFL`, `SIGTERM→SignalInterrupt`, `SIGUSR1→pdb`, FD
sanitization; `profile_check` (`main.py:1165`) refuses build/merge on a broken
profile; `finally` closes portdb caches, logs `*** terminating.`, resets the
xterm title (`:1367-1382`).

**Portage weaknesses**: REQUIRED two-pass parsing — early silent parse only to
learn roots, then `EMERGE_DEFAULT_OPTS` prepend plus full parse
(`main.py:1197,1234,1330-1365`) — combined with `insert_optional_args`
(`main.py:99`) pre-argparse `"True"` insertion and `-ab` expansion, plus the
installed-vs-non-installed `sys.path` branch (`bin/emerge:38-43`) and
close-event-loop-only-for-`__main__` multiprocessing guard (`:91-110`).

**Portuale strengths** (`portuale/02-emerge-cli.md:2.1`, `01-overview.md:1.2`):
`--help/-h/help` anywhere exits 0 before config load (`pretend.rs:2879,2892,
8390-8393`); `EMERGE_DEFAULT_OPTS` prepended via `shell_split:4391` so explicit
argv wins (`pretend.rs:8484-8488,4456`); `emerge_options::lookup:416` over
`Category:220` tables plus direct-parsed `--pretend/--ask`; stdin `-`
convention; one multicall binary (`main.rs:55-164`) for
`emerge/ebuild/mrg/portuale <applet>`.

**Portuale weaknesses/cuts**: `--config-root` recognized but inert; mode-mismatched
flags (e.g. `--ask` under `--pretend`) silently ignored rather than errored;
`--moo/--status/--rage-clean` are joke/legacy stubs (`portuale/02:2.2`);
`PORTAGE_CONFIGROOT`-only config root, no CLI flag (`portuale/01:1.3`).

## 3. Actions and configuration

**Portage strengths** (`portage/02-actions-and-config.md`): one dispatcher
(`run_action:3661`) centralizes global-updates+reload, digest/buildpkgonly/
getbinpkgonly normalization, `adjust_configs`, `profile_check`,
`apply_priorities` (niceness/ionice/`sched_setscheduler`), required-sets
fallback, set expansion, `--tree+--columns` / `--emptytree+--noreplace`
rejection, `--fetch-all-uri→fetchonly` / `--skipfirst→resume` /
pretend-drops-ask / ask-requires-tty, superuser/portage-group check with
ask→pretend fallback, emergelog routing with `SIGTERM→emergeexitsig
(128+signum)`; `action_build/config/depclean/deselect/info/regen/search/sync/
uninstall` each spell out guards (writable vartree/bintree, GPG unlock,
`AUTOCLEAN=yes` forcing, `/proc`-mounted check, empty-`CONFIG_PROTECT`
warning, missing/duplicate `repo_name` warnings).

**Portage weaknesses**: `actions.py` is 4300 lines — dispatcher plus every
`action_*` plus per-root assembly plus nested `_calc_depclean` helpers
(`show_invalid_depstring/unresolved_deps/show_parents/cmp_pkg_cpv/
create_cleanlist`, `:929-1303`). Option aliasing and `FEATURES`/`PORTAGE_*`
mapping (`adjust_config:2734`, `binpkg_selection_config:2837`,
`create_depgraph_params` side-effecting `--update`) are long implicit chains
with global env/mtimedb/title/signal effects.

**Portuale strengths** (`portuale/04-resolver-and-output.md:4.1-4.2`,
`01-overview.md:1.3`): `resolve_config:2490 → Config:389` folds profile
`parent` chains (incl. cross-repo `reponame:path`), cascading
`package.use/mask/unmask/keywords/use.mask/use.force/make.defaults`,
`make.conf/globals`, then CLI/env overrides, with incremental helpers
(`apply_incremental:1397`, `apply_use_incremental:1445`,
`ProfileUseLayer:364`, `UseMaskForceLevel:377`, `BinRepo:1135`); `find_repos:
1111 → RepoConfig:886` with priority/masters/aliases/cache-formats; missing main
repo is a clean `emerge: no main repo found in repos.conf`, exit 1
(`pretend.rs:8430-8433`).

**Portuale weaknesses/cuts**: narrower than Portage's per-root `RootConfig` +
`adjust_configs` + `doebuild_settings` cloning + `bintree.populate()` +
`--nobindeps`/repos.conf conflict resolution (`portage/02:2713-2837`); no
`check_procfs`, `config_protect_check`, or `PORTAGE_BACKGROUND` jobs/quiet
mapping equivalent is specified.

## 4. Dependency resolution and output

**Portage strengths** (`portage/03-dependency-resolution.md`, `04-package-model.md`):
MUST-level highest-visible-wins with lazy visibility; `||`/virtual/soname
minimized and deferred to the disjunctive stack so plain deps bind first
(`Step 2`); 5-string prioritized growth (`RDEPEND/IDEPEND/PDEPEND/DEPEND/BDEPEND`
with ESYSROOT/running-root/`test`/bdeps/root-deps filtering, `Step 5`);
satisfied non-slot-op parking in `_ignored_deps`; `PackageTracker` live
`(cp→to-merge/installed)` + provides index + `PackageConflict` emission;
`BacktrackParameter/_BacktrackNode/Backtracker` DFS with
`runtime_pkg_mask`-cycle rejection, `--backtrack(20)` depth, config-vs-conflict
feedback split, `get_best_run` for autounmask display, `max(1,(backtrack+1)//2)`
retries plus `autounmask=False` rerun; `--complete-graph` re-adds
`@system/@world` at `_UNREACHABLE_DEPTH` with deliberate conflicts;
`_SerializeFrontier` leaf-pull over `Normal/Satisfied` bands replaces `O(V)`
scans; `FakeVartree` resolves against an unlocked in-memory copy with dynamic
deps and in-memory `profiles/updates/*`.

**Portage weaknesses**: the state machine is enormous — frozen config
(`pkgsettings/roots/trees_orig/pkg_cache/required sets`), dynamic config
(`digraph/dep_stack/disjunctive_stack/package_tracker/filtered+graph trees/
runtime_pkg_mask/needed_USE/keyword/license/p_mask/backtrack_infos/
need_restart`), `_highest_pkg_cache` + prune invalidation, depth accounting
with `reset_depth` exemptions, cross-root/ESYSROOT flags, slot-op
probe/trigger/reinstall/rebuild lists, greedy non-slot-op solving, and a bounded
`2^n` USE search in `circular_dependency_handler` with `REQUIRED_USE` recheck;
blocker overlap → post-install uninstalls in `_serialize_tasks:9457`;
`slot_collision.py` only reports (`conflict_is_unspecific/is_a_version_conflict`),
leaving `--update/--newuse` to the user; tunables (`--backtrack/--deep/
--complete-graph`) change semantics; lazy `_PackageMetadataWrapper` +
`COUNTER/_mtime_` validation + highest-counter-per-slot + same-slot eviction are
subtle (`portage/04`).

**Portuale strengths** (`portuale/04-resolver-and-output.md:4.3-4.4`, `03:3.2`):
`list_candidates:2035` + `is_visible:4387/forced_or_masked:3648/effective_use:
3178`, binary pools (`list_binary:2396/read_index:2147/BinaryIndex:2187/
list_remote:2562/find_remote:2609`) with `--useoldpkg/--usepkg-exclude/include/
--buildpkg-exclude/changed-*DEPEND/ebuild-visibility` filters; interchangeable
`PubGrubResolver:810` / `ResolvoResolver:1019` behind `PretendOutcome:5970`
(`New/Upgrade/Downgrade/Reinstall/AlreadyInstalled/NoVisibleCandidate/Uninstall`)
plus `BlockerConflict/SlotConflict/AutounmaskChange/changed-deps/
buildpkgonly_deps_unsatisfied`; dependency-first `topological_merge_order`
(`--implicit-system-deps=n` drops the `@system`-first bias); output is a
contract in itself — `print_entry_line:1298` (~600 lines), `decorate_version:
488`, `use_suffix:525`, `sort_key:440/colorize:457/mask column:366`,
`root_suffix:622`, `localized_size:638`, counters `:657/:827/:843`,
`COLUMNS:262/:294`; blockers hidden/inline vs full (`:856/:917/:895/:999/:1011/
:1110/:1152/:1197/:1245`); cycle-safe `print_tree:1904` (`:2067-2108`);
`print_json:2768` (`:2304/:2320/:2355/:2581/:2621/:2634/:2649`); caret-annotated
slot conflicts (`:8022/:8068/:8140/:8257/:8288/:8362`); misspell suggestions via
`difflib:7943`.

**Portuale weaknesses/cuts**: unparsed dependency tokens are counted
(`note_unparsed_dep_token:524/unparsed_dep_tokens:532`) and the contract
requires zero — i.e. dropping is observable and must not happen, but the seam
exists where Portage errors inline (`InvalidDependString`); pregen md5-cache is
narrowly enabled (first format `md5-dict`, no `metadata-transfer`,
`pregen_md5_cache_enabled:1724/regen_writes:1751`); metadata falls through
pregen→depcachedir→`depend`-phase (`repo_aux_metadata:1508`,
`register_provider:1475`, wired `main.rs:139`; absent in unit tests a miss stays
a read error); rebuild scan (`:=` + `--rebuild-if-{unbuilt,new-rev,new-ver}` +
exclude/ignore + `_eliminate_rebuilds` undo + slot-move probe) and the
`@system`-bias flag show merge order still carries heuristics.

## 5. Scheduler and parallelism

**Portage strengths** (`portage/05-task-scheduler.md`): explicit async contract
(`AsynchronousTask`: cancel never blocks, listeners immediate-fire,
reentrancy-guarded `schedule`); 5 queues (merge/jobs/ebuild-locks/fetch/
unpack); `_background_mode` (parallel|quiet vs pretend/fetch vs interactive vs
single); `_set_graph_config` (adopts mergelist/digraph, precomputes
world atoms, drops the graph for `nodeps|jobs<2`, else deep-runtime/prune/
`_prevent_builddir_collisions` same-cpv buildtime edges); gates on jobs,
`--load-average`, `PORTAGE_TMPDIR` free space (`--jobs-tmpdir-require-free-gb`
default 18 + 1 GiB×jobs, warn-once+block), GNU jobserver fifo tokens,
merge-wait/`@system` serialization, early-memo; `merge()` resume banner, saved
resume, `INT/TERM→terminate+exit(128+sig)`, `USR2` merge-wait flush,
`CONT/WINCH` handling, keep-going loop (`_calc_resume_list` via
`resume_depgraph`), teardown with single-fail log or die-summary; postinst
failure nonfatal (`_failed_pkgs_all` vs `_failed_pkgs`); `JobStatusDisplay`
(width-clamped, throttled, xterm title), `ProgressHandler` (0.2 s),
`stdout_spinner` (`QUIET/STATIC/TWIRL/SCROLL`, `DECTCEM` hide/show + atexit
restore), `UserQuery` (prefix match, reprompt, `EOF/Ctrl-C→Interrupted.+
exit(128+SIGINT)`), `UseFlagDisplay` grouping.

**Portage weaknesses**: all those gates interact — jobs×unsatisfied-system,
`can_add`, `job_delay` (`SIGCONT` 5 s or `min(5×avg1/max_load,5)`), jobserver
`EAGAIN→requeue+reader`, installed→`PackageMerge+addFront` vs queued builds;
pty-vs-pipe branching, background/foreground tty logic, disk-space blocking
thresholds, and the in-flight-success-after-terminate counted-but-never-merged
edge make the scheduler the second-hardest subsystem after the resolver.

**Portuale strengths** (`portuale/03-emerge-flows.md:3.6`): `run_source_merge:270
→ run_merge_loop:440 → merge_one:522 → build:1079 + merge:1201` with
`run_build_scheduler:1309` building dependency-leaves first, at most `--jobs`
concurrent, throttled by `system_loadavg_1min:1294`; per-entry env
(`:606-900`), build logs (`:908/:983/:1047`); hard failure kills running builds
via the child registry (`ebuild_phases.rs:3058-3109`); `--keep-going` skips
dependents (`:830/:809/:1259`); resume persists remaining mergelist+options
(`mtimedb.rs:313`), success clears it (`:382`); niceness/ionice policy applied
(`:4509/:4565`).

**Portuale weaknesses**: no specified equivalent of Portage's jobserver tokens,
`merge_wait_queue` + `@system` one-at-a-time serialization, `SIGUSR2` flush,
`statvfs` space gate, `parallel-install` merge queue, or file+socket
observability publisher (see §8) — a deliberate simplicity-for-now gap that
matters for heavily parallel `raceme`-style merges.

## 6. Fetch

**Portage strengths** (`portage/06-fetch-phase.md`): `async_already_fetched`
(size + `_check_distfile` hash filter, silent-`False`); size-only prefetch
probe writing `* file size ;-) [ ok ]`; `BinpkgFetcher` resume iff `*.partial`
is in `bintree.invalids` else unlink; `BinpkgPrefetcher` background
fetch→verify→inject with local-rename vs remote-inject split; `BinpkgVerifier`
size/hash/`ebegin…eend(0)` flow with `_digest_exception` renaming to a
checksum-failure temp (Got/Expected preserved); userfetch privilege drop.

**Portage weaknesses**: `FETCHCOMMAND/RESUMECOMMAND` per-proto lookup,
`SSH_OPTS` varexpand, SELinux `PORTAGE_FETCH_T`, pty-when-foreground-tty
(`_pipe:430`, `:234`), forked `async_fetch` forcing `spawn` on py3.14 with
`havecolor`, and `SIGTERM→cancel+terminate/join/kill+suicide` in
`_target:268` — a lot of machinery around what is conceptually "download and
hash".

**Portuale strengths** (`portuale/05-execution.md:5.2`, `03:3.4`):
`fetch_src_uri:655` (`FetchOptions:136/221`) reduces USE conditionals, assembles
literal+custom+thirdparty+`GENTOO_MIRRORS` candidates (`:105/:265/:280/:338/
:397/:495/:563/:587`), groups per filename (`:455`, deduped), reuses/symlinks
verified `DISTDIR` copies, downloads via `FETCHCOMMAND`/`wget -c:257`,
verifies Manifest digests (rename-bad + next), honors
`RESTRICT=mirror/fetch/primaryuri` (incl. `mirror+/fetch+`), `RO_DISTDIRS`
symlinks, space/writability checks (`:612/:624`), per-distfile locks; first
unfetchable file fails the run with exit 1.

**Portuale weaknesses/cuts**: still shells out (`wget_fetch:257`,
`fetch_commands_from_config:280`); checksum-failure retry is capped then
primary-only (`checksum_failure_max_tries:424`); no Portage-style prefetch
size-ok log line or `.partial ∈ invalids` resume rule is specified.

## 7. Build and phases

**Portage strengths** (`portage/07-build-phase.md`): `EbuildBuild` driver
(`_check_temp_dir → SRC_URI → EMERGE_FROM/MERGE_TYPE → findname + setup env →
_check_manifest (strict∧¬digest→digestcheck) → prefetcher → pre-clean →
fetch → EbuildExecuter → buildpkg/record → install task`); `EbuildExecuter`
skips source phases exactly when (no `noauto`/live, empty `A`,
`DEFINED_PHASES∩{unpack,prepare,configure,compile,test}=∅`, no `.src_patches`);
`EbuildPhase` sets locale per EAPI (`posixish→split_LC_ALL→C.UTF-8/C`),
`PORTAGE_REPO_REVISIONS`, drops stale `$T/logging/phase` unless `.ed`,
allocates `PORTAGE_BINPKG_TMPFILE`, handles `pretend/prerm` saved-env,
`package→PackagePhase`, `unpack` fake distdir/filesdir, `install` filesdir
symlink, `test+test-fail-continue→.tested`, `install→write_metadata+uid_fix`,
`_PostPhaseCommands` with `selinux_only` filtering and async soname QA
(`eqawarn Unresolved soname`); `EbuildBuildDir` locking, `EbuildMetadataPhase`
`depend` extraction with `_eclasses_` + cache write-back, `BinpkgEnvExtractor`
(`bunzip2 -c saved > dest` + `.raw`), `PackagePhase` `PKG_INSTALL_MASK` via
`cp -pPR [-l]` + `install_mask_dir`.

**Portage weaknesses**: deep `CompositeTask` chains where every failure path
must elog+unlock exactly or deadlock/leak (`_fetch_failed/_nofetch_exit/
_async_unlock_builddir/_build_exit/_buildpkg_exit/_clean_exit/_install_exit`);
EAPI-conditional branching everywhere (old EAPI drops `prepare/configure`);
gz-aware temp-log append+unlink; fakeroot/sandbox/SELinux/userpriv interplay;
sentinel files (`.ed`, `.tested`, `.raw`); `AbstractEbuildProcess` diagnostics
(missing `BUILDDIR→1`, IPC exit-file vs `.exit_status`, 10 s grace timer+re-arm,
orphan warn, sesandbox/SELinux no-log, killed-by-signal/unexpected-exit/
bash-vs-die/hardware text).

**Portuale strengths** (`portuale/05-execution.md:5.1`): `ebuild run:122` over
`COMMANDS` (`install/merge/unmerge/package/qmerge/clean/pretend/setup/unpack/
prepare/configure/compile/test/preinst/postinst/prerm/postrm/config/info/
nofetch/help`); `compute_environment:398 → Environment:379`
(`split_package:276`, `parse_eapi:218` first-assign default `0`,
`phase_prerequisites:316`, `D/S/WORKDIR/T/FILESDIR/BUILDDIR` via
`create_directories:616` + fake-filesdir `:592`, `base_env:926/phase_env:2386/
phase_path:2627/eapi_path:2610/setup_script:2658/quote:2706`,
`RESTRICT/PROPERTIES` vs USE `:1030/:840/:877/:810-828/:1091`,
`package.env:1060`, `eclass_locations:2360`, post-install `:1428/soname:1583/
libc:1674/dir_size:1712`); `ShellBackend:1770` runs real `bash` by default
(`run_one_phase_bash:2941`, `real_bash_path:2800`), embedded `brush` opt-in
(`brush_phase_params:4055`); sandboxing (`network/fs_requested/exempt:2022/
2040/2050`, `sandbox_binary:2059`, `fs_for_phase:2086`, `Isolation:2108-2267`,
`unshare_combo_usable:2162`); `run_commands:3839/logged:3871` with
`LogSink/LogPump:3923-3971` + gzip; `run_depend_phase:3152/parse_aux:3352/
provider:3277/depcache:3384/:3367`; phase order contract
(`pkg_pretend→pkg_nofetch→pkg_setup→src_unpack→src_prepare→src_configure→
src_compile→src_test[test]→src_install→pkg_preinst/postinst→pkg_prerm/postrm→
pkg_config→pkg_info`; `merge`=install+merge, `qmerge`=merge only); pre-clean
before every source build and after buildpkgonly/merge unless `noclean`.

**Portuale weaknesses/cuts**: `brush` needs param shaping; `test` runs only
with the `test` feature; signing is gated (`require_signing_config:500`);
six real compressors + `MAKEOPTS→jobs` greedy-regex parity + `dyn_package` hook
+ `quickpkg_from_vdb:1050` are carried, not simplified.

## 8. Merge, unmerge, regen, post-run

**Portage strengths** (`portage/08-merge-unmerge-phase.md`):
`EbuildMerge(${D}+build-info→vartree)` + `Binpkg(EMERGE_FROM=binary,
fake BUILDDIR=image+build-info, REPLACING_VERSIONS, prefetcher,
BinpkgFetcher→BinpkgVerifier→local-rename|inject, unpack_metadata with
`CATEGORY/PF` backfill + `BINPKGMD5`, setup→unpack_contents→`chpathtool.py`
EPREFIX rewrite + move, `create_install_task→EbuildMerge(tree=bintree)`);
`EbuildBinpkg` post-`src_install` creator with `BUILD_ID` multi-instance;
`PackageMerge` status wrapper (binary-vs-source color, `Installing/
Uninstalling… cpv::repo [to/from root]`, nonfatal postinst split);
`PackageUninstall` (missing vardb→`EX_OK`, `prerm` env ignoring
`UnsupportedAPIException`, lock, `prepare_build_dirs(cleanup)`,
`_unmerge_display`, `MergeProcess(unmerge=True)`, world update, unlock);
`_unmerge_display` pure selection+preview (`selected/protected/omitted`,
locks vdb, expands system/old-virtuals, validates ebuild paths inside
`VDB_PATH`, prune-keeps-best-slot+counter+best, clean-keeps-max-counter,
portage/python-interpreter/`@set`-parents/system-profile guards, empty→1);
`MetadataRegen` parallel `depend` regen + stale cleanse with valid-cache skip
and `max_tries=3` fresh-instance retry; `post_emerge` (profile.env reload, elog
flush, mtimedb commit under vdb lock when `_pkgs_changed`, preserved-libs/
config-file/news display, `$CONFIGROOT/.../bin/post_emerge` hook,
`clean-logs` pruning, depclean suggestion, news notification); `emergelog`
locked append with secure perms; `search` over merged dedup `cp` stream with
`%`-regex, `@|/`-category, auto-regex, difflib fuzzy, per-db visibility and
manifest/binpkg sizes.

**Portage weaknesses**: `BinpkgExtractorAsync` pipes
`head -c tar+xpak | decomp | gtar -xp -C image` with `PIPESTATUS/SIGPIPE(141)`
tolerance and `{JOBS}`-from-`MAKEOPTS` decompressor probing; EPREFIX move
(`D/build_prefix→ED`, virtual→fresh `ED`); destructive `unmerge` depends on
`countdown(CLEAN_DELAY)`; `UninstallFailure(status=pargs[0] or 1)`; regen
success still returns nonzero while `cpv_failed−successful` remains; resume
depends on mtimedb correctness; keep-going still exits `FAILURE=1`; display
purity (except logs) is easy to violate via hooks.

**Portuale strengths** (`portuale/05:5.3-5.5`, `03:3.5-3.11,06:6.1,6.7-6.8`):
`run_package:528` (install chain → pack `${D}`+build-info; `format:401/
write:430` replace-on-rebuild; `extension:793`, `cat/pn/…:805`,
`build_id:822`, `checksums:866`); `merge_tree:1552` (dirs→create,
files→copy/rename via `needs_move:2001/files_equal:2019`, symlinks→recreate,
fifo/device `:1892`, `lchown:1962`, `CONTENTS:1511/mtime:1533`);
`find_owners:2814` via VDB, `blocked:2522/flat:2609`,
`protect-owned` abort on owned files, `--collision-protect` abort on unowned
but backup dir-symlink-over-real-dir, symlink-over-dir always aborts
(`collision_message:2867`); `protect_decision:694/is_protected:510`
(longest-match + mask), `._cfgNNNN_:561` with content-reuse, `backup:614`,
`cfgfiledict:766/:774/:787`; `next_counter:2067/write_vdb_entry:2115`
(`SLOT/USE/IUSE/EAPI/repository/BUILD_TIME/NEEDED.ELF.2/environment.bz2`),
`consolidated:2204/from_dir:2248`, ownership queries
(`:2315/:2341/:2377/:2399/:2424-2455/:1485/:1462/:1498-1505`); post-merge
`unmerge_replaced:3387/unmerge_one:3505/saved_env_phase:3600`,
`install_mask:2144`, `elog:2941`, `run_env_update` (§6.4) + scoped ldconfig,
`feature_enabled:2917` (`:492/:427/:387`), `source_distfiles:3096`;
`PlibRegistry:806` (`:822/:830-973/:1006/:1153/:1035/:1089/:1215/:1281/:1310/
:1384`); `run_unmerge:702` (`parse_contents:178/ContentsEntry:165 →
remove_contents:235 → remove_dirs:443 → cleanup_info_dir:397 →
unmerge_pkgfiles:577 → delete_vdb_dir:690`) keeping modified configs unless
`--unmerge-orphans`, keeping same-slot-mate-owned paths, tolerating shared/gone
entries; depclean lib-check default-on (`lib_consumer_scan:5662/rebuild,
apply_depclean_lib_check:5738`, `Depclean may break…` advisory,
`depclean_unresolved_halt:5790`); prune keeps highest per slot (`:5337`),
clean/prune-nodeps skip resolution (`:5436/:5468/:5492`); sub-slots never
self-collide; `regen run:175` parallel `--jobs:409` (`WorkItem:128`,
retry `:157/:167`, `regen_one:526`, `render:586/write:636`, `eclasses:672`,
`prune_stale:498`, cache-formats) ; `--metadata` transfer per
`FEATURES=metadata-transfer`; GLEP-42 `run_check_news:6635`
(`format:6746`, `valid:6760/relevant:6812`, `write_state_if_changed:6711`).

**Portuale weaknesses/cuts**: `Binary` entries skipped in buildpkgonly; live
`9999` builds only with the live-buildpkg feature (`:379/:409`);
`--buildpkg-exclude` (`:353`); `--config` ignores `--pretend` (`:7789`);
`--deselect=n` keep-value vs `-W` removal split (`:3523/:3914/:3324/:3271`);
exotic `sync-type` → `not supported` naming the type; `pms`-only repos skipped
with message + exit 1; `mrg` remote flags parse but never forward
(`mrg.rs:1641-1732`).

## 9. IPC, locks, support, data model

**Portage** (`portage/09-ipc-process-utils.md`, `10-portage-lib-support.md`,
`12-data-structures-invariants.md`): FIFO IPC (`FifoIpcDaemon:11`,
`EbuildIpcDaemon:19`) with `POLLHUP`-without-spinning (bug 339976) and
lock-guarded reopen on `EIO/HUP` (bug 401919), `NoGlobalsUnpickler`, commands
`die/exit/best_version/has_version` + elog/phase queries + exit-file handshake,
`ENXIO`-tolerant replies; `getloadavg` `/proc/loadavg` fallback,
`countdown(secs=5)`, `_flush_elog_mod_echo`, locked `emergelog`; observability
(`_observability.py:45-418`, `ObservabilityMonitor:418`) with snapshot schema,
`freeze_resources`, `status_dir`, socket-first-then-JSON dedup-by-pid,
`kill 0` liveness, rate-limited (`1 s` write / `2 s` refresh) atomic sorted JSON,
`chmod 0600` unix server, no-ops unless `observability ∈ FEATURES`; library of
`vartree/porttree/bintree` (`vardbapi` reentrant+fs+slot locks,
`counter_tick`, `dblink` ~62 methods with merge/unmerge, preserved-libs,
`CONFIG_PROTECT`, collision/security, `treewalk`, fork-safe `merge/unmerge`,
xattr `CONTENTS`; `portdbapi` pregen+`_run_metadata_phase`+`fetch_check`;
`binarytree` local+remote populate, trust helper, pkgindex maintenance,
`MissingSignature` on unsigned `SIZE`); `dep/` (`Atom`, `use_reduce`,
`paren_reduce/enclose`, slot-op rewriting, DNF, libc synthesis, soname ABI
map, `REQUIRED_USE` SAT); `versions.py` total `vercmp`
(numeric→letter→`_alpha<_beta<_pre<_rc<(release)<_p`→`-rN`); `eapi.py` ~37
`eapi_has_*/exports_*/supports_*/allows_*`; `locks.py` blocking `fcntl`,
unlink-race `_fstat_nlink` iteration, `lockf`-vs-`flock` selection, hardlink
NFS-safe dance; `process.py` spawn (`_setup_pipes/after_fork/_exec`,
pty, SELinux wrap, sandbox/fakeroot, `sanitize_fds`); `checksum.py`
(serial/parallel, `PORTAGE_CHECKSUM_FILTER`); `manifest.py` (thin/thick,
`Manifest2`, GPG sign/validate, `_apply_max_mtime`); `getbinpkg.py`
(resume/redirect/auth, `PackageIndex` inheritance); sets in `_sets/`
(world/system/security, `DbapiChain`, file/world/selected, preserved,
profiles, GLSA, shell); caches (flat-hash/fs/anydbm/metadata/xattr/sqlite/
volatile); `elog/` (core+`elog_process`, echo/save/mail/syslog/custom);
invariants: no two merges share `cp:slot`, topo order respects hardness bands,
uninstalls defer past blockers, same-`cpv` buildtime edges stop builddir
collisions (`portage/12:12.3-12.4`).

**Portage weaknesses**: no module purpose headers (purposes reverse-derived);
`sets/` vs `_sets/` path trap; `_legacy_globals` + `update.py` shims; lazy
global proxies; cache layers (metadata/auxdb/xattr/sqlite/volatile) with
close/flush discipline; ~37 EAPI predicates = combinatorial gating; hardlink/NFS
lock dance; unsigned-`SIZE→MissingSignature` edge; atom grammar EAPI gates
(blocker/wildcard/repo/build-id) callers must not forget (`isvalidatom`).

**Portuale** (`portuale/01-overview.md:1.3-1.4`, `06-support-and-function-index.md`):
determinism via one `portage-util::read_dir_entries` seam (sorted; seeded
shuffle only under test-only `PORTAGE_SHUFFLE_DIRS`); `ROOT` (`root_from_env:
828`) / config-root (`config_root_from_env:205`) / running-root
(`running_root_from_env:879`, only for `--root-deps=rdeps` suffix) split;
`VDB ${ROOT}/var/db/pkg/<cat>/<PF>/` (`CONTENTS/SLOT/USE/IUSE/EAPI/repository/
BUILD_TIME/NEEDED.ELF.2/environment.bz2`); privilege model
(`is_privileged:26`, `deny_superuser:68`); one static minimal-Linux binary (only
`bash`, `wget`/`FETCHCOMMAND`, optional `gpg/ldconfig/sendmail`);
xpak trailer walk (`binpkg.rs:856/:882/:933/:969/:870/:1470`) + gpkg outer/inner
scan (`:174/:187/:202/:279/:384/:406/:419/:433/:544/:606/:653/:356/:122/:331/
:1335/:1392/:1522/:1275`) + manifest verify (`:1015/:105`) + `GpgVerify:1874`
(`:1889-1892/:1920/:1929/:1954`, policy `:1983`, templating `:2002`, subprocess
`:2018`, detached `:2086`/clearsigned `:2115`); elog collect/filter/dispatch
(`elog.rs:77/:88/:151/:184/:204/:192/:219/:283/:308/:334/:347/:361/:410/:1133/
:1223/:1286`, gating `:96/:108/:115/:238`); color (`color.rs:231/:275/:302/
:330-374`); `run_env_update:239` (`:78/:96/:125/:165/:206`); install mask
(`install_mask.rs:28/:40/:49/:115/:164/:178`); merge-engine dispatch
(`merge_engines.rs:37/:115/:175/:134/:191/:154/:204`); ELF/soname
(`needed_elf.rs:47/:75/:110/:126/:150/:299/:333/:491/:563-615/:615/:620/:635/
:654/:661/:694/:803-825/:859/:962/:1087/:1196/:1225`); resume DB
(`mtimedb.rs:57/:73/:80/:86/:99/:313/:340/:356/:382`); remote SSH + bundles
(`remote.rs:33/:64/:87/:119-124/:131/:230/:339/:363-374/:404/:444/:535-621/
:685-775/:795-807/:891/:941/:1265/:2544`, `remote_bundle.rs:28/:45/:58/:129/
:145/:160/:185/:202/:228/:295/:315`); `mrg` clap→emerge-argV
(`mrg.rs:111/:127/:1125/:1093/:1148/:1160/:1231/:1332/:1385/:1301-1315`);
`difflib:24/:50` CPython-compatible; exhaustive non-test function index
(`06:6.13`); EAPI 5+ floor.

**Portuale weaknesses/cuts**: the floor is also a gap (older-EAPI branches out
of scope); the lock module is 55 lines of sibling files vs Portage's
cooperative+NFS-safe `locks.py`; `mrg` remote flags never forward; test helpers
are intentionally omitted from the index (fine for the spec, a gap for a reader
auditing coverage).

## 10. Better alternatives neither implements

Neither spec describes these; each is a concrete improvement over both designs:

1. **Solver with learning + explanations.** Both backtrack; Portage bounds a
   `2^n` USE search, Portuale hides heuristics behind outcome types. A real
   CDCL/PubGrub-with-clause-learning core over (atom, slot, USE, keyword,
   license, `REQUIRED_USE`) with conflict explanations and minimal-change
   suggestions would replace `slot_collision` reports, caret rendering as the
   *only* explanation, and `--backtrack/--deep/--complete-graph` tuning.
2. **Content-addressed store + atomic transactions.** Both merge `${D}`→`${ROOT}`
   file-by-file and unmerge via `CONTENTS`. A CAS (Nix/OSTree-style) with
   atomic switch, per-transaction rollback, and GC would obsolete
   `._cfgNNNN_` content-reuse hacks, `cfgfiledict` staleness collection,
   `*.partial ∈ invalids` resume rules, and post-failure resume-list replay.
3. **Structured IPC and event stream.** Both inherit FIFO/pickle (Portage) or
   phase-pump logs (Portuale). A single bidirectional protocol (gRPC/JSON-lines
   over a socket) carrying phase events, `die/exit/best_version/has_version`,
   progress, and resource usage would remove HUP-spin guards, exit-file
   handshakes, `ENXIO` tolerance, spinner/`DECTCEM` handling, and the
   socket+JSON dual observability channel.
4. **Hermetic, reproducible builds.** Both run ebuild bash with sandbox/unshare
   overlays and partial `SOURCE_DATE_EPOCH`. Fully hermetic builds (mount/user/
   network namespaces by default, fixed toolchain capture, SBOM + provenance
   attestation, bit-reproducible outputs) would subsume `userpriv/fakeroot/
   sandbox/SELinux` matrices and `.tested/.raw/.ed` sentinels.
5. **Modern fetch + trust.** Both use `FETCHCOMMAND`/`wget -c`, mirror lists,
   and GPG-`Packages`/Manifest verification. TUF-style repository metadata
   (threshold signatures, expiry, rollback protection) plus CAS downloads
   (zsync/casync/IPFS-shaped range fetching) would replace per-proto
   `FETCHCOMMAND/RESUMECOMMAND`, `SSH_OPTS` varexpand, checksum-fail-then-
   primary-only caps, and detached-vs-clearsigned branching.
6. **Transactional installed-DB.** Both keep flat-file VDB + `CONTENTS` +
   pickle/mtimedb resume. SQLite (or similar) with WAL transactions, monotonic
   `COUNTER` as a sequence, and leased locks would remove reentrant/fs/slot
   lock dances, `_bump_mtime` invalidation, `cpv_inject` same-slot eviction
   subtleties, and `flush iff ≥5 modified ∧ secpass≥2` pickle rules.
7. **Typed configuration.** Both fold profile chains + `make.conf` bash
   sourcing + incremental overrides. A typed, schema-validated config (TOML/CUE/
   Dhall-shaped) with `package.use/mask/keywords` as structured rules would
   remove `EMERGE_DEFAULT_OPTS` string prepending, `insert_optional_args`
   rewriting, `shell_split` quoting edge cases, and silent ignore of
   mode-mismatched flags.
8. **First-class observability.** Portage rate-limits file+socket snapshots;
   Portuale specifies logs and loadavg gates. An OpenTelemetry-shaped span/log/
   metric stream (per-package build spans, fetch spans, merge spans, queue
   depths, cache hits) would replace throttled `JobStatusDisplay`, 0.2 s
   progress handlers, and log-tail-on-failure excerpts.

## 11. Where Portuale could still borrow from Portage

- `profile_check`-style refusal of build/merge on broken profiles
  (`portage/01:1165`) as a hard gate, not just config-load failure.
- `check_procfs` / empty-`CONFIG_PROTECT` warnings (`portage/02:3213,3226`).
- `MissingSignature`-on-unsigned-`SIZE` strictness (`portage/10:bintree`).
- `get_missing_sets` / `ensure_required_sets` fallback (`portage/02:3389-3448`).
- `show_parents` reverse-dependency explanations inside depclean
  (`portage/02:1256).
- Nonfatal-postinst accounting split (`_failed_pkgs` vs `_failed_pkgs_all`,
  `portage/12:12.4`).

## 12. Where Portage could learn from Portuale

- Single sorted-`readdir` determinism seam (`portuale/01:1.4`) instead of
  scattered ordering assumptions.
- Validated 3-tier metadata fallback that never returns stale data
  (`portuale/04:4.2`).
- `PretendOutcome` + machine-readable JSON as a first-class contract
  (`portuale/04:4.4-4.5`), not display-only projections.
- Opt-in embedded shell (`brush`) for environments without a real `bash`
  (`portuale/05:5.1`).
- Resume DB that keeps its backup on clear (`mtimedb.rs:382`) and rotates only
  multi-item lists (`:340`).
