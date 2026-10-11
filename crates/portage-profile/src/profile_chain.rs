//! The profile chain: parent files and per-profile make.defaults.

use crate::*;

/// Processes one file's lines against the shared scalar/USE/ACCEPT_KEYWORDS
/// state, without any `source` support (used for make.defaults; make.conf
/// wraps this with `source` handling -- see `process_make_conf_file`).
pub(crate) fn process_lines(
    text: &str,
    scalars: &mut HashMap<String, String>,
    config: &mut Config,
) {
    for line in logical_lines(text) {
        let Some((key, raw_value)) = parse_kv_line(&line) else {
            continue;
        };
        let value = substitute(&raw_value, scalars);
        note_incremental(config, key, &value);
        note_higher(config, key);
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

/// Arguments of [`expand_parent_colon`].
pub(crate) struct ExpandParentColonArgs<'a> {
    pub(crate) parent: &'a str,
    pub(crate) current_repo: Option<&'a (String, PathBuf)>,
    pub(crate) repos: &'a [(String, PathBuf)],
    pub(crate) repo_aliases: &'a [(String, PathBuf)],
    pub(crate) parents_file: &'a Path,
    pub(crate) parent_colon_repos: &'a HashSet<String>,
}

/// Expands a profile `parent` line's cross-repo `:path`/`reponame:path`
/// syntax to `<repo_location>/profiles/<rest>`. A line with no `:` (the
/// plain relative-path form) is returned unchanged. A `:` line whose
/// current repo does not declare `portage-2` is returned unchanged too:
/// the `:` becomes a literal path component failing downstream, as in
/// Portage.
// Portage: LocationsManager._expand_parent_colon
pub(crate) fn expand_parent_colon(args: ExpandParentColonArgs<'_>) -> Result<String, Error> {
    let ExpandParentColonArgs {
        parent,
        current_repo,
        repos,
        repo_aliases,
        parents_file,
        parent_colon_repos,
    } = args;
    let Some(colon) = parent.find(':') else {
        return Ok(parent.to_string());
    };
    // `allow_parent_colon` defaults true and is only overridden when the
    // current node intersects a known repo; a node outside any repo keeps
    // the permissive default.
    let allowed = match current_repo {
        None => true,
        Some((name, _)) => parent_colon_repos.contains(name),
    };
    if !allowed {
        return Ok(parent.to_string());
    }
    let repo_loc = if colon == 0 {
        &current_repo
            .ok_or_else(|| Error::ParentNotFoundOutsideRepos {
                parent: parent.to_string(),
                parents_file: parents_file.display().to_string(),
            })?
            .1
    } else {
        let repo_name = &parent[..colon];
        // Canonical names and aliases share one namespace (an alias never
        // shadows a canonical name), so check `repos` first, then aliases.
        repos
            .iter()
            .chain(repo_aliases.iter())
            .find(|(name, _)| name == repo_name)
            .map(|(_, loc)| loc)
            .ok_or_else(|| Error::ParentNotFoundUnknownRepo {
                parent: parent.to_string(),
                parents_file: parents_file.display().to_string(),
                repo_name: repo_name.to_string(),
            })?
    };
    Ok(repo_loc
        .join("profiles")
        .join(&parent[colon + 1..])
        .to_string_lossy()
        .into_owned())
}

/// Recursively resolves the profile inheritance chain starting at `leaf`,
/// ancestors before descendants (parents in the order given),
/// cycle/diamond-safe via a visited set on the canonicalized directory.
/// `repos` is only needed for cross-repo `parent` entries (see
/// `expand_parent_colon`); a chain with none never consults it.
pub(crate) fn resolve_profile_chain(
    leaf: &Path,
    repos: &[(String, PathBuf)],
    repo_aliases: &[(String, PathBuf)],
    parent_colon_repos: &HashSet<String>,
) -> Result<Vec<PathBuf>, Error> {
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut chain: Vec<PathBuf> = Vec::new();
    visit_profile(VisitProfileArgs {
        dir: leaf,
        repos,
        repo_aliases,
        parent_colon_repos,
        visited: &mut visited,
        chain: &mut chain,
    })?;
    Ok(chain)
}
/// Arguments of [`visit_profile`].
pub(crate) struct VisitProfileArgs<'a> {
    pub(crate) dir: &'a Path,
    pub(crate) repos: &'a [(String, PathBuf)],
    pub(crate) repo_aliases: &'a [(String, PathBuf)],
    pub(crate) parent_colon_repos: &'a HashSet<String>,
    pub(crate) visited: &'a mut HashSet<PathBuf>,
    pub(crate) chain: &'a mut Vec<PathBuf>,
}

pub(crate) fn visit_profile(args: VisitProfileArgs<'_>) -> Result<(), Error> {
    let VisitProfileArgs {
        dir,
        repos,
        repo_aliases,
        parent_colon_repos,
        visited,
        chain,
    } = args;
    let canon = dir.canonicalize().map_err(|e| Error::ResolveProfile {
        path: dir.display().to_string(),
        source: e,
    })?;
    if !visited.insert(canon.clone()) {
        return Ok(());
    }
    let current_repo = repo_containing(&canon, repos);
    let parents_file = canon.join("parent");
    for parent in read_parent_lines(&canon)? {
        let expanded = expand_parent_colon(ExpandParentColonArgs {
            parent: &parent,
            current_repo: current_repo.as_ref(),
            repos,
            repo_aliases,
            parents_file: &parents_file,
            parent_colon_repos,
        })?;
        visit_profile(VisitProfileArgs {
            dir: &canon.join(&expanded),
            repos,
            repo_aliases,
            parent_colon_repos,
            visited,
            chain,
        })?;
    }
    chain.push(canon);
    Ok(())
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
