//! repos.conf discovery and per-repository configuration.

use crate::*;

#[derive(Debug, Clone)]
pub struct RepoConfig {
    pub name: String,
    pub location: PathBuf,
    pub priority: i32,
    /// Whether this is `repos.conf`'s `[DEFAULT] main-repo` -- needed by
    /// callers that read repo-level config keyed by repo identity (e.g.
    /// `portage_profile::resolve_config`'s own `main_repo_name` param,
    /// which scopes the main repo's own `package.mask`/`.unmask`/
    /// `package.use` family with `::reponame` the same way every
    /// overlay's own entries already are -- an overlay's own repo-level
    /// config is fully in scope now, not deliberately excluded; see that
    /// crate's own module doc comment for the overlay-scoping follow-ups
    /// that closed this out).
    pub is_main: bool,
    /// Resolved `masters` chain, in real declaration order, as absolute
    /// repo locations rather than names (the only thing `portage_profile::
    /// resolve_config`'s own masters-stacking actually needs). Real
    /// `config.py`'s own three-tier resolution
    /// (`RepoConfigLoader.__init__`, `lib/portage/repository/config.py:
    /// 237-245`/`484-490`), ported in `find_repos`: an explicit
    /// `repos.conf` `masters =` key wins outright; else the repo's own
    /// `metadata/layout.conf` `masters =` key (an empty one is a real
    /// "no masters", distinct from the key being absent); else the
    /// implicit default -- the main repo alone for every other repo,
    /// empty for the main repo itself (it can never be its own master).
    /// An unknown master name is silently dropped at every tier (real
    /// `config.py` only logs a warning, never a hard error).
    pub masters: Vec<PathBuf>,
    /// `metadata/layout.conf`'s own `profile-formats` list (real
    /// `parse_layout_conf`, `config.py:1516`), verbatim -- empty when
    /// `layout.conf` is absent or the key isn't set. The one value this
    /// portuale acts on is `portage-2`: real `LocationsManager`'s own
    /// `_allow_parent_colon = frozenset(["portage-2"])` gate
    /// (`_config/LocationsManager.py:47`/`259`) -- a profile `parent`
    /// line's `reponame:path`/`:path` cross-repo syntax is only expanded
    /// for a profile node whose own repo declares `portage-2` here.
    /// Threaded into `portage_profile::resolve_config` as the set of
    /// repo names that allow it. Real portage's EAPI-conditional default
    /// when the key is absent (`portage-1`/`portage-1-compat`) is *not*
    /// modeled -- absent simply means "no `portage-2`", i.e.
    /// colon-parent syntax disallowed for that repo (a documented
    /// simplification; every real Gentoo repo that uses the syntax
    /// declares it explicitly).
    pub profile_formats: Vec<String>,
    /// The repo's resolved `cache-formats` (real `parse_layout`,
    /// `config.py:1564-1577`): `layout.conf`'s value lowercased and
    /// split, or -- when that is empty -- auto-detected from directory
    /// presence, `md5-dict` first, then `pms`. Real
    /// `iter_pregenerated_caches` (`config.py:576-605`) yields a
    /// database per **known** name (`pms` -> `metadata/cache`,
    /// `md5-dict` -> `metadata/md5-cache`) in list order, ignoring
    /// unknown names, and `get_pregenerated_cache` (`config.py:607-616`)
    /// keeps only the **first** one -- so downstream, see
    /// [`pregen_md5_cache_enabled`]. Empty when neither directory exists
    /// and the key is unset.
    pub cache_formats: Vec<String>,
    /// This repo's own `aliases` (real `config.py:216-224`/`492-499`):
    /// `layout.conf`'s `aliases =` first, then `repos.conf`'s appended.
    /// Portuale acts on aliases in exactly one place -- the
    /// section-name-vs-repo-name mismatch escape hatch in `find_repos`
    /// (real `config.py:1121`: a repo whose resolved `name` differs from
    /// its `repos.conf` `[section]` name is dropped with an error
    /// *unless* the section name is one of its aliases). Deliberately
    /// NOT wired into `::alias` atom matching or `alias:path` profile
    /// parents -- a documented cut; `::`-constrained atoms and
    /// cross-repo parents both use the canonical name only.
    pub aliases: Vec<String>,
    /// `repos.conf`'s `sync-type` (real `RepoConfig.sync_type`) -- the
    /// merged value across the global `/usr/share/portage/config/repos.conf`
    /// and the user `<config_root>/etc/portage/repos.conf` (user wins).
    /// Display-only (surfaced by `emerge --info`'s `Repositories:` block).
    pub sync_type: Option<String>,
    /// `repos.conf`'s `sync-uri` (real `RepoConfig.sync_uri`), merged the
    /// same way. Display-only.
    pub sync_uri: Option<String>,
    /// `repos.conf`'s `volatile` (real `RepoConfig.volatile`): the
    /// explicit `true`/`yes` value, else real's heuristic -- `True`
    /// unless the location is under `/var/db/repos` and owned by
    /// `root`/`portage` (a repo the user is likely to hand-edit is
    /// "volatile"). Display-only.
    pub volatile: bool,
    /// Sync-module-specific `repos.conf` options that are set (real
    /// `RepoConfig.module_specific_options`, populated from
    /// `portage.sync.module_specific_options(repo)` filtered to keys the
    /// section actually carries), as ordered `(key, value)` pairs.
    /// Display-only.
    pub module_specific_options: Vec<(String, String)>,
}

