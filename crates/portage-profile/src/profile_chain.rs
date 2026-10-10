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
/// via the longest matching location prefix -- mirrors real
/// `LocationsManager._addProfile`'s own `intersecting_repos`/
/// `max(key=len)` logic, needed to resolve a same-repo `:path` profile
/// parent shorthand. Repo locations are canonicalized before comparing
/// (falling back to the raw path if that fails, e.g. a repo location
/// that doesn't exist on disk in a test fixture -- it simply won't match
/// any real profile dir either way), since `dir` itself is always
/// already canonicalized by `visit_profile`.
/// The `profile-formats` token list from a repo's own
/// `metadata/layout.conf` (real `parse_layout_conf`), or an empty list
/// when the file/key is absent. A tiny dedicated reader: `portage-profile`
/// can't depend on `portage-repo` (where the fuller `parse_layout_conf`
/// lives), and this is the only `layout.conf` key this crate needs --
/// the same "duplicate a small, stable chunk rather than share across a
/// dependency edge that only runs one way" precedent `keyword_masked_
/// only`/`is_visible` already set in `portage-repo`.
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

/// Expands a profile `parent` file line's real cross-repo `:path`/
/// `reponame:path` syntax (`LocationsManager._expand_parent_colon`):
/// a `:` with nothing before it means "this same repo" (`current_repo`),
/// anything else before the `:` is another repo's own name, looked up in
/// `repos`. Both forms expand to `<repo_location>/profiles/<rest>`. A
/// line with no `:` at all (the plain relative-path form) is returned
/// unchanged. Real portage only allows this syntax when the *current*
/// profile node's own repo declares `profile-formats = portage-2` in
/// `layout.conf` (real `_config/LocationsManager.py:47`/`259`'s own
/// `_allow_parent_colon = frozenset(["portage-2"])` gate) --
/// `parent_colon_repos` is the set of repo names that do (built from
/// `portage_repo::RepoConfig::profile_formats` at the CLI boundary). A
/// `parent` line containing a `:` whose current repo isn't in that set
/// is returned *unchanged*, matching real portage: the `:` then becomes
/// a literal path component that fails to resolve downstream exactly as
/// it would there.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn expand_parent_colon(
    parent: &str,
    current_repo: Option<&(String, PathBuf)>,
    repos: &[(String, PathBuf)],
    repo_aliases: &[(String, PathBuf)],
    parents_file: &Path,
    parent_colon_repos: &HashSet<String>,
) -> Result<String, Error> {
    let Some(colon) = parent.find(':') else {
        return Ok(parent.to_string());
    };
    // Real `if not abs_parent and allow_parent_colon:` gate
    // (`_config/LocationsManager.py:207`/`259`): `allow_parent_colon`
    // defaults `True` and is only *overridden* when the current profile
    // node intersects a known repo -- so a node outside any repo keeps
    // the permissive default, and a node inside a repo is gated on that
    // repo declaring `portage-2` in `layout.conf`.
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
        // Real `repositories.get_location_for_name`: canonical names and
        // aliases share one namespace (an alias never shadows a
        // canonical name -- real `config.py`'s own alias-registration
        // loop skips an already-taken name), so check `repos` first,
        // then `repo_aliases`.
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
/// ancestors before descendants (parents listed in a level's `parent`
/// file are visited in the order given), cycle/diamond-safe via a visited
/// set keyed on the canonicalized directory. `repos` (main + every
/// overlay, name + location) is only needed to resolve a cross-repo
/// `parent` entry (see `expand_parent_colon`); a chain with none never
/// consults it.
pub(crate) fn resolve_profile_chain(
    leaf: &Path,
    repos: &[(String, PathBuf)],
    repo_aliases: &[(String, PathBuf)],
    parent_colon_repos: &HashSet<String>,
) -> Result<Vec<PathBuf>, Error> {
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut chain: Vec<PathBuf> = Vec::new();
    visit_profile(
        leaf,
        repos,
        repo_aliases,
        parent_colon_repos,
        &mut visited,
        &mut chain,
    )?;
    Ok(chain)
}

#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn visit_profile(
    dir: &Path,
    repos: &[(String, PathBuf)],
    repo_aliases: &[(String, PathBuf)],
    parent_colon_repos: &HashSet<String>,
    visited: &mut HashSet<PathBuf>,
    chain: &mut Vec<PathBuf>,
) -> Result<(), Error> {
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
        let expanded = expand_parent_colon(
            &parent,
            current_repo.as_ref(),
            repos,
            repo_aliases,
            &parents_file,
            parent_colon_repos,
        )?;
        visit_profile(
            &canon.join(&expanded),
            repos,
            repo_aliases,
            parent_colon_repos,
            visited,
            chain,
        )?;
    }
    chain.push(canon);
    Ok(())
}

/// Every `USE=` value from a repo's top-level `profiles/make.defaults`
/// (real `config.py`'s `_repo_make_defaults`), in file order, each
/// `${VAR}`-substituted against `scalars`. A missing file yields an
/// empty list. `source` is not handled (a repo `make.defaults` never
/// uses it); `${USE}` self-reference resolves to whatever `make.conf`'s
/// `USE=` last set (a documented approximation -- real portage's own
/// `_repo_make_defaults` expand context differs).
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
