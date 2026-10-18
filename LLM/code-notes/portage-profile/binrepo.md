# crates/portage-profile/src/binrepo.rs
`binrepos.conf` plus implicit `PORTAGE_BINHOST` entries for `--getbinpkg` resolution.

## struct BinRepo
- Narrowed to what a `--pretend` resolution needs: no fetch/resume commands, no signature-verification config.
- `name`: Portage's shipped `cnf/binrepos.conf` sets `[DEFAULT] verify-signature = true`, so portuale defaults to `true` (it does not parse that global `[DEFAULT]` itself) and lets a section's own `verify-signature = false` override.
- `sync_uri`: Portage `_normalize_uri` (trailing `/` stripped).
- `priority`: Portage sorts a `None` priority as `0`.
- `location`: Portage fills a `<EPREFIX>/var/cache/binhost/<name>` fallback only when `PORTAGE_BINHOST` is unset; portuale mirrors that.
- `frozen`: a frozen binrepo's cached `Packages` index is used as-is, never refreshed (Portage `bintree._populate_remote_repo` raises `UseCachedCopyOfRemoteIndex("frozen")`, backlog #192).
- One `--getbinpkg` binary-package repository (Portage `portage.binrepo.config.BinRepoConfig`, narrowed to what a `--pretend` resolution needs: no fetch/resume commands, no signature-verification config).
- field `location`: `location` (Portage `BinRepoConfig.location`): the explicit `binrepos.conf` value, or `None`. Portage fills a `<EPREFIX>/var/cache/binhost/<name>` fallback only when `PORTAGE_BINHOST` is unset; portuale mirrors that. Display-only (surfaced by `emerge --info`'s `Binary Repositories:` block).
- field `verify_signature`: `verify-signature` (Portage `BinRepoConfig.verify_signature`, a `_bool_opts` member). Portage's shipped `cnf/binrepos.conf` sets `[DEFAULT] verify-signature = true`, so portuale defaults to `true` (it does not parse that global `[DEFAULT]` itself) and lets a section's own `verify-signature = false` override.
- field `frozen`: `frozen` (Portage `BinRepoConfig.frozen`, a `_bool_opts` member, default `"false"`): a frozen binrepo's cached `Packages` index is used as-is, never refreshed (Portage `bintree._populate_remote_repo` raises `UseCachedCopyOfRemoteIndex("frozen")`, backlog #192).
- field `getbinpkg_exclude` / `getbinpkg_include`: the section's own `getbinpkg-exclude/include` keys (Portage `BinRepoConfig`, backlog #347 S9, Q7 default yes), validated at parse time (bad atoms dropped with Portage's invalid-atoms warning, atoms in both lists dropped from both with its lists warning); folded with the CLI sets per refresh, never applied to local `PKGDIR` instances.
- `packages_dir`: The directory holding this binrepo's own `Packages` index on disk under `root` (Portage `bintree._populate_remote`'s own `pkgindex_file`, `bintree.py:1496-1504`): `<EROOT>/var/cache/edb/binhost/<host>/<url-path>/` for an `http(s)://`/`ssh://` `sync-uri`; the URI's own path for a `file://` URI or a bare filesystem path. `--pretend` never fetches, so a missing index just means "this binrepo contributes no candidates".

## impl BinRepo::packages_dir
- Portage `bintree._populate_remote`'s own `pkgindex_file` (`bintree.py:1496-1504`).

## fn parse_binrepos
- Portage `BinRepoConfigLoader` (`lib/portage/binrepo/config.py:97-172`), narrowed: `sync-uri`, `priority`, `location`, `verify-signature`, `frozen` and the `getbinpkg-exclude/include` keys are read (backlog #347 S9).
- Implicit entries iterate `PORTAGE_BINHOST` in reverse with an incrementing priority ("Convert PORTAGE_BINHOST entries into implicit binrepos.conf ones"); each URI is `_normalize_uri`'d.
- Documented narrowings: the implicit-entry name is `md5(uri)` in Portage; portuale has no md5, so it uses the URI's own `host/path` (still stable and unique per URI, only ever surfaced as a sort key here). `[DEFAULT]` interpolation, `fetchcommand`/`resumecommand`, signature-verification settings, and the `location =` fallback for a section without `PORTAGE_BINHOST` are all out (none affect a `--pretend` resolution).
- `${VAR}` in `sync-uri` expands from the environment (portuale convenience Portage's configparser lacks) so a fixture can use `file://${PORTAGE_CONFIGROOT}/binhost` instead of an absolute, non-relocatable path.
- `frozen` is a Portage `binrepo/config.py` `_bool_opts` member with `"false"` default: a frozen binrepo is never refreshed.
- Portage `BinRepoConfigLoader` (`lib/portage/binrepo/config.py:97-172`), narrowed. Parses `binrepos.conf`'s own `[section]` / `key = value` INI (`sync-uri`, `priority`, `location`, `verify-signature` and `frozen` are read), then appends one implicit `BinRepo` per whitespace-separated `PORTAGE_BINHOST` URI that isn't already a section's `sync-uri` (Portage "Convert PORTAGE_BINHOST entries into implicit binrepos.conf ones", iterated in reverse with an incrementing `priority`). Each URI is `_normalize_uri`'d (trailing `/` stripped). Returns the repos sorted by `(priority, name)` -- Portage `sorted(repos, key=lambda repo: (repo.priority or 0, repo.name or repo.name_fallback))`.
