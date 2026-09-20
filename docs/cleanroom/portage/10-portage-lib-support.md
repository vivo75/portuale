# 10 — Portage library surface consumed by `emerge`

`bin/emerge` directly imports only `portage.elog.mod_echo`,
`portage.exception`, and `portage.util._eventloop.global_event_loop`.
The `_emerge` engine consumes much more. There is **no
`lib/portage/sets/`** — package sets live in **`lib/portage/_sets/`**.
No module carries a prose purpose header (only copyright + code), so the
purposes below are derived from leading classes/functions/docstrings.

## 10.1 Top-level `lib/portage/` modules

| File | Purpose |
|---|---|
| `__init__.py` | Bootstrap: lazy global proxies (`settings`, trees), re-exports of `versions`/`dep` helpers used by `_emerge`. |
| `_global_updates.py` | Apply `profiles/updates/` move/slot migrations to vardb (`_global_updates`, `grab_updates`). |
| `_legacy_globals.py` | Shim for removed legacy globals (`_get_legacy_global`). |
| `_selinux.py` | SELinux-aware `spawn` wrapper. |
| `binpkg.py` | Binary format probe: `get_binpkg_format()` (XPAK/GPKG), compression helpers. |
| `checksum.py` | Hash core (see §10.3). |
| `const.py` | Filesystem/constant table (`PORTAGE_BASE_PATH`, `VDB_PATH`, `PRIVATE_PATH`, user/group names). |
| `data.py` | Computed runtime globals (`secpass`, `EPREFIX`/`ROOT`, uid/gid). |
| `debug.py` | Tracing helpers (`trace_handler`, `prefix_trimmer`, `set_trace`). |
| `dispatch_conf.py` | `dispatch-conf`/`archive-conf` config-file merge logic. |
| `eapi.py` | EAPI predicates + `Eapi` comparable (see §10.3). |
| `eclass_cache.py` | Eclass inheritance/path cache; feeds the metadata phase. |
| `exception.py` | Taxonomy: `PortageException`, `PackageNotFound`, `AmbiguousPackageName`, `InvalidDependString`, `CorruptionKeyError`, … |
| `getbinpkg.py` | Remote `Packages` index I/O (see §10.3). |
| `glsa.py` | GLSA advisory parsing/matching. |
| `gpg.py` | GPG signing/verification for Manifests/binpkgs. |
| `gpkg.py` | GPKG (new binary) format: streaming tar reader/writer, checksum + GPG, safe extract. |
| `installation.py` | Install-prefix layout constants. |
| `localization.py` | i18n: `_()`, `localized_size()`. |
| `locks.py` | Cooperative file locking (see §10.3). |
| `mail.py` | Elog-to-mail delivery (lazy `email` import). |
| `manifest.py` | Manifest parse/create/verify/sign (see §10.3). |
| `metadata.py` | `action_metadata()` bulk regen driver. |
| `module.py` | `Modules` plug-in loader (elog/sync modules). |
| `news.py` | GLEP-42 `NewsManager`/`NewsItem` unread-news handling. |
| `output.py` | Terminal output: color maps, `EOutput` (einfo/ewarn/eerror), progress titles. |
| `process.py` | Process spawning core (see §10.3). |
| `progress.py` | `ProgressBar`/`ProgressHandler` for fetch/merge display. |
| `update.py` | Legacy `profiles/updates/` applier (`update_dbentries`, `fixdbentries`, `parse_updates`). |
| `versions.py` | Version grammar (see §10.3). |
| `xpak.py` | Legacy `.tbz2` XPAK segments (`xpak()`, `encodeint/decodeint`, `xsplit/xjoin`). |

Also emerge-relevant: `package/ebuild/` (phases: `doebuild.py`,
`config.py`, `fetch.py`, `digestcheck/gen.py`, `_config/` managers for
keywords/license/mask/USE), `sync/` (repo sync drivers), `proxy/` (lazy
proxies), `env/`, `xml/`.

## 10.2 Subdirectories

### `dbapi/` — the three database APIs + helpers

