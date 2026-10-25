//! repos.conf discovery and per-repository configuration.

use crate::*;

#[derive(Debug, Clone)]
pub struct RepoConfig {
    pub name: String,
    pub location: PathBuf,
    pub priority: i32,
    /// Whether this is `repos.conf`'s `[DEFAULT] main-repo` -- needed by
    /// callers that read repo-level config keyed by repo identity.
    pub is_main: bool,
    /// Resolved `masters` chain, in declaration order, as absolute repo
    /// locations. An explicit `repos.conf` `masters =` wins; else
    /// `layout.conf`'s; else the main repo alone (empty for the main repo
    /// itself). An unknown master name is silently dropped.
    /// Portage: repository/config.py, RepoConfigLoader.
    pub masters: Vec<PathBuf>,
    /// `metadata/layout.conf`'s `profile-formats`, verbatim. Portuale acts
    /// on one value only: `portage-2` allows a profile `parent` line's
    /// cross-repo syntax; absent means disallowed (a documented
    /// simplification).
    /// Portage: LocationsManager's `_allow_parent_colon` gate.
    pub profile_formats: Vec<String>,
    /// The repo's resolved `cache-formats`: `layout.conf`'s value lowercased
    /// and split, or -- when empty -- auto-detected from directory presence
    /// (`md5-dict` first, then `pms`). See [`pregen_md5_cache_enabled`].
    /// Portage: config.py, parse_layout.
    pub cache_formats: Vec<String>,
    /// This repo's `aliases`: `layout.conf`'s first, then `repos.conf`'s.
    /// Portuale acts on aliases only in `find_repos`' section-name-vs-
    /// repo-name mismatch escape hatch; NOT in `::`-atom matching or
    /// profile parents (a documented cut).
    /// Portage: repository/config.py.
    pub aliases: Vec<String>,
    /// `repos.conf`'s `sync-type` (user value wins over the global
    /// `/usr/share/portage/config/repos.conf`). Display-only
    /// (`emerge --info`'s `Repositories:` block).
    pub sync_type: Option<String>,
    /// `repos.conf`'s `sync-uri`, merged the same way. Display-only.
    pub sync_uri: Option<String>,
    /// `repos.conf`'s `volatile`: the explicit `true`/`yes` value, else
    /// "volatile unless under `/var/db/repos` and owned by
    /// `root`/`portage`". Display-only.
    pub volatile: bool,
    /// Sync-module-specific `repos.conf` options that are set, as ordered
    /// `(key, value)` pairs. Display-only.
    /// Portage: RepoConfig.module_specific_options.
    pub module_specific_options: Vec<(String, String)>,
}

/// Whether `path`'s owning uid is `root` (0) or the `portage` user --
/// the ownership half of the `volatile` heuristic. Best-effort: the
/// `portage` uid is looked up from `/etc/passwd` (no libc dependency);
/// an unreadable path or passwd yields `false`.
/// Portage: repository/config.py.
#[cfg(unix)]
pub(crate) fn path_owned_by_root_or_portage(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    let uid = meta.uid();
    if uid == 0 {
        return true;
    }
    fs::read_to_string("/etc/passwd")
        .ok()
        .and_then(|passwd| {
            passwd.lines().find_map(|line| {
                let mut f = line.split(':');
                (f.next() == Some("portage"))
                    .then(|| f.nth(1).and_then(|u| u.parse::<u32>().ok()))
                    .flatten()
            })
        })
        .map(|portage_uid| portage_uid == uid)
        .unwrap_or(false)
}

#[cfg(not(unix))]
pub(crate) fn path_owned_by_root_or_portage(_path: &Path) -> bool {
    false
}

