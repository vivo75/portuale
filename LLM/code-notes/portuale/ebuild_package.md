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
- For `"gpkg"`, `bin/misc-functions.sh __dyn_package` shells out to the native `gpkg compress` helper exactly the way the `"xpak"` branch reaches the native `xpak recompose`, producing a genuine `${PKGDIR}/${CATEGORY}/${PF}.gpkg.tar` portuale's own `binpkg::read_gpkg_metadata` reader round-trips. Anything other than those two values is `Err("Unknown BINPKG_FORMAT ...")`, `__dyn_package`'s own `die`.
- `FEATURES=binpkg-signing` is implemented for gpkg (`PackageOptions::binpkg_signing`'s own doc comment): the unmodified helper signs the members (detached `.sig` sidecars) and clear-signs the `Manifest`, configured via the `BINPKG_GPG_SIGNING_*` env passthrough, with `_emerge/actions.py`'s own `!!! {var} is not set` pre-check.
- `FEATURES=binpkg-multi-instance` is implemented for both formats, opt-in (`PackageOptions::binpkg_multi_instance`'s own doc comment) -- the `<cat>/<pn>/<pf>-<build_id>.<suffix>` subdir layout Portage `_allocate_filename_multi` writes, `.xpak` suffix for xpak.
- `PORTAGE_COMPRESSION_COMMAND` resolution (Portage `_compressors`/`BINPKG_COMPRESS_FLAGS_*`, `doebuild.py:697-750`) is implemented -- see `resolve_compression_command`'s own doc comment for the exact mechanics and v1 narrowing (no full shell `varexpand`, host CPU count for `{JOBS}`).
- `PKGDIR`-index locking is implemented (`write_packages_index_entry`'s own doc comment), wrapping the whole read-modify-write sequence.
- `package_after_install`'s `use_flags` parameter is written verbatim (the `install` phase this binpkg is built from); standalone `ebuild <file> package`/`merge` have no resolved graph reaching that deep, so `""` stays the honest value there.

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
- `config_root`: Portage `PORTAGE_CONFIGROOT` -- see `ebuild_merge::MergeOptions::config_root`'s own doc comment for the exact Portage default/`Default` split this mirrors (only consulted by `ebuild_phases::eclass_locations_value`'s own masters-chain resolution).
- `binpkg_multi_instance`: both formats are supported: `bin/misc-functions.sh __dyn_package` builds the exact same archive bytes regardless of `BUILD_ID` -- an xpak `[image tarball][XPAK trailer]` (a `.xpak` file is byte-format-identical to a `.tbz2`; the extension is just the multi-instance marker), a self-contained gpkg container -- so multi-instance is purely Portage `bintree._allocate_filename_multi`'s own path convention: `<pkgdir>/<cat>/<pn>/<pf>-<build_id>.<suffix>`, with `<suffix>` `xpak` for xpak and `gpkg.tar` for gpkg (`allocate_binpkg_build_id`), plus the `BUILD_ID` env export `invoke_dyn_package` already does.
- `binpkg_signing`: the `__dyn_package` helper subprocess signs the gpkg's `metadata.tar`/`image.tar` members (detached `.sig` sidecars) and clear-signs the `Manifest` itself, via `checksum_helper(SIGNING)` spawning `binpkg_gpg_signing_base_command` below. Sourced from `FEATURES` at the CLI boundary like `buildpkg_live`/`binpkg_multi_instance`. xpak has no signature mechanism -- the flag is inert unless `binpkg_format == "gpkg"`.
- `binpkg_gpg_signing_gpg_home`: `BINPKG_GPG_SIGNING_GPG_HOME` has no `make.globals` default -- Portage `_emerge/actions.py:632-646` refuses to build with `binpkg-signing` when this or the key is unset: `!!! {var} is not set`, exit 1 -- mirrored by `run_package`'s own pre-check.

## fn PackageOptions::set_resolved_features
- Backlog #130, narrowed by #147 S1 ruling (i). Mirrors `ebuild_merge::MergeOptions::set_resolved_features`: the `emerge` paths call this right after resolving a per-entry `FEATURES` fold, so a call site sharing one `PackageOptions` across a run can clone and re-derive per entry. Unlike the raw-process-env default-on-unless-negated fallback, a negated token was already folded away by `regenerate()`/`fold_package_env_incremental`.
- `binpkg_multi_instance` keeps its construction value because Portage's binpkg layout is run-wide-only (the bintree binds its allocator once, `bintree.py:529-531` -- S0 A1x/A2 prove both directions); `package_after_install` takes the per-entry token separately for the `BUILD_ID` export gate (Portage's `EbuildBinpkg.py:47-48`). `binpkg_compress`/`binpkg_format`/GPG fields are config scalars, untouched.
- Re-derives the binpkg-affecting tokens from a **resolved**, already-folded `FEATURES` list -- the same "presence is the answer" reading `package_options_from_env`'s own `Some(resolved_features)` arm already uses (a negated token has already been folded away by `regenerate()`/`fold_package_env_incremental`, so a plain presence check is correct here, unlike the raw-process-env default-on-unless-negated fallback `PackageOptions::default`'s own `buildpkg_live` doc comment describes).
- `binpkg_compress`/`binpkg_format`/the GPG signing identity fields are config scalars, not `FEATURES` tokens, and are untouched here.

