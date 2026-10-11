//! `package.env` matching and its incremental variables.

use crate::ebuild_phases::*;

/// Portage's `INCREMENTALS`: appended by `_grab_pkg_env`, folded by `regenerate()`.
/// Kept complete even though the gate below drops most, so a future gate change
/// can't silently lose semantics.
/// Portage: const.py, config.py
pub(crate) const PACKAGE_ENV_INCREMENTALS: [&str; 12] = [
    "ACCEPT_KEYWORDS",
    "CONFIG_PROTECT",
    "CONFIG_PROTECT_MASK",
    "ENV_UNSET",
    "FEATURES",
    "IUSE_IMPLICIT",
    "PROFILE_ONLY_VARIABLES",
    "USE",
    "USE_EXPAND",
    "USE_EXPAND_HIDDEN",
    "USE_EXPAND_IMPLICIT",
    "USE_EXPAND_UNPREFIXED",
];

pub(crate) fn is_package_env_incremental(key: &str) -> bool {
    PACKAGE_ENV_INCREMENTALS.contains(&key)
}

/// Per-key acceptance gate: `USE` (folded via `Config::package_env_use`),
/// `ENV_BLACKLIST`, `ENVIRON_FILTER` (a phase-env-only layer, dropped here),
/// computed keys the phase runner owns, and profile-only variables are rejected.
/// Everything else is accepted, including empty values and `__` names. The
/// compression family (`BINPKG_COMPRESS[_FLAGS[_<NAME>]]`) is exempted.
/// Portage: config.py, _grab_pkg_env
pub(crate) fn package_env_key_allowed(key: &str, profile_only_variables: &[String]) -> bool {
    // Only the compression family is exempt (see fn doc); `PORTAGE_BZIP2_COMMAND`
    // is listed for symmetry (not on Portage's filter, so it exempts nothing).
    fn is_compression_key(key: &str) -> bool {
        key == "BINPKG_COMPRESS"
            || key == "BINPKG_COMPRESS_FLAGS"
            || key == "PORTAGE_BZIP2_COMMAND"
            || key
                .strip_prefix("BINPKG_COMPRESS_FLAGS_")
                .is_some_and(|rest| !rest.is_empty())
    }
    !key.is_empty()
        && key != "USE"
        && !portage_profile::ENV_BLACKLIST.contains(&key)
        && !(portage_profile::ENVIRON_FILTER.contains(&key) && !is_compression_key(key))
        && !portage_profile::PORTUALE_COMPUTED.contains(&key)
        && !profile_only_variables.iter().any(|k| k == key)
}

/// One key's incremental fold in layer order `[base-lower, pkg, calling-env]`:
/// `-*` clears, `-tok` removes; sorted and space-joined. The calling env folds
/// last, so its `-tok` prunes package.env tokens but not vice versa; re-applying
/// is idempotent, so `base_lower` may be the already-folded value.
/// Portage: config.py, regenerate
pub(crate) fn fold_package_env_incremental(
    base_lower: &str,
    pkg: &str,
    calling_env: &str,
) -> String {
    let mut set: std::collections::BTreeSet<String> =
        base_lower.split_whitespace().map(String::from).collect();
    for tok in pkg.split_whitespace().chain(calling_env.split_whitespace()) {
        if tok == "-*" {
            set.clear();
        } else if let Some(rest) = tok.strip_prefix('-') {
            set.remove(rest);
        } else {
            set.insert(tok.to_string());
        }
    }
    set.into_iter().collect::<Vec<_>>().join(" ")
}