/// The sync-module-specific option keys recognized for a given
/// `sync-type`, in a fixed order so `emerge --info`'s `Repositories:`
/// output is deterministic (Portage iterates a set).
pub(crate) fn module_specific_keys(sync_type: &str) -> &'static [&'static str] {
    match sync_type {
        "git" => &[
            "sync-git-clone-env",
            "sync-git-clone-extra-opts",
            "sync-git-env",
            "sync-git-pull-env",
            "sync-git-pull-extra-opts",
            "sync-git-verify-commit-signature",
            "sync-git-verify-max-age-days",
        ],
        "rsync" => &[
            "sync-rsync-extra-opts",
            "sync-rsync-vcs-ignore",
            "sync-rsync-verify-jobs",
            "sync-rsync-verify-max-age",
            "sync-rsync-verify-metamanifest",
        ],
        "webrsync" => &["sync-webrsync-verify-signature"],
        _ => &[],
    }
}

pub(crate) fn parse_ini(text: &str, sections: &mut HashMap<String, HashMap<String, String>>) {
    let mut current: Option<String> = None;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(inner) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            let name = inner.trim().to_string();
            sections.entry(name.clone()).or_default();
            current = Some(name);
            continue;
        }
        if let Some(section_name) = &current
            && let Some(eq) = line.find('=')
        {
            let key = line[..eq].trim().to_string();
            let value = line[eq + 1..].trim().to_string();
            sections
                .get_mut(section_name)
                .expect("section was inserted when its header was parsed")
                .insert(key, value);
        }
    }
}

/// Parses a repo's `metadata/layout.conf` -- a section-less `key = value`
/// file. Returns an empty map when the file is absent. Portuale reads five
/// keys only: `masters`, `repo-name`, `profile-formats`, `cache-formats`,
/// `aliases`.
/// Portage: repository/config.py, parse_layout_conf.
pub(crate) fn parse_layout_conf(repo_location: &Path) -> HashMap<String, String> {
    let Ok(text) = fs::read_to_string(repo_location.join("metadata/layout.conf")) else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(eq) = line.find('=') {
            out.insert(
                line[..eq].trim().to_string(),
                line[eq + 1..].trim().to_string(),
            );
        }
    }
    out
}

