//! The profile chain: parent files and per-profile make.defaults.

use crate::*;

/// Processes one file's lines against the shared scalar/USE/ACCEPT_KEYWORDS
/// state, without any `source` support (used for make.defaults; make.conf
/// wraps this with `source` handling -- see `process_make_conf_file`).
/// `accept_layer` tags the caller's file layer for the `Packages`-index
/// header's `ACCEPT_*` stacking (order-preserving, unlike the resolver's
/// last-wins scalars).
pub(crate) fn process_lines(
    text: &str,
    scalars: &mut HashMap<String, String>,
    config: &mut Config,
    accept_layer: AcceptRawLayer,
) {
    for line in logical_lines(text) {
        let Some((key, raw_value)) = parse_kv_line(&line) else {
            continue;
        };
        let value = substitute(&raw_value, scalars);
        note_incremental(config, key, &value);
        note_higher(config, key);
        if matches!(
            key,
            "ACCEPT_LICENSE" | "ACCEPT_PROPERTIES" | "ACCEPT_RESTRICT"
        ) {
            config.accept_raw_layers.push((
                accept_layer,
                key.to_string(),
                value.split_whitespace().map(String::from).collect(),
            ));
        }
        match key {
            "USE" => {
                apply_use_incremental(&value, &mut config.use_flags);
                config.use_tokens.push(value.clone());
            }
            "ACCEPT_KEYWORDS" => apply_incremental(&value, &mut config.accept_keywords),
            "USE_EXPAND" => apply_incremental(&value, &mut config.use_expand),
            "USE_EXPAND_UNPREFIXED" => apply_incremental(&value, &mut config.use_expand_unprefixed),
            "USE_EXPAND_IMPLICIT" => apply_incremental(&value, &mut config.use_expand_implicit),
            "IUSE_IMPLICIT" => apply_incremental(&value, &mut config.iuse_implicit),
            "USE_EXPAND_HIDDEN" => apply_incremental(&value, &mut config.use_expand_hidden),
            _ => {}
        }
        scalars.insert(key.to_string(), value);
    }
}

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

/// Every `USE=` value from a repo's top-level `profiles/make.defaults`,
/// in file order, each `${VAR}`-substituted against `scalars`. A missing
/// file yields an empty list. `source` is not handled.
pub(crate) fn read_repo_make_defaults_use(
    path: &Path,
    scalars: &HashMap<String, String>,
) -> Vec<String> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let (key, raw_value) = parse_kv_line(line.trim())?;
            (key == "USE").then(|| substitute(&raw_value, scalars))
        })
        .collect()
}
