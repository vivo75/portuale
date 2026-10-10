# crates/portage-repo/src/binary_index.rs
The binary package index (`Packages`) and binary candidates.

## fn read_packages_index
- NOT the same format `read_md5_cache` reads: `KEY: value` (colon-space, confirmed by reading Portage `getbinpkg.py`'s `PackageIndex` writer), blank-line-separated blocks. `PROFILE`/`ACCEPT_KEYWORDS`/etc. header shape confirmed live against this machine's `/var/cache/binpkgs/Packages`.
- Trust detail: Portage `make.globals` ships `pkgdir-index-trusted` in the default `FEATURES`, so `_populate_local` runs with `reindex=False` and verifies size/digest at merge via `BinpkgVerifier`. The reindex path's mtime-staleness revalidation (`bintree.py:1108-1136`, "avoid reading the xpak if possible" fast path) lives at `portuale::binpkg::populate_local_pkgdir`, which calls this function as its fast-path cache and re-derives fresh metadata via `read_xpak_metadata`/`read_gpkg_metadata` for anything stale. This function alone is pub, format-parsing-only, with no opinion on trust. A synced remote index has no local file to revalidate against. A missing file means "PKGDIR simply has nothing yet", same tolerance as a missing repo directory in `list_candidates`.

## fn parse_packages_index
- Remote-index freshness checks live in `bintree._populate_remote_repo`. The header carries `TIMESTAMP`/`TTL`/`DOWNLOAD_TIMESTAMP` (Portage `getbinpkg.PackageIndex.header`).
- Trust detail: Portage `make.globals` ships `pkgdir-index-trusted` in the default `FEATURES`, so `_populate_local` runs with `reindex=False` and verifies size/digest at merge via `BinpkgVerifier`. The reindex path's mtime-staleness revalidation (`bintree.py:1108-1136`, "avoid reading the xpak if possible" fast path) lives at `portuale::binpkg::populate_local_pkgdir`, which calls this function as its fast-path cache and re-derives fresh metadata via `read_xpak_metadata`/`read_gpkg_metadata` for anything stale. This function alone is pub, format-parsing-only, with no opinion on trust. A synced remote index has no local file to revalidate against.

## fn parse_packages_index
- Remote-index freshness checks live in `bintree._populate_remote_repo`.

## struct BinaryIndex
- `by_cp` rationale: consulted once per resolved atom; the old linear `CPV`-prefix scan per lookup was quadratic on a big graph.

## fn BinaryIndex::from_pkgdir
- Portage `bintree`'s index format: `KEY: value`, blank-line-separated blocks, first block the skipped global header.

## fn BinaryIndex::from_entries
- From `portuale`'s `binpkg::scan_pkgdir`. Each entry is a synthesized `Packages`-style record (`CPV`, `SLOT`, `KEYWORDS`, the dep strings, `SIZE`, `REPO`, ...).

## fn cached_binary_index
- From `portuale::emerge_getbinpkg` (Portage `bintree._populate_remote`). Safe because `emerge` is one-shot and a synced index doesn't change mid-run. The in-memory index is used even when the edb cache write failed (Portage ignores an unwritable cache dir, `bintree.py:1819-1823`, backlog #192); on a non-`--pretend` fetch failure even a stale copy is suppressed (Portage sets `pkgindex = None`, `bintree.py:1809`).

## static REMOTE_INDEX_OVERRIDE
- Keyed by the same `packages_dir` the resolver reads; set once per binrepo by the refresh, before any resolution. `emerge` is one-shot, but an in-process multi-run caller (or tests sharing an EROOT) must not inherit a previous run's indexes. Unit tests use pid+nanos-unique temp dirs.

## fn clear_remote_binary_index_overrides
- Called where a run starts (review M4).

## fn build_local_binpkg_index
- Reads `config.scanned_binpkgs` (the CLI layer's scan) or `<pkgdir>/Packages` via `Path::new(&config.pkgdir)`. Built once per resolution (`ResolveCtx::new`); the old memo's ABA hazard (a second `scanned_binpkgs` assignment in one process reusing a raw-pointer key) is gone.
- `--quickpkg-direct`: Portage does `dbapi.cpv_inject` on each installed package; portuale synthesizes the same `Packages`-style records so `list_binary_candidates` / `read_binary_metadata_any` / the dependency walk pick them up transparently. Portage `bintree.isremote` treats a local dup as already-present. Set once by the CLI layer -- the env-free process-global pattern `--package-moves` / `--useoldpkg-atoms` already use.

## fn set_quickpkg_direct_root
- Default (never called) is `None` -- a strict no-op. `None` also when `--usepkg` is absent or the source root coincides with the target `ROOT` (all resolved by the CLI layer).

## fn quickpkg_direct_index_entries
- Portage `bintree._populate_additional`'s `_pkg_str(cpv, metadata=dict(zip(aux_keys, repo.aux_get(cpv, aux_keys))))` -- the source is the vdb, so the deps/USE/keywords are that root's installed-time snapshot.

## fn list_binary_candidates
- Portage `depgraph.py`: `if myopts.get("--usepkg") is True: dbs.append((bindb, "binary", ...))`, confirmed by reading it. `""`/`0` pseudo values are this candidate's own `PATH`/pseudo values.

## fn binary_candidates_from_index
- A `--getbinpkg` binary of `sys-fs/fuse` shows `::gentoo` at `-pv` because Portage records the source repo per package. Falls back to `portage.versions._unknown_repo`.
- `repo_priority = i32::MIN`: deliberately lower than any repo's own priority (main defaults to Portage's own -1000), so the existing `vercmp` -> `repo_priority` tie-break prefers an identical-version ebuild over a binary, matching Portage's `dbs` list order (`"ebuild"` always before `"binary"`) with no special-casing elsewhere. A binary candidate is never re-read from md5-cache (`read_md5_cache` is never called for it), so the pseudo fields go unused, not silently wrong.

## fn dedup_binary_instances
- `--binpkg-respect-use` has already run on `candidates` before this; visibility and the other per-instance checks are Portage `_iter_match_pkgs`'. Ebuilds pass straight through; the kept instance takes the group's first position.

## fn list_remote_binary_candidates
- `root` is the `EROOT` under which the `http(s)://`/`ssh://` cache lives (`BinRepo::packages_dir`). `BinRepo::packages_dir`: the edb cache for `http(s)`/`ssh`, the URI's own directory for `file://`. Portage treats "present in `$PKGDIR/Packages`" as downloaded (`_additional_pkgs`). Binrepos are consulted in `config.binrepos` order (already `(priority, name)`-sorted); a later binrepo does not shadow an earlier one for the same version -- matching Portage's binrepo-priority order.
- Portage `bintree.isremote` is per `cpv` + `build_id` under binpkg-multi-instance.