/// Reads `<repo_location>/profiles/repo_name`, the first line trimmed.
/// `None` when the file is absent or empty (the caller falls back to the
/// `repos.conf` `[section]` name).
/// Portage: repository/config.py, _read_repo_name.
pub(crate) fn read_repo_name_file(repo_location: &Path) -> Option<String> {
    let text = fs::read_to_string(repo_location.join("profiles/repo_name")).ok()?;
    let name = text.lines().next().unwrap_or("").trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// Portage's installed global `repos.conf`: `GLOBAL_CONFIG_PATH`
/// (`/usr/share/portage/config`) joined to the compile-time `EPREFIX`
/// (`LocationsManager.py:413-417`) -- no environment variable moves it.
/// Portuale has no `EPREFIX` build constant, so this is the bare path.
const GLOBAL_REPOS_CONF: &str = "/usr/share/portage/config/repos.conf";

/// Parses `repos.conf` (a file, or a directory of `*.conf` files merged
/// in sorted-filename order) and returns every `[reponame]` section with
/// a `location`, sorted ascending by `(priority, name)` -- so a tie between
/// two repos providing the identical version breaks toward the
/// higher-priority one.
/// Portage: repository/config.py's `prepos_order`.
pub fn find_repos(config_root: &Path) -> Result<Vec<RepoConfig>, Error> {
    find_repos_impl(config_root, true)
}

/// [`find_repos`] for a process-internal config probe that runs before
/// the CLI's own load: stays quiet, so the run prints the section-name
/// line exactly as often as before the probe existed.
pub fn find_repos_quiet(config_root: &Path) -> Result<Vec<RepoConfig>, Error> {
    find_repos_impl(config_root, false)
}

/// The raw `repos.conf` data of one repository, aligned by index with the
/// repositories being built.
struct SectionInfo {
    section_name: String,
    repos_conf_masters: Option<Vec<String>>,
    repos_conf_aliases: Vec<String>,
}

/// [`find_repos_impl_with`]/[`repo_usepkg_atoms_impl`] read the global
/// file from [`GLOBAL_REPOS_CONF`] and `PORTAGE_REPOSITORIES` from the
/// process environment; unit tests inject both through here instead and
/// stay hermetic. Unless `PORTAGE_REPOSITORIES` is set -- then exactly
/// that text replaces both files.
/// Portage: repository/config.py, load_repository_config.
pub(crate) fn read_repo_sections_impl(
    config_root: &Path,
    global_repos_conf: &Path,
    portage_repositories: Option<String>,
) -> Result<HashMap<String, HashMap<String, String>>, Error> {
    let repos_conf_path = config_root.join("etc/portage/repos.conf");
    let mut sections: HashMap<String, HashMap<String, String>> = HashMap::new();

    if let Some(text) = portage_repositories {
        parse_ini(&text, &mut sections);
        return Ok(sections);
    }

    // The global repos.conf is read *before* the user's, so a user
    // `[section]` key overrides the global one and unset keys fall through.
    // An absent global file contributes nothing (Portage's installed path;
    // the `cnf/repos.conf` fallback is only for a non-installed Portage).
    if global_repos_conf.is_file()
        && let Ok(text) = fs::read_to_string(global_repos_conf)
    {
        parse_ini(&text, &mut sections);
    }

    // A `repos.conf` directory is walked recursively, same as `package.*`.
    // Portage: repository/config.py, RepoConfigLoader._parse.
    if repos_conf_path.symlink_metadata().is_ok() {
        let entries: Vec<PathBuf> = portage_util::recursive_config_files(&repos_conf_path)
            .map_err(|e| Error::ReadFile {
                path: repos_conf_path.display().to_string(),
                source: e,
            })?;
        for path in entries {
            let text = fs::read_to_string(&path).map_err(|e| Error::ReadFile {
                path: path.display().to_string(),
                source: e,
            })?;
            parse_ini(&text, &mut sections);
        }
    } else {
        return Err(Error::NoReposConf {
            path: repos_conf_path.display().to_string(),
        });
    }
    Ok(sections)
}

/// Per-repo `usepkg-exclude` / `usepkg-include` atoms from `repos.conf`, each
/// rewritten to `atom::reponame` (invalid atoms dropped with Portage's
/// warning; an atom in both lists is dropped from both).
/// Portage: repository/config.py:253-282; _emerge/depgraph.py:384-392
pub fn repo_usepkg_atoms(config_root: &Path) -> (Vec<String>, Vec<String>) {
    repo_usepkg_atoms_impl(
        config_root,
        Path::new(GLOBAL_REPOS_CONF),
        std::env::var("PORTAGE_REPOSITORIES").ok(),
    )
}

/// [`repo_usepkg_atoms`] with the global-file path and the
/// `PORTAGE_REPOSITORIES` text injected, so unit tests stay hermetic
/// (see [`read_repo_sections_impl`]).
pub(crate) fn repo_usepkg_atoms_impl(
    config_root: &Path,
    global_repos_conf: &Path,
    portage_repositories: Option<String>,
) -> (Vec<String>, Vec<String>) {
    let Ok(sections) =
        read_repo_sections_impl(config_root, global_repos_conf, portage_repositories)
    else {
        return (Vec::new(), Vec::new());
    };
    let mut names: Vec<&String> = sections.keys().filter(|n| *n != "DEFAULT").collect();
    names.sort();
    // The warnings print once per process: the resolver asks on every pass.
    static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let warn = !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed);
    let (mut exclude, mut include) = (Vec::new(), Vec::new());
    for section in names {
        let kv = &sections[section];
        let Some(location) = kv.get("location") else {
            continue;
        };
        let location = PathBuf::from(location);
        let location = if location.is_absolute() {
            location
        } else {
            config_root.join(location)
        };
        let name = read_repo_name_file(&location).unwrap_or_else(|| section.clone());
        let mut lists: Vec<Vec<String>> = Vec::new();
        for opt in ["usepkg-exclude", "usepkg-include"] {
            let atoms: Vec<String> = kv
                .get(opt)
                .map(|v| v.split_whitespace().map(String::from).collect())
                .unwrap_or_default();
            let (good, bad): (Vec<String>, Vec<String>) =
                atoms.into_iter().partition(|a| usepkg_atom_ok(a));
            if warn && !bad.is_empty() {
                eprintln!(
                    "\n!!! The following atoms are invalid in {opt} attribute for repo [{name}] \
                     (only package names and slot atoms allowed):\n\n    {}\n",
                    bad.join("\n    ")
                );
            }
            lists.push(good);
        }
        let conflicted: Vec<String> = lists[0]
            .iter()
            .filter(|a| lists[1].contains(a))
            .cloned()
            .collect();
        if warn && !conflicted.is_empty() {
            eprintln!(
                "\n!!! The following atoms appear in both the usepkg-exclude \
                 usepkg-include lists for repo [{name}]:\n\n    {}\n",
                conflicted.join("\n    ")
            );
        }
        for (list, out) in lists.iter().zip([&mut exclude, &mut include]) {
            out.extend(
                list.iter()
                    .filter(|a| !conflicted.contains(a))
                    .map(|a| format!("{a}::{name}")),
            );
        }
    }
    (exclude, include)
}

