# crates/portuale/src/ebuild_phases/fetch_sources.rs
Fetching and linking the distfiles a build needs (`A`/`AA`).

## fn depend_use_set
- Portage's `setcpv` config `USE` is profile + `make.conf` + user `package.use`; `PORTAGE_RESTRICT`/`PORTAGE_PROPERTIES` come from the already-USE-reduced `_pkg` accessor (`_flatten` over `settings["PORTAGE_USE"]`). Portuale's equivalent is `Config::use_flags` (same config-global set, no per-candidate layer).
- Atom matching would need the target's `cat/pkg-ver:slot/sub` identity, which the `depend` path deliberately never resolves, so metadata extraction stays on the config-global set.

## fn resolved_fetch_config
- Reads `FETCHCOMMAND`/`RESUMECOMMAND` (and the `_<PROTO>` variants) plus `PORTAGE_SSH_OPTS` out of `mysettings`.

## fn resolved_ro_distdirs
- `shlex.split(PORTAGE_RO_DISTDIRS)`; each candidate is re-checked at fetch time, but Portage filters the list up front.

## fn package_a
- Portage's `A` is the `use_reduce(SRC_URI, uselist=USE)` distfile list, the same set `fetch_src_uri` returns.

## fn fetch_sources
- `config_root` defaults to `/` when unset; see `FetchOptions::config_root`.
- The `distlocks`/`force-mirror`/`PORTAGE_FETCH_CHECKSUM_TRY_MIRRORS` env-var shortcuts mirror the `collision_protect`/`protect_owned`/`unmerge_orphans` precedent (no full config resolution on this path); see `FetchOptions::distlocks`.
- `fetch_commands` is `fetch.py`'s command family; `portage_ssh_opts` is `mysettings.get("PORTAGE_SSH_OPTS")`.
- `pkg_nofetch` prints custom "get it from <URL> and drop it in <DISTDIR>" instructions. The phase not being defined (or its own failure) never masks the fetch error.
