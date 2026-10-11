# crates/mrg-director/src/lib.rs
The `mrg` director component contracts: traits, their filesystem-backed implementations, and the `Director` wiring struct.

## module
- A trait in the same crate as its only implementation, with no other implementor, is usually dead abstraction. It earns its place here for one specific reason: the director needs a stable spine before the interchangeable algorithms land. The contracts are the agreement the future algorithms must satisfy; writing them against the existing implementations (and pinning that in tests) makes the `portage_repo::Resolver` precedent explicit, and gives every later algorithm a fixed seam to plug into.
- Slots whose production path runs through the trait prove the seam carries traffic: `SchedulerPolicy` via `run_build_scheduler`, `MergeEngine` via the merge dispatch, `NewsSelector` via `--check-news`, `Fetcher` via `portuale::fetch::fetch_src_uri`'s candidate loop, `PackagesDb`'s `contents_files` via the merge collision walk. The rest stay swappable behind `Director`'s delegation methods until their second algorithm lands.
- `Fetcher`'s transport is shared with `portuale::fetch` via `portage_fetch::download_via_wget`; Manifest verification stays at that call site (the seam's documented narrowing).
- A separate remote binpkg download (from a `PORTAGE_BINHOST`/`binrepos.conf` `Packages` index, the `g` bracket column) lives in `portage-repo` and is not named behind `Fetcher`; a director that needs it wraps that function behind an identical shaped trait.
- Each contract documents the Portage source it names, the single portuale/portage-repo implementation satisfying it today, and the rationale for the method set (why this input, this output).
- Tests pin both the contract shape and the backed implementations against on-disk layouts (a temp vdb, a temp md5-cache dir, a temp distdir).

## re-export Resolver
- Not a second, parallel definition: the director (and every tool that wants a plan) calls `Resolver::resolve`, and a component is swapped by changing the request.
- Portage grounding detail: `ResolveRequest` is the whole `create_depgraph_params` + user-visible options surface as one value (Portage builds `depgraph` from `create_depgraph_params(...)` plus myopts/trees/settings: `depgraph.__init__`, `actions.py:1028-1030`); `GraphResult` is the merge list plus the notices (`SlotConflict`, autounmask changes, abi rebuilds, circular deps) Portage's `depgraph.display_problems()` reports.

## trait PackagesDb
- The "vdb" half of the "edb, vdb and all caches" assignment; the "edb" is the same installed-db read through the `dbapi` facade.
- What the director needs from "/var/db/pkg" (Portage `portage/dbapi/vartree.py`'s `vartree`/`vardbapi`, the "vdb" half of the assignment's "edb, vdb and all caches") to decide ownership, reverse deps, and slot occupancy.
- `contents_files`: the paths one installed CPV owns are its vdb `CONTENTS` `obj`/`sym`/`dir`/`dev`/`fif`/`bin` entries' path fields, with one leading `/` stripped (the vdb records `${ROOT}`-absolute paths, the seam speaks `${ROOT}`-relative ones).
- `reverse_dependents`: every installed package whose `DEPEND`/`RDEPEND`/`BDEPEND`/`PDEPEND` pulls in an atom that `consumer_cpv` satisfies, per `portage_dep::match_from_list`.
- Deliberately not a faithful transcription of `portage/dbapi/vartree.py`'s `vartree` class (6,667 lines with the whole dblink merge machinery). The director only needs the read side resolution consults: the same subset portuale already reads ad hoc via `portage_repo::{installed_candidates, installed_refs, installed_slot_owners, protected_set_member, get_installed_...}` and `pretend.rs`'s `installed_cp_versions`.
- Future implementations (an in-memory snapshot for benchmarking, or the full `vartree` facade that also owns `unmerge`) must satisfy these same queries; directors and resolvers are written against exactly these.