| File | Purpose |
|---|---|
| `__init__.py` | Abstract `dbapi`: `match`, `xmatch`, `aux_get`, `cp_list`, visibility/masking contract. |
| `vartree.py` | Installed DB (`/var/db/pkg`): `vardbapi`, `vartree`, `dblink` merge/unmerge engine (see §10.3). |
| `porttree.py` | Ebuild-repo DB: `portdbapi` (metadata cache + `aux_get`), `portagetree`, `FetchlistDict` (see §10.3). |
| `bintree.py` | Binary DB (`PKGDIR` + remote binrepos): `bindbapi`, `binarytree` incl. Packages-index populate (see §10.3). |
| `virtual.py` | `fakedbapi` overlay presenting virtuals/`package.provided` as installable. |
| `DummyTree.py` | Minimal stub tree for tests/fallbacks. |
| `IndexedPortdb.py` / `IndexedVardb.py` | In-memory `cp→cpv` index mixins for fast resolution. |
| `cpv_expand.py` | `cpv_expand()`: bare `pn`/`cp` → qualified `cpv` list. |
| `dep_expand.py` | `dep_expand()`: atom → matching `cpv` list. |
| `_expand_new_virt.py` | `expand_new_virt()`: `virtual/` → providers (no pre-2011 old-style virtuals). |
| `_similar_name_search.py` | Typo helper: near-miss `cp` suggestions. |
| `_MergeProcess.py` | `MergeProcess(ForkProcess)`: forked `dblink.merge()` worker. |
| `_SyncfsProcess.py` | `SyncfsProcess(ForkProcess)`: post-merge `syncfs()` flush. |
| `_ContentsCaseSensitivityManager.py` | `CONTENTS` collision tracking for case-folding filesystems. |

### `dep/` — atom grammar + USE-conditional evaluation

`__init__.py` (Atom, `use_reduce()`, `paren_reduce/enclose`,
`match_from_list`, REQUIRED_USE — see §10.3); `dep_check.py`
(`dep_check()`: full `DEPEND` satisfiability solver used by the
resolver); `_slot_operator.py` (`:slot=` rewriting); `_dnf.py`
(`dnf_convert()` → DNF normal form); `libc.py`
(`find_libc_deps()` synthesizes `sys-libs/glibc` deps); `soname/`
(`SonameAtom.py`, `parse.py`, `multilib_category.py` ABI→category map).

### `util/` (top-level files) and other dirs

`__init__.py` (`ensure_dirs`, `shlex_split`, `writemsg`, path/stack
helpers); `digraph.py` (directed graph for merge order);
`path.py`/`_path.py`, `listdir.py` (cached listing), `movefile.py`
(atomic rename + copy fallback), `file_copy.py` (reflink-aware copy),
`mtimedb.py` (`MTIMEDBKEYS`), `env_update.py` (`ld.so.conf` + `env.d`
regen), `install_mask.py` (`INSTALL_MASK` filtering), `hooks.py`
(`EBUILD_HOOK` runner), `changelog.py`, `cpuinfo.py`, `cgroup.py`
(cgroup-v2 accounting, `FEATURES=cgroup`), `compression_probe.py`,
`configparser.py`, `formatter.py`, `human_readable.py`
(`bytes_to_human()`), `lafilefixer.py`, `locale.py`, `netlink.py`,
`pickle.py` (`NoGlobalsUnpickler`), `portage_lru_cache.py`,
`shelve.py`, `socks5.py`, `time.py`, `whirlpool.py`,
`writeable_check.py`, `backoff.py` (`ExponentialBackoff`),
`bin_entry_point.py`, `ExtractKernelVersion.py`, `SlotObject.py`,
`_compare_files.py`, `_ctypes.py`, `_desktop_entry.py`,
`_get_vm_info.py`, `_info_files.py`, `_pty.py`, `_urlopen.py`,
`_xattr.py`. Subdirs: `_async/` (futures), `_eventloop/` (global
asyncio loop), `futures/`, `iterators/`, `elf/` (`header.py`,
`constants.py` ELF parser for `NEEDED`/`SONAME`), `endian/`,
`_dyn_libs/` (preserved-lib linker map).

