//! binrepos.conf entries.

use crate::*;

/// One `--getbinpkg` binary-package repository (real
/// `portage.binrepo.config.BinRepoConfig`, narrowed to what a
/// `--pretend` resolution needs: no fetch/resume commands, no
/// signature-verification config, no `getbinpkg-exclude`/`-include` --
/// those gate an actual download, which `--pretend` never does).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinRepo {
    /// The `[section]` name, or -- for an implicit `PORTAGE_BINHOST`
    /// entry -- the real `md5(uri)` hex-digest fallback.
    pub name: String,
    /// `sync-uri`, real `_normalize_uri` (trailing `/` stripped).
    pub sync_uri: String,
    /// `priority` (real `int(priority)`, or `0` on parse failure /
    /// absence -- real sorts a `None` priority as `0`).
    pub priority: i32,
    /// `location` (real `BinRepoConfig.location`): the explicit
    /// `binrepos.conf` value, or `None`. Real fills a
    /// `<EPREFIX>/var/cache/binhost/<name>` fallback only when
    /// `PORTAGE_BINHOST` is unset; portuale mirrors that. Display-only
    /// (surfaced by `emerge --info`'s `Binary Repositories:` block).
    pub location: Option<String>,
    /// `verify-signature` (real `BinRepoConfig.verify_signature`, a
    /// `_bool_opts` member). Real's shipped `cnf/binrepos.conf` sets
    /// `[DEFAULT] verify-signature = true`, so portuale defaults to
    /// `true` (it does not parse that global `[DEFAULT]` itself) and lets
    /// a section's own `verify-signature = false` override.
    pub verify_signature: bool,
    /// `frozen` (real `BinRepoConfig.frozen`, a `_bool_opts` member,
    /// default `"false"`): a frozen binrepo's cached `Packages` index is
    /// used as-is, never refreshed (real `bintree._populate_remote_repo`
    /// raises `UseCachedCopyOfRemoteIndex("frozen")`, backlog #192).
    pub frozen: bool,
}

impl BinRepo {
    /// The directory holding this binrepo's own `Packages` index on disk
    /// under `root` (real `bintree._populate_remote`'s own
    /// `pkgindex_file`, `bintree.py:1496-1504`):
    /// `<EROOT>/var/cache/edb/binhost/<host>/<url-path>/` for an
    /// `http(s)://`/`ssh://` `sync-uri`; the URI's own path for a
    /// `file://` URI or a bare filesystem path. `--pretend` never
    /// fetches, so a missing index just means "this binrepo contributes
    /// no candidates".
    pub fn packages_dir(&self, root: &std::path::Path) -> std::path::PathBuf {
        let uri = &self.sync_uri;
        if let Some(rest) = uri.strip_prefix("file://") {
            return std::path::PathBuf::from(rest);
        }
        for scheme in ["https://", "http://", "ssh://"] {
            if let Some(rest) = uri.strip_prefix(scheme) {
                let (hostport, path) = rest.split_once('/').unwrap_or((rest, ""));
                let host = hostport.split(':').next().unwrap_or(hostport);
                return root
                    .join("var/cache/edb/binhost")
                    .join(host)
                    .join(path.trim_matches('/'));
            }
        }
        std::path::PathBuf::from(uri)
    }
}

