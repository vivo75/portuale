# crates/portage-repo/src/binary_index.rs
The binary package index (`Packages`) and binary candidates.

## fn read_packages_index
- Parses `<pkgdir>/Packages` (Portage `lib/portage/dbapi/bintree.py`'s own index file) into one `HashMap` per package entry. NOT the same format `read_md5_cache` reads: `KEY: value` (colon-space, confirmed by reading Portage `getbinpkg.py`'s `PackageIndex` writer), blank-line-separated blocks. `PROFILE`/`ACCEPT_KEYWORDS`/etc. header shape confirmed live against this machine's `/var/cache/binpkgs/Packages`.
- Trust detail: Portage `make.globals` ships `pkgdir-index-trusted` in the default `FEATURES`, so `_populate_local` runs with `reindex=False` and verifies size/digest at merge via `BinpkgVerifier` (backlog #174). The reindex path's mtime-staleness revalidation (`bintree.py:1108-1136`, "avoid reading the xpak if possible" fast path) lives at `portuale::binpkg::populate_local_pkgdir`, which calls this function as its fast-path cache and re-derives fresh metadata via `read_xpak_metadata`/`read_gpkg_metadata` for anything stale. This function alone is pub, format-parsing-only, with no opinion on trust. A synced remote index has no local file to revalidate against. A missing file means "PKGDIR simply has nothing yet", same tolerance as a missing repo directory in `list_candidates`.

## fn parse_packages_index
- Remote-index freshness checks live in `bintree._populate_remote_repo`. The header carries `TIMESTAMP`/`TTL`/`DOWNLOAD_TIMESTAMP` (Portage `getbinpkg.PackageIndex.header`).
- Trust detail: Portage `make.globals` ships `pkgdir-index-trusted` in the default `FEATURES`, so `_populate_local` runs with `reindex=False` and verifies size/digest at merge via `BinpkgVerifier`. The reindex path's mtime-staleness revalidation (`bintree.py:1108-1136`, "avoid reading the xpak if possible" fast path) lives at `portuale::binpkg::populate_local_pkgdir`, which calls this function as its fast-path cache and re-derives fresh metadata via `read_xpak_metadata`/`read_gpkg_metadata` for anything stale. This function alone is pub, format-parsing-only, with no opinion on trust. A synced remote index has no local file to revalidate against.

## fn parse_packages_index
- Remote-index freshness checks live in `bintree._populate_remote_repo`.

## struct BinaryIndex
- Either the `KEY: value` blocks of a `<pkgdir>/Packages` file (Portage `bintree`'s own on-disk index), or the entries a `$PKGDIR` directory scan synthesized from each binpkg file's own embedded metadata when there is no `Packages` at all (Portage `bintree._populate_local`). Every downstream binary-candidate function takes `&BinaryIndex` rather than re-reading a path, so the "where did these entries come from" decision -- read the index file, or scan the files -- is made exactly once, at the CLI boundary.
- `by_cp` rationale: consulted once per resolved atom; the old linear `CPV`-prefix scan per lookup was quadratic on a big graph.

## fn BinaryIndex::from_pkgdir
- Portage `bintree`'s index format: `KEY: value`, blank-line-separated blocks, first block the skipped global header.

## fn BinaryIndex::from_entries
- From `portuale`'s `binpkg::scan_pkgdir`. Each entry is a synthesized `Packages`-style record (`CPV`, `SLOT`, `KEYWORDS`, the dep strings, `SIZE`, `REPO`, ...).

## fn cached_binary_index
- A remote binhost's `Packages` index can be ~20 MB and is re-consulted once per resolved atom, so parsing it afresh every time made a large `emerge -p --getbinpkg` graph take minutes. From `portuale::emerge_getbinpkg` (Portage `bintree._populate_remote`). Safe because `emerge` is one-shot and a synced index doesn't change mid-run. The in-memory index is used even when the edb cache write failed (Portage ignores an unwritable cache dir, `bintree.py:1819-1823`, backlog #192); on a non-`--pretend` fetch failure even a stale copy is suppressed (Portage sets `pkgindex = None`, `bintree.py:1809`).

## static REMOTE_INDEX_OVERRIDE
- Keyed by the same `packages_dir` the resolver reads; set once per binrepo by the refresh, before any resolution. `emerge` is one-shot, but an in-process multi-run caller (or tests sharing an EROOT) must not inherit a previous run's indexes, so `pretend::run` calls `clear_remote_binary_index_overrides` where a run starts. Unit tests use pid+nanos-unique temp dirs.

## fn set_remote_binary_index_override
- Portage `bintree`'s fetched-then-maybe-cached `pkgindex` for the run: `Some(index)` resolves from it, `None` resolves nothing (a failed non-`--pretend` refresh drops even a stale cache, like Portage).

## fn clear_remote_binary_index_overrides
- Called where a run starts (review M4).

## fn build_local_binpkg_index
- Reads `config.scanned_binpkgs` (the CLI layer's scan) or `<pkgdir>/Packages` via `Path::new(&config.pkgdir)`. Backlog #123: built, not memoised -- the old per-inputs memo keyed on the scanned `Vec`'s raw pointer (an ABA hazard the moment a second `scanned_binpkgs` assignment exists in one process), never re-read `<pkgdir>/Packages` once `None` was cached, and ignored the quickpkg-direct vdb entirely. Built once per resolution (`ResolveCtx::new`); the resulting `Arc` is threaded to every consumer instead of being re-derived from `&Config` at each call site.
- `--quickpkg-direct` (Portage `actions.py:150-164` + `bintree._populate_additional`): every package installed in the quickpkg source root is injected into the binary-package pool as a candidate for the target-`ROOT` build, using that root's own vdb-recorded metadata. Portage does `dbapi.cpv_inject` on each installed package; portuale synthesizes the same `Packages`-style records so `list_binary_candidates` / `read_binary_metadata_any` / the dependency walk pick them up transparently. Portage `bintree.isremote` treats a local dup as already-present. Set once by the CLI layer -- the env-free process-global pattern `--package-moves` / `--useoldpkg-atoms` already use.

## fn set_quickpkg_direct_root
- `--quickpkg-direct-root` (Portage `actions.py:134-149`): the root whose installed packages `--quickpkg-direct` offers as binary candidates. Default (never called) is `None` -- a strict no-op. `None` also when `--usepkg` is absent or the source root coincides with the target `ROOT` (all resolved by the CLI layer). Set by `pretend.rs` from `--quickpkg-direct[-root]` before any resolution. A process-global so it needn't thread through `resolve_pretend`'s / `build_local_binpkg_index`'s signatures.

## fn quickpkg_direct_index_entries
- Portage `bintree._populate_additional`'s `_pkg_str(cpv, metadata=dict(zip(aux_keys, repo.aux_get(cpv, aux_keys))))` -- the source is the vdb, so the deps/USE/keywords are that root's installed-time snapshot. Keys mirror what `binary_candidates_from_index` / `read_binary_metadata` read: `CPV`, `SLOT` (with sub-slot), `KEYWORDS`, `USE`, `IUSE`, `LICENSE`, `PROPERTIES`, `RESTRICT`, the five `*DEPEND`, `REPO`.

## fn list_binary_candidates
- Portage `depgraph.py`: `if myopts.get("--usepkg") is True: dbs.append((bindb, "binary", ...))`, confirmed by reading it. A binary candidate's own `CPV` field (`category/package-version[-rN]`) is matched the exact same way an ebuild filename already is: filtered to this specific `category/package`, then `strip_version_prefix` peels the version off -- no general `pkgsplit` needed, since the target package name is already known here, same reasoning `list_candidates` itself already relies on. `""`/`0`/`"__binary__"` pseudo values are this candidate's own `PATH`/pseudo values -- a binary candidate is never re-read from a repo's own `metadata/md5-cache` the way an ebuild one is (`read_md5_cache` is never called for it anywhere), so those fields go unused for this source, not silently wrong.

## fn binary_candidates_from_index
- Shared body of `list_binary_candidates` (local `$PKGDIR/Packages`, `remote = false`) and `list_remote_binary_candidates`'s per-binrepo scan (`remote = true`). `repo_name` comes from the index entry's own `REPO` field (Portage records the source repo per package, so a `--getbinpkg` binary of `sys-fs/fuse` shows `::gentoo` at `-pv`), falling back to `portage.versions._unknown_repo` (`"__unknown__"`) -- the same sentinel an installed package with no vdb `repository` file already uses.
- `repo_priority = i32::MIN`: deliberately lower than any repo's own priority (main defaults to Portage's own -1000), so the existing `vercmp` -> `repo_priority` tie-break prefers an identical-version ebuild over a binary, matching Portage's `dbs` list order (`"ebuild"` always before `"binary"`) with no special-casing elsewhere. A binary candidate is never re-read from md5-cache (`read_md5_cache` is never called for it), so the pseudo fields go unused, not silently wrong.
- `eapi` comes from the index entry's own `EAPI` field (S4, #341; same EAPI-0-when-absent default as the ebuild path) -- the single constructor for index, scan-synthesized, remote and quickpkg-direct candidates, so all four carry it.

## fn dedup_binary_instances
- Portage `_iter_match_pkgs` yields same-version instances newest-`BUILD_TIME` first (`dbapi._cmp_cpv`, reversed) and the selection loop `break`s on the first instance that passes every per-instance check. `--binpkg-respect-use` has already run on `candidates` before this; this applies the atom-`[use]` check (`atom`/`config`) and, per `cpv:slot::repo` group, keeps the highest-`(BUILD_TIME, BUILD_ID)` instance that satisfies it -- or, if none in the group do, the newest one anyway (so the group still flows to the ordinary `NoVisibleCandidate` / ebuild-fallback path exactly as a lone candidate would). Ranks an instance: `(satisfies-atom-use, BUILD_TIME, BUILD_ID)` -- a satisfying instance always outranks a non-satisfying one. Ebuilds pass straight through; the kept instance takes the group's first position.
- S9 (#341): a masked instance never wins while a visible one exists (`is_visible` per instance; Portage stops at the first *acceptable* one), so an `=`+build-id `package.mask` line falls back to the older build instead of emptying the pool. When every instance is masked the ranked newest is still kept, so the masked report lists it.

## fn list_remote_binary_candidates
- `root` is the `EROOT` under which the `http(s)://`/`ssh://` cache lives (`BinRepo::packages_dir`). `BinRepo::packages_dir`: the edb cache for `http(s)`/`ssh`, the URI's own directory for `file://`. Portage treats "present in `$PKGDIR/Packages`" as downloaded (`_additional_pkgs`). Binrepos are consulted in `config.binrepos` order (already `(priority, name)`-sorted); a later binrepo does not shadow an earlier one for the same version -- matching Portage's binrepo-priority order.
- Portage `bintree.isremote` is per `cpv` + `build_id` under binpkg-multi-instance (backlog #300): a local build 6 must not hide a remote build 7. An entry with no `BUILD_ID` is its own `None` key, keeping the pre-multi-instance version-level shadowing. The shadow set is only grown a whole binrepo at a time (all of that binrepo's instances stay; `dedup_binary_instances` collapses them later by `(cpv, slot, repo)` keeping the newest `BUILD_TIME`).

## fn pick_binary_instance
- Picks one `Packages` entry of `cat/pkg-version` (binpkg-multi-instance aware, backlogs #299/#301). `Some(id)` selects the entry whose `BUILD_ID` is exactly `id` (the build the resolver planned); `None` selects the newest build -- highest `(BUILD_TIME, BUILD_ID)`, Portage `dbapi._cmp_cpv` order -- which for an index with a single (or `BUILD_ID`-less) entry is simply that entry.

## fn find_remote_binpkg
- The raw `Packages` index record for `category/package-version` from the first binrepo (in `binrepos` order) whose cached index carries it -- the repo itself (its `sync_uri` for the download, its `verify_signature` for Portage `gpkg.__init__`'s own per-binrepo signature policy, `gpkg.py:792-819`) plus the index record (`PATH`, `SIZE`, the `SHA*`/`MD5` digests). `None` if no configured binrepo lists that exact CPV. When the binrepo lists several builds of the version, the newest one is returned; use `find_remote_binpkg_instance` to ask for a specific `BUILD_ID`.

## fn find_remote_binpkg_instance
- `find_remote_binpkg` for one specific build (backlog #301): with `Some(build_id)` the binrepos are scanned in order for the record carrying exactly that `BUILD_ID`; if no binrepo has it (or `None` was given) the newest build of the version from the first binrepo that lists the version is returned.
