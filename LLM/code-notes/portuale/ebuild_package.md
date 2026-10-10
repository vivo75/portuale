# crates/portuale/src/ebuild_package.rs
Binary-package building (`ebuild package`, `--buildpkg`, quickpkg). Context moved out of the code comments (#336 Phase 7); the code keeps only the short why.

## module
- Ports `doebuild()`'s `"package"` action (`actionmap_deps["package"] == ["install"]`, lib/portage/package/ebuild/doebuild.py): runs the `install` chain, then the unmodified `bin/misc-functions.sh __dyn_package` (a *separate* script invocation, not a `bin/ebuild.sh` phase; see `ebuild_phases::run_misc_functions`). The xpak branch shells out to the native `xpak recompose` helper (#326 S5, `helpers/xpak.rs`, via `bin/portuale-python`); the gpkg branch to the native `gpkg compress` helper (#326 S4, `helpers/gpkg.rs`, a field-for-field port of `portage.gpkg.gpkg().compress()`).
- Origin: task #54's sibling to `merge`; the reader side is task #53/#63 (`portage_repo`'s binary-package reader, which never parses a `.tbz2`/XPAK file's content, only the plain-text `<PKGDIR>/Packages` index).
- Gaps, all v1 scope ("narrow v1, document the cut"): no `packdebug`/`splitdebug` handling, no RPM (`__dyn_rpm`) format. Portage's `EbuildBinpkg.bindbapi.inject()` (in-memory binary-package database update, distinct from the on-disk `Packages` write) has no equivalent: portuale has no long-lived `bindbapi` process, only re-reads `Packages` fresh each invocation.
- `USE` in the Packages index entry is `package_after_install`'s `use_flags` parameter: an `emerge <atom> -b` build's resolved flags (the same `USE` `MergeOptions::build_env`/`entry_build_env` already resolved for the `install` phase) are written verbatim, matching Portage's `Package.use.enabled`. Still `""` for standalone `ebuild <file> package`/`merge`: no resolved graph reaches that deep.
- `SLOT`/`KEYWORDS`/`IUSE`/`LICENSE`/`PROPERTIES`/`RESTRICT`/the `*DEPEND` family in the index entry come from the ebuild's repo's `metadata/md5-cache` (via `portage_repo::read_md5_cache`, the same source `emerge --pretend` trusts) when the containing repo is found by walking up for `profiles/repo_name`; empty throughout for a standalone ebuild outside any repo. The archive's own appended XPAK metadata gets the same keys independently from `build-info` (`bin/phase-functions.sh` + `ebuild_phases::write_post_install_metadata`) -- for a USE-conditional dep string the two can differ (index is flat md5-cache, XPAK is `use_reduce`'d against the empty phase-side USE set); `--pretend` reads the index, so this is cosmetic. Reading the index entry from `build-info` too, for a single source of truth, is a documented follow-up.
- `PKGDIR`-index locking matches Portage's `bintree.inject`/`update_pkgindex`/`remove` (bintree.py:948/:1999/:2059) via the same `flock(2)`-based `PortageLockfile` that `fetch.rs`'s distlocks use.
- Config layering (backlog #180): `PKGDIR`/`BINPKG_COMPRESS`/`BINPKG_COMPRESS_FLAGS[_<NAME>]`/`PORTAGE_BZIP2_COMMAND` follow Portage's `config` precedence -- calling env over `make.conf`/profile/`make.globals` via `portage_profile::env_over_config_scalar` -- falling back to `make.globals` defaults (`PKGDIR="/var/cache/binpkgs"` :31, `BINPKG_COMPRESS="zstd"` :39, `PORTAGE_BZIP2_COMMAND="bzip2"` :105, empty flags). Exception: the gpkg compression pair, which Portage's `environ_filter` keeps out of the helper's rebuilt settings, so only a config-file value reaches it (the Q6 rule, pmtest `bf0692f`: calling-env `BINPKG_COMPRESS`/`FLAGS` win for `xpak`, never for `gpkg`; env-only `gzip` still compresses `zst`, `make.conf` `gzip` compresses `.gz`).
- "Real's" in the old docs meant Portage throughout.

## fn is_package_command
- `ebuild.rs` checks this alongside `ebuild_phases::is_phase_command` / `ebuild_merge::is_merge_command` / `ebuild_unmerge::is_unmerge_command` before routing to execution.

## struct PackageOptions
- Bundled to avoid positional-parameter pain (the lesson `ebuild_merge::MergeOptions` already applied).
- `pkgdir` chain-resolved at both construction sites (`ebuild.rs` standalone `package`, `pretend.rs` `package_options_from_env`).
- `binpkg_format`: Portage's default is `gpkg` (`cnf/make.globals:43`); `xpak` is only the first entry of `SUPPORTED_GENTOO_BINPKG_FORMATS` (`const.py:296`), not the shipped default. Backlog #173: both construction sites used to read only the process env over a hardcoded `"xpak"`, ignoring `make.conf`/profile and naming the wrong default.
- `binpkg_multi_instance`: Portage's default is `true` (`bintree.py:526-531`, also a `make.globals` default `FEATURES` token); `false` here is deliberate conservatism, not a capability gap. `binpkg::populate_local_pkgdir` walks the subdir layout back on the no-index scan.
- `binpkg_signing`: Portage's `gpkg.gpkg.create_signature` (`gpkg.py:790`) via `checksum_helper(SIGNING)` spawning `BINPKG_GPG_SIGNING_BASE_COMMAND` with Portage's own `[PORTAGE_CONFIG]` substitution; configured via the `BINPKG_GPG_SIGNING_*` env passthrough with `_emerge/actions.py`'s `!!! {var} is not set` pre-check. Sourced from `FEATURES` at the CLI boundary. Portage's `binpkg-request-signature` ignores xpak.
- `binpkg_gpg_signing_base_command`: Portage's `make.globals` default is `"/usr/bin/flock /run/lock/portage-binpkg-gpg.lock /usr/bin/gpg --sign --armor [PORTAGE_CONFIG]"`; exporting `""` would override the default through the env layer, hence the non-empty gate.
- `binpkg_gpg_signing_digest`: Portage default `"SHA512"`.
- `buildpkg_live`: Portage default `true` (a `make.globals` default `FEATURES` token); explicit `FEATURES=-buildpkg-live` is the only way to skip packaging a live build (Portage's `binpkg_wanted`, `_emerge/Package.py:621-637`).

## fn PackageOptions::set_resolved_features
- Backlog #130, narrowed by #147 S1 ruling (i). Mirrors `ebuild_merge::MergeOptions::set_resolved_features`: the `emerge` paths call this right after resolving a per-entry `FEATURES` fold, so a call site sharing one `PackageOptions` across a run can clone and re-derive per entry. Unlike the raw-process-env default-on-unless-negated fallback, a negated token was already folded away by `regenerate()`/`fold_package_env_incremental`.
- `binpkg_multi_instance` keeps its construction value because Portage's binpkg layout is run-wide-only (the bintree binds its allocator once, `bintree.py:529-531` -- S0 A1x/A2 prove both directions); `package_after_install` takes the per-entry token separately for the `BUILD_ID` export gate (Portage's `EbuildBinpkg.py:47-48`). `binpkg_compress`/`binpkg_format`/GPG fields are config scalars, untouched.

## fn compress_template
- Six compressors only. `{JOBS}` is a plain pre-`varexpand` substitution (Portage's `doebuild.py:721-724`/`:740-743`).

## fn find_binary
- Ports `find_binary` (lib/portage/process.py).

## fn resolve_compression_command
- Ports `PORTAGE_COMPRESSION_COMMAND` resolution (`doebuild.py:697-750`). `{JOBS}` is the host CPU count, matching `makeopts_to_job_count`'s `get_cpu_count()` fallback -- portuale's `MAKEOPTS` is always unset, the path Portage takes whenever `MAKEOPTS` lacks `-j`/`--jobs=`. The `${VAR}` substitution is deliberately narrow (no full shell `varexpand`: none of the six templates or realistic flag values need more); whitespace-splitting is a deliberate narrowing of `shlex.split` (no quoting need). An unknown name or missing binary leaves `mysettings["PORTAGE_COMPRESSION_COMMAND"]` unset in Portage too (only warned about via `writemsg`, not reproduced -- this module's execution path prints nothing anywhere else), so unmodified `bin/misc-functions.sh` hits its own `[[ -z "${PORTAGE_COMPRESSION_COMMAND}" ]] && die "PORTAGE_COMPRESSION_COMMAND is unset"` guard naturally.

## fn makeopts_to_job_count
- Ports `makeopts_to_job_count` (`util/cpuinfo.py:55-70`): the greedy `.*(j|--jobs=\s)\s*([0-9]+)` match, else `get_cpu_count()` (`sched_getaffinity`).

## fn phase_compression_command
- Set for every build phase regardless of `BINPKG_FORMAT`, from the resolved config.

## fn resolve_binpkg_format
- Same `lookup` shape as `phase_compression_command`; chain value wins verbatim. A `make.conf` assignment overrides `make.globals` with `""` and `__dyn_package` then dies the same way.

## fn resolve_pkgdir
- Backlog #180, same two construction sites. `PKGDIR` is in Portage's `environ_whitelist` (`_config/special_env_vars.py:125`) and not in `environ_filter`, so the calling-env value reaches every consumer -- `bintree.pkgdir` (full config, env layer highest) for the tmpfile path and the phase env for `${PKGDIR}` -- for both formats alike.

## fn resolve_binpkg_compress
- Backlog #180. For `xpak` the full chain (Portage's `doebuild.py:697` reads the package's own `mysettings`, env layer highest, for the tar pipe); for `gpkg` the files-only chain (Portage's `environ_filter`, `_config/special_env_vars.py:280`, keeps calling-env `BINPKG_COMPRESS` out of the phase env, and `bin/gpkg-helper.py` rebuilds `portage.settings` from that filtered env plus config files). Empty-`BINPKG_COMPRESS`-disables-compression is Portage's `doebuild.py:706-710` / `bin/quickpkg:161-167` arm.

## fn resolve_binpkg_compress_flags
- Override-then-generic order, the same both readers use (`doebuild.py:710-717`, `gpkg.py:1876-1882`). Generic `BINPKG_COMPRESS_FLAGS` is `environ_filter`ed alongside `BINPKG_COMPRESS`, and `BINPKG_COMPRESS_FLAGS_<NAME>` never reaches the phase env (not in `environ_whitelist`; `config.environ()`'s `filter_calling_env`, `config.py:3275-3305`).

## fn resolve_portage_bzip2_command
- Backlog #180. In Portage's `environ_whitelist` (`_config/special_env_vars.py:143`), not in `environ_filter`, so it propagates into the phase env and reaches even the gpkg helper's rebuilt settings.

## fn refresh_entry_compression_command
- Backlog #147 S2. The command is what `__dyn_package` reads -- via the saved `${T}/environment` `bin/ebuild.sh:565-580` sources for the `package` phase -- not `PackageOptions::binpkg_compress`, which stays run-wide on purpose: the gpkg helper reads the global settings (`bin/gpkg-helper.py:49`, `gpkg.py:785`), so a per-entry value must never reach it (S0 B-gpkg: zst members; S0 B-xpak: a per-package `BINPKG_COMPRESS=gzip` reaches the xpak tar pipe, `bin/misc-functions.sh:593-596`, even when run-wide is `zstd`).
- The layered pairs already carry the matched `package.env` scalars appended after the run-wide base (`matched_package_env_vars`), and the calling env is already folded into the run-wide pairs upstream -- no separate process lookup, also unsound under `cargo test`'s shared process env. The config tail (`config.other_vars`) keeps a `make.conf` `BINPKG_COMPRESS` on the pipe: the run-wide base carries no pair for it (the key is `environ_filter`ed out of `phase_environ`), so without the tail the recompute would fall back to Portage's own `.get` default (`bzip2`) and clobber the `make.conf` command. Order mirrors Portage's `env:pkg:conf` `USE_ORDER`: a calling-env value already masked any same-key `package.env` match upstream (#101), so pairs outrank raw env, both outrank files.

## fn packages_index_header
- Portage's reader is `bintree._load_pkgindex` via `_pkgindex_version_supported` (`bintree.py:2429-2437`): `False` unless the header carries `VERSION` <= `bintree._pkgindex_version`. Portage's writer always stamps `VERSION: <n>` (`bintree.py:2388`/`:2397`); a `TIMESTAMP`-only header is not a valid index to Portage. Found as L2 S0 `l2-pkgindex-version-missing`: without it, under `pkgdir-index-trusted` Portage never walks the directory to rebuild, every portuale-built archive is invisible, and `emerge -k` silently falls back to a from-source build. Portuale's own reader (`portage_repo::read_packages_index`) unconditionally skips the first blank-line-terminated block.

## fn omit_stanza_default
- Portage restores defaults on read (`readBody`, `getbinpkg.py:143-145`, `d.setdefault(k, v)`). S0 probe (backlog #188, Portage in `localhost/test-portuale:latest`): `l32/dep-a` (`SLOT="0"`) has no `SLOT` line, `l32/slotpkg` (`SLOT="1"`) has `SLOT: 1`.

## fn sort_stanza_fields_for_write
- Full mechanics: `keys.sort()` over the *internal* names (`getbinpkg.py:153-177`), `_write_translation_map` on the way out (`_writepkgindex`, `getbinpkg.py:123-126`). Of the translated keys (`_pkgindex_translated_keys`, `bintree.py:638-642`) either writer emits only `_mtime_` -> `MTIME` and `repository` -> `REPO` (`DESCRIPTION` -> `DESC` never reaches a stanza); both internals sort after every all-uppercase key (`_` is 0x5F, `r` is 0x72). The `--buildpkg` assembly is already hand-sorted (no-op guard against future drift); the quickpkg assembly relies on the sort (backlog #203: its fixed order was Portage-divergent).

## struct BuiltBinpkgStanza
- `REPO_REVISIONS`: `"{}"` for sync-less repos up to sync-revision JSON, written verbatim when non-empty.

## fn built_binpkg_stanza_fields
- `REPO_REVISIONS` sorts between `RDEPEND` and `REQUIRES` (`D` < `E`, `P` < `Q`).

## fn write_packages_index_entry
- Index format per `portage_repo::read_packages_index`: `KEY: value` lines, blank-line-separated blocks, first block a header.
- Lock order is Portage's: `os.makedirs(self.pkgdir, exist_ok=True)` *then* acquire (`bintree.py:2057-2059`).
- Dedup matches `_inject_file` (`bintree.py:2237-2247`): primarily by `PATH` (a different `PATH` -- e.g. a different `BUILD_ID` under multi-instance -- is genuinely different and survives, letting multiple builds of the same `CPV` coexist), falling back to `CPV` alone only with no `PATH`.
- `_lock` drops at the end: the same effect as Portage's `unlockfile()`.

## fn require_signing_config
- Ports `doebuild()`'s first step for `"package"` always running the `install` chain first (`actionmap_deps["package"] == ["install"]`); run here directly, the "run it myself" shape `ebuild_merge::run_merge` established for its own `["install"]` prerequisite. Shared gate for `run_package` (Portage refuses before building anything) and `package_after_install` (the `FEATURES=buildpkg` side-effect path).

## fn run_package
- `build_env`: `--buildpkgonly` passes the full `phase_environ`-derived env plus the entry's own `USE` (#37 S2); standalone passes `&[]` (no graph, no resolved config -- `phase_env_vars`' curated base stands).
- The per-entry multi-instance token comes from the `--buildpkgonly` per-entry fold when one matched, the run-wide value otherwise; standalone falls back to construction, preserving standalone behavior (Portage's `doebuild` package branch has no per-package `BUILD_ID` logic).

## fn package_after_install
- Backlog #147 S1 ruling (i): layout run-wide (Portage's once-bound bintree allocator), `BUILD_ID` export + index entry per-entry (Portage's `EbuildBinpkg.py:47-48`). Standalone callers pass `false`. Run-wide single + per-entry token writes no `BUILD_ID` anywhere, where Portage writes index `None` and vdb `-1` (S1 D1): a documented `str(None)`-wart divergence, not mirrored.
- `EbuildBinpkg._start`'s per-package gate (`EbuildBinpkg.py:47-48`): a negated entry in a run-wide-multi run takes the multi path with no `BUILD_ID` (S0 A2 shape); a tokened entry in a run-wide-single run takes the single path with none either.
- `_pkgindex_entry` + `PackageIndex.write` (`bintree.py:2289-2312`, `getbinpkg.py:155-175`); digests are Portage's fixed `_pkgindex_hashes = ["MD5", "SHA1"]` (`bintree.py:548`) via `perform_multiple_checksums(pkg_path, hashes=...)` (`bintree.py:2302`), taking no configuration -- S0 probe (backlog #188) confirmed a `PORTAGE_CHECKSUM_FILTER='-SHA1'` rebuild still writes both. Portage hashes streaming, once per checksum; portuale's whole-file read matches the pre-existing `MD5`-only shape.
- `PATH` always written (`bintree.py:2311`), not gpkg-only; remote fetch falls back to a bare `<pf>.<ext>` guess without it (Portage's `gettbz2`: `if not rel_url: rel_url = pkgname + ".tbz2"`). Multi-instance form is `_allocate_filename_multi`'s `os.path.join(pkgdir, cpv.cp, pf)`.
- `BINPKGMD5`/`BUILD_ID` recording ports `EbuildBuild._record_binpkg_info` (`EbuildBuild.py:502-523`, `--buildpkg`/`--buildpkgonly`) and `doebuild`'s `package` branch (`doebuild.py:1522-1544`): `BINPKGMD5` (the index `MD5`) always, `BUILD_ID` when carried -- overwriting `__dyn_package`'s own newline-less `BUILD_ID` (`misc-functions.sh:560-561`).
- `build-info` first because `_pkgindex_entry` reads `cpv._metadata` (the *archive's* metadata): `RDEPEND` with its libc injection, `PROVIDES`/`REQUIRES` from the ELF scan, `IUSE`/`IUSE_EFFECTIVE`, `SIZE` (#39).

## fn binpkg_extension
- Ports `__dyn_package`'s `die "Unknown BINPKG_FORMAT ${BINPKG_FORMAT}"`.

## fn multi_instance_binpkg_suffix
- Ports `_allocate_filename_multi` (`bintree.py:2620-2626`).

## fn allocate_binpkg_build_id
- Ports `_allocate_filename_multi`'s search (`bintree.py:2607-2669`): Portage's `_max_build_id` equivalent; the `while True` "avoid races" retry loop narrowed to existence checks (no `open(..., "x")` placeholder dance for a single-process CLI that never races itself).

## fn binpkg_checksums
- One import covers both hashers (`digest::Digest` re-exported by both crates).

## fn package_makeopts
- #326 S4 (`{JOBS}` for the native gpkg compressor); #328 (global fallback).

## fn invoke_dyn_package
- Ports `EbuildBinpkg._start`'s condition verbatim: `if "binpkg-multi-instance" in self.settings.features: self.settings["BUILD_ID"] = str(build_id)` (`EbuildBinpkg.py:47-48`). Previously only the index entry carried `BUILD_ID`, so `binpkg::populate_local_pkgdir`'s filename-inference fallback was silently doing the archive's job; exporting it also gives `__generate_packdebug` the debug tarball name it needs under `FEATURES=packdebug` (`bin/misc-functions.sh:557-558`).
- `PORTAGE_PYTHONPATH` over `PORTAGE_PYM_PATH` is `bin/misc-functions.sh`'s own `PYTHONPATH=${PORTAGE_PYTHONPATH:-${PORTAGE_PYM_PATH}}`.
- gpkg compressor env: `gpkg._get_binary_cmd` reads `BINPKG_COMPRESS`, `BINPKG_COMPRESS_FLAGS[_<NAME>]`, `${PORTAGE_BZIP2_COMMAND}` via `varexpand` from the helper's rebuilt settings -- by construction identical to the helper's own files-only rebuild (the `BINPKG_COMPRESS`/`FLAGS` values here are files-only for `gpkg` per the Q6 rule; `PORTAGE_BZIP2_COMMAND` keeps the calling env, whitelisted, never filtered).
- `MAKEOPTS` export: `gpkg._get_binary_cmd` is `makeopts_to_job_count(settings.get("MAKEOPTS", "1"))` from the helper's `portage.settings` (`make.conf` included). #328: the ebuild-path chain needs a repo above the ebuild, but `quickpkg_from_vdb` packages a scratch copy of the vdb ebuild with none; `MAKEOPTS` is global so it comes from `resolve_unmerge_config`, as `BINPKG_FORMAT` does on that path (#173). Portage reads `settings["MAKEOPTS"]` there too and logs `zstd -T5` for `make.conf` `MAKEOPTS=-j5` (pmtest `differential-test-bed/scripts/328-quickpkg-makeopts.sh`, Portage 3.0.82.2).
- Signing export ports `gpkg.gpkg.create_signature` (`gpkg.py:790`) via `checksum_helper(SIGNING)` spawning `BINPKG_GPG_SIGNING_BASE_COMMAND` with Portage's `[PORTAGE_CONFIG]` substitution; `FEATURES` flows through `phase_env_vars`.

## fn quickpkg_from_vdb
- Ports `dblink.quickpkg` + `_quickpkg_dblink` (`vartree.py:2307` / `:6296`): byte-shape identical to Portage's own `tar_contents(...)` + `xpak.tbz2(...).recompose_mem(xpak.xpak(dbdir))` (`bin/quickpkg:147` packs the whole vdb dir via `xpak.xpak(dblnk.dbdir)`).
- Entry files read through the installed-database backend since feat#157 S1.6 (`files`: one `open` each, as before); database backends (S4.1) get a scratch materialized copy.
- No `BUILD_TIME` idempotency check (backlog #202): Portage's check iterates `bintree.dbapi.match("=cpv")` (`vartree.py:6297-6313`), but in every removal flow that reaches it the bintree was never populated -- `binarytree.__init__` only records `populated = 0`, and `run_action` populates it solely for `action in ("search", None)` with `--usepkg` (`actions.py:3725-3765`; the rest serve the depgraph merge flow with remote binpkgs). An empty dbapi matches nothing, so Portage always shells out to `bin/quickpkg` and rebuilds -- while consulting the raw `<pkgdir>/Packages` file here skipped the backup whenever a stale stanza (e.g. an unlinked first backup) shared the re-merged package's second-granularity `BUILD_TIME`, silently producing no second backup with rc 0.
- `Ok(None)` when the archive path exists is portuale's own conservative cut (`bin/quickpkg` overwrites). fifo/device skip is the same `CAP_MKNOD` limitation as the merge side's `create_special_node`.
- Multi-instance path is Portage's `bin/quickpkg` -> `bintree.inject` -> `getname(..., allocate_new=True)`.
- Stanza assembly: the same archive-metadata source `_pkgindex_entry` (`bintree.py:2289-2313`) reads; `EAPI` verbatim unless `"0"` (the vdb `EAPI` file always exists); `REPO_REVISIONS` verbatim when carried (Portage writes `build-info/REPO_REVISIONS` whenever `PORTAGE_REPO_REVISIONS` is non-empty, `phase-functions.sh:769`, so even sync-less repos record `"{}"`); `repository` verbatim as `REPO` (`repository` is in Portage's stanza aux-key set, `bindbapi._aux_cache_keys`, `bintree.py:96-124`, emitted translated last after `MTIME`); no `PF`/`CATEGORY` lines (neither is in the aux-key set -- the n203 probe's Portage quickpkg stanza carries neither although the vdb has both; backlog #188's "extras match real" claim was wrong). `REPO` is never inherited from the header in the shapes portuale writes: Portage drops a stanza value equal to a non-empty header value (`_pkgindex_inherited_keys`, `bintree.py:630`, `getbinpkg.py:164-168`), but the header default for `repository` is `""` (`bintree.py:633-636`), `_update_pkgindex_header` never sets it (only `_pkgindex_header_keys`, which excludes it), and portuale's header carries only `TIMESTAMP`+`VERSION` -- so the guard's `if v` is always falsy and `REPO` is always written, like Portage's fresh-index shape (the n203 probe ends `..., MTIME, REPO: l32`).
- Temp-file-then-rename: Portage's `bin/quickpkg` does it too (`binpkg_tmpfile = bintree.pkgdir + cpv + ".tbz2." + str(portage.getpid())`, later `bintree.inject` renamed).

## fn copy_dir_recursive
- Stages the vdb dir as `${PORTAGE_BUILDDIR}/build-info` for `quickpkg_from_vdb`; files, symlinks-as-symlinks, subdirs.

## tests: package_makeopts_falls_back_to_the_global_chain_outside_a_repo
- #328. The scratch layout `quickpkg_from_vdb` uses has no repo ancestor, so the ebuild-path chain resolves nothing; without the global fallback the native gpkg compressor counted CPUs (`zstd -T<ncpu>`) where Portage runs `zstd -T5`.

## tests: set_resolved_features_rederives_the_binpkg_affecting_tokens
- Backlog #130. A folded list that lost a token turns it back off: full re-derivation, not additive merge. Config scalars untouched.

## tests: resolve_compression_command_substitutes_jobs_for_xz_and_zstd
- `{JOBS}` is the host CPU count, not pinned.

## tests: phase_compression_command_reads_the_resolved_config
- The L2 oracle's `zstd -T{JOBS}` + `MAKEOPTS=-j1` is `zstd -T1` the same way.

## tests: resolve_binpkg_format_defaults_to_gpkg_and_obeys_the_chain
- Backlog #173; `BINPKG_FORMAT="gpkg"` is `cnf/make.globals:43`. Chain precedence (`portage_profile::env_over_config_scalar` over the resolved config) lives in each call site's `lookup`.

## tests: resolve_pkgdir_compress_flags_and_bzip2_obey_the_chain
- Backlog #180; `PKGDIR="/var/cache/binpkgs"` is `:31`, `BINPKG_COMPRESS="zstd"` is `:39`, `PORTAGE_BZIP2_COMMAND="bzip2"` is `:105`, no `FLAGS` default anywhere. The gpkg compression pair uses files-only `other_vars` per the Q6 rule. Empty `BINPKG_COMPRESS` disables compression downstream (Portage's `doebuild.py:706-710`), so it round-trips verbatim.

## tests: refresh_entry_compression_command_rederives_the_command_from_the_layered_env
- Backlog #180 (the `make.conf` tail case). The per-compressor override order is Portage's `doebuild.py:710-717` / `gpkg.py:1876-1882`.

## tests: write_packages_index_entry_creates_a_header_then_the_entry
- Portage's `_pkgindex_version_supported` gate (L2 S0 `l2-pkgindex-version-missing`).

## tests: probe_shaped_stanza
- The S0 probe's `l32/dep-a` stanza shape (Portage in `localhost/test-portuale:latest`, default config). Digests are Portage's fixed `_pkgindex_hashes` pair (`bintree.py:548`), written even with `PORTAGE_CHECKSUM_FILTER='-SHA1'`. `REPO_REVISIONS: {}` is Portage's sync-less value: the overlay repo has no sync type, so `_setup_repo_revisions` records an empty dict, which `phase-functions.sh:769` still writes (the `"{}"` string is non-empty), and the stanza carries it verbatim (no `_pkgindex_default_pkg_data` entry, only the falsy guard).

## tests: quickpkg_from_vdb_writes_portage_ordered_stanza_with_eapi_and_repo_revisions
- Backlog #203 (#226 for the `REPO` part); n203 container probe (`quickpkg =l32/dep-a-1.0`, `/tmp/opencode/n203/probe.log`): `EAPI: 8` verbatim (omitted iff `"0"` like `--buildpkg`); `REPO_REVISIONS` `{}` (merged from a sync-less build).

## tests: real_package_builds_a_real_xpak_tbz2_and_a_real_packages_entry
- Xpak building is codec-agnostic (the `portage_repo` reader never parses a `.tbz2`'s content, only `Packages`); "bzip2" pinned because it is a near-universal base package, "xpak" pinned per backlog #173. Test fn name and `XPAKPACK` assert message keep "real" as code tokens.

## tests: package_after_install_stanza_omits_default_slot_and_writes_both_digests
- Backlog #188; S0 Portage probe, default config, `SLOT="0"` fixture `dev-libs/packagepkg` (same shape as the probe's `l32/dep-a`): no `SLOT` line (`_pkgindex_default_pkg_data`, `bintree.py:609-629`, dropped by `PackageIndex.write`, `getbinpkg.py:169-172`), both fixed digests (`_pkgindex_hashes`, `bintree.py:548`), translated `MTIME` (never internal `_mtime_`).

## tests: quickpkg_from_vdb_with_gpkg_format_builds_a_readable_gpkg
- Re-run as a child process with no checkout by `helpers::gpkg::tests::quickpkg_from_vdb_gpkg_works_without_the_checkout`.

## tests: real_multi_instance_xpak_uses_the_dot_xpak_extension_and_scans_back
- Portage's `bin/misc-functions.sh` builds `[image][XPAK]` regardless of `BUILD_ID`.

## tests: real_package_with_binpkg_multi_instance_writes_the_cat_pn_subdir_layout
- Backlog #174: vouched files are trusted on the index's word (like Portage's default `FEATURES=pkgdir-index-trusted`), so scanned entries are the written stanzas verbatim; Portage's `_pkgindex_aux_keys` carries no `PF`, so `PF` is no longer asserted (downstream splits it from `CPV`, exactly like Portage).

## tests: package_records_binpkgmd5_and_build_id_in_build_info_like_portage
- Ports Portage's `EbuildBuild._record_binpkg_info` (`EbuildBuild.py:502-523`) and `doebuild`'s `package` branch (`doebuild.py:1522-1544`).

## tests: a_failed_packaging_phase_never_touches_the_final_binpkg_path
- Ports `EbuildBinpkg`'s temp-file-then-rename (`_emerge/EbuildBinpkg.py:37-67`): `PORTAGE_BINPKG_TMPFILE` is a same-directory `NamedTemporaryFile` with a PID suffix; `_package_phase_exit` unlinks it on non-`EX_OK`. The xpak tar pipe's `[[ -z "${PORTAGE_COMPRESSION_COMMAND}" ]]` guard (backlog #173's deterministic die) fires before the tar pipe ever opens.