/// Whether `path`'s owning uid is `root` (0) or the `portage` user --
/// the ownership half of real `RepoConfig`'s `volatile` heuristic
/// (`config.py:445`). Best-effort: `portage`'s uid is looked up from
/// `/etc/passwd` (no libc dependency); an unreadable path or passwd
/// yields `false`, matching real's own "fail safe -> volatile" default.
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

/// The sync-module-specific `repos.conf` option keys real portage
/// recognizes for a given `sync-type` (`portage.sync.module_specific_
/// options`), in a fixed order so `emerge --info`'s `Repositories:`
/// output is deterministic (real iterates a set).
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

/// Parses a repo's own `metadata/layout.conf` (real `parse_layout_conf`,
/// `lib/portage/repository/config.py:1516`) -- a section-less `key =
/// value` file. Returns an empty map when the file is absent (every key
/// portuale reads has a real "absent" default). Portuale reads
/// exactly five keys -- `masters`, `repo-name`, `profile-formats`,
/// `cache-formats`, `aliases` -- out of the ~20 real ones
/// (`sign-manifests`, `manifest-hashes`, `eapis-banned`,
/// `use-manifests`, ...); the rest are real but out of portuale's scope.
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

/// Reads `<repo_location>/profiles/repo_name` (real `_read_repo_name`,
/// `config.py:670-688`), the first line trimmed. `None` when the file is
/// absent or empty -- real portage falls back to `"x-" + basename` +
/// `missing = True`, but portuale's own fallback (the `repos.conf`
/// `[section]` name) is applied by the caller, so a missing file is
/// simply `None` here.
pub(crate) fn read_repo_name_file(repo_location: &Path) -> Option<String> {
    let text = fs::read_to_string(repo_location.join("profiles/repo_name")).ok()?;
    let name = text.lines().next().unwrap_or("").trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// Parses `repos.conf` (a file, or -- as on portuale's dev machine -- a
/// directory of `*.conf` files merged in sorted-filename order) and
/// returns every `[reponame]` section that has a `location` (the main
/// repo plus any overlays), sorted ascending by `(priority, name)` --
/// matching real portage's own `prepos_order` (see
/// `lib/portage/repository/config.py`), which is also the order
/// `list_candidates` below iterates them in, so a tie between two repos
/// providing the identical version is broken toward the higher-priority
/// one.
pub fn find_repos(config_root: &Path) -> Result<Vec<RepoConfig>, Error> {
    find_repos_impl(config_root, true)
}

/// [`find_repos`] for a process-internal config probe that runs before
/// the CLI's own load (the VDB backend's make.conf lookup, #318): no
/// "Section ... has name different from repository name" line, so the
/// run prints it exactly as often as before the probe existed.
pub fn find_repos_quiet(config_root: &Path) -> Result<Vec<RepoConfig>, Error> {
    find_repos_impl(config_root, false)
}

/// [`find_repos`] without the user-facing "Section ... has name different
/// from repository name" line, for the internal location-keyed lookups
/// (`repo_config_for_location` -- and therefore `repo_masters_for_location`
/// -- and `metadata_transfer_in_config`): the CLI layers already report a
/// mismatch once, and repeating it on every internal config probe is pure
/// noise (the pre-existing note at the `eprintln` site).
pub(crate) fn find_repos_impl(
    config_root: &Path,
    report_section_mismatch: bool,
) -> Result<Vec<RepoConfig>, Error> {
    let repos_conf_path = config_root.join("etc/portage/repos.conf");
    let mut sections: HashMap<String, HashMap<String, String>> = HashMap::new();

    // Real `RepoConfigLoader` reads the global
    // `<PORTAGE_BASE_PATH>/cnf/repos.conf` (installed at
    // `/usr/share/portage/config/repos.conf`) *before* the user's, so a
    // user `[section]` key overrides the global one and unset keys fall
    // through (e.g. the shipped `[gentoo]` `sync-git-verify-commit-
    // signature = true`). Read `config_root`-relative, which resolves to
    // that absolute path for a live `PORTAGE_CONFIGROOT=/` run and stays
    // absent under a test / fixture `config_root`.
    let global_repos_conf = config_root.join("usr/share/portage/config/repos.conf");
    if global_repos_conf.is_file()
        && let Ok(text) = fs::read_to_string(&global_repos_conf)
    {
        parse_ini(&text, &mut sections);
    }

    // Real `RepoConfigLoader._parse` reads every path through
    // `_recursive_file_list` (`repository/config.py:943-950`): a
    // `repos.conf` directory is walked recursively with the same
    // file/dir exclusion and in-place ordering as `package.*`.
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

    let main_repo = sections
        .get("DEFAULT")
        .and_then(|d| d.get("main-repo"))
        .ok_or(Error::NoMainRepo)?
        .clone();

    // First pass: one entry per `[section]` with a `location`. `name` is
    // resolved now to `profiles/repo_name` (real `_read_repo_name`) when
    // present, else the section name -- the `layout.conf` `repo-name`
    // override and the section-name-vs-resolved-name mismatch check both
    // come after. `sect` is the parallel raw-data buffer (section name,
    // `repos.conf` `masters`/`aliases`), aligned with `repos` by index.
    struct SectionInfo {
        section_name: String,
        repos_conf_masters: Option<Vec<String>>,
        repos_conf_aliases: Vec<String>,
    }
    let mut repos: Vec<RepoConfig> = Vec::new();
    let mut sect: Vec<SectionInfo> = Vec::new();
    for (name, kv) in &sections {
        if name == "DEFAULT" {
            continue;
        }
        let Some(location) = kv.get("location") else {
            continue;
        };
        let location = PathBuf::from(location);
        // Real repos.conf always uses absolute locations; relative ones
        // are a portuale/testing convenience so the fixture tree under
        // fixtures can be committed without a machine-specific
        // absolute path baked in.
        let location = if location.is_absolute() {
            location
        } else {
            config_root.join(location)
        };
        let priority = kv
            .get("priority")
            .and_then(|p| p.parse::<i32>().ok())
            .unwrap_or(if *name == main_repo { -1000 } else { 0 });
        // Real name resolution: `profiles/repo_name` file first, else
        // the section name.
        let resolved_name = read_repo_name_file(&location).unwrap_or_else(|| name.clone());
        let sync_type = kv.get("sync-type").filter(|s| !s.is_empty()).cloned();
        let sync_uri = kv.get("sync-uri").filter(|s| !s.is_empty()).cloned();
        // Real `RepoConfig.volatile` (config.py:423): explicit `true`/
        // `yes`, else the heuristic -- volatile unless the tree is under
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
            // `None` = key absent (fall through to `layout.conf`);
            // `Some([])` = an explicit empty key (real "no masters").
            repos_conf_masters: kv
                .get("masters")
                .map(|s| s.split_whitespace().map(String::from).collect()),
            repos_conf_aliases: kv
                .get("aliases")
                .map(|s| s.split_whitespace().map(String::from).collect())
                .unwrap_or_default(),
        });
    }

    // Second pass: each repo's own `layout.conf`. `repo-name` overrides
    // the name (real `config.py:500-505`); `aliases` are prepended
    // before the `repos.conf` ones (real `config.py:492-499`);
    // `profile-formats` is captured; `layout.conf` `masters` is the
    // middle tier.
    let mut layout_masters: Vec<Option<Vec<String>>> = Vec::with_capacity(repos.len());
    for (repo, s) in repos.iter_mut().zip(&sect) {
        let layout = parse_layout_conf(&repo.location);
        repo.profile_formats = layout
            .get("profile-formats")
            .map(|v| v.split_whitespace().map(String::from).collect())
            .unwrap_or_default();
        // Real `parse_layout` (`config.py:1564-1577`): lowercase the
        // whole value, split, then -- only when empty -- auto-detect
        // `md5-dict` before `pms` from directory presence. An explicit
        // unknown format stays in the list (real keeps it and
        // `iter_pregenerated_caches` skips it).
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

    // Real `config.py:1121-1136`: a repo whose resolved `name` differs
    // from its own `repos.conf` `[section]` name is dropped with an
    // error -- unless the section name is one of its aliases (the real
    // escape hatch for deliberately having two enabled copies of one
    // repo). Ported faithfully, including the drop (not a soft warning).
    // NOTE: this `eprintln!` can fire more than once per `--pretend` run
    // -- `find_repos` is called both at the CLI layer (to build
    // `resolve_config`'s inputs) and again inside `resolve_pretend_graph`;
    // a pre-existing double-call, harmless except for the repeated line.
    // Internal probes (`find_repos_impl(.., false)`) stay quiet so the
    // #55 config lookups do not add to the repeats.
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

    // `masters` resolution (real three-tier, `config.py:237-245`/
    // `484-490`): an explicit `repos.conf` `masters =` wins; else the
    // repo's own `layout.conf` `masters =`; else the implicit default
    // (main repo alone for every non-main repo). Needs every repo's own
    // final name+location known first, so `location_by_name` is
    // snapshotted before the mutable loop.
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
