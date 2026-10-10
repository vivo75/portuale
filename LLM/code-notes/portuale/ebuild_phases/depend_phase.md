# crates/portuale/src/ebuild_phases/depend_phase.rs
The `depend` phase and its metadata cache (run + depcachedir rung).

## fn run_depend_phase
- `bin/ebuild.sh`'s `depend` branch writes the full key list (`DEPEND RDEPEND SLOT SRC_URI RESTRICT HOMEPAGE LICENSE DESCRIPTION KEYWORDS INHERITED IUSE REQUIRED_USE PDEPEND BDEPEND EAPI PROPERTIES DEFINED_PHASES IDEPEND INHERIT`) to `${PORTAGE_PIPE_FD}` (Portage's `_metadata_fd`), so incidental stdout/stderr cannot corrupt the metadata. The wrapper is a tiny `exec 9>` shell prefix (no `unsafe`, no extra crate).
- `EbuildMetadataPhase` reports every outcome as a `returncode` (`os.EX_OK` / 1 for expected failures like invalid metadata / anything else unexpected); `MetadataRegen._task_exit` keys `cp_retry` off it (`returncode != 1` retries). Hence the `DependError` struct.
- Sandbox-log rewrite to 2 does not apply: `depend` is outside `_doebuild_spawn`'s `SANDBOXED_SRC_PHASES`.
- Exit 0 with no metadata is Portage's `metadata_valid == False` arm (returncode 1, never retried).

## fn depend_phase_metadata
- Registered via `portage_repo::register_aux_metadata_provider`; Portage's ebuild fallback when `metadata/md5-cache` has no entry (`_pull_valid_cache` miss -> `doebuild(mydo="depend")`).
- The depcachedir rung sits exactly where Portage's `_pull_valid_cache` has it: consult after the repo pregen cache, before `EbuildMetadataPhase`; write after success via `portdb._write_cache`. Flat layout `<depcachedir>/<repo path without leading '/'>/<cat>/<pf>` (e.g. `/var/cache/edb/dep/var/db/repos/porttest/porttest/docs-1.0`). An unwritable dir is Portage's `secpass < 1` read-only branch (volatile `_ro_auxdb`); here the in-memory memo.
- The writable depcachedir is `mtime_md5_database`: its `_eclasses_` carries eclass dir paths (`name\tpath\tmd5`, `store_eclass_paths = True`).
- Unsupported EAPI: Portage skips the phase and the package is EAPI-masked (`masked by: EAPI 9999`); portuale returns a failure instead (documented narrowing: the cp drops from candidates rather than rendering as an EAPI mask). Default head EAPI is `"0"`.
- `PORTAGE_TMPDIR` comes from `portage_tmpdir_from_env`, same CLI-boundary default as the rest.

## fn parse_aux_entry
- Portage's cache reader (`_parse_data`) raises `CacheCorruption` for lines without `=`; the phase pipe is written by `bin/ebuild.sh` and never emits one, so skipping is safe.

## fn depcachedir
- Not relative to `ROOT` (Portage rebases under `eroot` only in the unprivileged non-`/` target-root fallback, which never applies here).
- Narrowing: Portage resolves the setting through the full config stack (a `make.conf` assignment wins over the environment); portuale reads the process env only, the same CLI-boundary default the phase env whitelist already carries.