/// Portage `_find_bad_atoms`: a package name or slot atom, wildcards allowed,
/// no operator, blocker, repo or USE dependency.
fn usepkg_atom_ok(atom: &str) -> bool {
    let probe = if atom.split(':').next().unwrap_or(atom).contains('/') {
        atom.to_string()
    } else {
        format!("dummy-category/{atom}")
    };
    let opts = portage_dep::AtomOptions {
        eapi: portage_dep::AtomEapi::None,
        allow_wildcard: true,
        allow_repo: Some(false),
        allow_build_id: None,
    };
    portage_dep::parse_atom_with(&probe, &opts, "").is_ok_and(|a| {
        a.operator == portage_dep::Operator::None
            && a.blocker == portage_dep::Blocker::None
            && a.use_deps.is_none()
    })
}

/// [`find_repos`] without the user-facing section-name-mismatch line, for
/// internal location-keyed lookups: the CLI layers already report a
/// mismatch once, and repeating it on every internal probe is noise.
pub(crate) fn find_repos_impl(
    config_root: &Path,
    report_section_mismatch: bool,
) -> Result<Vec<RepoConfig>, Error> {
    find_repos_impl_with(
        config_root,
        report_section_mismatch,
        Path::new(GLOBAL_REPOS_CONF),
        std::env::var("PORTAGE_REPOSITORIES").ok(),
    )
}