/// Atom-matches `package_env_vars` against one `cpv_slot` string: files of one
/// entry in order, matching entries in list order; an incremental appends, a
/// scalar replaces, an empty value blanks. Incrementals fold in layer order; a
/// scalar is dropped when the calling environment carries the same key (the
/// process value wins). Shared by the merge and standalone paths, which thread
/// the calling environment so the precedence holds on both.
/// Portage: config.py, _grab_pkg_env
pub(crate) fn match_package_env_vars(
    package_env_vars: &[(String, Vec<(String, String)>)],
    cpv_slot: &str,
    profile_only_variables: &[String],
    base_env: &[(String, String)],
    calling_env: &[(String, String)],
) -> Vec<(String, String)> {
    let mut container: Vec<(String, String)> = Vec::new();
    for (atom, vars) in package_env_vars {
        if !portage_dep::match_from_list(atom, &[cpv_slot]).is_some_and(|m| !m.is_empty()) {
            continue;
        }
        for (k, v) in vars {
            if !package_env_key_allowed(k, profile_only_variables) {
                continue;
            }
            if is_package_env_incremental(k) {
                match container.iter_mut().rev().find(|(ck, _)| ck == k) {
                    Some((_, current)) => {
                        if !current.is_empty() && !v.is_empty() {
                            current.push(' ');
                        }
                        current.push_str(v);
                    }
                    None => container.push((k.clone(), v.clone())),
                }
            } else {
                container.retain(|(ck, _)| ck != k);
                container.push((k.clone(), v.clone()));
            }
        }
    }
    container
        .into_iter()
        .filter_map(|(k, v)| {
            if is_package_env_incremental(&k) {
                let base = base_env
                    .iter()
                    .rev()
                    .find(|(bk, _)| *bk == k)
                    .map(|(_, bv)| bv.as_str())
                    .unwrap_or("");
                let calling = calling_env
                    .iter()
                    .rev()
                    .find(|(ck, _)| *ck == k)
                    .map(|(_, cv)| cv.as_str())
                    .unwrap_or("");
                Some((k, fold_package_env_incremental(base, &v, calling)))
            } else if calling_env.iter().any(|(ck, _)| *ck == k) {
                // The process value wins for a scalar: drop the package.env value.
                None
            } else {
                Some((k, v))
            }
        })
        .collect()
}

/// One `package.env` key, raw: scalars replace (last match wins), incrementals
/// append — matching entries in list order, files of one entry in order. No
/// acceptance gate, no folding: for per-key recomputes a flat pair would corrupt.
/// Portage: config.py, _grab_pkg_env
pub(crate) fn match_package_env_scalar_raw(
    package_env_vars: &[(String, Vec<(String, String)>)],
    cpv_slot: &str,
    key: &str,
) -> Option<String> {
    let mut matched: Option<&str> = None;
    for (atom, vars) in package_env_vars {
        if !portage_dep::match_from_list(atom, &[cpv_slot]).is_some_and(|m| !m.is_empty()) {
            continue;
        }
        for (k, v) in vars {
            if k == key {
                matched = Some(v.as_str());
            }
        }
    }
    matched.map(String::from)
}

pub(crate) fn match_package_env_incremental_raw(
    package_env_vars: &[(String, Vec<(String, String)>)],
    cpv_slot: &str,
    key: &str,
) -> String {
    let mut container = String::new();
    for (atom, vars) in package_env_vars {
        if !portage_dep::match_from_list(atom, &[cpv_slot]).is_some_and(|m| !m.is_empty()) {
            continue;
        }
        for (k, v) in vars {
            if k == key {
                if !container.is_empty() && !v.is_empty() {
                    container.push(' ');
                }
                container.push_str(v);
            }
        }
    }
    container
}

/// The `Config` and `cpv_slot` identity for an ebuild with no resolved graph entry
/// (standalone runs): the path gives `category`/`pf`, the md5-cache gives `SLOT`.
/// `None` outside a repo or on an unparsable path; shared so path-parsing happens once.
pub(crate) fn standalone_package_env_lookup(
    ebuild_path: &Path,
    config_root: &Path,
    eroot: &Path,
) -> Option<(portage_profile::Config, String)> {
    let ebuild_abs = ebuild_path.canonicalize().ok()?;
    let pkg_dir = ebuild_abs.parent()?.to_path_buf();
    let file_name = ebuild_abs.file_name()?.to_str()?;
    let pf = file_name.strip_suffix(".ebuild")?;
    let category = pkg_dir.parent()?.file_name()?.to_str()?;
    let config = resolve_standalone_config(&pkg_dir, config_root, eroot)?;
    let cpv_slot = standalone_cpv_slot(&pkg_dir, category, pf)?;
    Some((config, cpv_slot))
}
