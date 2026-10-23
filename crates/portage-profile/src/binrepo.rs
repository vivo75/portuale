//! binrepos.conf entries for `--getbinpkg` resolution.

use crate::*;

/// One `--getbinpkg` binary-package repository.
// Portage: binrepo/config.py, BinRepoConfig
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BinRepo {
    /// The `[section]` name, or the `md5(uri)` hex-digest fallback for an
    /// implicit `PORTAGE_BINHOST` entry.
    pub name: String,
    /// `sync-uri` with any trailing `/` stripped.
    pub sync_uri: String,
    /// `priority` (`int(priority)`, `0` on parse failure or absence).
    pub priority: i32,
    /// `location`: explicit value or `None`. Display-only (`emerge --info`).
    pub location: Option<String>,
    /// `verify-signature`: an absent key means `true` — stock Portage
    /// ships `[DEFAULT] verify-signature = true` in its global
    /// `binrepos.conf` (`cnf/binrepos.conf` in the checkout,
    /// `/usr/share/portage/config/binrepos.conf` installed), which every
    /// section inherits, and `BinRepoConfig` keeps `None` (falsy only where
    /// a tri-state is read; the GPG layer and `--info` treat it as true).
    pub verify_signature: bool,
    /// `frozen` (default `false`): cached `Packages` used as-is, never
    /// refreshed.
    pub frozen: bool,
    /// `fetchcommand`: per-repo override from `binrepos.conf` (`BinRepoConfig`).
    pub fetchcommand: Option<String>,
    /// `resumecommand`: per-repo override from `binrepos.conf` (`BinRepoConfig`).
    pub resumecommand: Option<String>,
    /// `getbinpkg-exclude`: validated remote atoms (bad ones dropped with
    /// Portage's warning at parse time). Folded with the CLI set per
    /// refresh; local `PKGDIR` instances are never filtered.
    pub getbinpkg_exclude: Vec<String>,
    /// `getbinpkg-include`: validated remote atoms (same rule). A
    /// non-empty effective set is a whitelist.
    pub getbinpkg_include: Vec<String>,
}