## fn compress_template
- Six compressors only. `{JOBS}` is a plain pre-`varexpand` substitution (Portage's `doebuild.py:721-724`/`:740-743`).
- Portage `_compressors` (`lib/portage/util/compression_probe.py:10-53`), narrowed to the `"compress"` half only -- this module only ever *builds* a binpkg, never installs from one, so the `"decompress"`/`"decompress_alt"` fields (relevant only when *installing* from a binpkg) have no equivalent here. `${...}` placeholders are resolved afterward by `resolve_compression_command`.

## fn find_binary
- Ports Portage `find_binary()` (`lib/portage/process.py`): the first `PATH` entry containing an executable file named `name`.

## fn resolve_compression_command
- Ports `PORTAGE_COMPRESSION_COMMAND` resolution (`doebuild.py:697-750`). `{JOBS}` is the host CPU count, matching `makeopts_to_job_count`'s `get_cpu_count()` fallback -- portuale's `MAKEOPTS` is always unset, the path Portage takes whenever `MAKEOPTS` lacks `-j`/`--jobs=`. The `${VAR}` substitution is deliberately narrow (no full shell `varexpand`: none of the six templates or realistic flag values need more); whitespace-splitting is a deliberate narrowing of `shlex.split` (no quoting need). An unknown name or missing binary leaves `mysettings["PORTAGE_COMPRESSION_COMMAND"]` unset in Portage too (only warned about via `writemsg`, not reproduced -- this module's execution path prints nothing anywhere else), so unmodified `bin/misc-functions.sh` hits its own `[[ -z "${PORTAGE_COMPRESSION_COMMAND}" ]] && die "PORTAGE_COMPRESSION_COMMAND is unset"` guard naturally.
- Looks up `binpkg_compress` in the `_compressors` table, substitutes `{JOBS}` and `${PORTAGE_BZIP2_COMMAND}`/`${BINPKG_COMPRESS_FLAGS}` (a plain, narrow `${VAR}` substitution -- not a full shell `varexpand`, since none of the six templates or realistic flag values need anything beyond that), `shlex.split()`s the result (narrowed to whitespace-splitting -- same reasoning, no quoting need), and confirms the resolved binary is `PATH`-findable (`find_binary()`).
- Returns `None` -- the caller omits `PORTAGE_COMPRESSION_COMMAND` from the exported environment entirely -- for an unknown `binpkg_compress` name or a compressor whose binary isn't installed.

## fn compression_command_argv
- The one template-to-`argv` resolution (`gpkg.py:_get_binary_cmd`, compress half): `compress_template` lookup (unknown name is `InvalidCompressionMethod`), `{JOBS}`/`${PORTAGE_BZIP2_COMMAND}`/`${BINPKG_COMPRESS_FLAGS}` substitution, whitespace split, `find_binary` on the binary (missing is `CompressorNotFound`). `resolve_compression_command_jobs` delegates to it (joined with spaces); the global-updates gpkg rewrite calls it directly with its writer halves, so a moved gpkg compresses exactly like a fresh build. The helper-subprocess `compression_cmd` (`helpers/gpkg.rs`) keeps its own variant: it `varexpand`s against the full helper environment (nested `${VAR}` refs inside flag values resolve there), where this one substitutes four already-resolved halves.

## fn makeopts_to_job_count
- Ports `makeopts_to_job_count` (`util/cpuinfo.py:55-70`): the greedy `.*(j|--jobs=\s)\s*([0-9]+)` match, else `get_cpu_count()` (`sched_getaffinity`).
- The digits after the *last* `j` (or `--jobs=<whitespace>`) that is followed by optional whitespace and a number -- the greedy `.*(j|--jobs=\s)\s*([0-9]+)` match -- else the CPU count (`get_cpu_count()` = `sched_getaffinity`). Returned as the matched text, as Portage does.

## fn phase_compression_command
- Set for every build phase regardless of `BINPKG_FORMAT`, from the resolved config.
- Portage `doebuild_environment()`'s `PORTAGE_COMPRESSION_COMMAND` (`doebuild.py:697-750`), from the resolved config (`lookup`: calling env over `make.conf`/profile/`make.globals`): `BINPKG_COMPRESS` (default `bzip2`; empty = `cat`), `BINPKG_COMPRESS_FLAGS_<NAME>` replacing `BINPKG_COMPRESS_FLAGS` when set, `{JOBS}` from `MAKEOPTS` (the already-defaulted phase value). `None` for an unknown compressor or a missing binary, like Portage (left unset, only warned about).

## fn resolve_binpkg_format
- Same `lookup` shape as `phase_compression_command`; chain value wins verbatim. A `make.conf` assignment overrides `make.globals` with `""` and `__dyn_package` then dies the same way.
- The chain value when set anywhere, else Portage `cnf/make.globals:43`'s own default `"gpkg"`. The two `PackageOptions` construction sites (`ebuild.rs`'s standalone `package`, `pretend.rs`'s `package_options_from_env`) used to read only the process env over a hardcoded `"xpak"`, so a `make.conf`/profile value was ignored and the default named the wrong format.

## fn resolve_pkgdir
- Backlog #180, same two construction sites. `PKGDIR` is in Portage's `environ_whitelist` (`_config/special_env_vars.py:125`) and not in `environ_filter`, so the calling-env value reaches every consumer -- `bintree.pkgdir` (full config, env layer highest) for the tmpfile path and the phase env for `${PKGDIR}` -- for both formats alike.
- The chain value when set anywhere, else Portage `cnf/make.globals:31`'s own default (`PackageOptions::default().pkgdir`). An empty value falls back (Portage has no `PKGDIR`-empty die; an empty `bintree.pkgdir` only ever yields broken relative paths).

## fn resolve_binpkg_compress
- Backlog #180. For `xpak` the full chain (Portage's `doebuild.py:697` reads the package's own `mysettings`, env layer highest, for the tar pipe); for `gpkg` the files-only chain (Portage's `environ_filter`, `_config/special_env_vars.py:280`, keeps calling-env `BINPKG_COMPRESS` out of the phase env, and `bin/gpkg-helper.py` rebuilds `portage.settings` from that filtered env plus config files). Empty-`BINPKG_COMPRESS`-disables-compression is Portage's `doebuild.py:706-710` / `bin/quickpkg:161-167` arm.
- The chain value when set anywhere, else Portage `cnf/make.globals:39`'s own default (`PackageOptions::default().binpkg_compress`). The caller picks the lookup per format (the Q6 rule, pmtest `bf0692f`); the Q6 probe: env-only `gzip` still compresses `zst`, `make.conf` `gzip` compresses `.gz`. An empty value is honored verbatim; only an *unset* chain falls back.

## fn resolve_binpkg_compress_flags
- Override-then-generic order, the same both readers use (`doebuild.py:710-717`, `gpkg.py:1876-1882`). Generic `BINPKG_COMPRESS_FLAGS` is `environ_filter`ed alongside `BINPKG_COMPRESS`, and `BINPKG_COMPRESS_FLAGS_<NAME>` never reaches the phase env (not in `environ_whitelist`; `config.environ()`'s `filter_calling_env`, `config.py:3275-3305`).

## fn resolve_portage_bzip2_command
- Backlog #180. In Portage's `environ_whitelist` (`_config/special_env_vars.py:143`), not in `environ_filter`, so it propagates into the phase env and reaches even the gpkg helper's rebuilt settings.
- The chain value when set anywhere, else Portage `cnf/make.globals:105`'s own default (`PackageOptions::default().portage_bzip2_command`). Unlike the compression pair, the calling env wins for **both** formats. Only substituted when `binpkg_compress == "bzip2"` (the `${PORTAGE_BZIP2_COMMAND}` template arm).

## fn refresh_entry_compression_command
- Backlog #147 S2. The command is what `__dyn_package` reads -- via the saved `${T}/environment` `bin/ebuild.sh:565-580` sources for the `package` phase -- not `PackageOptions::binpkg_compress`, which stays run-wide on purpose: the gpkg helper reads the global settings (`bin/gpkg-helper.py:49`, `gpkg.py:785`), so a per-entry value must never reach it (S0 B-gpkg: zst members; S0 B-xpak: a per-package `BINPKG_COMPRESS=gzip` reaches the xpak tar pipe, `bin/misc-functions.sh:593-596`, even when run-wide is `zstd`).
- The layered pairs already carry the matched `package.env` scalars appended after the run-wide base (`matched_package_env_vars`), and the calling env is already folded into the run-wide pairs upstream -- no separate process lookup, also unsound under `cargo test`'s shared process env. The config tail (`config.other_vars`) keeps a `make.conf` `BINPKG_COMPRESS` on the pipe: the run-wide base carries no pair for it (the key is `environ_filter`ed out of `phase_environ`), so without the tail the recompute would fall back to Portage's own `.get` default (`bzip2`) and clobber the `make.conf` command. Order mirrors Portage's `env:pkg:conf` `USE_ORDER`: a calling-env value already masked any same-key `package.env` match upstream (#101), so pairs outrank raw env, both outrank files.

## fn packages_index_header
- Portage's reader is `bintree._load_pkgindex` via `_pkgindex_version_supported` (`bintree.py:2429-2437`): `False` unless the header carries `VERSION` <= `bintree._pkgindex_version`. Portage's writer always stamps `VERSION: <n>` (`bintree.py:2388`/`:2397`); a `TIMESTAMP`-only header is not a valid index to Portage. Found as L2 S0 `l2-pkgindex-version-missing`: without it, under `pkgdir-index-trusted` Portage never walks the directory to rebuild, every portuale-built archive is invisible, and `emerge -k` silently falls back to a from-source build. Portuale's own reader (`portage_repo::read_packages_index`) unconditionally skips the first blank-line-terminated block.

## fn omit_stanza_default
- Portage restores defaults on read (`readBody`, `getbinpkg.py:143-145`, `d.setdefault(k, v)`). S0 probe (backlog #188, Portage in `localhost/test-portuale:latest`): `l32/dep-a` (`SLOT="0"`) has no `SLOT` line, `l32/slotpkg` (`SLOT="1"`) has `SLOT: 1`.
- `PackageIndex.write` drops a stanza value that equals Portage's `_pkgindex_default_pkg_data` default for that key (`getbinpkg.py:169-172`, defaults at `bintree.py:609-629`), and `readBody` restores it on the next read. Of the whole default map only `SLOT: "0"` and `EAPI: "0"` can reach a stanza non-empty (every other defaulted key defaults to `""`, which `format_packages_entry` -- like Portage's `if metadata[k]` guard at `getbinpkg.py:176` -- skips anyway), so those two are the whole rule: a default-`SLOT` package's on-disk stanza carries no `SLOT` line at all, and a non-default one carries it verbatim, sub-slot included (only the exact `"0"` is default).

## fn sort_stanza_fields_for_write
- Full mechanics: `keys.sort()` over the *internal* names (`getbinpkg.py:153-177`), `_write_translation_map` on the way out (`_writepkgindex`, `getbinpkg.py:123-126`). Of the translated keys (`_pkgindex_translated_keys`, `bintree.py:638-642`) either writer emits only `_mtime_` -> `MTIME` and `repository` -> `REPO` (`DESCRIPTION` -> `DESC` never reaches a stanza); both internals sort after every all-uppercase key (`_` is 0x5F, `r` is 0x72). The `--buildpkg` assembly is already hand-sorted (no-op guard against future drift); the quickpkg assembly relies on the sort (backlog #203: its fixed order was Portage-divergent).

## struct BuiltBinpkgStanza
- `REPO_REVISIONS`: `"{}"` for sync-less repos up to sync-revision JSON, written verbatim when non-empty.
- One freshly built binary package's `<pkgdir>/Packages` stanza fields: the archive metadata's own values (Portage `_pkgindex_entry`, `bintree.py:2289-2313`), ordered as Portage's `keys.sort()` writes them (`getbinpkg.py:173-177`) -- the *internal* names `_mtime_` and `repository` sort after every uppercase key, so the translated `MTIME`/`REPO` land last. Default-valued (`SLOT: "0"`, `EAPI: "0"`) and empty values are omitted by [`omit_stanza_default`] and [`format_packages_entry`], like Portage's write path.
- `REPO_REVISIONS` has no `_pkgindex_default_pkg_data` entry (`bintree.py:609-629`), so only Portage's falsy guard (`getbinpkg.py:176`) applies: a non-empty archive value -- `"{}"` for sync-less repos (see [`built_binpkg_stanza_fields`]'s field doc below) up to sync-revision JSON -- is written verbatim, a missing/empty one is omitted.

## struct BuiltBinpkgStanza (USER_PATCHES, #540)
- `user_patches` reads `build-info/USER_PATCHES` (written by the vendored `eapply_user` during prepare); empty omits the key, like every other empty field -- which is exactly the host rule (the no-patch `up-d` stanza has no key). It sorts between `USE` and `MTIME`. No writer change was needed for `metadata.tar`: `generate_metadata_from_dir` already packs every build-info file (`USER_PATCHES` + `user_patch.digests` ride along). Oracle `LLM/evidence/2026-10-23-509/` (`build-info-*.txt`, `up-a` stanza); pin `buildpkgonly_records_user_patches_in_metadata_and_index` (real build under patches + patch-free build, digests recomputed independently with `sha2`).

## fn built_binpkg_stanza_fields
- `REPO_REVISIONS` sorts between `RDEPEND` and `REQUIRES` (`D` < `E`, `P` < `Q`).
- Hand-sorted into Portage's `keys.sort()` order (see [`sort_stanza_fields_for_write`]); the sort call is a no-op guard so a future field lands ordered too. `REPO_REVISIONS` sorts between `RDEPEND` and `REQUIRES` (`RDEPEND` < `REPO_...` < `REQUIRES`: `D` < `E`, `P` < `Q`).

## fn write_packages_index_entry
- Index format per `portage_repo::read_packages_index`: `KEY: value` lines, blank-line-separated blocks, first block a header.
- Lock order is Portage's: `os.makedirs(self.pkgdir, exist_ok=True)` *then* acquire (`bintree.py:2057-2059`).
- Dedup matches `_inject_file` (`bintree.py:2237-2247`): primarily by `PATH` (a different `PATH` -- e.g. a different `BUILD_ID` under multi-instance -- is genuinely different and survives, letting multiple builds of the same `CPV` coexist), falling back to `CPV` alone only with no `PATH`.
- `_lock` drops at the end: the same effect as Portage's `unlockfile()`.
- Writes (creating the file, and its own header block, if necessary) or replaces `cpv`'s own entry in `<pkgdir>/Packages`. A pre-existing entry for the *same* `cpv` (a rebuild) is replaced in place, not duplicated -- every other entry (including other versions of the same package) is preserved verbatim.
- Portage `bintree.inject`/`update_pkgindex`/`remove` (`bintree.py:948`/`:1999`/`:2059`) all wrap this "reread the index, in case another process changed it, then update it" sequence in a blocking `lockfile(self._pkgindex_file, wantnewlockfile=1)` -- `PortageLockfile::acquire`, the same primitive `fetch.rs`'s own distfile locking already uses. A genuinely concurrent `emerge -b`/`--buildpkg` racing another write to the same `Packages` file could otherwise interleave (portuale's own single-invocation-at-a-time CLI usage rarely exercises this, but it's cheap and correct to hold the same lock Portage does regardless).

## fn require_signing_config
- Ports `doebuild()`'s first step for `"package"` always running the `install` chain first (`actionmap_deps["package"] == ["install"]`); run here directly, the same "run it myself, don't require the caller to" shape `ebuild_merge::run_merge` already established for its own `["install"]` prerequisite. Shared gate for `run_package` (Portage refuses before building anything) and `package_after_install` (the `FEATURES=buildpkg` side-effect path).
- Portage `_emerge/actions.py:623-646`'s own "not set" gate, shared by `run_package` (before the `install` chain -- Portage refuses before building anything) and `package_after_install` (which the `FEATURES=buildpkg` side-effect path reaches directly): with `FEATURES=binpkg-signing` on a gpkg build, the signing keyring and key must be configured, else `!!! {var} is not set`.

## fn run_package
- `build_env`: `--buildpkgonly` passes the full `phase_environ`-derived env plus the entry's own `USE` (#37 S2); standalone passes `&[]` (no graph, no resolved config -- `phase_env_vars`' curated base stands).
- The per-entry multi-instance token comes from the `--buildpkgonly` per-entry fold when one matched, the run-wide value otherwise; standalone falls back to construction, preserving standalone behavior (Portage's `doebuild` package branch has no per-package `BUILD_ID` logic).
- `skip_noop_src_phases` (#458): forwarded to `run_commands`; `true` on the scheduler-driven `--buildpkgonly` path (`run_buildpkgonly`, whose `EbuildBuild` goes through `EbuildExecuter`), `false` for direct `ebuild <file> package` (manual `doebuild()` mode). Portage: `_emerge/EbuildBuild.py`, `_emerge/EbuildExecuter.py`.

## fn package_after_install
- Backlog #147 S1 ruling (i): layout run-wide (Portage's once-bound bintree allocator), `BUILD_ID` export + index entry per-entry (Portage's `EbuildBinpkg.py:47-48`). Standalone callers pass `false`. Run-wide single + per-entry token writes no `BUILD_ID` anywhere, where Portage writes index `None` and vdb `-1` (S1 D1): a documented `str(None)`-wart divergence, not mirrored.
- `EbuildBinpkg._start`'s per-package gate (`EbuildBinpkg.py:47-48`): a negated entry in a run-wide-multi run takes the multi path with no `BUILD_ID` (S0 A2 shape); a tokened entry in a run-wide-single run takes the single path with none either.
- `_pkgindex_entry` + `PackageIndex.write` (`bintree.py:2289-2312`, `getbinpkg.py:155-175`); digests are Portage's fixed `_pkgindex_hashes = ["MD5", "SHA1"]` (`bintree.py:548`) via `perform_multiple_checksums(pkg_path, hashes=...)` (`bintree.py:2302`), taking no configuration -- S0 probe (backlog #188) confirmed a `PORTAGE_CHECKSUM_FILTER='-SHA1'` rebuild still writes both. Portage hashes streaming, once per checksum; portuale's whole-file read matches the pre-existing `MD5`-only shape.
- `PATH` always written (`bintree.py:2311`), not gpkg-only; remote fetch falls back to a bare `<pf>.<ext>` guess without it (Portage's `gettbz2`: `if not rel_url: rel_url = pkgname + ".tbz2"`). Multi-instance form is `_allocate_filename_multi`'s `os.path.join(pkgdir, cpv.cp, pf)`.
- `BINPKGMD5`/`BUILD_ID` recording ports `EbuildBuild._record_binpkg_info` (`EbuildBuild.py:502-523`, `--buildpkg`/`--buildpkgonly`) and `doebuild`'s `package` branch (`doebuild.py:1522-1544`): `BINPKGMD5` (the index `MD5`) always, `BUILD_ID` when carried -- overwriting `__dyn_package`'s own newline-less `BUILD_ID` (`misc-functions.sh:560-561`).
- `build-info` first because `_pkgindex_entry` reads `cpv._metadata` (the *archive's* metadata): `RDEPEND` with its libc injection, `PROVIDES`/`REQUIRES` from the ELF scan, `IUSE`/`IUSE_EFFECTIVE`, `SIZE` (#39).
- Multi-instance: Portage `FEATURES=binpkg-multi-instance` (`bintree.py:529-531`'s own `_allocate_filename = self._allocate_filename_multi` swap-in) -- both formats (`PackageOptions::binpkg_multi_instance`'s own doc comment has the full grounding). `_allocate_filename_multi` (`bintree.py:2607-2669`) writes to `<pkgdir>/<cat>/<pn>/<pf>-<build_id>.<suffix>` -- a `<cat>/<pn>` subdir, a `-<build_id>` suffix, and `<suffix>` `xpak` (not `tbz2`) for the xpak format. The archive bytes are the single-instance form either way, so this is a pure path/naming convention plus the `BUILD_ID` env export in `invoke_dyn_package`.
- Temp-file-then-rename: Portage `EbuildBinpkg._start`/`_package_phase_exit` (`_emerge/EbuildBinpkg.py:37-67`): `__dyn_package` never writes the discoverable `Packages`-index path directly -- it writes a same-directory `PORTAGE_BINPKG_TMPFILE` (a `NamedTemporaryFile` with a PID suffix) instead, and only `bintree.inject()` renames it into place, and only after the packaging phase has already exited `EX_OK`. A mid-write failure (tar/compressor/xpak-helper crash, disk full) therefore never leaves a truncated or corrupt archive sitting at the final path for a later scan to pick up -- the temp file is unlinked instead (`_package_phase_exit`'s own `os.unlink(self._binpkg_tmpfile)` on non-`EX_OK`). Mirrored here with `std::fs::rename` (atomic, same directory => same filesystem) rather than a `NamedTemporaryFile`, since this is a single-process CLI with no concurrent packaging of the same CPV to race against.

## fn binpkg_extension
- Ports `__dyn_package`'s `die "Unknown BINPKG_FORMAT ${BINPKG_FORMAT}"`.

## fn multi_instance_binpkg_suffix
- Ports `_allocate_filename_multi` (`bintree.py:2620-2626`).

## fn allocate_binpkg_build_id
- Ports `_allocate_filename_multi`'s search (`bintree.py:2607-2669`): Portage's `_max_build_id` equivalent; the `while True` "avoid races" retry loop narrowed to existence checks (no `open(..., "x")` placeholder dance for a single-process CLI that never races itself).
- Starts one past the highest `BUILD_ID` already used by this `cp/pf` (scanning `<pkgdir>/<cat>/<pn>/<pf>-*.<suffix>`, Portage's own `_max_build_id` equivalent -- portuale has no long-lived `bindbapi` to consult instead, so this re-derives it from the filesystem each call), then increments past any that's since become occupied.

## fn binpkg_checksums
- One import covers both hashers (`digest::Digest` re-exported by both crates).
- `_pkgindex_entry` (`bintree.py:2302`'s own `perform_multiple_checksums(pkg_path, hashes=self._pkgindex_hashes)`, `_pkgindex_hashes = ["MD5", "SHA1"]` at `bintree.py:548`) writes both an `MD5` and a `SHA1` digest of the whole binpkg file into its own `Packages` entry. Both are computed here in a single read. Without this, a binpkg portuale itself built would carry no digests at all, silently skipping `download_and_verify`'s own integrity checks for anyone fetching it from a portuale-served pkgdir as a remote binhost.

## fn package_makeopts
- #326 S4 (`{JOBS}` for the native gpkg compressor); #328 (global fallback).

## fn invoke_dyn_package
- Ports `EbuildBinpkg._start`'s condition verbatim: `if "binpkg-multi-instance" in self.settings.features: self.settings["BUILD_ID"] = str(build_id)` (`EbuildBinpkg.py:47-48`). Previously only the index entry carried `BUILD_ID`, so `binpkg::populate_local_pkgdir`'s filename-inference fallback was silently doing the archive's job; exporting it also gives `__generate_packdebug` the debug tarball name it needs under `FEATURES=packdebug` (`bin/misc-functions.sh:557-558`).
- `PORTAGE_PYTHONPATH` over `PORTAGE_PYM_PATH` is `bin/misc-functions.sh`'s own `PYTHONPATH=${PORTAGE_PYTHONPATH:-${PORTAGE_PYM_PATH}}`.
- gpkg compressor env: `gpkg._get_binary_cmd` reads `BINPKG_COMPRESS`, `BINPKG_COMPRESS_FLAGS[_<NAME>]`, `${PORTAGE_BZIP2_COMMAND}` via `varexpand` from the helper's rebuilt settings -- by construction identical to the helper's own files-only rebuild (the `BINPKG_COMPRESS`/`FLAGS` values here are files-only for `gpkg` per the Q6 rule; `PORTAGE_BZIP2_COMMAND` keeps the calling env, whitelisted, never filtered).
- `MAKEOPTS` export: `gpkg._get_binary_cmd` is `makeopts_to_job_count(settings.get("MAKEOPTS", "1"))` from the helper's `portage.settings` (`make.conf` included). #328: the ebuild-path chain needs a repo above the ebuild, but `quickpkg_from_vdb` packages a scratch copy of the vdb ebuild with none; `MAKEOPTS` is global so it comes from `resolve_unmerge_config`, as `BINPKG_FORMAT` does on that path (#173). Portage reads `settings["MAKEOPTS"]` there too and logs `zstd -T5` for `make.conf` `MAKEOPTS=-j5` (pmtest `differential-test-bed/scripts/328-quickpkg-makeopts.sh`, Portage 3.0.82.2).
- Signing export ports `gpkg.gpkg.create_signature` (`gpkg.py:790`) via `checksum_helper(SIGNING)` spawning `BINPKG_GPG_SIGNING_BASE_COMMAND` with Portage's `[PORTAGE_CONFIG]` substitution; `FEATURES` flows through `phase_env_vars`.
- Runs the unmodified `bin/misc-functions.sh __dyn_package` against `ebuild_path` (tars `${D}` + appends `${PORTAGE_BUILDDIR}/build-info` as the xpak segment / hands both to `gpkg-helper.py`), with the compressor / `PKGDIR` / `PORTAGE_BINPKG_TMPFILE` environment Portage sets. Shared by `run_package` (fresh `src_install` image) and `quickpkg_from_vdb` (installed-files image).
- Portage `bin/misc-functions.sh`'s own `xpak-helper.py`/`gpkg-helper.py` invocation prefers `PORTAGE_PYTHONPATH` over `PORTAGE_PYM_PATH` (`PYTHONPATH=${PORTAGE_PYTHONPATH:-${PORTAGE_PYM_PATH}}`) -- set directly so the unmodified helper subprocess imports `portage` from *this* checkout, not whatever else might be system-installed. Without a checkout nothing is pushed (`pythonpath_value`).
- `MAKEOPTS` export: `makeopts_to_job_count(settings.get("MAKEOPTS", "1"))`, from the helper's own `portage.settings`, i.e. `make.conf` included. A calling-env `MAKEOPTS` is whitelisted and already reaches the helper through the phase env; the `make.conf`/profile side does not, so it is exported here, from the same global (not `package.env`-matched) chain as the other scalars. Left unset when nothing resolves: the helper then counts CPUs, as Portage does.

## fn quickpkg_from_vdb
- Ports `dblink.quickpkg` + `_quickpkg_dblink` (`vartree.py:2307` / `:6296`): byte-shape identical to Portage's own `tar_contents(...)` + `xpak.tbz2(...).recompose_mem(xpak.xpak(dbdir))` (`bin/quickpkg:147` packs the whole vdb dir via `xpak.xpak(dblnk.dbdir)`).
- Entry files read through the installed-database backend since feat#157 S1.6 (`files`: one `open` each, as before); database backends (S4.1) get a scratch materialized copy.
- No `BUILD_TIME` idempotency check (backlog #202): Portage's check iterates `bintree.dbapi.match("=cpv")` (`vartree.py:6297-6313`), but in every removal flow that reaches it the bintree was never populated -- `binarytree.__init__` only records `populated = 0`, and `run_action` populates it solely for `action in ("search", None)` with `--usepkg` (`actions.py:3725-3765`; the rest serve the depgraph merge flow with remote binpkgs). An empty dbapi matches nothing, so Portage always shells out to `bin/quickpkg` and rebuilds -- while consulting the raw `<pkgdir>/Packages` file here skipped the backup whenever a stale stanza (e.g. an unlinked first backup) shared the re-merged package's second-granularity `BUILD_TIME`, silently producing no second backup with rc 0.
- `Ok(None)` when the archive path exists is portuale's own conservative cut (`bin/quickpkg` overwrites). fifo/device skip is the same `CAP_MKNOD` limitation as the merge side's `create_special_node`.
- Stanza assembly: the same archive-metadata source `_pkgindex_entry` (`bintree.py:2289-2313`) reads; `EAPI` verbatim unless `"0"` (the vdb `EAPI` file always exists); `REPO_REVISIONS` verbatim when carried (Portage writes `build-info/REPO_REVISIONS` whenever `PORTAGE_REPO_REVISIONS` is non-empty, `phase-functions.sh:769`, so even sync-less repos record `"{}"`); `repository` verbatim as `REPO` (`repository` is in Portage's stanza aux-key set, `bindbapi._aux_cache_keys`, `bintree.py:96-124`, emitted translated last after `MTIME`); no `PF`/`CATEGORY` lines (neither is in the aux-key set -- the n203 probe's Portage quickpkg stanza carries neither although the vdb has both; backlog #188's "extras match Portage" claim was wrong). `REPO` is never inherited from the header in the shapes portuale writes: Portage drops a stanza value equal to a non-empty header value (`_pkgindex_inherited_keys`, `bintree.py:630`, `getbinpkg.py:164-168`), but the header default for `repository` is `""` (`bintree.py:633-636`), `_update_pkgindex_header` never sets it (only `_pkgindex_header_keys`, which excludes it), and portuale's header carries only `TIMESTAMP`+`VERSION` -- so the guard's `if v` is always falsy and `REPO` is always written, like Portage's fresh-index shape (the n203 probe ends `..., MTIME, REPO: l32`).
- Temp-file-then-rename: Portage's `bin/quickpkg` does it too (`binpkg_tmpfile = bintree.pkgdir + cpv + ".tbz2." + str(portage.getpid())`, later `bintree.inject` renamed).
- `FEATURES=unmerge-backup`'s pre-unmerge binpkg of the **installed** package, built from its files under `${ROOT}` per `CONTENTS` rather than from a fresh `src_install`. Stages the installed tree into `${PORTAGE_BUILDDIR}/image` and a verbatim copy of the vdb dir into `${PORTAGE_BUILDDIR}/build-info`, then runs the same unmodified `__dyn_package` the `ebuild <file> package` / `--buildpkgonly` paths already use. A `$PKGDIR/Packages` entry is written from the vdb's own recorded dependency metadata (not md5-cache -- the package may no longer be in any repo). `include_config=False`: a CONFIG_PROTECT'd (not -MASK'd) file is left out of the image.
- Under `FEATURES=binpkg-multi-instance` the archive is written to `_allocate_filename_multi`'s `<pkgdir>/<cat>/<pn>/<pf>-<build_id>.<suffix>` (Portage `bin/quickpkg` -> `bintree.inject` -> `getname(..., allocate_new=True)`); otherwise the bare `<cat>/<pf>.<ext>`.
- Temp-file-then-rename, same dance as `package_after_install`: Portage `bin/quickpkg` does this too (its own `binpkg_tmpfile = bintree.pkgdir + cpv + ".tbz2." + str(portage.getpid())`, later `bintree.inject` renamed), so a failed compressor never leaves a corrupt archive at `binpkg_path` (the `if binpkg_path.exists() { return Ok(None) }` check above means this only ever runs when nothing was there before -- but leaving a corrupt file behind would still poison the next `--getbinpkg`/`--usepkg` scan of this pkgdir).
- `PATH` is always written (as in `package_after_install`; also needed for `populate_local_pkgdir`'s mtime-staleness fast path to find this entry at all), derived from `binpkg_path` relative to `$PKGDIR` so the multi-instance `<cat>/<pn>/<pf>-<id>.<suffix>` subdir path is carried through.

## fn stage_contents_for_quickpkg
- Split out of `quickpkg_from_vdb` (#339 M1): the per-CONTENTS-line staging into the image. The fifo/device skip and the CONFIG_PROTECT rule are described under `quickpkg_from_vdb` above.

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
- Unmatched entry: no `BINPKG_COMPRESS` pair anywhere, so the run-wide fallback recomputes byte-identically (bzip2 default, matching Portage's own `.get` default and the `run_wide_phase_env` value already in the env).

## tests: write_packages_index_entry_creates_a_header_then_the_entry
- Portage's `_pkgindex_version_supported` gate (L2 S0 `l2-pkgindex-version-missing`).

## tests: probe_shaped_stanza
- The S0 probe's `l32/dep-a` stanza shape (Portage in `localhost/test-portuale:latest`, default config). Digests are Portage's fixed `_pkgindex_hashes` pair (`bintree.py:548`), written even with `PORTAGE_CHECKSUM_FILTER='-SHA1'`. `REPO_REVISIONS: {}` is Portage's sync-less value: the overlay repo has no sync type, so `_setup_repo_revisions` records an empty dict, which `phase-functions.sh:769` still writes (the `"{}"` string is non-empty), and the stanza carries it verbatim (no `_pkgindex_default_pkg_data` entry, only the falsy guard).

## tests: quickpkg_from_vdb_writes_portage_ordered_stanza_with_eapi_and_repo_revisions
- Backlog #203 (#226 for the `REPO` part); n203 container probe (`quickpkg =l32/dep-a-1.0`, `/tmp/opencode/n203/probe.log`): `EAPI: 8` verbatim (omitted iff `"0"` like `--buildpkg`); `REPO_REVISIONS` `{}` (merged from a sync-less build).

## tests: real_package_builds_a_real_xpak_tbz2_and_a_real_packages_entry
- Xpak building is codec-agnostic (the `portage_repo` reader never parses a `.tbz2`'s content, only `Packages`); "bzip2" pinned because it is a near-universal base package, "xpak" pinned per backlog #173. Test fn name and `XPAKPACK` assert message keep "real" as code tokens.
- A real file, real bzip2+tar+XPAK content: Portage's own `xpak.py` writes a real `XPAKPACK` magic marker (`lib/portage/xpak.py`'s own `xpak()`) right before the metadata blob it appends, via the unmodified `xpak-helper.py recompose` subprocess.
- `_pkgindex_entry` always writes an `MD5` and a `SHA1` of the whole binpkg file (`binpkg_checksums`'s own doc comment) -- without them, `emerge_getbinpkg::download_and_verify` would silently skip integrity-checking anyone fetching this same file from a portuale-served pkgdir as a remote binhost. (`md5::Digest` is the shared `digest::Digest` trait both hashers implement -- one import covers both.)

## tests: package_after_install_stanza_omits_default_slot_and_writes_both_digests
- Backlog #188; S0 Portage probe, default config, `SLOT="0"` fixture `dev-libs/packagepkg` (same shape as the probe's `l32/dep-a`): no `SLOT` line (`_pkgindex_default_pkg_data`, `bintree.py:609-629`, dropped by `PackageIndex.write`, `getbinpkg.py:169-172`), both fixed digests (`_pkgindex_hashes`, `bintree.py:548`), translated `MTIME` (never internal `_mtime_`).

## tests: quickpkg_from_vdb_with_gpkg_format_builds_a_readable_gpkg
- Re-run as a child process with no checkout by `helpers::gpkg::tests::quickpkg_from_vdb_gpkg_works_without_the_checkout`.

## tests: real_multi_instance_xpak_uses_the_dot_xpak_extension_and_scans_back
- Portage's `bin/misc-functions.sh` builds `[image][XPAK]` regardless of `BUILD_ID`.
- Portage `bintree._allocate_filename_multi`: an xpak multi-instance binpkg is `<pkgdir>/<cat>/<pn>/<pf>-<build_id>.xpak` -- the `.xpak` extension (not `.tbz2`), but byte-format-identical to a `.tbz2`. The no-`Packages`-index scan (`populate_local_pkgdir`) must walk the subdir, read the file with the same xpak reader, and carry `BUILD_ID` back.

## tests: real_package_with_binpkg_multi_instance_writes_the_cat_pn_subdir_layout
- Backlog #174: vouched files are trusted on the index's word (like Portage's default `FEATURES=pkgdir-index-trusted`), so scanned entries are the written stanzas verbatim; Portage's `_pkgindex_aux_keys` carries no `PF`, so `PF` is no longer asserted (downstream splits it from `CPV`, exactly like Portage).
- Portage `bintree._allocate_filename_multi`: `<pkgdir>/<cat>/<pn>/<pf>-<build_id>.gpkg.tar`, not a bare `<cat>/<pf>` name.

## tests: package_records_binpkgmd5_and_build_id_in_build_info_like_portage
- Ports Portage's `EbuildBuild._record_binpkg_info` (`EbuildBuild.py:502-523`) and `doebuild`'s `package` branch (`doebuild.py:1522-1544`).

## tests: a_failed_packaging_phase_never_touches_the_final_binpkg_path
- Ports `EbuildBinpkg`'s temp-file-then-rename (`_emerge/EbuildBinpkg.py:37-67`): `PORTAGE_BINPKG_TMPFILE` is a same-directory `NamedTemporaryFile` with a PID suffix; `_package_phase_exit` unlinks it on non-`EX_OK`. The xpak tar pipe's `[[ -z "${PORTAGE_COMPRESSION_COMMAND}" ]]` guard (backlog #173's deterministic die) fires before the tar pipe ever opens.

## tests: resolve_compression_command_is_none_when_the_bzip2_var_names_a_missing_binary
- `repo_root_for` itself now lives in, and is tested in, `ebuild_phases.rs` (shared with `fetch_sources`'s own repo lookup -- see that module's own doc comment on why it moved).

## tests: stanza_field_sort_matches_portage_keys_sort_with_mtime_repo_last
- Portage `PackageIndex.write` sorts the *internal* names (`getbinpkg.py:173-174`) and translates on write (`:175-177`): `_mtime_`/`repository` sort after every uppercase key, so `MTIME`/`REPO` land last in that relative order. A shuffled full key set must come out in exactly the probe's order.

## tests: package_after_install_writes_the_resolved_use_flags_into_the_packages_index
- Portage `Package.use.enabled` (`emerge_build.rs`'s own `build_one_source_entry`/`ebuild_merge.rs`'s own `run_merge` both extract this from their own already-resolved build env and pass it straight through) -- exercised here directly against `package_after_install` itself, the shared tail both callers route through.

## tests: real_package_with_gpkg_format_builds_a_real_gpkg_tar_this_pilots_reader_round_trips
- A real `.gpkg.tar` -- unmodified `bin/gpkg-helper.py compress` (Portage `portage.gpkg.gpkg().compress()`) built it: an outer tar whose members are the `gpkg-1` version marker, the compressed `metadata.tar.<comp>`, the compressed `image.tar.<comp>`, and a `Manifest`.
- `write_post_install_metadata` (Portage `_post_src_install_write_metadata`) put the dep strings into `build-info`, and Portage `gpkg._generate_metadata_from_dir` carried them into `metadata.tar`.
- And the directory scan (Portage `bintree._populate_local`) reads the file directly -- the just-written index entry's own `_mtime_`/`SIZE` still agree with the file (nothing else touched it since), so this hits the mtime-staleness fast path rather than re-parsing the gpkg archive.

## tests: run_package_with_src_phase_skip_runs_setup_then_install_only
- Backlog #458: `run_package` with `skip_noop_src_phases: true` on `dev-libs/schedleaf-a` (sourceless, `DEFINED_PHASES=install`) -- stamp-level proof the wiring works: `.pretended`/`.setuped`/`.installed` exist (the skip is decided lazily after `setup`, like `EbuildExecuter._setup_exit`), the five skippable stamps (`.unpacked`/`.prepared`/`.configured`/`.compiled`/`.tested`) are absent (skipped phases never spawn, never stamp), and the binpkg still lands (the packaging tail is untouched).

## tests: run_package_with_src_phase_skip_still_runs_a_defined_src_phase
- Backlog #458 guard: the flag threads the same `can_skip_source_phases` gate, so `dev-libs/unsetcflagspkg` (defines `src_compile`) still runs the full chain -- `.compiled` exists.
