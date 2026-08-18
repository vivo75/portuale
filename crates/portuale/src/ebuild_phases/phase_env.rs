//! The environment variables and PATH a phase runs with.

use crate::ebuild_phases::*;

/// The env block every phase and every `misc-functions.sh` `__dyn_*` command needs,
/// as raw pairs. `Brush` formats them into `export` source (`phase_setup_script`);
/// `Bash` passes them as subprocess env (no quoting step, no expansion risk).
/// Shared by `run_one_phase`/`run_misc_functions`; `extra_env` appends verbatim.
/// `PORTAGE_ECLASS_LOCATIONS`: shell-single-quoted path tokens (as Portage's
/// `eclass_locations_string` builds them) for `inherit()` to search — own repo
/// first, then masters in declared order. Resolution failure degrades to the
/// containing repo alone; outside a repo the value is empty (nothing to look in).
/// Portage: config.py, eclass_cache.py, bin/ebuild.sh
pub(crate) fn eclass_locations_value(pkg_dir: &Path, config_root: &Path) -> String {
    let Some(repo_root) = repo_root_for(pkg_dir) else {
        return String::new();
    };

    let masters: Vec<PathBuf> = (|| -> Option<Vec<PathBuf>> {
        let repos = portage_repo::find_repos(config_root).ok()?;
        let repo = repos.iter().find(|r| r.location == repo_root)?;
        Some(repo.masters.clone())
    })()
    .unwrap_or_default();

    let mut locations = masters;
    if !locations.contains(&repo_root) {
        locations.push(repo_root);
    }
    locations.reverse();

    locations
        .iter()
        .map(|p| format!("'{}'", p.display().to_string().replace('\'', r"'\''")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `PORTAGE_PYTHON`: the `portuale-python` shim, or `/usr/bin/python` under the
/// oracle handle.
pub(crate) fn portage_python_value(bin_dir: &Path) -> String {
    if std::env::var("PORTUALE_PYTHON_HELPERS").as_deref() == Ok("real") {
        "/usr/bin/python".to_string()
    } else {
        bin_dir.join("portuale-python").display().to_string()
    }
}

/// Trailing `/` on `D`/`ED`/`ROOT`/`EROOT` only for EAPI 6 and below; later EAPIs
/// strip it (so `ROOT=/` exports `""`). `value` arrives with its slash.
/// Portage: config.py, eapi.py
pub(crate) fn eapi_path_var(eapi: &str, value: &str) -> String {
    let trailing = matches!(eapi, "0" | "1" | "2" | "3" | "4" | "5" | "6");
    let base = value.trim_end_matches('/');
    if trailing {
        format!("{base}/")
    } else {
        base.to_string()
    }
}

/// Helper dir first, then the base `PATH` entries; the `xattr` dir goes in front
/// under `FEATURES=xattr`. The base is the threaded config `PATH` when present
/// (only when `env.d` sets it), else the process env.
/// Portage: doebuild.py, _doebuild_path
pub(crate) fn phase_path(
    helpers_dir: &Path,
    extra_env: &[(String, String)],
    features: &str,
) -> String {
    let base = extra_env
        .iter()
        .rev()
        .find(|(k, _)| k == "PATH")
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| std::env::var("PATH").unwrap_or_default());
    let helpers = helpers_dir.display().to_string();
    let xattr = helpers_dir.join("xattr").display().to_string();
    let mut parts = Vec::new();
    if feature_token_present(features, "xattr") {
        parts.push(xattr.as_str());
    }
    parts.push(helpers.as_str());
    let mut seen: std::collections::HashSet<&str> = parts.iter().copied().collect();
    for p in base.split(':') {
        if seen.insert(p) {
            parts.push(p);
        }
    }
    parts.join(":")
}

/// A POSIX single-quoted word: everything inside is literal; an embedded `'` is
/// the `'\''` dance. Used for every brush `export` value.
pub(crate) fn shell_single_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for c in value.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}