## trait RepoCache
- Portage abstracts the repo cache behind `portage/cache/template.py::database`: a key/value store whose value is the flat aux dictionary (`DEPEND`, `RDEPEND`, `KEYWORDS`, `SLOT`, `EAPI`, `IUSE`, ...) with the `EAPI`/`KEYWORDS`/`SLOT`/`repository` `_pkg_str_aux_keys` special-cased by `portdbapi.aux_get`. Derived backends live in `portage/cache/` (`flat_hash.py`, `sqlite.py`, `anydbm.py`, `volatile.py`). Portuale's implementation today is the flat md5-cache directory read by `portage_repo::read_md5_cache` (the `metadata/md5-cache/<cat>/<pf>` files), exactly `flat_hash.py`'s layout.
- The method set (one aux dict, one category listing) is the two operations the resolver needs, kept minimal so a backend only satisfies read access, never the eclass-serialization/commit machinery (`_eclasses_` reconstruction, `_mtime_`/`_md5_` validation, LCD) that Portage's `database.__getitem__` owns but portuale's md5-cache reader already narrows out.
- `RepoCache::category`: directory entries under `metadata/md5-cache/<category>` when the repo has a usable md5-cache, else the `<pkg>-<version>` stems of the ebuilds under `<repo>/<category>/<pkg>/` (the repo without a cache, whose metadata must come from the ebuild itself).

## trait Fetcher
- Portage downloads one `SRC_URI` file at a time via `FETCHCOMMAND`/`RESUMECOMMAND` (Portage's `wget` by default) into `DISTDIR`, gated on a `FEATURES=distlocks` lock, after the offline candidate-resolution half (`flatten_src_uri` + `resolve_mirror_candidates` + `MirrorDistfiles` + `file_getsize`/Manifest).
- `FetchRequest` deliberately carries no `SRC_URI` text or `SrcUriEntry`: the exact resolved candidate URI the caller's candidate loop popped (a `mirror_url` for a mirror root, else the entry's literal/expanded URI).
- Manifest verification and the local-`fsmirror` copy stay in `portuale::fetch` itself: `fsmirrors`/`copy_from_fsmirrors` (Portage `fetch.py:1503`, the `/`-rooted `custommirrors["local"]`/`GENTOO_MIRRORS` dirs copied via `shutil.copyfile` before any `FETCHCOMMAND`) and the `verify_digests` call after each candidate attempt. The fsmirror copy is a pre-download step of that one call-site sequence, not a swappable transport, so it is no second `Fetcher` implementation either.
- `request.dest` is never re-derived inside the seam, so an implementation cannot drift from the path the caller locks and verifies.
- `request.resume` picks Portage's `RESUMECOMMAND` (`make.globals` default, byte-for-byte `FETCHCOMMAND` plus `-c`) over `FETCHCOMMAND`.
- The one Manifest-derived value the transport does need is Portage's `DIGESTS` command variable (`fetch.py:1797-1803`), carried pre-formatted in `FetchRequest::vars` so the seam still never sees a `Manifest`.

## struct FetchRequest
- The resolved `FETCHCOMMAND`/`RESUMECOMMAND` family (Portage `fetch.py:1652-1700`); `None` uses the `make.globals` defaults (`portage_fetch::default_commands`).
- Portage `fetch.py:1795-1811`'s `DIGESTS` and `PORTAGE_SSH_OPTS` substitution inputs, pre-formatted by the caller (the `DIGESTS` string needs the file's `Manifest` entry, which the request deliberately does not carry). `Default` leaves both unset; an unset variable expands to empty, same as Portage.

## enum MergeKind
- A source package goes through the `EbuildBuild` chain, a `"binary"` one through `_emerge/Binpkg.py`, an already-installed one straight to the uninstall path. `emerge_getbinpkg::run_merge_plan` dispatches per resolved entry on exactly this bit (`Binary` -> `merge_binpkg`, else -> `merge_one_source_entry`).

