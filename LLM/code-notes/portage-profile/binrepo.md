# crates/portage-profile/src/binrepo.rs
`binrepos.conf` plus implicit `PORTAGE_BINHOST` entries for `--getbinpkg` resolution.

## struct BinRepo
- Narrowed to what a `--pretend` resolution needs: no fetch/resume commands, no signature-verification config, no `getbinpkg-exclude`/`-include`.
- `name`: Portage's shipped `cnf/binrepos.conf` sets `[DEFAULT] verify-signature = true`, so portuale defaults to `true` (it does not parse that global `[DEFAULT]` itself) and lets a section's own `verify-signature = false` override.
- `sync_uri`: Portage `_normalize_uri` (trailing `/` stripped).
- `priority`: Portage sorts a `None` priority as `0`.
- `location`: Portage fills a `<EPREFIX>/var/cache/binhost/<name>` fallback only when `PORTAGE_BINHOST` is unset; portuale mirrors that.
- `frozen`: a frozen binrepo's cached `Packages` index is used as-is, never refreshed (Portage `bintree._populate_remote_repo` raises `UseCachedCopyOfRemoteIndex("frozen")`, backlog #192).

## impl BinRepo::packages_dir
- Portage `bintree._populate_remote`'s own `pkgindex_file` (`bintree.py:1496-1504`).

## fn parse_binrepos
- Portage `BinRepoConfigLoader` (`lib/portage/binrepo/config.py:97-172`), narrowed: `sync-uri`, `priority`, `location`, `verify-signature` and `frozen` are read.
- Implicit entries iterate `PORTAGE_BINHOST` in reverse with an incrementing priority ("Convert PORTAGE_BINHOST entries into implicit binrepos.conf ones"); each URI is `_normalize_uri`'d.
- Documented narrowings: the implicit-entry name is `md5(uri)` in Portage; portuale has no md5, so it uses the URI's own `host/path` (still stable and unique per URI, only ever surfaced as a sort key here). `[DEFAULT]` interpolation, `getbinpkg-exclude`/`-include`, `fetchcommand`/`resumecommand`, signature-verification settings, and the `location =` fallback for a section without `PORTAGE_BINHOST` are all out (none affect a `--pretend` resolution).
- `${VAR}` in `sync-uri` expands from the environment (portuale convenience Portage's configparser lacks) so a fixture can use `file://${PORTAGE_CONFIGROOT}/binhost` instead of an absolute, non-relocatable path.
- `frozen` is a Portage `binrepo/config.py` `_bool_opts` member with `"false"` default: a frozen binrepo is never refreshed.