`repository/`: `__init__.py` (`RepoConfig` handle), `config.py`
(`repos.conf` loader, `RepoConfigLoader`), `storage/
hardlink_quarantine.py`, `hardlink_rcu.py`, `inplace.py`,
`interface.py`. `_sets/`: `__init__.py` (`SETPREFIX` registry +
`load_default_config()` wiring world/system/security sets),
`base.py` (`PackageSet` base), `dbapi.py` (`DbapiChain`/owner sets),
`files.py` (file-backed sets incl. world file), `libs.py`
(preserved-library set), `profiles.py` (profile-derived sets),
`security.py` (GLSA set), `shell.py` (shell-output set),
`ProfilePackageSet.py` (`packages.build` specialization).
`cache/`: `__init__.py` (registry), `template.py` (`database` base),
`flat_hash.py`, `fs_template.py`, `anydbm.py`, `mappings.py`,
`metadata.py` (schema), `ebuild_xattr.py`, `sqlite.py`,
`sql_template.py`, `volatile.py`, `cache_errors.py`, `index/
pkg_desc_index.py + IndexStreamIterator.py`. `elog/`:
`__init__.py` (core + `elog_process()` dispatch), `messages.py`,
`filtering.py`, `mod_echo.py`, `mod_save.py`/`mod_save_summary.py`,
`mod_mail.py`/`mod_mail_summary.py`, `mod_syslog.py`,
`mod_custom.py`. `binrepo/`: `__init__.py`, `config.py`
(`binrepos.conf` loader).

## 10.3 Emerge-critical function inventories (behavioral)

### `dbapi/vartree.py` — installed DB + merge engine

- `class vardbapi(dbapi)` — `/var/db/pkg` reader/writer: reentrant
  `lock/unlock`, fs + slot locks (`_fs_lock`, `_slot_lock`),
  `_bump_mtime` invalidation, `cpv_exists/cpv_counter/cpv_inject/
  isInjected/move_ent`, `cp_list/cp_all/cpv_all`, caching `match()`,
  `findname()`, `aux_get()` (metadata + `_aux_env_search` inside
  `environment.bz2`), `aux_update()`, coroutines
  `unpack_metadata/unpack_contents`, `counter_tick*` monotonic COUNTER
  allocator, `_dblink()` factory, `removeFromContents/
  writeContentsToContentsFile`.
- `class vartree` — `vartdb` façade: `zap/inject`,
  `get_provide/get_all_provides`, `dep_bestmatch/dep_match`,
  `exists_specific/getallcpv/getallnodes/getebuildpath/getslot/populate`.
- `class dblink` (~62 methods) — per-CPV installer: `exists/delete/
  clearcontents/getcontents`, `quickpkg()` tarball builder,
  `unmerge()` incl. `_unmerge_pkgfiles/_unmerge_protected_symlinks/
  _unmerge_dirs`, `isowner/_match_contents`, preserved-libs
  (`_find_libs_to_preserve/_add_preserve_libs_to_contents/
  _find_unused_preserved_libs/_remove_preserved_libs`),
  `CONFIG_PROTECT` (`isprotected/updateprotect/_protect`),
  collision/security checks
  (`_collision_protect/_security_check/_lstat_inode_map`), `treewalk()`
  dispatcher, `_merge_contents/mergeme/merge()` livefs merger with
  `_new_backup_path/_pre_merge_backup/_post_merge_sync`, plus
  `getstring/copyfile/getfile/setfile/getelements/setelements/isregular`.
- Module `merge()`/`unmerge()`: fork-safe entries used by the
  scheduler; `write_contents()`/`tar_contents()` serialize CONTENTS
  with xattr support.

### `dbapi/porttree.py` — ebuild-repo DB

- `class portdbapi(dbapi)`: `_set_porttrees/_get_porttrees`, event-loop
  + pregen-cache init, `close_caches/flush_cache`,
  `findLicensePath/findname/findname2` (CPV → ebuild path + overlay),
  repo mapping (`getRepositoryPath/Name/Repositories`,
  `getMissingRepoNames/getIgnoredRepos`), cache write-back
  (`_write_cache/_pull_valid_cache`), `aux_get()`/`async_aux_get()` via
  `_run_metadata_phase`, `getFetchMap/async_fetch_map/getfetchsizes/
  fetch_check` (SRC_URI → filename + URI + restrict map),
  `cpv_exists/cp_all/cp_list`, `freeze/melt`, caching
  `xmatch/async_xmatch/match`, visibility pipeline
  `gvisible/visible/_iter_visible/_visible` (mask/keyword/profile).
