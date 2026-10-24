# crates/portuale/src/remote/merge_script.rs
The phase and merge shell scripts sent to the client.

## module
- Client merge (slice 4); client phases (slice 3).

## fn eapi_exports_merge_type
- Portage `eapi_exports_merge_type` (`lib/portage/eapi.py:63`, attrs table `:300`: `exports_merge_type = eapi >= Eapi("4")`): `MERGE_TYPE` reaches the phase env only for EAPI 4+ (Portage `config.py:3335-3336` pops it otherwise). An unsupported or unparseable EAPI takes Portage's `_get_eapi_attrs` fallback row (`eapi.py:247-258`), which exports.

## fn phase_exports
- The replaced instance's prerm/postrm used to go through the merge driver's own `run_old_hook`, which deliberately exported no `MERGE_TYPE`: Portage's unmerge re-creates the old instance's settings (`vartree.py:4535-4540`) and never passes through the scheduler that sets it. Since #382 the flow retires the old instance through the Rust unmerge instead (`merge_flow.sh`), and `run_old_hook` survives only for its own tests. `unit_bin` is the shipped runtime dir (`$UNIT/bin`, `$OLD_TMP/bin` symlink or copy; callers decide).
- `MERGE_TYPE=binary` rides the same line as `EMERGE_FROM` (Portage `_emerge/Binpkg.py:92` sets it for every binpkg merge; `mrg` only merges binpkgs).
- `portuale_bin` is the S8.1-resolved absolute client path (#326 D5). `PORTAGE_PYTHON` is the shipped `portuale-python` shim through the script's own `$UNITBIN` (`$UNIT/bin`, defined by each script header), `PORTAGE_PYM_PATH` is `/` (no checkout on the client), and `PORTAGE_IPC_DAEMON` is unset (no IPC daemon, #326 D6; this also scrubs a value inherited over the local transport).

## fn phase_script
- Recreates the local `run_phase_from_saved_env` + `run_one_phase_bash` setup client-side. Path overrides are exported with client-side values; everything else (EAPI, PN/PV/…, USE) rides the sourced saved environment, whose stale path copies ebuild.sh's own `__preprocess_ebuild_env` strips (the `environment.raw` marker enables that filtering, mirroring local).
- D keeps local's trailing slash. `UNITBIN` is `$UNIT/bin` (#326 D5/S8.3).

## const REGEN_LOCALE_VARS
- Portage's saved locale comes from the calling environment through `environ_whitelist` (`special_env_vars.py`: `LANG` plus the `LC_*` list), and for `mrg` the calling environment is the server process.
- `LOCPATH` rides the same list (backlog #516): it sits in Portage's `environ_whitelist` (`special_env_vars.py`), so a `locale-gen`d client root keeps working after the regen.

## fn collect_server_locale
- One entry per variable the getter reports as set, even empty: a set-but-empty `LANG` is still the calling environment's value, which is what Portage's whitelist passes through.

## fn eapi_is_posixish
- Portage's `posixish_locale` EAPI attribute (`eapi.py:182,309`). Only posixish phases run the `LC_ALL` split below (`EbuildPhase.py:51-56`); older EAPIs keep `LC_ALL` as-is. Portage rejects an unknown EAPI outright; the local phase path's unconditional split takes the same modern direction.

## const PORTAGE_LOCALE_CATEGORIES
- Portage's `locale_categories` (`portage/util/locale.py:22-36`): the six POSIX categories, `LC_PAPER`, and the five GNU extensions (`LC_ADDRESS`, `LC_IDENTIFICATION`, `LC_MEASUREMENT`, `LC_NAME`, `LC_TELEPHONE`). Portage's split operates on settings, and for a binpkg-merge postinst `$T/environment` does not exist yet, so `config.environ()` applies no whitelist filter (`config.py:3286-3299`) and all twelve reach the phase env and the `PORTAGE_UPDATE_ENV` save (`EbuildPhase.py:158,259` runs the env extractor for `pretend`/`prerm` only). Probed against Portage's own `split_LC_ALL` + `environ()`: with `LC_ALL` set, all twelve export when `$T/environment` is absent, seven when present.

## fn split_server_locale
- Port of Portage's `split_LC_ALL` (`portage/util/locale.py:160`). Portage copies `LC_ALL` over the same list, unconditionally overwriting, then blanks it and `config.environ()` deletes the placeholder (`config.py:3374-3385`).
- Portage's split never touches `LANG`, but with `LC_ALL` set and `LANG` unset the effective locale is `LC_ALL`'s value everywhere, and the bed shows Portage's saved env carrying `LANG` alongside the split categories. `LANGUAGE` (not a locale category) rides along per the l171b brief. An explicitly set `LANG` is never clobbered. A set-but-empty `LC_ALL` is dropped without fanning out (Portage's `if lc_all:` is falsy, then `environ()` deletes the placeholder). Non-posixish EAPIs are untouched because Portage never calls the split there.

## fn postinst_regen_script
- Backlog #171: Portage `vartree.py:5334` sets `PORTAGE_UPDATE_ENV=<dbpkgdir>/environment.bz2` around *every* merge's postinst phase, and `bin/phase-functions.sh:1072-1082` re-saves the vdb env from the live phase env through it, so the vdb env carries the merge-time config, not the binpkg's build-time one. Portage's `prerm|postrm|preinst|postinst|config|info` case (`phase-functions.sh:1061-1083`) runs `__ebuild_phase_with_hooks pkg_${1}` unconditionally (`__ebuild_phase` is a no-op for an undefined function) and *then* the `PORTAGE_UPDATE_ENV` block. `pkg_postinst` itself still runs only if defined (the same no-op rule; nothing faked).
- `FEATURES` / `PORTAGE_FEATURES` carry the resolved client list (the local `refresh_features` rule), so the env records the merge-time features (e.g. no `buildpkg`); the regen'd env carries them exactly as Portage's calling-environment whitelist does. The script exits with the phase rc (a non-zero postinst is non-fatal, Portage `_postinst_failure`, as long as the regen file exists).
- `O` is unset and `SHELL` unset before the phase runs: Portage's phase shell never has `O` (`config.environ()` drops it via `special_env_vars.py: environ_filter` even though `doebuild.py:475` sets `mysettings["O"]`), while `mrg` exported a unit-local `O` and usually inherits an exported `SHELL`. Its saved `SHELL` is `declare --` (bash initializes its own; `SHELL` is in neither the whitelist nor the config). Backlog #171c: `export -n` would keep a non-default server value.
- The exports below the unsets carry the pre-split values.
- Plan §6: the client must have no `bzip2`.

## const MERGE_HELPERS
- Pure bash, no placeholders except the colormap/PATH roots baked by the caller template below.

## const MERGE_FLOW
- Client merge flow (slice 4). Shell vars (`UNIT`, `ROOT`, `VDB`, `VDBROOT`, `NEWPF`, `PKG`, `MAINS`, `PROTECT`, `MASK`, `STATELESS`, …) come from `merge_script`'s header.
- Any existing file/symlink destination outside the `CONFIG_PROTECT` divert is a fail-closed collision, and old hooks never run in stateless mode.
- Since #382 the old instance is retired by the shipped Rust binary (`$PORTUALE_BIN ebuild … unmerge`), which reads and writes the client vdb through `portage-vdb` like any local unmerge; the "no portuale code on the client" shape (feat#157 design §6.3) now covers only the copy/vdb-write half of the driver. Only the server-side `VdbShadow` read goes through `portage-vdb` from the server. New-postinst runs separately afterwards (see `run_bundle_stage`).

## fn merge_script
- The `--remote-binpkg` trial path keeps no ledger; the resolve path always records. `PORTAGE_PYM_PATH` is `/` and `PORTAGE_IPC_DAEMON` is unset, like the phase scripts. The old-instance unmerge child inherits the header's exports (`PORTUALE_BIN`).