impl BinRepo {
    /// This binrepo's on-disk `Packages` index dir under `root`. A
    /// missing index means "no candidates" (`--pretend` never fetches).
    // Portage: bintree.py, _populate_remote
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

/// Portage `_emerge/is_valid_package_atom.py:19-27`
/// (`insert_category_into_atom`): splice `category/` before the first
/// alphanumeric/`_`/`*` character (ASCII). `None` when the string holds
/// none (the caller then validates the string as-is, which fails).
fn insert_category_into_atom(atom: &str, category: &str) -> Option<String> {
    let pos = atom
        .char_indices()
        .find(|(_, c)| *c == '*' || c.is_ascii_alphanumeric() || *c == '_')
        .map(|(i, _)| i)?;
    Some(format!("{}{}/{}", &atom[..pos], category, &atom[pos..]))
}

/// Portage `portage/repository/config.py:91-116` (`_find_bad_atoms`,
/// strict mode): the bad entries of a `getbinpkg-exclude/include` value,
/// in order. Only package names and slot atoms are allowed: any
/// operator, blocker, use dep or repo form is bad (build-ids only ever
/// parse on a `=` operator, so the operator rule covers them).
fn find_bad_getbinpkg_atoms(value: &str) -> Vec<&str> {
    use portage_dep::{AtomEapi, AtomOptions, Blocker, Operator, parse_atom_with};
    let mut bad = Vec::new();
    for x in value.split_whitespace() {
        let probe: String = if x.split(':').next().unwrap_or(x).contains('/') {
            x.to_string()
        } else {
            match insert_category_into_atom(x, "dummy-category") {
                Some(inserted) => inserted,
                None => x.to_string(),
            }
        };
        let opts = AtomOptions {
            eapi: AtomEapi::None,
            allow_wildcard: true,
            allow_repo: Some(false),
            allow_build_id: None,
        };
        let is_bad = match parse_atom_with(&probe, &opts, "") {
            Err(_) => true,
            Ok(atom) => {
                atom.operator != Operator::None
                    || atom.blocker != Blocker::None
                    || atom.use_deps.is_some()
            }
        };
        if is_bad {
            bad.push(x);
        }
    }
    bad
}

/// One section's `getbinpkg-exclude/include` values, validated: bad
/// atoms are dropped with Portage's warning, then atoms in both lists
/// are dropped from both with its warning (both keep file order).
// Portage: binrepo/config.py, BinRepoConfig.__init__
fn validated_getbinpkg_keys(
    name: &str,
    raw_exclude: &str,
    raw_include: &str,
    warnings: &mut Vec<ProfileWarning>,
) -> (Vec<String>, Vec<String>) {
    let mut exclude: Vec<String> = Vec::new();
    let mut include: Vec<String> = Vec::new();
    for (raw, opt) in [
        (raw_exclude, "getbinpkg-exclude"),
        (raw_include, "getbinpkg-include"),
    ] {
        let bad = find_bad_getbinpkg_atoms(raw);
        if !bad.is_empty() {
            warnings.push(ProfileWarning {
                rank: WARN_RANK_REPO,
                text: format!(
                    "\n!!! The following atoms are invalid in {opt} attribute for \
                     binrepo [{name}] (only package names and slot atoms allowed):\n\n    {}",
                    bad.join("\n    ")
                ),
            });
            let bad_set: std::collections::HashSet<&str> = bad.into_iter().collect();
            let kept: Vec<String> = raw
                .split_whitespace()
                .filter(|a| !bad_set.contains(a))
                .map(str::to_string)
                .collect();
            if opt == "getbinpkg-exclude" {
                exclude = kept;
            } else {
                include = kept;
            }
        } else if opt == "getbinpkg-exclude" {
            exclude = raw.split_whitespace().map(str::to_string).collect();
        } else {
            include = raw.split_whitespace().map(str::to_string).collect();
        }
    }
    let in_both: Vec<String> = {
        let include_set: std::collections::HashSet<&str> =
            include.iter().map(String::as_str).collect();
        let mut seen = std::collections::HashSet::new();
        exclude
            .iter()
            .filter(|a| include_set.contains(a.as_str()) && seen.insert(a.as_str()))
            .cloned()
            .collect()
    };
    if !in_both.is_empty() {
        warnings.push(ProfileWarning {
            rank: WARN_RANK_REPO,
            text: format!(
                "\n!!! The following atoms appear in both the getbinpkg-exclude \
                 getbinpkg-include lists for binrepo [{name}]:\n\n    {}",
                in_both.join("\n    ")
            ),
        });
        let both: std::collections::HashSet<&str> = in_both.iter().map(String::as_str).collect();
        exclude.retain(|a| !both.contains(a.as_str()));
        include.retain(|a| !both.contains(a.as_str()));
    }
    (exclude, include)
}

/// Parses `binrepos.conf`, appends implicit `PORTAGE_BINHOST` entries,
/// and returns the repos sorted by `(priority, name)`.
/// `getbinpkg-exclude/include` keys are validated like Portage's
/// `BinRepoConfig` (bad atoms dropped with its warning; atoms in both of
/// one repo's lists dropped from both with its warning); the texts ride
/// `warnings` for the config-load position on stderr.
// Portage: binrepo/config.py, BinRepoConfigLoader
pub(crate) fn parse_binrepos(
    binrepos_conf: &str,
    portage_binhost: &str,
    warnings: &mut Vec<ProfileWarning>,
) -> Vec<BinRepo> {
    let normalize = |u: &str| u.trim_end_matches('/').to_string();
    let mut repos: Vec<BinRepo> = Vec::new();
    let mut seen_uris: std::collections::HashSet<String> = std::collections::HashSet::new();

    // INI sections.
    let mut section: Option<String> = None;
    let mut sync_uri: Option<String> = None;
    let mut priority: i32 = 0;
    let mut location: Option<String> = None;
    // Absent means true: stock Portage ships `[DEFAULT] verify-signature
    // = true` in the global binrepos.conf, inherited by every section.
    let mut verify_signature = true;
    let mut frozen = false;
    let mut fetchcommand: Option<String> = None;
    let mut resumecommand: Option<String> = None;
    let mut raw_exclude = String::new();
    let mut raw_include = String::new();
    #[allow(clippy::type_complexity, clippy::too_many_arguments)]
    let flush = |section: &mut Option<String>,
                 sync_uri: &mut Option<String>,
                 priority: &mut i32,
                 location: &mut Option<String>,
                 verify_signature: &mut bool,
                 frozen: &mut bool,
                 fetchcommand: &mut Option<String>,
                 resumecommand: &mut Option<String>,
                 raw_exclude: &mut String,
                 raw_include: &mut String,
                 repos: &mut Vec<BinRepo>,
                 seen: &mut std::collections::HashSet<String>,
                 warnings: &mut Vec<ProfileWarning>| {
        if let (Some(name), Some(uri)) = (section.take(), sync_uri.take()) {
            let uri = uri.trim_end_matches('/').to_string();
            seen.insert(uri.clone());
            // Portage skips the key validation for a `DEFAULT` section
            // (its keys are defaults, not a repo); the section is dropped
            // below either way.
            let (getbinpkg_exclude, getbinpkg_include) = if name == "DEFAULT" {
                (Vec::new(), Vec::new())
            } else {
                validated_getbinpkg_keys(&name, raw_exclude, raw_include, warnings)
            };
            repos.push(BinRepo {
                name,
                sync_uri: uri,
                priority: *priority,
                location: location.take(),
                verify_signature: *verify_signature,
                frozen: *frozen,
                fetchcommand: fetchcommand.take(),
                resumecommand: resumecommand.take(),
                getbinpkg_exclude,
                getbinpkg_include,
            });
        }
        *priority = 0;
        *location = None;
        *verify_signature = true;
        *frozen = false;
        *fetchcommand = None;
        *resumecommand = None;
        raw_exclude.clear();
        raw_include.clear();
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
                &mut fetchcommand,
                &mut resumecommand,
                &mut raw_exclude,
                &mut raw_include,
                &mut repos,
                &mut seen_uris,
                warnings,
            );
            section = Some(inner.trim().to_string());
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            match k.trim() {
                // `${VAR}` expands from the environment (relocatable fixtures).
                "sync-uri" => sync_uri = Some(substitute(v.trim(), &HashMap::new())),
                "priority" => priority = v.trim().parse().unwrap_or(0),
                "location" => location = Some(substitute(v.trim(), &HashMap::new())),
                "fetchcommand" => fetchcommand = Some(v.trim().to_string()),
                "resumecommand" => resumecommand = Some(v.trim().to_string()),
                "verify-signature" => {
                    // Portage: `in ("true", "yes")` only.
                    verify_signature =
                        matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "yes")
                }
                "frozen" => {
                    // Portage `_bool_opts`: `in ("true", "yes")` only.
                    frozen = matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "yes")
                }
                // One section may repeat a key; Portage's config parser
                // keeps the last, so later lines overwrite.
                "getbinpkg-exclude" => raw_exclude = v.trim().to_string(),
                "getbinpkg-include" => raw_include = v.trim().to_string(),
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
        &mut fetchcommand,
        &mut resumecommand,
        &mut raw_exclude,
        &mut raw_include,
        &mut repos,
        &mut seen_uris,
        warnings,
    );
    repos.retain(|r| r.name != "DEFAULT");

    // Implicit PORTAGE_BINHOST entries, reversed with incrementing priority.
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
                // No `<EPREFIX>/var/cache/binhost/<name>` fallback with `PORTAGE_BINHOST`.
                location: None,
                // Implicit entries carry no keys (Portage reads them from
                // the config sections only): the global `[DEFAULT]` still
                // applies, so true.
                verify_signature: true,
                frozen: false,
                fetchcommand: None,
                resumecommand: None,
                // Implicit entries carry no keys (Portage reads them from
                // the config sections only).
                getbinpkg_exclude: Vec::new(),
                getbinpkg_include: Vec::new(),
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
