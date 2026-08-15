//! The profile chain: parent files and per-profile make.defaults.

use crate::*;

pub(crate) fn read_parent_lines(profile_dir: &Path) -> Result<Vec<String>, Error> {
    let parent_path = profile_dir.join("parent");
    if !parent_path.is_file() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&parent_path).map_err(|e| Error::ReadFile {
        path: parent_path.display().to_string(),
        source: e,
    })?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect())
}

/// Finds which of `repos` (each `(name, location)`) `dir` lives inside,
/// via the longest matching location prefix. Repo locations are
/// canonicalized before comparing (falling back to the raw path), since
/// `dir` is always already canonicalized by `visit_profile`.
/// The `profile-formats` token list from a repo's `metadata/layout.conf`,
/// or empty when the file/key is absent. A tiny dedicated reader: this
/// crate cannot depend on `portage-repo`, where the fuller parser lives.
// Portage: parse_layout_conf
pub(crate) fn repo_profile_formats(repo_location: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(repo_location.join("metadata/layout.conf")) else {
        return Vec::new();
    };
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=')
            && key.trim() == "profile-formats"
        {
            return value.split_whitespace().map(String::from).collect();
        }
    }
    Vec::new()
}

/// Finds which of `repos` (each `(name, location)`) `dir` lives inside,
/// via the longest matching location prefix.
// Portage: LocationsManager._addProfile (intersecting_repos, max by len)
pub(crate) fn repo_containing(
    dir: &Path,
    repos: &[(String, PathBuf)],
) -> Option<(String, PathBuf)> {
    repos
        .iter()
        .filter_map(|(name, loc)| {
            let canon_loc = loc.canonicalize().unwrap_or_else(|_| loc.clone());
            dir.starts_with(&canon_loc)
                .then_some((name.clone(), canon_loc))
        })
        .max_by_key(|(_, loc)| loc.as_os_str().len())
}
