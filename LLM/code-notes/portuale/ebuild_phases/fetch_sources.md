# crates/portuale/src/ebuild_phases/fetch_sources.rs
Fetching and linking the distfiles a build needs (`A`/`AA`).

## fn depend_use_set
- Portage's `setcpv` config `USE` is profile + `make.conf` + user `package.use`; `PORTAGE_RESTRICT`/`PORTAGE_PROPERTIES` come from the already-USE-reduced `_pkg` accessor (`_flatten` over `settings["PORTAGE_USE"]`). Portuale's equivalent is `Config::use_flags` (same config-global set, no per-candidate layer).
- Atom matching would need the target's `cat/pkg-ver:slot/sub` identity, which the `depend` path deliberately never resolves, so metadata extraction stays on the config-global set.

## fn resolved_fetch_config
- Reads `FETCHCOMMAND`/`RESUMECOMMAND` (and the `_<PROTO>` variants) plus `PORTAGE_SSH_OPTS` out of `mysettings`.

## fn resolved_ro_distdirs
- Portage `fetch.py:1055-1059`: `shlex.split(PORTAGE_RO_DISTDIRS)` filtered to directories that exist (each candidate is re-checked at fetch time, but Portage filters the list up front).

## fn package_a
- Portage's `A` is the `use_reduce(SRC_URI, uselist=USE)` distfile list, the same set `fetch_src_uri` returns.

## fn fetch_sources
- `config_root` defaults to `/` when unset; see `FetchOptions::config_root`.
- The `distlocks`/`force-mirror`/`PORTAGE_FETCH_CHECKSUM_TRY_MIRRORS` env-var shortcuts mirror the `collision_protect`/`protect_owned`/`unmerge_orphans` precedent (no full config resolution on this path); see `FetchOptions::distlocks`.
- `fetch_commands` is `fetch.py`'s command family; `portage_ssh_opts` is `mysettings.get("PORTAGE_SSH_OPTS")`.
- `pkg_nofetch` prints custom "get it from <URL> and drop it in <DISTDIR>" instructions. The phase not being defined (or its own failure) never masks the fetch error.
- Portage `"distlocks" in self.settings.features` -- same env-var-not-full-config-resolution shortcut `collision_protect`/`protect_owned`/`unmerge_orphans` already use, defaulting to Portage `true` (see `FetchOptions::distlocks`'s own doc comment).
- Portage `FEATURES=force-mirror` -- same env-var shortcut as `distlocks` above (no full config resolution on this path), defaulting to Portage `false`.
- Portage `fetch.py:1652-1700`'s command family out of the same settings object the phases use.
- Portage `mysettings.get("PORTAGE_SSH_OPTS")` (`fetch.py:1806`).

## fn distfile_names
- The distfile names `src_uri` lists once each, in first-seen order, for the `flag?` groups `active` keeps (Portage `_parse_uri_map`'s keys). The one place both `AA` (every group) and `A` (the phase `USE`) are read from.

## struct FetchSourcesArgs
- Arguments of [`fetch_sources()`].