/// Real `BinRepoConfigLoader` (`lib/portage/binrepo/config.py:97-172`),
/// narrowed. Parses `binrepos.conf`'s own `[section]` / `key = value`
/// INI (`sync-uri`, `priority`, `location`, `verify-signature` and
/// `frozen` are read), then appends one
/// implicit `BinRepo` per whitespace-separated `PORTAGE_BINHOST` URI
/// that isn't already a section's `sync-uri` (real "Convert
/// PORTAGE_BINHOST entries into implicit binrepos.conf ones", iterated in
/// reverse with an incrementing `priority`). Each URI is
/// `_normalize_uri`'d (trailing `/` stripped). Returns the repos sorted
/// by `(priority, name)` -- real `sorted(repos, key=lambda repo:
/// (repo.priority or 0, repo.name or repo.name_fallback))`.
///
/// **Documented narrowings**: the implicit-entry name is `md5(uri)` in
/// real portage; portuale has no md5, so it uses the URI's own
/// `host/path` (still stable and unique per URI, only ever surfaced as a
/// sort key here). `[DEFAULT]` interpolation, `getbinpkg-exclude`/
/// `-include`, `fetchcommand`/`resumecommand`, signature-verification
/// settings, and the `location =` fallback for a section without
/// `PORTAGE_BINHOST` are all out (none affect a `--pretend` resolution).
pub(crate) fn parse_binrepos(binrepos_conf: &str, portage_binhost: &str) -> Vec<BinRepo> {
    let normalize = |u: &str| u.trim_end_matches('/').to_string();
    let mut repos: Vec<BinRepo> = Vec::new();
    let mut seen_uris: std::collections::HashSet<String> = std::collections::HashSet::new();

    // INI sections.
    let mut section: Option<String> = None;
    let mut sync_uri: Option<String> = None;
    let mut priority: i32 = 0;
    let mut location: Option<String> = None;
    let mut verify_signature = true;
    let mut frozen = false;
    #[allow(clippy::type_complexity)]
    let flush = |section: &mut Option<String>,
                 sync_uri: &mut Option<String>,
                 priority: &mut i32,
                 location: &mut Option<String>,
                 verify_signature: &mut bool,
                 frozen: &mut bool,
                 repos: &mut Vec<BinRepo>,
                 seen: &mut std::collections::HashSet<String>| {
        if let (Some(name), Some(uri)) = (section.take(), sync_uri.take()) {
            let uri = uri.trim_end_matches('/').to_string();
            seen.insert(uri.clone());
            repos.push(BinRepo {
                name,
                sync_uri: uri,
                priority: *priority,
                location: location.take(),
                verify_signature: *verify_signature,
                frozen: *frozen,
            });
        }
        *priority = 0;
        *location = None;
        *verify_signature = true;
        *frozen = false;
    };
    for line in binrepos_conf.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(inner) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            flush(
                &mut section,
                &mut sync_uri,
                &mut priority,
                &mut location,
                &mut verify_signature,
                &mut frozen,
                &mut repos,
                &mut seen_uris,
            );
            section = Some(inner.trim().to_string());
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            match k.trim() {
                // `${VAR}` in `sync-uri` expands from the environment
                // (portuale convenience -- real configparser doesn't -- so a
                // fixture can use `file://${PORTAGE_CONFIGROOT}/binhost`
                // instead of an absolute, non-relocatable path).
                "sync-uri" => sync_uri = Some(substitute(v.trim(), &HashMap::new())),
                "priority" => priority = v.trim().parse().unwrap_or(0),
                "location" => location = Some(substitute(v.trim(), &HashMap::new())),
                "verify-signature" => {
                    verify_signature = matches!(
                        v.trim().to_ascii_lowercase().as_str(),
                        "true" | "yes" | "1" | "on"
                    )
                }
                // Real `binrepo/config.py` `_bool_opts` (`"frozen":
                // "false"` default): a frozen binrepo is never refreshed.
                "frozen" => {
                    frozen = matches!(
                        v.trim().to_ascii_lowercase().as_str(),
                        "true" | "yes" | "1" | "on"
                    )
                }
                _ => {}
            }
        }
    }
    flush(
        &mut section,
        &mut sync_uri,
        &mut priority,
        &mut location,
        &mut verify_signature,
        &mut frozen,
        &mut repos,
        &mut seen_uris,
    );
    repos.retain(|r| r.name != "DEFAULT");

    // Implicit PORTAGE_BINHOST entries (real: reversed, incrementing priority).
    let mut current_priority = 0;
    for uri in portage_binhost.split_whitespace().rev() {
        let uri = normalize(uri);
        if seen_uris.insert(uri.clone()) {
            current_priority += 1;
            let name = uri
                .split_once("://")
                .map(|(_, rest)| rest.to_string())
                .unwrap_or_else(|| uri.clone());
            repos.push(BinRepo {
                name,
                sync_uri: uri,
                priority: current_priority,
                // Real: with `PORTAGE_BINHOST` set, the
                // `<EPREFIX>/var/cache/binhost/<name>` location fallback
                // is skipped -- an implicit entry keeps `location = None`.
                location: None,
                verify_signature: true,
                frozen: false,
            });
        }
    }

    repos.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| a.name.cmp(&b.name))
    });
    repos
}