/// [`find_repos_impl`] with the global-file path and the
/// `PORTAGE_REPOSITORIES` text injected, so unit tests stay hermetic
/// (see [`read_repo_sections_impl`]).
pub(crate) fn find_repos_impl_with(
    config_root: &Path,
    report_section_mismatch: bool,
    global_repos_conf: &Path,
    portage_repositories: Option<String>,
) -> Result<Vec<RepoConfig>, Error> {
    let sections = read_repo_sections_impl(config_root, global_repos_conf, portage_repositories)?;

    let main_repo = sections
        .get("DEFAULT")
        .and_then(|d| d.get("main-repo"))
        .ok_or(Error::NoMainRepo)?
        .clone();

    // First pass: one entry per `[section]` with a `location`. `sect` is
    // the parallel raw-data buffer, aligned with `repos` by index; the
    // `layout.conf` overrides and the name-mismatch check come after.
    let mut repos: Vec<RepoConfig> = Vec::new();
    let mut sect: Vec<SectionInfo> = Vec::new();
    for (name, kv) in &sections {
        if name == "DEFAULT" {
            continue;
        }
        let Some(location) = kv.get("location") else {
            // Portage: repository/config.py (missing-attribute drop).
            eprintln!("!!! Section '{name}' in repos.conf is missing location attribute");
            continue;
        };
        let location = PathBuf::from(location);
        // Relative locations are a portuale/testing convenience so the
        // fixture tree can be committed without a machine-specific path.
        let location = if location.is_absolute() {
            location
        } else {
            config_root.join(location)
        };
        // The resolved name first: the missing-directory rule below keeps
        // `gentoo` by name, and a missing directory yields no
        // `profiles/repo_name`, so this is the section name there.
        let resolved_name = read_repo_name_file(&location).unwrap_or_else(|| name.clone());
        if !location.is_dir() {
            // Portage: repository/config.py (nonexistent-directory drop,
            // except `gentoo`, kept so a first sync is possible).
            eprintln!(
                "!!! Section '{name}' in repos.conf has location attribute set to nonexistent directory: '{}'",
                location.display()
            );
            if resolved_name != "gentoo" {
                continue;
            }
        }
        let priority = kv
            .get("priority")
            .and_then(|p| p.parse::<i32>().ok())
            .unwrap_or(if *name == main_repo { -1000 } else { 0 });
        // `profiles/repo_name` first, else the section name (resolved above
        // for the missing-directory rule).
        let sync_type = kv.get("sync-type").filter(|s| !s.is_empty()).cloned();
        let sync_uri = kv.get("sync-uri").filter(|s| !s.is_empty()).cloned();
        // Explicit `true`/`yes`, else the heuristic: volatile unless under
        // `/var/db/repos` and owned by `root`/`portage`.
        let volatile = match kv.get("volatile").map(String::as_str) {
            Some("true") | Some("yes") => true,
            Some(_) => false,
            None => {
                !location.starts_with("/var/db/repos") || !path_owned_by_root_or_portage(&location)
            }
        };
        let module_specific_options: Vec<(String, String)> = sync_type
            .as_deref()
            .map(module_specific_keys)
            .unwrap_or(&[])
            .iter()
            .filter_map(|k| kv.get(*k).map(|v| (k.to_string(), v.clone())))
            .collect();
        repos.push(RepoConfig {
            name: resolved_name,
            location,
            priority,
            is_main: *name == main_repo,
            masters: Vec::new(),
            profile_formats: Vec::new(),
            cache_formats: Vec::new(),
            aliases: Vec::new(),
            sync_type,
            sync_uri,
            volatile,
            module_specific_options,
        });
        sect.push(SectionInfo {
            section_name: name.clone(),
            // `None` = key absent (fall through); `Some([])` = explicit
            // empty key ("no masters").
            repos_conf_masters: kv
                .get("masters")
                .map(|s| s.split_whitespace().map(String::from).collect()),
            repos_conf_aliases: kv
                .get("aliases")
                .map(|s| s.split_whitespace().map(String::from).collect())
                .unwrap_or_default(),
        });
    }

    let mut layout_masters = apply_layout_conf(&mut repos, &sect);

    // A repo whose resolved `name` differs from its `[section]` name is
    // dropped with an error -- unless the section name is one of its
    // aliases (the escape hatch for two enabled copies of one repo).
    // Portage: repository/config.py.
    // NOTE: this `eprintln!` can fire more than once per `--pretend` run
    // (`find_repos` runs both at the CLI layer and inside
    // `resolve_pretend_graph`); internal probes stay quiet.
    let keep: Vec<bool> = repos
        .iter()
        .zip(&sect)
        .map(|(repo, s)| {
            let ok = repo.name == s.section_name || repo.aliases.contains(&s.section_name);
            if !ok && report_section_mismatch {
                eprintln!(
                    "!!! Section '{}' in repos.conf has name different from repository name '{}' set inside repository",
                    s.section_name, repo.name
                );
            }
            ok
        })
        .collect();
    if keep.iter().any(|k| !k) {
        let mut i = 0;
        repos.retain(|_| {
            i += 1;
            keep[i - 1]
        });
        let mut i = 0;
        sect.retain(|_| {
            i += 1;
            keep[i - 1]
        });
        let mut i = 0;
        layout_masters.retain(|_| {
            i += 1;
            keep[i - 1]
        });
    }

    if !repos.iter().any(|r| r.name == main_repo) {
        return Err(Error::NoRepoLocation {
            repo: main_repo.clone(),
        });
    }

    // `masters` resolution, three-tier: explicit `repos.conf` wins, else
    // `layout.conf`, else the implicit default (main repo alone, except
    // for the main repo itself). Needs every repo's final name+location
    // first, hence the snapshot.
    let location_by_name: HashMap<String, PathBuf> = repos
        .iter()
        .map(|r| (r.name.clone(), r.location.clone()))
        .collect();
    let main_repo_location = location_by_name.get(&main_repo).cloned();
    let resolve_names = |names: &[String]| -> Vec<PathBuf> {
        names
            .iter()
            .filter_map(|n| location_by_name.get(n).cloned())
            .collect()
    };
    for (i, repo) in repos.iter_mut().enumerate() {
        repo.masters = match (&sect[i].repos_conf_masters, &layout_masters[i]) {
            (Some(names), _) => resolve_names(names),
            (None, Some(names)) => resolve_names(names),
            (None, None) => {
                if repo.name == main_repo {
                    Vec::new()
                } else {
                    main_repo_location.clone().into_iter().collect()
                }
            }
        };
    }

    repos.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(repos)
}