- `class portagetree`: `dep_bestmatch/dep_match/exists_specific/
  getallnodes/getslot`. `class FetchlistDict(Mapping)`: lazy per-CPV
  fetch-map view. Helpers `_async_manifest_fetchlist()` (bounded-parallel
  Manifest fetch-list build), `_parse_uri_map()` (SRC_URI + `USE` +
  `RESTRICT` → fetch/restrict sets).

### `dbapi/bintree.py` — binary DB

- `class bindbapi(fakedbapi)`: `writable/match/cpv_exists/cpv_inject/
  cpv_remove`, `aux_get/aux_update`, coroutines
  `unpack_metadata/unpack_contents`, `cp_list/cp_all/cpv_all/
  getfetchsizes` (raises `MissingSignature` when SIZE unsigned).
- `class binarytree`: `populate()` = `_populate_local` (scan PKGDIR) +
  `_run_trust_helper` + `_populate_remote/_populate_remote_repo` (fetch
  remote `Packages` index) + `_populate_additional`;
  `inject/remove` with `_read_metadata/_inject_file/
  _inject_repo_revisions`; Packages-index maintenance
  (`_pkgindex_write/_pkgindex_entry/_new_pkgindex/
  _merge_pkgindex_header/_propagate_config/_update_pkgindex_header/
  _pkgindex_version_supported`); USE evaluation `_eval_use_flags`;
  instance disambiguation (`exists_specific/_is_specific_instance/
  _max_build_id/_allocate_filename{,_multi}/_parse_build_id/
  getname_build_id`); remote logic (`isremote/download_required/
  get_local_repo{,_location}/get_pkgindex_uri/gettbz2/_load_pkgindex/
  _get_digests/digestCheck/getslot`).

### `dep/__init__.py` — atoms + depstrings

- `paren_reduce()` string → nested lists; `paren_enclose()` inverse;
  `paren_normalize`; `_c_*` C tokenizer/normalizer/flattener.
- `use_reduce(depstr, uselist, masklist, matchall, excludeall,
  is_src_uri, eapi, opconvert, flat, …)`: strip false `use?()` branches;
  honor `||`/`^^`/`??` groups (EAPI-gated empty-group semantics);
  `dep_opconvert()`; `flatten()`.
- `class Atom`: exposes `category/package_name/cp/cpv/is_versioned/
  version/repo/slot/sub_slot/slot_operator/operator/blocker/eapi/
  build_id/use/soname`; `without_use/with_cp/with_repo/with_slot/
  without_{repo,slot}`; `intersects()`; `evaluate_conditionals()/
  violated_conditionals()`; `match(pkg)`; immutable copy semantics.
- `class _use_dep`: `foo[bar=,baz?]` sub-object.
- Accessors: `get_operator/dep_getcpv/dep_getslot/dep_getrepo/
  remove_slot/dep_getusedeps/dep_getkey/catsplit`-family;
  `isvalidatom()` (blocker/wildcard/repo/build-id gates per EAPI);
  `isjustname()/isspecific()`; `match_to_list/best_match_to_list/
  match_from_list` (best = highest per `vercmp`); `ExtendedAtomDict`,
  `extended_cp_match`.
- REQUIRED_USE: `check_required_use()`, `get_required_use_flags()`,
  `human_readable_required_use()`, `extract_affecting_use()`.

### `versions.py` / `eapi.py` / `locks.py` / `process.py` / `checksum.py` / `manifest.py` / `getbinpkg.py`