## struct MergeProgress
- Portage refs: `3rdparty/portage/lib/_emerge/Scheduler.py:110-114`, `:296-304`. `curval` is the 1-based ordinal in start order; the `Emerging`, `Installing`, `Completed` lines print it.
- Portage `Scheduler._pkg_count` (`3rdparty/portage/lib/_emerge/Scheduler.py:110-114`, `:296-304`): the per-package `(curval, maxval)` snapshot each `MergeListItem` carries. `maxval` counts only the `operation == "merge"` entries (uninstalls and nomerge nodes are not counted); `curval` is the 1-based ordinal in start order.

## struct MergeUnit
- `replaces_same_slot`: served by `ebuild_merge::unmerge_replaced_same_slot`.
- `remote_binary`: a binary unit whose binpkg is not under local `$PKGDIR` may be fetched from a binhost (`GraphEntry::remote_binary`, set by the resolver for a binhost-sourced candidate). Source units ignore it.
- `build_id`: a build-id-qualified binary (`GraphEntry::build_id`) selects the exact `$PKGDIR` file among multi-instance same-version binpkgs. `None` means "the unqualified file". Source units ignore it.
- `slot`/`sub_slot`: `GraphEntry::slot`/`sub_slot`; the source engine threads them into the build-phase env (slot-qualified `package.env` matching). `None` behaves like the resolver's own `unwrap_or("0")` default.
- `use_flags`: `GraphEntry::use_flags_display` (`(flag, enabled)` pairs); the source engine exports the enabled ones as the build-phase `USE`.
- `progress`: set by whoever derives the unit from a merge run.

## struct MergeContext
- `jobs`: 1 = serial, like Portage's default single `EbuildBuild` at a time for the merge step.
- An engine needing finer control (build-log capture, sandbox flags, resume lists) takes it from its own constructor, not from this call.

## enum MergeOutcome
- The coarse three-valued outcome Portage reports in the `>>>`/`!!!` merge lines, plus the resume diagnostic `--keep-going`/`--resume` needs. An engine returns one per `MergeUnit`.
- The director turns a run of outcomes into the scheduler's report (merged count, failed list, resume list) without knowing which engine produced them.

## trait MergeEngine
- Portage: `_emerge/PackageMerge.py` (`PackageMerge._start`, via the `EbuildMerge.py` chain). Portuale's is `ebuild_merge::{run_merge, run_qmerge, merge_binpkg}` driven by `emerge_build`'s serial loop and the `-jN` parallel scheduler (which keeps the vdb merge itself serial, like Portage). A future alternate method (content-addressed, container-based) satisfies this one method and the director never notices.