/// Read each repository's layout.conf: name, aliases, profile formats and masters.
fn apply_layout_conf(repos: &mut [RepoConfig], sect: &[SectionInfo]) -> Vec<Option<Vec<String>>> {
    // Second pass: each repo's `layout.conf` (`repo-name` overrides the
    // name; `aliases` prepend before the `repos.conf` ones;
    // `profile-formats` captured; `layout.conf` `masters` is the middle tier).
    // Portage: repository/config.py.
    let mut layout_masters: Vec<Option<Vec<String>>> = Vec::with_capacity(repos.len());
    for (repo, s) in repos.iter_mut().zip(sect) {
        let layout = parse_layout_conf(&repo.location);
        repo.profile_formats = layout
            .get("profile-formats")
            .map(|v| v.split_whitespace().map(String::from).collect())
            .unwrap_or_default();
        // Lowercase the whole value, split, then -- only when empty --
        // auto-detect `md5-dict` before `pms` from directory presence. An
        // explicit unknown format stays in the list.
        let mut cache_formats: Vec<String> = layout
            .get("cache-formats")
            .map(|v| {
                v.to_lowercase()
                    .split_whitespace()
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        if cache_formats.is_empty() {
            if repo.location.join("metadata/md5-cache").is_dir() {
                cache_formats.push("md5-dict".to_string());
            }
            if repo.location.join("metadata/cache").is_dir() {
                cache_formats.push("pms".to_string());
            }
        }
        repo.cache_formats = cache_formats;
        let mut aliases: Vec<String> = layout
            .get("aliases")
            .map(|v| v.split_whitespace().map(String::from).collect())
            .unwrap_or_default();
        aliases.extend(s.repos_conf_aliases.iter().cloned());
        repo.aliases = aliases;
        if let Some(new_name) = layout.get("repo-name").filter(|v| !v.is_empty()) {
            repo.name = new_name.clone();
        }
        layout_masters.push(
            layout
                .get("masters")
                .map(|v| v.split_whitespace().map(String::from).collect()),
        );
    }
    layout_masters
}
