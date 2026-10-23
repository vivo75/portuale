# crates/portuale/src/emerge_getbinpkg.rs
`emerge --getbinpkg` / `--getbinpkgonly` execution: binhost index refresh, resolve, per-entry merge.

## module
- Portage `emerge --getbinpkg <atom>` / `--getbinpkgonly <atom>` execution: refresh each remote binhost's live index (in `--pretend` exactly like in a Portage merge, backlog #192), resolve the graph, then merge every resolved entry -- dispatching per entry on its `source` (a `Binary` candidate is downloaded+merged, anything else is built+merged from source).
- The `--pretend` half of `--getbinpkg`/`--getbinpkgonly` already shipped (Portage `bintree`'s `binrepos.conf`/`PORTAGE_BINHOST` parsing, remote binhost candidates from each binhost's `Packages` index, the `g` bracket column). This module is the other half: the live index refresh + the file download + the merge.
- `refresh_binhost_indexes` ports Portage `bintree._populate_remote`: fetch `<sync_uri>/Packages` (a `file://` binhost is read from its own directory) and best-effort-cache it at the same `<EROOT>/var/cache/edb/binhost/<host>/<path>/Packages` location `list_remote_binary_candidates` reads, with the fetched index handed to the resolver in memory. Runs BEFORE resolution, in `--pretend` exactly like in a Portage merge (Portage `actions.py:3752` passes `getbinpkg_refresh=True` unconditionally; `pretend` only selects the stale-fallback message, never skips the fetch, `bintree.py:917-920`, backlog #192), so the resolver sees the fresh pool.
- `run_merge_plan` iterates the resolved entries (already in dependency-first merge order), dispatching per entry on `source`: `Binary` -> `merge_one_binary_entry` (find its `Packages` record via `find_remote_binpkg`, `wget`/copy into `$PKGDIR`, `SIZE`-check, `ebuild_merge::merge_binpkg`); else -> `emerge_build::merge_one_source_entry`. Portage `--getbinpkg`'s own "prefer a binary, fall back to source" is the resolver's job, not this loop's; `--getbinpkgonly` (binary-only resolve) simply never produces a non-`Binary` entry.
- Compressed remote index (`Packages.gz` / `Packages.zst`): tried now (`refresh_binhost_indexes` fetches `Packages.<ext>` and pipes it through `gzip -dc` / `zstd -dc` into the plain `Packages` cache file), with plain `Packages` as fallback. Not modelled: `Packages.bz2`/`.lz4` and Portage's exact preference ordering.
- No live `layout.conf` negotiation (`binpkg-multi-instance`, path layout) -- the index `PATH` field (or the default `<cat>/<pf>.tbz2`) is trusted outright, same "trust the index" stance the `--pretend` half takes.
- Digest verification checks `SIZE` and the `Packages` record's `MD5` and `SHA1` (the md5/sha1 of the whole `.tbz2`, Portage `bintree`'s own fields, `_pkgindex_hashes = ["MD5", "SHA1"]`). Each field present is verified (Portage `_get_digests` collects every valid checksum key present, `digestCheck` verifies them all). A downloaded gpkg additionally has its internal `Manifest` (`DATA` BLAKE2B/SHA512 lines) and its GPG signature layer (detached `.sig` sidecars + clear-signed `Manifest`, Portage `_verify_binpkg` -- see `binpkg::GpgVerify`) verified at merge time (`binpkg::extract_binpkg` -> `verify_gpkg_manifest`).

## fn hide_binhost_passwd
- Ports `fetch.py::_hide_url_passwd` verbatim (`fetch.py:73-74`): `re.sub(r"//([^:\s]+):[^@\s]+@", r"//\1:*password*@", url)`. Portage applies it to the binhost URL in its own `Error fetching binhost package info` line (`bintree.py:1794-1796`).
- The single global substitution is implemented exactly (same pattern and replacement text), so edge behaviour matches: a password containing whitespace is left alone (`[^@\s]+` cannot span it), while a user containing `/` is still masked (`[^:\s]+` allows it).

## fn wget_http_error
- Ports Portage `urllib.error.HTTPError`'s own `str` (`"HTTP Error {code}: {reason}"`, verified against the stdlib: `HTTPError(url, 500, "Internal Server Error", ...)` stringifies to exactly `"HTTP Error 500: Internal Server Error"`): recovers that shape from a captured wget transcript. wget's own `HTTP request sent, awaiting response... {code} {reason}` line carries the server's status line verbatim, so the reconstruction is byte-identical whenever the server sent a reason phrase. `None` when the transcript holds no response line (DNS failure, connection refused, ...): Portage would print its own urllib `URLError` string there, which wget cannot reproduce -- the caller falls back to the fetch summary instead (documented, not invented parity).

## fn binhost_fetch_warning
- Ports Portage `bintree._populate_remote`'s `except OSError` pair (`bintree.py:1793-1798`): the leading-blank-line `!!! [<name>] Error fetching binhost package info from '<url>'` line plus the `!!! [<name>] <detail>` line and its trailing blank line, as one stderr block. `detail` is Portage's `str(err)` -- the `HTTP Error 500: Internal Server Error` shape for a 500 (see `wget_http_error`), the fetch summary otherwise.