## struct SourceMergeEngine
- Portage refs: `MergeListItem.py:87-106` (`EbuildBuild`). The binary crate's `RealSourceEngine`/`RealBinaryEngine` adapters execute through the seam.
- The real phase chain + vdb write live in the `portuale` binary crate (`ebuild_merge::{run_merge, run_qmerge}` driven by `emerge_build`'s serial loop and the `-jN` parallel scheduler, not linkable from a library -- the standing pattern).

## struct BinaryMergeEngine
- Portage refs: `MergeListItem.py:108+` (`Binpkg`). Real work is `emerge_getbinpkg::merge_binpkg` via `run_merge_plan`'s per-entry dispatch.

## trait BinpkgIndex
- Portage `bintree` presents both backends through the same `dbapi.cp_list`/`dbapi.aux_get` read facade. Local store: `<pkgdir>/Packages` index, or a directory scan of each binpkg's embedded metadata when there is no `Packages` (`bintree._populate_local`, `packagesFile()`/`files()`). Remote store: synced-`Packages` over the remote `Packages` (`bintree._populate`, `packagesFile()`), the `--getbinpkg`/`g` bracket candidate source.
- The method set mirrors the `"binary"` side of `portage_repo::CandidateSource` and the `Packages` `KEY: value` aux format `BinaryIndex` already parses. A future content-addressed or OCI-backed store is one more `impl`.
- `metadata`: the `CPV`, `SLOT`, `USE`, `SIZE`, `BUILD_TIME`, dep strings, etc.
- `metadata_with_source`: `PkgdirBinIndex` returns its `$PKGDIR` path; `RemoteBinhostIndex` returns the owning binrepo's section name, scanning the configured binrepos in order with no local fallback (Portage `bintree._populate_remote`/`_get_remote_pkg`'s fetch lookup, where the record travels with the `sync-uri`/`verify-signature` of the binrepo it came from -- see `portuale::emerge_getbinpkg::merge_one_binary_entry`).
- `metadata_with_source_instance` (backlog #301): `Some(build_id)` must return the record with exactly that `BUILD_ID`; it replaces `portage_repo::find_remote_binpkg`'s scan on the merge path.

## trait NewsSelector
- Grounding: Portage `portage/news/` defines the news-item model. The in-scope (`--check-news`) backend is `Item.isRelevant` (`news.py`, Portage `lib/portage/news.py`), which decides a `metadata/news/<id>` item should be shown by matching its `Display-If-Installed` atom (or `Display-If-Keyword`/`Display-If-Profile`) against the installed db; `isValid` (`news.py`) gates on `News-Item-Format`/EAPI.
- In scope (`--check-news`): `Item.isRelevant` decides a `metadata/news/<id>` item is shown by matching its `Display-If-Installed` atom (or `Display-If-Keyword`/`Display-If-Profile`) against the installed db; `isValid` gates on `News-Item-Format`/EAPI. Portuale's reading is `pretend.rs::run_check_news` (`news_item_valid` + `news_item_relevant`).
- The trait is named a "news/GLSA selector" because a GLSA `@security` selector (Portage `_emerge/glsa.Class.glsl`/`GlsaSet`) would implement the same seam: match a security item against the installed set. Gated on `@security` entering scope (`scope-backlog.md` Part 3 non-goal); `MetadataNews` is the only implementation today.
- `unread_ids`: Portage `NewsManager.updateItems`'s unread/skip accumulation, narrowed to the pure relevance decision.

## trait SchedulerPolicy
- Portage `_run`: never start an additional build while the system 1-minute load average is above the limit; the first build is always allowed so the scheduler cannot deadlock. Portuale's reading is `emerge_build.rs::run_build_scheduler`'s `in_flight < jobs` + `system_loadavg_1min` gate. Callers enforce `running < max_jobs` before consulting the policy.
- Callers guarantee `running < [`Self::max_jobs`]` is enforced *before* consulting the policy, and that at least one build is always permitted, so an implementer may treat `running == 0` as unconditional.
- Future policies (serial `--jobs=1`, deadline-aware queue, remote build-farm throttle) change only this method.

## struct VdbReader
- Production traffic today runs through `contents_files` only: the merge's collision paths (`portuale::ebuild_merge::find_owners` and `owns_path`) build a `VdbReader` over the merge root and read every installed entry's file list through it. `installed_versions` and `reverse_dependents` are exercised only by `Director`'s own wiring tests.
- `reverse_dependents` has no production consumer yet: depclean's reverse-dependency display is computed inside `portage_repo::depclean_cleanlist` (`DepcleanResult::kept_parents`/`unresolved`) from `(InstalledPackage, Vec<String>)` edges, a shape this trait's flat `Vec<String>` CPV list does not model; and `portage-repo` cannot depend on this contract crate to be handed a `VdbReader` from there.
- `installed_versions` sorts with the same `vercmp` every other version ordering in portuale uses.

## struct MemoryDb
- Portage `_emerge/FakeVartree.py::FakeVartree`: "an in-memory copy of a vartree instance that provides all the interfaces required for use by the depgraph", built so dependency calculations run without holding a lock on the vardb. Serves resolution-shaped reads with no filesystem (benchmarks, tests, callers that already hold the facts).
- Narrowing 1: versions come back in recorded order; callers provide highest-first (Portage `dbapi.cp_list` sorts with `_cmp_cpv`), the snapshot preserves.
- Narrowing 2: reverse dependents are recorded edges via `MemoryDb::add_package`, not recomputed matches. Portage's `FakeVartree` re-resolves dep strings live, but recomputation needs `portage_dep` atom matching, a dependency this contract crate deliberately does not take.

## struct Md5Cache
- Delegates to `portage_repo::repo_aux_metadata`. Because `repo_aux_metadata` is the #41 decision point, this also carries the ebuild fallback (depcachedir + depend phase) for a cache-less repo.
- `new` wraps one repo location under its `repos.conf` name (the `::reponame` provenance `RepoCache::repo` reports).

## struct VolatileCache
- Grounding: Portage `portage/cache/volatile.py::database` -- the dict-backed cache backend (`self._data = {}`, which `deepcopy`s on every read and write (`__getitem__`/`_setitem`) so callers can never alias the store's internals). This port mirrors that ownership rule: `insert` clones in, `RepoCache::metadata` clones out.
- `entries` is the `volatile._data` dict. Portage marks `serialize_eclasses = False` / `store_eclass_paths = False` (no eclass-string round-trip); the trait's read-only surface already narrows that machinery out, so there is nothing further to port.
- Uses beyond interchangeability: a pre-loaded snapshot cache for benchmarks and tests (no filesystem), and the read side of a generated cache before it is ever written.
- `from_entries`: a snapshot read, or a generated cache not yet flushed to disk.

## struct WgetFetcher
- One request, one `wget` invocation: `request.resume` picks Portage `RESUMECOMMAND` (`wget -c`) over `FETCHCOMMAND`, and `request.dest` is written exactly as given.
- Runs `portage_fetch::download_with_commands` (not just `download_via_wget`, so per-request `commands`/`vars` overrides ride along; absent overrides fall back to `portage_fetch::default_commands`).
- The already-materialized short-circuit lives at the call sites (`Director::fetch`'s `distdir/<filename>` check and `fetch_src_uri`'s own `already_verified` check).

## struct PkgdirBinIndex
- `index` is the `BinaryIndex::from_pkgdir` / directory-scan value the resolution already built.

## struct RemoteBinhostIndex
- `config`: the resolved binhost configuration (`binrepos.conf` / `PORTAGE_BINHOST`), Portage `BinRepoConfig` values.
- `root`: the `root` whose `$PKGDIR` the sync lands in (Portage `BinRepo.packages_dir(root)`).
- Delegates to `portage_repo::list_remote_binary_candidates` for the per-binrepo scan (which shadows a locally present version, the `bintree.isremote` rule) and `read_binary_metadata_any` for the aux record; the download-side lookup delegates to `find_remote_binpkg_instance`.
- `source_name` joins the configured binrepo names (empty with none configured).

## struct MetadataNews
- The current `NewsSelector`: Portage `--check-news` relevance over the repo's `metadata/news/<id>/<id>.en.txt` items, gated by `News-Item-Format`/EAPI validity + `Display-If-Installed` / `Display-If-Profile` / `Display-If-Keyword` matching against the installed db, the profile path and `ARCH` (Portage `portage/news.py::Item.isRelevant` / `isValid`; portuale's `pretend.rs::news_item_valid` / `news_item_relevant`).
- `FilesystemNews` in the binary crate is the live reading (evaluation over `metadata/news`, on the `--check-news` path); `MetadataNews` stays the state-free shape pin. One selector is built per repo by the CLI layer; the marker owns none.

## struct LoadAwarePolicy
- `jobs` is `--jobs`/`max_jobs`; `load_average` is the `--load-average` ceiling (`None` disables load gating). The constructor is the shape `run_build_scheduler` runs under.
- The first build is always allowed (Portage `Scheduler._run` never gates `in_flight == 0`), and additional ones only while under the ceilings.
- Default: serial and ungated (Portage's own default scheduler is unbounded, bare `-j`).

## struct UnlimitedPolicy
- Portage's `max_jobs is True` branch: the CLI maps bare `-j` to `usize::MAX`. Portage `PollScheduler._can_add_job` load-checks whenever `max_jobs is True or max_jobs > 1`, hence the gate here.

## struct Director
- Portage: `actions.py::action_build` builds the depgraph from `create_depgraph_params(...)` (`actions.py:268`), resolves it, and hands the merge list to `_emerge/Scheduler.py` (`actions.py:679`), which walks it (fetch -> build -> merge per `MergeListItem`, `--keep-going` resume on failure). A different solver, database/cache backend, or merge method is one constructor argument, never a call-site change.
- The director is that orchestration shape with every algorithm behind a trait: a different solver (`portage_solver`, `pubgrub`, `resolvo`), a different database or cache backend, or a different merge method is one constructor argument, never a call-site change.
- V1 narrowness: `emerge_getbinpkg::run_merge_plan` executes each entry's `MergeUnit` via its engine; `pretend.rs::run_check_news` reads each repo's unread ids via its news selector. The `mrg` applet still calls `pretend::run` directly until a second algorithm lands in one of the remaining read slots.

## mod tests
- `tempdir`: process id + nanos, so parallel `cargo test` workers never share one.
- `write_vdb_entry`: the `(filename, content)` files are `CONTENTS`, `USE`, `RDEPEND`, etc.
- `resolver_seam_is_the_single_re_export`: pins the seam; actual resolution is portage-repo's own fixture-driven suite, never here.
- `packages_db_vdb_reader_reads_a_real_vdb_tree`: the consumer entry (vdb `RDEPEND` naming `dev-libs/example`) shows up as a reverse dependent for `example-2.0`, never for itself; `db.root()` reads back the root like `MemoryDb`'s owned root.
- `repo_cache_is_read_only_query_by_key`: different backends (flat/sqlite/volatile) are all read-only views, so writes are intentionally out of contract; like Portage `flat_hash` on a missing dir.
- `md5_cache_category_lists_cache_entries_or_ebuild_stems`: the cache-less tree is the shape whose metadata must be generated from the ebuild.
- `repo_cache_volatile_is_the_in_memory_second_backend`: mutating the returned dict leaves the store intact.
- `fetcher_takes_one_resolved_candidate_request`: no panic, no half-written file left behind (the fresh-fetch cleanup is the shared transport's own); the unit constructors carry the common-case defaults (local, unqualified, no replace). `Fetcher::fetch` takes one already-assembled `FetchRequest` (resolved candidate URI + local `dest` + fresh-vs-resume) and downloads it through the shared transport; consuming the resolved request keeps candidate assembly and Manifest verification at the caller (`portuale::fetch::fetch_src_uri`'s candidate loop, where the `has_partial` bit and the `Manifest` entry live -- the already-materialized short-circuit is the caller's too).
- `director_holds_eight_slots_and_plans_through_its_solver`: swapping the solver swaps the plan, other slots ride along; field access plus a bound assertion that the wired solver really is a `Resolver`. The delegation body is one line (`self.solver.resolve(req)`), left to inspection.
- `director_delegates_every_stage_through_its_slots`: the production paths (`run_merge_plan`, `run_check_news`, the unmerge reads) call these same methods rather than reaching past the director. `PkgdirBinIndex` borrows its index and pkgdir; the test leaks both so the director can own a `'static` view. The fetch leg uses a pre-materialized distdir file and never touches the network.
- `binpkg_index_admits_both_local_and_remote_backends`: candidates come out as `portage_repo::Candidate`s, metadata as the raw `Packages` aux dict, the source named for the `g` bracket provenance. With no binrepos, `metadata` still falls back to the local store while `metadata_with_source` (download-side) reports nothing.
- `news_selector_is_unread_ids_plus_repo`: the GLSA half is still a Part 3 non-goal; the evaluation is `pretend.rs`'s, live-tested there.
- `scheduler_policy_gates_by_jobs_and_load_average`: a different policy (serial, deadline-aware, remote build-farm) is one `impl` of the same two methods.
- `scheduler_policy_unlimited_never_caps_but_still_load_gates`: `max_jobs` is `usize::MAX`, the value the CLI maps bare `-j` to.