- `versions.py`: `ververify()`; `vercmp(v1,v2)` total order over
  numeric/letter/suffix (`_alpha/_beta/_pre/_rc/_p`) + revision
  (`-rN`) → −1/0/1; `pkgcmp()`; `best(matches)`; `_pkgsplit/
  cpkgsplit(pkgsplit/catpkgsplit/cpv_getkey/cpv_getversion)`;
  `catsplit()`; `cpv_sort_key(eapi)`; `class _pkg_str(str)` carrying
  `.cpv/.category/.package/.version/.rev/.slot/.repo/.stable`.
- `eapi.py`: ~37 `eapi_has_*/eapi_exports_*/eapi_supports_*/eapi_allows_*`
  predicates (IUSE defaults/effective, slot deps/operator, SRC_URI
  arrows, USE deps + defaults, strong blocks, `src_prepare/
  src_configure`, prefix, exported vars, `pkg_pretend`, implicit
  RDEPEND, REQUIRED_USE + at-most-one, POSIX locale, repo deps, BDEPEND/
  IDEPEND, empty-group truth, trailing-slash vars, BROOT/SYSROOT,
  symlink rewriting, profile eapi default); `class Eapi` comparable;
  `_get_eapi_attrs()` parses/validates (lenient when None).
- `locks.py`: `lockfile(path, wantnewlockfile, unlinkfile,
  waiting_msg, flags)` blocking fcntl lock → opaque tuple;
  `_lockfile_iteration()` (unlink-race detection via `_fstat_nlink`);
  `unlockfile()`; `lockdir/unlockdir`;
  `hardlink_lockfile/unhardlink_lockfile` (NFS-safe hardlink dance);
  `_get_lock_fn()` prefers `lockf` after validation else `flock`;
  `_close_fds()`; `class _lock_manager`.
- `process.py`: `spawn(cmd, env, fd_pipes, returnpid/returnproc,
  uid/gid/groups/umask/cwd/logfile, path_lookup, pre_exec,
  unshare_{net,ipc,mount,pid}, …)` with `_setup_pipes{,_after_fork}`,
  `_exec/_exec2/_exec_wrapper` (env-size guard, IPv6/loopback,
  `unshare` validation, signal forwarding), `find_binary()`;
  `spawn_bash/spawn_sandbox/spawn_fakeroot`;
  `Process/MultiprocessingProcess(AbstractProcess)`; `sanitize_fds()`;
  `atexit_register/run_exitfuncs/run_coroutine_exitfuncs`.
- `checksum.py`: `perform_checksum(file, hashname)` /
  `perform_multiple_checksums(file, hashes)` (serial vs parallel);
  `checksum_str(data, hashname)`; `perform_md5/perform_all`;
  `get_valid_checksum_keys()` (per-`PORTAGE_CHECKSUM_FILTER`);
  `get_hash_origin()`; `verify_all(file, dict, strict)`;
  `_open_file/_raise_checksum_oserror/_perform_checksums`.
- `manifest.py`: `class Manifest(pkgdir, distdir, fetchlist_dict,
  from_scratch, thin, allow_missing, …)`:
  `_readManifest/_parseManifestLines/_parseDigests` (Manifest2
  `TYPE FILE HASH… SIZE`), `_getDigestData/_createManifestEntries`,
  `create()` (thin per-CPV vs thick per-package),
  `addFile/removeFile/hasFile/findFile`,
  `updateAll{File,Type,}Hashes/updateCpvHashes/updateHashesGuessType`,
  `checkAllHashes/checkTypeHashes/checkFileHashes/checkCpvHashes`
  (+ `_getCpvDistfiles/getDistfilesSize/getFileData/getVersions`),
  `checkIntegrity()`, `write(sign, force)` + `sign/validateSignature`
  (GPG), `_apply_max_mtime`, `_getAbsname/getFullname/getDigests/
  getTypeDigests`; helpers `manifest2AuxfileFilter/
  manifest2MiscfileFilter/guessManifestFileType/
  guessThinManifestFileType`, `FileNotInManifestException`.
- `getbinpkg.py`: `file_get(baseurl, dest, conn, fcmd, filename,
  fcmd_vars)` (resume/redirect/auth); `_cmp_cpv()`;
  `class PackageIndex`: `read/readHeader/readBody` (header dict +
  per-CPV entries with key translation/inheritance),
  `_readpkgindex/_writepkgindex/write`.