## fn binhost_no_timestamp_warning
- Ports Portage `bintree._populate_remote_repo`'s no-`TIMESTAMP` drop (`bintree.py:1722-1732`): `\n\n!!! [<name>] Binhost package index  has no TIMESTAMP field.\n` on stderr. The double space is Portage's own implicit concatenation (`"index " " has no ..."`). An unparseable stamp takes the same arm (Portage only tests falsiness there) rather than Portage's `int()` `ValueError` unwind.

## fn binhost_unsupported_version_warning
- Ports the version-gate drop (`bintree.py:1735-1744`): Portage formats `header.get("VERSION")`, i.e. `None` when absent.

## fn refresh_binhost_indexes
- Portage `bintree._populate_remote`: refresh every binrepo's live `Packages` index and hand it to the resolver in memory -- in `--pretend` exactly like in a Portage merge (Portage `actions.py:3752` passes `getbinpkg_refresh=True` unconditionally; `pretend` only selects the stale-fallback message, never skips the fetch, `bintree.py:917-920`, backlog #192).
- S9 (#347): remote entries matching the effective `--getbinpkg-exclude` (or missing from a non-empty `--getbinpkg-include`) never reach the in-memory pool. Each binrepo's own keys fold with the CLI sets and conflicting atoms warn per repo (`GetbinpkgFilter::for_binrepo`, in refresh order — Portage populates before adjusting configs, so the CLI-overlap warning in `warn_getbinpkg_cli_overlap` prints after, S0 `g03-both`). The on-disk/EDB cache keeps the unfiltered index (a later run without the option is unpoisoned); local `PKGDIR` instances are never filtered.
- Per binrepo (Portage `_populate_remote_repo`): a `frozen` repo, or one whose cached index is still within its `TTL`, is used from cache with Portage's own `[name] Local copy of remote index is ... and will be used.` note (stdout); otherwise the remote index is fetched (a `file://` repo is read from its own directory, `Packages.gz` first like Portage; `http(s)` tries `Packages.gz` / `Packages.zst` / `Packages`); a fetched index with a `TIMESTAMP` no newer than the cache keeps the cache silently; a newer one (or any fetch with no cache to compare against) is cached with a fresh `DOWNLOAD_TIMESTAMP` and used -- the cache write is best-effort (an unwritable cache dir is ignored, `bintree.py:1819-1823`; only a write error on a *writable* dir warns, where Portage would re-raise); a failed fetch warns Portage's `!!! [repo] Error fetching ...` pair (stderr) and then -- `--pretend`: Portage's `[name] Local copy of unavailable remote index will be used due to --pretend` note (stdout), resolving from the stale cache; real merge: nothing more, and even a stale cache is dropped (Portage `pkgindex = None`, `bintree.py:1809`).
- A fetched index with no `TIMESTAMP` header, or an unparseable one, is dropped with Portage's own `!!! [name] Binhost package index  has no TIMESTAMP field.` (stderr, `bintree.py:1722-1732`); a fetched index whose `VERSION` is missing, unparseable, or newer than Portage's `_pkgindex_version` (`0`, `bintree.py:2429-2437`) is dropped with `!!! [name] Binhost package index version is not supported: '<ver>'` (`bintree.py:1735-1744`, `'None'` when the header is absent).
- Deliberate narrowings (all documented, none invented parity): see the bullets below.
- A failed refresh is NON-FATAL either way (backlog #175).
- Conditional-GET (`If-Modified-Since` / `304 Not Modified`) is not sent -- wget always downloads the body, so a repeat run against an unchanged index re-downloads where Portage would print `... is up-to-date and will be used.` Resolution is identical (the `TIMESTAMP` compare keeps the cache silently either way); only that one stdout line differs, and only across runs.
- Either drop (no/unparseable `TIMESTAMP`, unsupported `VERSION`) discards even a stale cache for the run (Portage `pkgindex = None`), in `--pretend` exactly like in a non-pretend merge -- Portage draws no pretend distinction here.
- Not modelled: `--verbose`'s `Last-Modified` mismatch warning, `ssh://` transports, trust-helper/`gpkg_only` gating.

## fn refresh_one_binrepo
- Portage warns with `base_url` as configured (`bintree.py:1488,1795`).
- Frozen-repo short-circuit ports `if local_timestamp and (repo.frozen or not getbinpkg_refresh)` (`bintree.py:1528-1533`).
- `DOWNLOAD_TIMESTAMP + TTL` freshness ports `bintree.py:1535-1545`.
- Fetch-before-write ports Portage fetching first and only then trying `ensure_dirs` + the atomic write (`bintree.py:1463-1823`) -- never `create_dir_all`-then-abort like portuale used to (backlog #192).
- Pretend-fallback ports `bintree.py:1801-1808`; non-pretend drop ports `pkgindex = None` (`bintree.py:1809`).
- `TIMESTAMP` gate ports `bintree.py:1722-1732`; `VERSION` gate ports `_pkgindex_version_supported` (`bintree.py:2429-2437`, `_pkgindex_version = 0`, `bintree.py:547`) with the `!!! ... version is not supported: ...` warning (`bintree.py:1735-1744`).
- Newer-remote rule ports `not local_timestamp or int(local) < int(remote)` (`bintree.py:1739-1744`).
- Best-effort cache write ports `ensure_dirs` + `atomic_ofstream` (`bintree.py:1811-1818`): an unwritable cache dir is ignored ("that's alright"); only a write error on a writable dir warns, where Portage would re-raise -- portuale refreshes stay non-fatal per backlog #175.

## struct RefreshOutcome
- One binrepo of `refresh_binhost_indexes`: installs that repo's run index via `portage_repo::set_remote_binary_index_override` (keyed by the same `packages_dir` the resolver reads) and returns Portage's messages for the caller to print -- the `!!! [repo] ...` warning block for stderr, the `Local copy ...` note for stdout.

## fn edb_cache_packages_file
- Portage `bintree._populate_remote_repo`'s own cache file: `<EROOT>/var/cache/edb/binhost/<host>/<url-path>/Packages` (`bintree.py:1497-1504`). For `http(s)`/`ssh` that is exactly `BinRepo::packages_dir`; for `file://` it is the EROOT-side copy (Portage caches even local indexes: `host` is empty, so `file:///srv/pkgs` caches under `<EROOT>/var/cache/edb/binhost/srv/pkgs/Packages`).

## fn use_cached_note
- Ports the skip note (`bintree.py:1785-1789`).

## fn cache_dir_writable
- Ports Portage `os.access(dirname, W_OK)` (`bintree.py:1821`) without libc. Like `access(2)` this is a permission-bit check rather than a write attempt, so on a read-only filesystem with writable bits a failed write still warns (where Portage would re-raise -- portuale refreshes stay non-fatal per backlog #175).

## fn stamp_download_timestamp
- Ports `pkgindex.header["DOWNLOAD_TIMESTAMP"] = "%d" % time.time()` (`bintree.py:1806`).

## fn fetch_remote_index_text
- `file://` handling ports the `("Packages.gz", "Packages")` loop (`bintree.py:1595-1604`): `Packages.gz` first, a missing `.gz` falls through to the plain file, a corrupt `.gz` fails outright. Anything else goes over wget into temp files (never the cache itself), preferring a compressed index Portage serves (`Packages.gz`, then portuale's `Packages.zst` superset cut, then plain `Packages`). Every attempt runs quiet (`download_via_wget_quiet`): the transcript is captured for message shaping, never inherited onto stdout/stderr.
- Corrupt-compressed-index behaviour: Portage's `gzip.BadGzipFile` subclasses `OSError` (verified against the stdlib), escaping the `("Packages.gz", "Packages")` loop straight to the outer `except OSError` (`bintree.py:1790-1809`).
- Prefers a compressed index when the binhost serves one, decompressing it into the plain cache file.

## fn http_or_summary
- Prefers `str(HTTPError)` like Portage.

## struct RunMergePlanArgs::mode
- Backlog #197: the scheduler-display mode (background + Jobs visibility), computed once by the caller. A `--quiet` binary merge prints no leading blanks, exactly like Portage's background scheduler. Stated cut: Portage drives binary tasks through the same display (Portage `Scheduler._merge_exit` / `_do_merge_exit`, `Scheduler.py:1532-1560`), so a Portage `--quiet --verbose` (or `-jN`) binary merge shows `>>> Jobs:` lines while portuale shows none here -- unprobed against Portage, Jobs events for the mixed plan are a later slice.

## fn run_merge_plan
- Portage `emerge --getbinpkg <atom>` / `--getbinpkgonly <atom>` (no `--pretend`): merge every resolved entry (already in Portage dependency-first merge order), dispatching per entry on `entry.source` -- Portage `--getbinpkg`'s own "prefer a binary package, fall back to a source build" is entirely the resolver's job (it already stamped the right `source` on each `GraphEntry`); this just executes the plan. A `Binary` entry is downloaded (if remote) and merged (`merge_one_binary_entry` -> `ebuild_merge::merge_binpkg`, all four `pkg_*` hooks + same-slot replace); anything else is built + merged from source (`emerge_build::merge_one_source_entry` -> `ebuild_merge::run_merge`); `AlreadyInstalled` is a silent no-op either way. `--getbinpkgonly` (binary-only resolve, `usepkgonly`) simply never yields a non-`Binary` entry, so the same function serves both.
- #72 B3: a removal installs no binary (execution is a non-goal).
- Backlog #322: the vendored-runtime gate -- a hookless gpkg never spawns a shell, and one with hooks reports the same message at its first phase.
- `Scheduler._pkg_count` for this run (backlog #177).
- Backlog #197: Portage `Scheduler` owns one `JobStatusDisplay` per merge run.
- The director seam executes every unit through the source/binary engines (Portage `MergeListItem._start`'s own `type_name` routing).
- `failed_install_pkg_msg` (#444): the merge-phase half of `failed_pkg_msg` -- `>>> Failed to install <cpv>[ to <root>][, Log file:]` plus the `>>>  '<log>'` tail (Portage `Scheduler._failed_pkg_msg` with `install`/`to`, the postinst-failure wording).
- `run_merge_plan` returns the merge-pass report (#444) so a source entry's postinst failure (reachable through the mixed plan) skips the world record downstream.

## fn merge_one_binary_entry
- The per-package line ports `MergeListItem._start`'s binary-entry line (backlog #177): this is the `>>> Merging binary package ...` line's shape.
- Only a `$PKGDIR` file on disk or a *remote* entry (`entry.remote_binary`, set by the resolver for a binhost-sourced candidate) may produce a file here. A resumed entry always lands with `remote_binary: false` (`resume_entry` records only `cat/pkg-ver`, so "was this fetched remotely" is not re-derived) -- and Portage replays a resumed binary against its local bintree, matched by the recorded `mtimedb["resume"]["binpkgs"]` build metadata, never by re-hitting the binhost. A resumed binary never downloaded therefore fails here (Portage: `PackageNotFound`, "An expected package is not available") instead of silently fetching a possibly-different file. Same for a fresh local-`$PKGDIR` entry: its fetch already happened, so a missing file is an error, not a refetch.
- `gpkg.__init__(verify_signature=...)`: a repo fetched from a configured binrepo uses that repo's own policy, not the bare `FEATURES` default (backlog #43). `None` for a local `$PKGDIR` file, which has no binrepo and keeps `merge_options.gpg_verify`.
- The H3 remote-lookup seam (`RemoteBinhostIndex`) goes through the director's `BinpkgIndex` seam and scans the configured binrepos in order exactly like `portage_repo::find_remote_binpkg` did -- the record travels with the owning binrepo's section name, which is how the download recovers its `sync-uri` and `verify-signature` policy.
- Missing-file arm ports `_emerge/BinpkgVerifier._start`'s `os.stat` failure arm (backlog #187): Portage's scheduler prints the `>>> Emerging` line before the verifier runs; output goes to stdout AND the package `build.log` (Portage `SchedulerInterface.output(msg, log_path)` appends to both); `local_index_lists_cpv` is Portage's own branch condition (`bintree.dbapi.cpv_exists`, `BinpkgVerifier.py:48`).
- Digest arm ports `_emerge/Binpkg.py` + `BinpkgVerifier` (backlog #174): Portage `bintree._get_digests` reads the index, never the re-scanned file; the honoured digest list is fixed (see `binpkg::verify_binpkg_against_index`); the mismatch renames the file to `._checksum_failure_.<rand>` (Portage `_checksum_failure_temp_file`) and fails with Portage's `Scheduler._failed_pkg_msg(..., "emerge", "for")` shape (the `, Log file:` suffix and `>>>  '<log>'` line appear because the just-written `build.log` is non-empty, Portage `_locate_failure_log`); a record without `SIZE` verifies nothing either (Portage's own `if "size" not in digests: return OK`); remote downloads were already checked against the remote record (Portage `_get_digests` prefers the remote metadata too). Portage prints nothing else for this failure: no resume-list notice (Portage's notices -- `actions.py:348,363`, `Scheduler.py:2493` -- fire only for resolution-time `UnsatisfiedResumeDep` / `PackageNotFound`, never for a merge-time package failure, and the end-of-run failed-packages summary only for `> 1` failure or `--keep-going`) and no `emerge: ...` line (a merge failure exits via `FAILURE`, not the action error path).
- `Binpkg._start` runs `prepare_build_dirs` + `clean_log` before `BinpkgVerifier`, and `_digest_exception` appends the block to the fresh `PORTAGE_LOG_FILE`: the log holds exactly this block. `build_log_path` is that same `${PORTAGE_BUILDDIR}/temp/build.log` (plus Portage's `PORTAGE_LOGDIR`/compress naming).
- The between-line ports `PackageMerge._start`'s per-package line (backlog #177): the binpkg is located/fetched above (Portage's `Binpkg` chain), the vdb merge runs below (Portage's `EbuildMerge` chain).
- Backlog #333: Portage's per-task `config.reload()` refreshes the protect lists its `dblink` reads (`emerge_build::reloaded_config_protect`).
- The completed line ports `PackageMerge._install_exit`'s per-package line (backlog #177): `Completed (N of M) cpv::repo`.

## fn missing_binpkg_block
- Portage `_emerge/BinpkgVerifier._start`'s `os.stat` ENOENT text (`BinpkgVerifier.py:40-60`, backlog #187), as one block (each line `\n`-terminated, no surrounding blanks -- the bed oracle's bytes): the stale-index arm when the cpv is still in the `<pkgdir>/Packages` index (the file was removed without refreshing it), else Portage's `!!! Fetching Binary failed` arm. Pure shaper, for tests and the `build.log` write alike.

## fn local_index_record
- The `<pkgdir>/Packages` index record vouching for a local binpkg merge (Portage `bintree._get_digests`' own index read, backlog #174): the stanza whose `CPV` is `<category>/<package>-<version>`. With several (multi-instance `BUILD_ID`s), the `BUILD_ID` picks -- else the stanza whose `PATH` basename is the located file's own name, so a moved file still verifies against its own record. `None` when no stanza vouches (the file was synthesized at scan from its own bytes). A record without `SIZE` still returns here -- Portage's "verifies nothing without one" (`BinpkgVerifier._start`'s early OK) lives in `verify_binpkg_against_index`, not in the lookup.

## fn checksum_filter_from_config
- Reads `PORTAGE_CHECKSUM_FILTER` from the process env first, then `config.other_vars` (backlog #436d).

## fn apply_checksum_filter
- Ports `portage.checksum._hash_filter` + `_apply_hash_filter`: filters the uppercase hash names against `PORTAGE_CHECKSUM_FILTER`, always preserving at least one hash (backlog #436d).

## fn verified_digest_names_filtered
- Ports `BinpkgVerifier._start` / `_display_success`: uppercase hash names present in the index record (filtered by `PORTAGE_CHECKSUM_FILTER`) plus `size`, ASCII-sorted (`MD5 SHA1 size`); `None` without a parseable `SIZE` (backlog #428, #436d).

## fn download_and_verify
- Portage `bintree.gettbz2` / `_get_digests` + `digestCheck`: uses per-binrepo `fetchcommand`/`resumecommand` when set in `binrepos.conf` (backlog #436a), else `FETCHCOMMAND`/`RESUMECOMMAND` from config (backlog #428); the `Packages` record's `MD5`/`SHA1` fields surviving `PORTAGE_CHECKSUM_FILTER` (backlog #436d) are verified against the downloaded file, and a mismatch removes the file and fails. A record without both `SIZE` and a checksum is refused first (see `enum DownloadError`, backlog #501).

## enum DownloadError
- `MissingDigests { partial }` is the remote-record refusal (backlog #501): `download_and_verify` fetches into `<file>.partial` first, then refuses a record without a parseable `SIZE` *and* a checksum key (`index_has_size_and_checksum`), leaving the `.partial` in place. Portage: `BinpkgVerifier._start` runs after the download, so the fetch is attempted (oracle `LLM/evidence/2026-10-23-500-501/`, casts `s2-hb-*`: GET of the gpkg is logged, then rc 1). Callers: the merge path prints the block and the `>>> Failed to emerge` tail; `--fetchonly` prints the block; `remote/plan.rs` uses the one-line `Display` text (its output shape is not pinned against Portage).

## fn report_digest_failure
- Shared by the local-digest and the missing-digest refusals. The local branch also writes the block to `build.log` and names the log in the tail; the remote refusal has no log (`has_log: false`): Portage's `PORTAGE_LOG_FILE` is not set when the verifier refuses a freshly fetched binary (oracle: empty `PORTAGE_TMPDIR/portage`, tail `>>> Failed to emerge <cpv> for <ROOT>/` with no `, Log file:`). Known older difference, not touched: Portage prints ROOT with a trailing slash, portuale echoes the string it got.

## fn failed_pkg_msg
- Ports Portage `Scheduler._failed_pkg_msg(pkg, "emerge", "for")` (`Scheduler.py:2366-2381`, backlog #174): each `_status_msg` emits its own leading blank line, then `>>> <msg>`; the log line carries Portage's own leading space (`f" '{log}'"`). Each `_status_msg` writes its own leading blank line first (`writemsg_level("\n")` before `displayMessage`), and the `>>> ` prefix is `JobStatusDisplay._format_msg`; the ` for <root>` suffix appears when `ROOT != "/"` (Portage `pkg.root_config.settings["ROOT"]`).

## fn local_index_lists_cpv
- A `BUILD_ID` matching nothing still leaves the plain `CPV` hit: Portage's `cpv_exists` passes a plain string, which `_instance_key_multi_instance` resolves to the latest instance (`virtual.py:53-68`) -- any surviving instance keeps it true.

## fn resolve_local_binpkg
- Ports the two naming contracts (Portage `bintree.getname` / `_allocate_filename` vs `_allocate_filename_multi`): single-instance `<pkgdir>/<cat>/<pf>.{tbz2,gpkg.tar}`; `FEATURES=binpkg-multi-instance` `<pkgdir>/<cat>/<pn>/<pf>-<build_id>.{xpak,gpkg.tar}`. Without a `build_id` the `<pn>/` subdir is still scanned for a `<pf>-<id>` file (an older index with no `BUILD_ID` field), preferring the highest `<id>`.

## tests
- `serve`/`serve_with_status`: loopback HTTP stubs. `serve` serves each `routes` entry (`"/path" -> body`) over plain HTTP on `127.0.0.1`, one response per connection, for `requests` connections total -- enough for the `Packages` fetch + N binpkg fetches a `--getbinpkg` run makes (each `wget` uses its own `Connection: close`). `serve_with_status` takes an explicit status line per route (`"/path" -> ("500 Internal Server Error", body)`), for the fault-injection refresh tests -- unlisted paths still 404.
- `packages_index` helper stamps both headers Portage gates on.
- `resolve_local_binpkg_finds_the_multi_instance_layout`: the `noto-20260901-1.gpkg.tar` layout `resolve_local_binpkg` used to miss entirely; exact `build_id`, no-`build_id` subdir scan, highest wins.
- `merge_one_binary_entry_fails_a_truncated_index_vouched_binpkg_at_merge`: backlog #174, end to end -- Portage `emerge --oneshot --usepkgonly l32/faultpkg` with the gpkg truncated to half selects the binary and fails at merge with `!!! Digest verification failed:` (`Failed on size verification`, `Got`/`Expected`), `>>> Failed to emerge <cpv> for <root>, Log file:` + `>>>  '<log>'` (Portage `Scheduler._failed_pkg_msg`) -- rc 1 with a clean root (no vdb entry, no installed file; merge fails before unpacking). `>>>` lines to stdout (pinned by the pmtest contract test). Scan half: the truncated file is accepted on the index's word (no `!!! Invalid binary package`). `SchedulerInterface.output(msg, log_path)` lands the block in `build.log` too, non-empty so Portage `_locate_failure_log` reports it.
- `merge_one_binary_entry_aborts_a_stale_index_binary_like_portage`: backlog #187, the bed's F3a shape (`l32/faultpkg` removed from `$PKGDIR`, stanza kept -- the bed's `$nopkg` after `find -delete`). Portage selects the indexed binary (trusted index, `bintree._populate_local(reindex=False)`); `BinpkgVerifier._start` stats the missing file, finds the cpv still in the dbapi (`cpv_exists`), prints the stale-index pair to stdout AND `build.log` (byte for byte Portage's `BinpkgVerifier.py:50-51` text), followed by the `>>> Failed to emerge ...` tail (byte-pinned by `failed_pkg_msg_matches_portage_failed_pkg_msg_bytes`; `>>>` lines pinned by the pmtest contract test). Scan half: the removed file's stanza rejoins the pool. Portuale used to fail here with its own `no binpkg file under ...` error.
- `merge_one_binary_entry_reports_fetching_failed_without_an_index_stanza`: backlog #187, Portage's other `BinpkgVerifier._start` ENOENT arm (`BinpkgVerifier.py:57`) -- cpv NOT in the index (`cpv_exists` false).
- `failed_pkg_msg_matches_portage_failed_pkg_msg_bytes`: ports `Scheduler._failed_pkg_msg(..., "emerge", "for")` (`Scheduler.py:2366`); each `_status_msg` emits its own leading blank line, then `>>> <msg>`; the log line carries Portage's own leading space (`f" '{log}'"`); ` for <root>` only when `ROOT != "/"`. The bed oracle's tail is the `ROOT == "/"` row, byte for byte. Only a lone failure is silent: a `--keep-going` combined message keeps the normal tail.
- `test_gpg_home`: a writable copy of Portage's committed GnuPG test keyring (see `binpkg.rs`'s `test_gpg_home`); `gpg` refuses a homedir it doesn't own outright, so the committed tree itself is never used directly.
- `merge_binpkg_rejects_a_tampered_signed_gpkg`: flip a byte via unpack-mutate-repack (tar framing stays parseable); must fail on the detached `.sig` before unpacking. Lists the prefix's files, not the prefix dir: a `<prefix>/` directory member would be refused by Portage's one-level structure check (backlog #58 S5).
- `merge_binpkg_installs_a_real_tbz2_into_the_vdb`: the saved env + ebuild are kept in the vdb (Portage does; portuale needs them for pkg_preinst/pkg_postinst); this fixture's DEFINED_PHASES is `install` only, so no hook ran. Portage `_consolidate_to_metadata_file`: the vdb entry carries a consolidated `metadata` file -- `#format=1` header, sorted `KEY=value` lines, `#dir_mtime=` last and matching the entry dir's own `st_mtime_ns` (Portage's reader rejects a stale value, which is what broke `emerge -C`).
- `merge_binpkg_gpkg_strips_the_image_prefix_and_records_binpkgmd5_and_metadata`: regression for the broken `/var/db/pkg/<cat>/<pf>` a gpkg merge used to write (`/image`-prefixed CONTENTS, no BINPKGMD5, no consolidated metadata -- blocking `emerge -C` in Portage and portuale). BINPKGMD5 = md5 of the whole binpkg file (Portage `_emerge/Binpkg._start_task`).
- `merge_binpkg_collision_protect_aborts_when_another_package_owns_the_file`: Portage `dblink.merge()`'s `_collision_protect`, now shared with the source `merge_after_install`. A pre-existing vdb entry owns `/usr/share/packagepkg/hello.txt`; `FEATURES=collision-protect` must abort the binpkg merge rather than overwrite it.
- `merge_binpkg_runs_pkg_preinst_and_pkg_postinst_from_the_saved_env`: the fixture's `pkg_preinst` `die`s if the payload is merged and `pkg_postinst` `die`s if not -- proving the genuine treewalk() ordering (preinst before the copy, postinst after).
- `merge_binpkg_runs_pkg_pretend_before_pkg_setup`: ports Portage `Scheduler._run_pkg_pretend`, which runs `pkg_pretend` for a binary package too.
- `merge_binpkg_regenerates_a_curated_merge_time_vdb_environment`: backlog L1-c -- Portage `dblink.treewalk` regenerates the vdb `environment.bz2` from the live merge-time environment (`PORTAGE_UPDATE_ENV` around the `postinst` phase). The brush-wrapper's `___`-prefixed locals must not leak. Backlog L1-f: the phase env is filtered to Portage's `environ_whitelist` (`cargo test` always sets `CARGO_MANIFEST_DIR`, proving the filter).
- `merge_binpkg_filters_non_whitelisted_build_env_from_the_vdb_environment`: ports `config.environ()`'s `filter_calling_env` (L3 finding: the L2 set control leg leaked `PORTAGE_RUNNING_ROOT` / `L1_SKIP_PORTAGE_UPGRADE` / `GNUMAKEFLAGS`).
- `merge_binpkg_replace_runs_the_replaced_versions_pkg_prerm_and_pkg_postrm`: prerm+postrm run from 1.0's own vdb-stored environment.bz2, inside treewalk()'s replace loop.
- `download_and_verify_fetches_then_size_and_md5_checks`: committed fixture .tbz2 digests (`md5sum`/`sha1sum`, not invented); Portage `digestCheck` verifies every digest present, not just MD5.
- `refresh_binhost_indexes_warns_immediately_on_a_corrupt_packages_gz`: review fix (backlog #175) -- pre-fix the cache would hold the plain body (fallback tried); post-fix no cache file is left at all.
- `refresh_warns_with_the_untrimmed_sync_uri_but_fetches_stripped_urls`: review fix (backlog #175) -- unstripped doubleslash paths would 404 with a wget summary instead.
- `wget_http_error_recovers_urllibs_http_error_shape`: Portage `str(urllib.error.HTTPError(url, 500, "Internal Server Error", ...))` is `"HTTP Error 500: Internal Server Error"` (verified against the stdlib, not invented). wget's own `awaiting response... 500 Internal Server Error` line carries the server's status line verbatim, so the shaped detail is byte-identical -- the transcript is a live `wget` capture against the bed's own 500 stub shape (2026-09-27 capture bytes), not hand-written.
- `hide_binhost_passwd_masks_only_a_userinfo_password`: Portage `fetch.py::_hide_url_passwd` (`fetch.py:73-74`); `[^@\s]+` cannot span a space; a `/` inside the user is still masked: Portage's `[^:\s]+` allows it.
- `binhost_fetch_warning_matches_portages_two_line_shape`: Portage `bintree.py:1793-1798` for the bed's F3 stub (repo `l32-500`, every GET/HEAD 500).
- `refresh_binhost_indexes_warns_and_continues_on_a_500ing_binhost`: backlog #175 -- returns nothing now; stderr not capturable from a unit test, but the run log shows the pair and no wget transcript.
- `file_binhost_unwritable_cache_case` (backlog #192): no `create_dir_all` before fetching, in `--pretend` exactly like in a non-pretend merge; a `file://` binhost goes through the same cache machinery (Portage caches even local indexes).
- `refresh_pretend_failure_falls_back_to_the_stale_cache` (backlog #192): Portage `--pretend` keeps the stale copy with `[name] Local copy of unavailable remote index will be used due to --pretend` (`bintree.py:1801-1808`); a failed refresh never rewrites the cache.
- `refresh_real_merge_failure_drops_the_stale_cache` (backlog #192): `pkgindex = None` (`bintree.py:1809`).
- `refresh_within_ttl_uses_the_cache_without_fetching`: `DOWNLOAD_TIMESTAMP + TTL` freshness (`bintree.py:1535-1545`); zero expected connections (any fetch would hang the stub); a TTL hit rewrites nothing.
- `refresh_promotes_a_newer_remote_index_over_a_stale_cache`: `int(local) < int(remote)` (`bintree.py:1739-1744`); promotion stamps `DOWNLOAD_TIMESTAMP` like Portage (`bintree.py:1806`); gz + zst 404 then plain = 3 connections.
- `refresh_keeps_a_newer_cache_over_a_stale_remote_index`: the mirror arm, no rewrite (`bintree.py:1739-1744`).
- `refresh_frozen_repo_uses_the_cache_without_fetching`: `UseCachedCopyOfRemoteIndex("frozen")` (`bintree.py:1528-1533`).
- `timestamp_or_version_drop_case` (backlog #192 fix round): drops with `pkgindex = None`, so even a stale cache contributes nothing; a `file://` binhost serves `remote_body`; the seeded stale cache proves suppression, not resurrection; a dropped index never rewrites the cache.
- `run_merge_plan_*`: refresh tries Packages.gz + Packages.zst (both 404) before plain Packages, then the binpkg download = 4 connections; a failed refresh warns and resolves against the local pool (backlog #175); packagepkg-0.9 owns a soon-orphaned file; the mixed plan covers one `Binary` + one `Source` entry in one pass.
- `merge_one_binary_entry_never_refetches_for_a_non_remote_entry`: the `file://` binhost serves a genuine fixture binary, so pre-fix code downloads it into `$PKGDIR` and merges successfully -- post-fix the pkgdir stays empty and the error names the local dir. Never portuale's old `no binpkg file under ...` text; nothing may be downloaded into `$PKGDIR`.

## fn binrepo_download_dir
Backlog #428. Portage `BinRepoConfigLoader` gives a `location`-less binrepo `<EPREFIX>/var/cache/binhost/<name>` only when `PORTAGE_BINHOST` is unset (config.py:157-162); this port does the same, with `EPREFIX` = `PORTAGE_OVERRIDE_EPREFIX` (else empty, so `ROOT` plays no part; tests set the override to their root so they never write to the host) -- #436(b). Both `merge_one_binary_entry` and fetch-only use it, so the two paths agree. `Binpkg._verifier_exit` only renames `.partial` there, and no `Packages` index is written into a `location` other than `$PKGDIR` (b428 captures).

## fn verified_digest_names
Backlog #428. `bintree._get_digests` keys checksums uppercase and `size` lowercase; `BinpkgVerifier._display_success` prints `sorted(digests)`, so `MD5 SHA1 size`. Nothing is verified or printed without a parseable `SIZE` (`BinpkgVerifier._start`).

## fn merge_one_binary_entry (#459)
- A postinst-failed binary merge prints `>>> Completed` and returns the shared `postinst_failed_error` marker (like `merge_one_source_entry`), so the loop skips the world record, keeps the resume entry, and merges dependents in a resumed pass. `per_entry.log_file` is set only in background mode (hooks capture to the build log); in the foreground they inherit the terminal like the source serial path. Ground truth: `LLM/evidence/2026-10-21-f459/`.

## fn binrepo_gpkg_only
Backlog #511. Portage 3.0.83's per-binrepo `gpkg_only` (`bintree._populate_remote`, `7e2dcd0bd`): `binpkg-request-signature in FEATURES or repo.verify_signature`. The per-repo (not global-any) derivation is the 3.0.83 change; pre-3.0.83 it was global. An absent `verify-signature` key counts as true via the shipped global `[DEFAULT]` (see `binrepo.md`).

## fn apply_gpkg_only
Backlog #511. Runs after the exclude/include filter, like Portage's populate order (`_populate_remote_repo`: exclude/include, then the local-override `cpv_exists` skip, then `gpkg_only`). A `.gpkg.tar` `PATH` is kept; `.tbz2`/`.xpak` (`SUPPORTED_XPAK_EXTENSIONS`) dropped with the once-per-repo ignored warning (`78607215e` text, `remote_base_uri` = the index's `URI` header else the base URL, unmasked like Portage); any other (unknown or absent, read as `''`) `PATH` dropped with one `Unsupported binary package format from '…'` line each (Portage `get_binpkg_format(..., remote=True)` raising `InvalidBinaryPackageFormat`). Colour from Portage's `colorize("WARN", …)` is not modelled (plain like the other binhost warnings). Residue #537: Portage's `cpv_exists` skip precedes the check, so an entry an earlier (higher-priority, descending order) repo already provides is skipped silently — portuale filters per repo independently. Residue #538: the local `_populate_local` `gpkg_only` (from `FEATURES` only) is not modelled.

## fn refresh_binhost_indexes (processing order)
Backlog #511. The loop runs `binrepos.iter().rev()`: Portage processes `reversed(...)` over the ascending config (descending priority), so every per-repo message (fetch warnings, the exclude/include conflict warnings, `Local copy …` notes, `gpkg_only` warnings) prints highest-priority first. Resolution order is untouched (still ascending; see residue #537).

## tests (gpkg_only)
- `binrepo_gpkg_only_follows_request_signature_or_verify_signature`: the derivation truth table.
- `apply_gpkg_only_keeps_gpkg_warns_once_for_xpak_and_per_unknown_path`: exact warning strings, grounded on `LLM/evidence/2026-10-23-511/captures/01-sig-gpkg.portage.err`.
- `refresh_binhost_indexes_applies_gpkg_only_per_binrepo`: two `file://` repos end to end (verifying drops xpak, plain keeps, request-signature filters the plain one).
- The three `verify_signature: true` download/merge tests serve `.tbz2`, so they now declare `verify_signature = false` explicitly (Portage 3.0.83 would ignore those entries from a verifying repo); the `filecache`/`drop` cases assert pool mechanics, not signature policy, so they opt out the same way. The `Packages.gz` cache test serves a `PATH`-less entry and asserts the on-disk cache only (kept unfiltered, like Portage), so it is untouched.
