// Repo/metadata/vdb access for the single-atom `emerge --pretend` pilot
// slice (see PORTING/PROMPT.md's depgraph/config-resolution follow-up
// work, and PORTING/README.md for the full scope writeup).
//
// Overlays: every `[reponame]` section in `repos.conf` with a `location`
// (not just `[DEFAULT] main-repo`) is now a candidate source -- see
// `find_repos`. Candidates for a given category/package are gathered from
// ALL configured repos (mirroring real `portdbapi.cp_list`, which does
// the same: an overlay isn't "consulted only if the main repo has
// nothing," every repo's ebuilds are real candidates), sorted ascending
// by `(priority, name)` exactly like real portage's own `prepos_order`,
// so a tie between two repos providing the identical version is broken
// in favor of the higher-priority one (see `resolve_pretend`'s final
// `max_by`). A repo's `priority` is its explicit `repos.conf` value if
// present, else `-1000` for the main repo (real portage's own default --
// see `lib/portage/repository/config.py`) or `0` for anything else.
//
// KNOWN, DOCUMENTED SCOPE CUTS for v1 (all confirmed with the user before
// implementing):
//   - No per-repo `package.mask`/`.unmask`/`profiles/`, no `masters`
//     (eclass inheritance across repos), no `::repo`-constrained atoms
//     (out of `portage-dep`'s v1 grammar already) -- overlays only widen
//     *which ebuilds are candidates*, nothing about how they're
//     evaluated once found.
//   - Ebuild metadata comes from `metadata/md5-cache/<cat>/<pf>` (plain
//     `KEY=value` text -- confirmed against a real vendored tree), never
//     from executing the ebuild in bash. This is deliberate: it lets
//     `--pretend` work without the bash dependency that real phase
//     execution will eventually require (see PROMPT.md's "Deferred:
//     ebuild phase execution").
//   - No virtuals, no backtracking.
//
// USE/ACCEPT_KEYWORDS/package.mask/.unmask/.accept_keywords/.use are no
// longer hardcoded: they're computed by `portage_profile::resolve_config`
// (real profile chain + make.conf + package.*, with its own documented
// scope cuts -- see that crate's doc comment) and threaded through
// `resolve_pretend`/`resolve_pretend_graph` as a `&portage_profile::Config`.
// `package.use` in particular is applied per package, not globally: see
// `effective_use_flags` and its use in `resolve_pretend_graph` below --
// this is the one place `Config`'s fields aren't just handed to
// `use_reduce_flat`/`is_visible` unchanged, since matching a `package.use`
// entry against a specific candidate (to support slotted/versioned
// entries, not just bare atoms) needs that candidate's resolved SLOT,
// which only exists at this repo-aware layer.
//
// Dependency recursion (see `resolve_pretend_graph` below) walks DEPEND
// and RDEPEND only, with its own documented scope cuts -- see that
// function's doc comment.
//
// Config/target roots are read from the real `PORTAGE_CONFIGROOT` and
// `ROOT` environment variables (portage's own mechanism for relocating
// `/etc/portage` and the install target -- see lib/portage/const.py),
// defaulting to `/` when unset. This is not a pilot-only convention: it's
// what lets tests point at a fixture tree without needing anything
// pilot-specific.

use portage_versions::vercmp;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

pub fn config_root_from_env() -> PathBuf {
    std::env::var_os("PORTAGE_CONFIGROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub fn root_from_env() -> PathBuf {
    std::env::var_os("ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

#[derive(Debug, Clone)]
pub struct RepoConfig {
    pub name: String,
    pub location: PathBuf,
    pub priority: i32,
    /// Whether this is `repos.conf`'s `[DEFAULT] main-repo` -- needed by
    /// callers that read repo-level config from the main repo specifically
    /// (e.g. `portage_profile::resolve_config`'s repo-level
    /// `profiles/package.mask`), since an overlay's own repo-level config
    /// is deliberately out of scope (see `resolve_pretend_graph`'s doc
    /// comment on the overlays follow-up).
    pub is_main: bool,
}

fn parse_ini(text: &str, sections: &mut HashMap<String, HashMap<String, String>>) {
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
        if let Some(section_name) = &current {
            if let Some(eq) = line.find('=') {
                let key = line[..eq].trim().to_string();
                let value = line[eq + 1..].trim().to_string();
                sections
                    .get_mut(section_name)
                    .expect("section was inserted when its header was parsed")
                    .insert(key, value);
            }
        }
    }
}

/// Parses `repos.conf` (a file, or -- as on this pilot's dev machine -- a
/// directory of `*.conf` files merged in sorted-filename order) and
/// returns every `[reponame]` section that has a `location` (the main
/// repo plus any overlays), sorted ascending by `(priority, name)` --
/// matching real portage's own `prepos_order` (see
/// `lib/portage/repository/config.py`), which is also the order
/// `list_candidates` below iterates them in, so a tie between two repos
/// providing the identical version is broken toward the higher-priority
/// one.
pub fn find_repos(config_root: &Path) -> Result<Vec<RepoConfig>, String> {
    let repos_conf_path = config_root.join("etc/portage/repos.conf");
    let mut sections: HashMap<String, HashMap<String, String>> = HashMap::new();

    if repos_conf_path.is_dir() {
        let mut entries: Vec<PathBuf> = fs::read_dir(&repos_conf_path)
            .map_err(|e| format!("reading {}: {e}", repos_conf_path.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file())
            .collect();
        entries.sort();
        for path in entries {
            let text = fs::read_to_string(&path)
                .map_err(|e| format!("reading {}: {e}", path.display()))?;
            parse_ini(&text, &mut sections);
        }
    } else if repos_conf_path.is_file() {
        let text = fs::read_to_string(&repos_conf_path)
            .map_err(|e| format!("reading {}: {e}", repos_conf_path.display()))?;
        parse_ini(&text, &mut sections);
    } else {
        return Err(format!(
            "no repos.conf found at {}",
            repos_conf_path.display()
        ));
    }

    let main_repo = sections
        .get("DEFAULT")
        .and_then(|d| d.get("main-repo"))
        .ok_or("no [DEFAULT] main-repo in repos.conf")?
        .clone();

    let mut repos: Vec<RepoConfig> = Vec::new();
    for (name, kv) in &sections {
        if name == "DEFAULT" {
            continue;
        }
        let Some(location) = kv.get("location") else {
            continue;
        };
        let location = PathBuf::from(location);
        // Real repos.conf always uses absolute locations; relative ones
        // are a pilot/testing convenience so the fixture tree under
        // PORTING/fixtures can be committed without a machine-specific
        // absolute path baked in.
        let location = if location.is_absolute() {
            location
        } else {
            config_root.join(location)
        };
        // An explicit "priority" wins; otherwise the main repo defaults
        // to -1000 (real portage's own default -- see
        // lib/portage/repository/config.py) and every other repo to 0.
        let priority = kv
            .get("priority")
            .and_then(|p| p.parse::<i32>().ok())
            .unwrap_or(if *name == main_repo { -1000 } else { 0 });
        repos.push(RepoConfig {
            name: name.clone(),
            location,
            priority,
            is_main: *name == main_repo,
        });
    }

    if !repos.iter().any(|r| r.name == main_repo) {
        return Err(format!("no location for repo {main_repo:?} in repos.conf"));
    }

    repos.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(repos)
}

/// Reads `metadata/md5-cache/<category>/<pf>` (`pf` = "package-version",
/// e.g. "foo-1.2.3-r1") as a plain `KEY=value` map.
pub fn read_md5_cache(
    repo_location: &Path,
    category: &str,
    pf: &str,
) -> Result<HashMap<String, String>, String> {
    let path = repo_location
        .join("metadata")
        .join("md5-cache")
        .join(category)
        .join(pf);
    let text = fs::read_to_string(&path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let mut map = HashMap::new();
    for line in text.lines() {
        if let Some(eq) = line.find('=') {
            map.insert(line[..eq].to_string(), line[eq + 1..].to_string());
        }
    }
    Ok(map)
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub version: String,
    pub keywords: Vec<String>,
    pub slot: String,
    /// Which repo this candidate's ebuild/metadata actually lives in --
    /// needed once there's more than one (see `list_candidates`), both to
    /// re-read this exact package's own DEPEND/RDEPEND later
    /// (`resolve_pretend_graph`) and to break a same-version tie between
    /// two repos toward the higher-priority one (`resolve_pretend`).
    pub repo_location: PathBuf,
    pub repo_priority: i32,
    /// The repo's own `repos.conf` section name (`RepoConfig::name`),
    /// e.g. "gentoo" -- appended as a `::name` suffix to every candidate
    /// string this crate builds for `portage_dep::match_from_list`
    /// (except the two paths noted in the module doc comment's
    /// `::reponame` bullet), so a `::repo`-constrained atom actually
    /// filters. Real portage cross-checks this against each repo's own
    /// `profiles/repo_name` file too; this pilot reuses the already-read
    /// `repos.conf` name as-is rather than reading a second file.
    pub repo_name: String,
}

/// A directory entry's name is only accepted as `<package>-<version>` if
/// what follows the "<package>-" prefix looks like a version (starts with
/// a digit) -- otherwise a package like "foo" would wrongly absorb a
/// sibling package's directory like "foo-bar-2.0" as if it were "foo"
/// version "bar-2.0". Same disambiguation principle as the PMS package-name
/// grammar ported in portage-dep's atom parser.
fn strip_version_prefix<'a>(dir_name: &'a str, package: &str) -> Option<&'a str> {
    let rest = dir_name.strip_prefix(package)?.strip_prefix('-')?;
    if rest.starts_with(|c: char| c.is_ascii_digit()) {
        Some(rest)
    } else {
        None
    }
}

/// Lists every version of `category/package` that has an ebuild in ANY of
/// `repos`, with metadata (KEYWORDS, SLOT) from each repo's own md5-cache
/// -- mirroring real `portdbapi.cp_list`, which gathers candidates from
/// every configured repo the same way, not just the first one that has
/// the package. `repos` is iterated in the order given (see
/// `find_repos`'s ascending `(priority, name)` sort), and each resulting
/// `Candidate` remembers which repo it came from. A candidate whose cache
/// entry is missing or unreadable is silently skipped (v1 doesn't
/// distinguish "stale cache" from "doesn't exist" -- both just mean "not
/// visible"); a repo with no directory at all for this category/package
/// simply contributes nothing, same as before.
pub fn list_candidates(
    repos: &[RepoConfig],
    category: &str,
    package: &str,
) -> Result<Vec<Candidate>, String> {
    let mut candidates = Vec::new();
    for repo in repos {
        let pkg_dir = repo.location.join(category).join(package);
        if !pkg_dir.is_dir() {
            continue;
        }
        let entries =
            fs::read_dir(&pkg_dir).map_err(|e| format!("reading {}: {e}", pkg_dir.display()))?;

        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            let Some(stem) = file_name.strip_suffix(".ebuild") else {
                continue;
            };
            let Some(version) = strip_version_prefix(stem, package) else {
                continue;
            };
            let Ok(metadata) = read_md5_cache(&repo.location, category, stem) else {
                continue;
            };
            let keywords = metadata
                .get("KEYWORDS")
                .map(|s| s.split_whitespace().map(String::from).collect())
                .unwrap_or_default();
            let slot = metadata
                .get("SLOT")
                .map(|s| s.split('/').next().unwrap_or("0").to_string())
                .unwrap_or_else(|| "0".to_string());
            candidates.push(Candidate {
                version: version.to_string(),
                keywords,
                slot,
                repo_location: repo.location.clone(),
                repo_priority: repo.priority,
                repo_name: repo.name.clone(),
            });
        }
    }
    Ok(candidates)
}

/// Whether `entry` (a `package.mask`/`.unmask`/`.accept_keywords` line)
/// matches this specific candidate. Two-tier: try the real, already-
/// verified `portage_dep::match_from_list` first (covers the vast
/// majority of real entries: versioned, slotted, bare atoms), and only
/// fall back to portage-dep's separate, bounded wildcard-atom matcher
/// (`*/*`, `category/*`, `*/package`) if that fails to parse `entry` at
/// all.
fn matches_config_entry(entry: &str, candidate_str: &str, category: &str, package: &str) -> bool {
    if let Some(matches) = portage_dep::match_from_list(entry, &[candidate_str]) {
        return !matches.is_empty();
    }
    match portage_dep::parse_wildcard_atom(entry) {
        Some(w) => portage_dep::wildcard_atom_matches(&w, category, package),
        None => false,
    }
}

/// The USE flags in effect for one specific package: `base` (the global,
/// profile/make.conf-derived set) with every matching `package.use` entry's
/// tokens layered on top, in file order, via the same incremental
/// `-flag`/`flag`/`+flag` semantics `USE` itself uses (see
/// `portage_profile::apply_incremental`). Unlike `is_visible`'s mask/
/// keywords checks (which only ever add to an accepted set), this can both
/// add and remove flags relative to `base`, and does so per package -- a
/// `package.use` entry never affects any other package's own resolution.
fn effective_use_flags(
    base: &HashSet<String>,
    package_use: &[(String, Vec<String>)],
    package_use_force: &[(String, Vec<String>)],
    package_use_mask: &[(String, Vec<String>)],
    candidate_str: &str,
    category: &str,
    package: &str,
) -> HashSet<String> {
    let mut use_flags = base.clone();
    for (entry, tokens) in package_use {
        if matches_config_entry(entry, candidate_str, category, package) {
            portage_profile::apply_incremental(&tokens.join(" "), &mut use_flags);
        }
    }
    // package.use.mask/.force, layered on top of package.use, force
    // winning first then mask -- see specificity_ordered_flags's own doc
    // comment for how a conflict between multiple matching entries is
    // resolved, and the module doc comment's own `package.use.mask`/
    // `.force` bullet for the full scope writeup.
    for flag in specificity_ordered_flags(package_use_force, candidate_str, category, package) {
        use_flags.insert(flag);
    }
    for flag in specificity_ordered_flags(package_use_mask, candidate_str, category, package) {
        use_flags.remove(&flag);
    }
    use_flags
}

/// Computes the final per-candidate flag set from `entries` (raw
/// `package.use.mask`/`.force` `(atom, tokens)` pairs): filters to
/// entries whose atom actually matches `candidate_str`, orders the
/// matches from least to most specific (see `atom_specificity`, a
/// simplified port of real `best_match_to_list`'s own ranking table,
/// used by `ordered_by_atom_specificity`), then applies each one's own
/// tokens via the same incremental `-flag`/`flag`/`+flag` semantics
/// `package.use` itself uses (`apply_incremental`), onto an initially
/// empty set -- so a more-specific atom's own `-flag` can cancel a
/// less-specific atom's own mask/force, exactly mirroring real
/// portage's own `stack_lists(incremental=True)` applied to the
/// specificity-ordered entry list.
fn specificity_ordered_flags(
    entries: &[(String, Vec<String>)],
    candidate_str: &str,
    category: &str,
    package: &str,
) -> HashSet<String> {
    let mut matching: Vec<&(String, Vec<String>)> = entries
        .iter()
        .filter(|(entry, _)| matches_config_entry(entry, candidate_str, category, package))
        .collect();
    // Stable sort: ties (including every comparison-operator atom, which
    // this pilot deliberately doesn't further distinguish -- see the
    // module doc comment) keep their original file/stacking order.
    matching.sort_by_key(|(entry, _)| atom_specificity(entry));
    let mut flags = HashSet::new();
    for (_, tokens) in matching {
        portage_profile::apply_incremental(&tokens.join(" "), &mut flags);
    }
    flags
}

/// Simplified port of real `best_match_to_list`'s own specificity
/// ranking table: versioned/slotted bare atoms and this pilot's own
/// bounded wildcard atoms only, matching `portage-dep`'s v1 grammar
/// scope. A bounded wildcard atom (`*/*`, `category/*`, `*/package`)
/// always ranks below every real atom, at `-2` -- real portage's own
/// code has three separate extended-syntax tiers (`0` for a wildcard
/// combined with `=*`, `-1` for one combined with a slot, `-2` for a
/// bare one with neither), and this pilot's own wildcard grammar has no
/// slot or glob concept at all, so every wildcard entry always falls
/// into that third, lowest tier.
fn atom_specificity(entry: &str) -> i32 {
    let Some(atom) = portage_dep::parse_atom(entry) else {
        return -2;
    };
    let op_val = match atom.operator {
        portage_dep::Operator::Eq => 6,
        portage_dep::Operator::Tilde => 5,
        portage_dep::Operator::EqGlob => 4,
        portage_dep::Operator::Gt
        | portage_dep::Operator::Ge
        | portage_dep::Operator::Lt
        | portage_dep::Operator::Le => 2,
        portage_dep::Operator::None => 1,
    };
    let slot_val = if atom.slot.is_some() { 3 } else { i32::MIN };
    op_val.max(slot_val)
}

/// A candidate is visible if it isn't masked (matches a `package.mask`
/// entry and no `package.unmask` entry) and its KEYWORDS intersect the
/// accepted set -- the global `config.accept_keywords`, plus any extra
/// keywords contributed by a matching `package.accept_keywords` entry,
/// with a `"**"` token in such an entry meaning "accept unconditionally"
/// for matching candidates (even ones with empty/no KEYWORDS).
pub fn is_visible(
    candidate: &Candidate,
    category: &str,
    package: &str,
    config: &portage_profile::Config,
) -> bool {
    let candidate_str = format!(
        "{category}/{package}-{}:{}::{}",
        candidate.version, candidate.slot, candidate.repo_name
    );

    let masked = config
        .package_mask
        .iter()
        .any(|m| matches_config_entry(m, &candidate_str, category, package))
        && !config
            .package_unmask
            .iter()
            .any(|u| matches_config_entry(u, &candidate_str, category, package));
    if masked {
        return false;
    }

    let mut accept_any = false;
    let mut extra_keywords: HashSet<&str> = HashSet::new();
    for (atom, keywords) in &config.package_accept_keywords {
        if matches_config_entry(atom, &candidate_str, category, package) {
            if keywords.iter().any(|k| k == "**") {
                accept_any = true;
            }
            extra_keywords.extend(keywords.iter().map(String::as_str));
        }
    }
    if accept_any {
        return true;
    }

    candidate
        .keywords
        .iter()
        .any(|k| config.accept_keywords.contains(k) || extra_keywords.contains(k.as_str()))
}

fn vercmp_ordering(a: &str, b: &str) -> Ordering {
    match vercmp(a, b) {
        Some(n) if n > 0 => Ordering::Greater,
        Some(n) if n < 0 => Ordering::Less,
        _ => Ordering::Equal,
    }
}

/// Lists every installed `(version, slot)` pair for `category/package`
/// found in the vdb under `root`
/// (`<root>/var/db/pkg/<category>/<package>-<version>/`), reading each
/// entry's `SLOT` file (defaulting to `"0"` if missing, same fallback as
/// `list_candidates`). Used for blocker matching, which needs slots to
/// support slotted blocker atoms -- `installed_versions` below doesn't
/// need this and stays a plain version list for its existing callers.
fn installed_candidates(root: &Path, category: &str, package: &str) -> Vec<(String, String)> {
    let cat_dir = root.join("var/db/pkg").join(category);
    let Ok(entries) = fs::read_dir(&cat_dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let version = strip_version_prefix(&name, package)?.to_string();
            let slot = fs::read_to_string(e.path().join("SLOT"))
                .ok()
                .map(|s| s.trim().split('/').next().unwrap_or("0").to_string())
                .unwrap_or_else(|| "0".to_string());
            Some((version, slot))
        })
        .collect()
}

/// Lists every installed version of `category/package` found in the vdb
/// under `root` (`<root>/var/db/pkg/<category>/<package>-<version>/`).
pub fn installed_versions(root: &Path, category: &str, package: &str) -> Vec<String> {
    installed_candidates(root, category, package)
        .into_iter()
        .map(|(version, _slot)| version)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PretendOutcome {
    NoVisibleCandidate,
    New {
        version: String,
    },
    Upgrade {
        from: String,
        to: String,
    },
    AlreadyInstalled {
        version: String,
    },
    /// `--newuse`: already installed at this exact version, but this
    /// package's currently-effective USE differs from what the vdb
    /// recorded at merge time -- see `reinstall_flags_for_newuse`.
    /// `changed_flags` is the sorted set of flag names that triggered it
    /// (real depgraph's own `_reinstall_for_flags` return value, kept here
    /// purely for display, matching `Upgrade`'s own `from`/`to` pattern).
    Reinstall {
        version: String,
        changed_flags: Vec<String>,
    },
}

/// Reads `<root>/var/db/pkg/<category>/<package>-<version>/<filename>`
/// (a vdb aux file, e.g. `USE` or `IUSE` -- same directory `SLOT`/
/// `CATEGORY` already come from) as a set of flag names, one per
/// whitespace-separated token, with any `+`/`-` IUSE default-marker
/// prefix stripped (irrelevant here: a reinstall check only cares which
/// flags exist/are enabled, not their declared defaults). A missing file
/// (e.g. an older vdb entry from before this pilot's fixtures modeled
/// USE/IUSE at all) is an empty set, not an error -- same "absence is a
/// real, valid state" precedent `read_world_atoms` (pretend.rs) already
/// established.
fn read_vdb_flag_set(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
    filename: &str,
) -> HashSet<String> {
    let path = root
        .join("var/db/pkg")
        .join(category)
        .join(format!("{package}-{version}"))
        .join(filename);
    fs::read_to_string(path)
        .unwrap_or_default()
        .split_whitespace()
        .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
        .collect()
}

/// `candidate`'s own current IUSE (read fresh from its own md5-cache
/// entry -- the current tree's metadata, not the vdb) and its own
/// effective (computed) USE set, via `effective_use_flags`. Shared by
/// `reinstall_flags_for_use_change`'s own "cur_iuse"/"cur_use" (the
/// current-tree side of a `--newuse`/`--changed-use` comparison) and
/// `resolve_pretend`'s own USE-dep filtering (`use_deps_satisfied`,
/// portage-dep) -- both need exactly this same pair for a candidate
/// that's about to be installed or is already installed, computed the
/// same way regardless of which. Returns `None` if this candidate's own
/// metadata can't be read at all (e.g. `IUSE` missing).
fn candidate_iuse_and_use(
    candidate: &Candidate,
    category: &str,
    package: &str,
    config: &portage_profile::Config,
) -> Option<(HashSet<String>, HashSet<String>)> {
    let pf = format!("{package}-{}", candidate.version);
    let metadata = read_md5_cache(&candidate.repo_location, category, &pf).ok()?;
    // A missing IUSE key is a real, valid "declares no USE flags at all"
    // state (same "absence is real, not an error" precedent
    // read_vdb_flag_set already sets for a missing vdb IUSE/USE file),
    // not a reason to treat this whole candidate as unreadable -- unlike
    // a missing md5-cache entry entirely (the `?` just above), which
    // really does mean "can't tell anything about this candidate".
    let iuse: HashSet<String> = metadata
        .get("IUSE")
        .map(|s| s.as_str())
        .unwrap_or_default()
        .split_whitespace()
        .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
        .collect();
    let candidate_str = format!(
        "{category}/{package}-{}:{}::{}",
        candidate.version, candidate.slot, candidate.repo_name
    );
    let use_flags = effective_use_flags(
        &config.use_flags,
        &config.package_use,
        &config.package_use_force,
        &config.package_use_mask,
        &candidate_str,
        category,
        package,
    );
    Some((iuse, use_flags))
}

/// `--newuse`/`--changed-use`: ports both the `newuse` and `elif
/// changed_use` branches of real `depgraph.py`'s `_reinstall_for_flags`
/// -- whether `candidate` (a version already installed) needs
/// reinstalling because its currently-effective USE differs from what
/// the vdb recorded at merge time. Returns the sorted list of flags
/// that triggered it, or `None` if nothing did. Only ever called when
/// at least one of `newuse`/`changed_use` is set; if both are (real
/// emerge accepts giving both at once), `newuse` wins -- matching real
/// portage's own `if newuse or (...): ... elif changed_use or (...):
/// ...`, which checks `newuse` first.
///
/// Both branches share one term: `(orig_iuse∩orig_use) ^
/// (cur_iuse∩cur_use)` (which flags were enabled, among those actually
/// declared on each side) -- this alone is the *entire* `--changed-use`
/// formula, deliberately narrower than `--newuse`: it only ever reacts
/// to an *enablement* change of a flag that exists in IUSE on both
/// sides, never to IUSE gaining or losing a flag entirely. `--newuse`
/// adds a second term on top, `(orig_iuse ^ cur_iuse) - forced_flags`
/// (`config.use_force ∪ config.use_mask`, subtracted here and only
/// here -- real portage's own `flags -= forced_flags` line sits between
/// the `^=` and the final `|=`, so a flag forced on/off by the profile
/// never *by itself* triggers a `--newuse` reinstall, but can still
/// contribute via the shared enablement term above, exactly like real
/// portage): whether a flag exists in IUSE changed at all, regardless of
/// whether it's even enabled.
fn reinstall_flags_for_use_change(
    root: &Path,
    category: &str,
    package: &str,
    candidate: &Candidate,
    config: &portage_profile::Config,
    newuse: bool,
) -> Option<Vec<String>> {
    let version = &candidate.version;
    let orig_use = read_vdb_flag_set(root, category, package, version, "USE");
    let orig_iuse = read_vdb_flag_set(root, category, package, version, "IUSE");

    let (cur_iuse, cur_use) = candidate_iuse_and_use(candidate, category, package, config)?;

    let orig_enabled: HashSet<String> = orig_iuse.intersection(&orig_use).cloned().collect();
    let cur_enabled: HashSet<String> = cur_iuse.intersection(&cur_use).cloned().collect();
    let mut flags: HashSet<String> = orig_enabled
        .symmetric_difference(&cur_enabled)
        .cloned()
        .collect();

    if newuse {
        let mut presence_diff: HashSet<String> =
            orig_iuse.symmetric_difference(&cur_iuse).cloned().collect();
        for forced in config.use_force.union(&config.use_mask) {
            presence_diff.remove(forced);
        }
        flags.extend(presence_diff);
    }

    if flags.is_empty() {
        return None;
    }
    let mut sorted: Vec<String> = flags.into_iter().collect();
    sorted.sort();
    Some(sorted)
}

/// The single-atom v1 `emerge --pretend` decision: find the best visible
/// candidate matching `atom_str` across all of `repos` (the main repo and
/// any overlays -- see `find_repos`), compare it against what's
/// installed. `atom_str` may be a full atom (operator, slot -- anything
/// portage-dep's v1 grammar supports), not just a bare category/package:
/// this is what lets dependency atoms extracted from DEPEND/RDEPEND (see
/// `resolve_pretend_graph`) reuse the exact same resolution logic as the
/// top-level CLI atom. `newuse`/`changed_use` each enable their own
/// reinstall check (see `reinstall_flags_for_use_change`) for an
/// already-installed match, `newuse` winning if both are set; `false`
/// for both reproduces this function's pre-`--newuse`/`--changed-use`
/// behavior exactly.
///
/// `update` (`--update`/`-u`) gates real `_wrapped_select_pkg_highest_available_imp`'s
/// own `avoid_update`/`dont_miss_updates` behavior (`lib/_emerge/depgraph.py`,
/// lines 7814 and 8448): `"--update" not in myopts` is real portage's
/// *default*, and when it holds, an already-installed version that
/// itself still satisfies the atom is returned immediately, without ever
/// searching for a newer one -- real emerge does NOT offer an upgrade
/// just because `emerge cat/pkg` was run with no other flags; that's
/// what `--update`/`-u` is for. Ported below as an early return, checked
/// before the "always resolve to the single best visible candidate"
/// logic that already existed: if `!update` and some installed version
/// both matches `atom_str` and still has a visible candidate in `visible`
/// (mask/keyword-filtered above), the highest such version is used
/// as-is, exactly like the pre-existing "installed version equals best"
/// branch below, `newuse`/`changed_use` included. Requiring a *visible*
/// candidate (not just checking the vdb directly) is deliberate, not an
/// oversight: it's what lets an installed version that's since become
/// masked fall through to the ordinary best-visible-candidate path
/// below unchanged, matching real portage's own "enable upgrade or
/// downgrade to a version with visible KEYWORDS when the installed
/// version is masked" comment right above its own `avoid_update` check.
/// When `update` is true, or no installed version qualifies this way,
/// behavior is exactly as before this parameter existed.
pub fn resolve_pretend(
    repos: &[RepoConfig],
    root: &Path,
    atom_str: &str,
    config: &portage_profile::Config,
    newuse: bool,
    changed_use: bool,
    update: bool,
) -> Result<PretendOutcome, String> {
    let atom =
        portage_dep::parse_atom(atom_str).ok_or_else(|| format!("invalid atom {atom_str:?}"))?;

    let candidates = list_candidates(repos, &atom.category, &atom.package)?;
    let visible: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| is_visible(c, &atom.category, &atom.package, config))
        .collect();
    if visible.is_empty() {
        return Ok(PretendOutcome::NoVisibleCandidate);
    }

    // Reuses portage-dep's already-verified match_from_list rather than
    // re-deriving version/slot matching rules here; candidate strings are
    // round-tripped back to their Candidate via `by_str` below.
    let candidate_strs: Vec<String> = visible
        .iter()
        .map(|c| {
            format!(
                "{}/{}-{}:{}::{}",
                atom.category, atom.package, c.version, c.slot, c.repo_name
            )
        })
        .collect();
    let candidate_str_refs: Vec<&str> = candidate_strs.iter().map(String::as_str).collect();
    let matched = portage_dep::match_from_list(atom_str, &candidate_str_refs)
        .ok_or_else(|| format!("invalid atom {atom_str:?}"))?;

    let mut by_str: HashMap<&str, &Candidate> = HashMap::new();
    for (s, c) in candidate_str_refs.iter().zip(visible.iter()) {
        by_str.insert(*s, *c);
    }

    // USE deps (`dev-libs/foo[bar]`/`[-bar]`, `(+)`/`(-)` defaults --
    // PMS 8.3.4): a post-filter on top of match_from_list's own version/
    // slot/repo matching, exactly where real portage's own
    // `match_from_list` applies its equivalent USE-dep post-pass too --
    // see `use_deps_satisfied`'s own doc comment (portage-dep) for the
    // ported algorithm and why match_from_list itself doesn't do this.
    // Each surviving candidate's own current-tree IUSE/effective-USE
    // (`candidate_iuse_and_use`) decides it -- a candidate whose own
    // metadata can't even be read is dropped, same "can't tell, so
    // exclude" precedent `reinstall_flags_for_use_change` already sets.
    let matched: Vec<&str> = match &atom.use_deps {
        Some(use_deps) if !use_deps.is_empty() => matched
            .into_iter()
            .filter(|m| {
                let Some(candidate) = by_str.get(m) else {
                    return false;
                };
                let Some((iuse, use_flags)) =
                    candidate_iuse_and_use(candidate, &atom.category, &atom.package, config)
                else {
                    return false;
                };
                portage_dep::use_deps_satisfied(use_deps, &iuse, &use_flags)
            })
            .collect(),
        _ => matched,
    };

    let installed = installed_versions(root, &atom.category, &atom.package);

    // --update/-u: see this function's own doc comment.
    if !update {
        if let Some(installed_best) = matched
            .iter()
            .filter_map(|m| by_str.get(m).copied())
            .filter(|c| installed.iter().any(|v| v == &c.version))
            .max_by(|a, b| {
                vercmp_ordering(&a.version, &b.version).then(a.repo_priority.cmp(&b.repo_priority))
            })
        {
            if newuse || changed_use {
                if let Some(changed_flags) = reinstall_flags_for_use_change(
                    root,
                    &atom.category,
                    &atom.package,
                    installed_best,
                    config,
                    newuse,
                ) {
                    return Ok(PretendOutcome::Reinstall {
                        version: installed_best.version.clone(),
                        changed_flags,
                    });
                }
            }
            return Ok(PretendOutcome::AlreadyInstalled {
                version: installed_best.version.clone(),
            });
        }
    }

    // Ties on identical version (possible once more than one repo can
    // provide it) are broken toward the higher-priority repo, matching
    // real portage's own `(pkg.version, repo.priority)` sort in
    // `portdbapi.cp_list`.
    let Some(best) = matched
        .iter()
        .filter_map(|m| by_str.get(m).copied())
        .max_by(|a, b| {
            vercmp_ordering(&a.version, &b.version).then(a.repo_priority.cmp(&b.repo_priority))
        })
    else {
        return Ok(PretendOutcome::NoVisibleCandidate);
    };

    if installed.iter().any(|v| v == &best.version) {
        if newuse || changed_use {
            if let Some(changed_flags) = reinstall_flags_for_use_change(
                root,
                &atom.category,
                &atom.package,
                best,
                config,
                newuse,
            ) {
                return Ok(PretendOutcome::Reinstall {
                    version: best.version.clone(),
                    changed_flags,
                });
            }
        }
        return Ok(PretendOutcome::AlreadyInstalled {
            version: best.version.clone(),
        });
    }

    match installed.iter().max_by(|a, b| vercmp_ordering(a, b)) {
        Some(current) => Ok(PretendOutcome::Upgrade {
            from: current.clone(),
            to: best.version.clone(),
        }),
        None => Ok(PretendOutcome::New {
            version: best.version.clone(),
        }),
    }
}

/// A blocker atom (from a package's own dependency strings) that matches
/// either a currently-installed package or another package this same
/// `resolve_pretend_graph` run would also newly merge/upgrade. Purely
/// informational (see `resolve_pretend_graph`'s doc comment): v1 makes no
/// attempt to resolve or refuse anything on account of a blocker, the
/// same "report, don't enforce" spirit as `PretendOutcome::NoVisibleCandidate`
/// for an unresolvable dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockerConflict {
    /// The raw blocker atom text, e.g. `"!!dev-libs/foo"`.
    pub atom_str: String,
    /// `true` for a strong (`!!`) blocker, `false` for a weak (`!`) one.
    pub strong: bool,
    pub matched_category: String,
    pub matched_package: String,
    pub matched_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEntry {
    pub category: String,
    pub package: String,
    pub outcome: PretendOutcome,
    /// Only ever non-empty for `New`/`Upgrade` entries -- an entry's own
    /// DEPEND/RDEPEND (and therefore its blockers) are only read when it
    /// would newly merge or upgrade, same as dependency recursion itself.
    pub blockers: Vec<BlockerConflict>,
    /// `Some(slot)` only for `New`/`Upgrade` entries (the ones a slot can
    /// meaningfully be tracked for -- see `resolve_pretend_graph`'s doc
    /// comment on multi-slot support), `None` for `AlreadyInstalled`/
    /// `NoVisibleCandidate`.
    pub slot: Option<String>,
    /// This package's own IUSE-declared flags (default markers stripped),
    /// each paired with whether `effective_use_flags` resolved it enabled
    /// -- alphabetically sorted, matching real `--pretend -v`'s own
    /// ordering. Only ever non-empty for `New`/`Upgrade` entries whose
    /// md5-cache metadata was readable, same as `blockers`. Always
    /// computed regardless of `--verbose` (cheap; the CLI layer decides
    /// whether to print it) -- see pretend.rs.
    pub use_flags_display: Vec<(String, bool)>,
}

/// A blocker atom found while flattening one package's own dependency strings,
/// not yet matched against anything -- collected during the BFS in
/// `resolve_pretend_graph` and resolved in a single post-pass (see
/// `resolve_blockers`) once the whole graph's New/Upgrade set is known, so
/// a match doesn't depend on BFS discovery order (two packages can block
/// each other regardless of which one the queue reaches first).
struct PendingBlocker {
    atom_str: String,
    strong: bool,
    target_category: String,
    target_package: String,
    owner_key: (String, String),
    owner_version: String,
}

/// Matches each `pending` blocker's target `category/package` against
/// both currently-installed candidates (`installed_candidates`) and this
/// graph's own resolved New/Upgrade set (`entries`, which may now hold
/// more than one slot for the same category/package -- every one of them
/// is a real candidate, not just the first), reusing
/// `portage_dep::match_from_list` exactly as every other atom-vs-candidate
/// check in this crate does (it ignores an atom's `blocker` field
/// entirely, so a `!`/`!!`-prefixed atom string matches candidates by
/// category/package/version/slot exactly like a normal one). A match
/// against the owner package's own resolved version is dropped
/// defensively (a package blocking itself is nonsensical, but cheap to
/// guard against). Returns `(owner_key, conflict)` pairs rather than
/// mutating `entries` directly, since `entries` is still needed
/// immutably to build the graph-resolved candidate list here.
fn resolve_blockers(
    root: &Path,
    pending: &[PendingBlocker],
    entries: &[GraphEntry],
) -> Vec<((String, String), BlockerConflict)> {
    let mut conflicts = Vec::new();
    for pb in pending {
        let target_key = (pb.target_category.clone(), pb.target_package.clone());
        let mut candidates = installed_candidates(root, &pb.target_category, &pb.target_package);
        for entry in entries {
            if entry.category != pb.target_category || entry.package != pb.target_package {
                continue;
            }
            let version = match &entry.outcome {
                PretendOutcome::New { version } => Some(version.clone()),
                PretendOutcome::Upgrade { to, .. } => Some(to.clone()),
                PretendOutcome::Reinstall { version, .. } => Some(version.clone()),
                _ => None,
            };
            let (Some(version), Some(slot)) = (version, entry.slot.clone()) else {
                continue;
            };
            if !candidates.iter().any(|(v, s)| *v == version && *s == slot) {
                candidates.push((version, slot));
            }
        }
        let candidate_strs: Vec<String> = candidates
            .iter()
            .map(|(v, s)| format!("{}/{}-{v}:{s}", pb.target_category, pb.target_package))
            .collect();
        let refs: Vec<&str> = candidate_strs.iter().map(String::as_str).collect();
        let Some(matched) = portage_dep::match_from_list(&pb.atom_str, &refs) else {
            continue;
        };
        let by_str: HashMap<&str, &(String, String)> = candidate_strs
            .iter()
            .map(String::as_str)
            .zip(candidates.iter())
            .collect();
        for m in matched {
            let Some((version, _slot)) = by_str.get(m).copied() else {
                continue;
            };
            if target_key == pb.owner_key && *version == pb.owner_version {
                continue;
            }
            conflicts.push((
                pb.owner_key.clone(),
                BlockerConflict {
                    atom_str: pb.atom_str.clone(),
                    strong: pb.strong,
                    matched_category: pb.target_category.clone(),
                    matched_package: pb.target_package.clone(),
                    matched_version: version.clone(),
                },
            ));
        }
    }
    conflicts
}

/// A genuine slot conflict: two different dependency atoms, both landing
/// on the same category/package/slot, whose independently-resolved
/// "best" versions are incompatible -- the atom that reached this slot
/// *second* doesn't actually accept the version the *first* one already
/// caused to be resolved (and recursed into). This is distinct from two
/// atoms simply requesting *different* slots of the same package (e.g.
/// `dev-lang/python:3.11` and `dev-lang/python:3.12`), which real portage
/// -- and this pilot, see `resolve_pretend_graph`'s doc comment -- treats
/// as normal, valid coexistence, not a conflict at all.
///
/// Purely informational, the same "report, don't enforce" spirit as
/// `BlockerConflict`: real portage's own depgraph treats an unresolved
/// slot conflict as fatal and refuses to proceed; this pilot instead
/// reports it and keeps going (using whichever version was resolved
/// first), consistent with the rest of this follow-up series --
/// `--pretend` itself never touches anything real, so nothing here is
/// truly "fatal" to calculate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotConflict {
    pub category: String,
    pub package: String,
    pub slot: String,
    /// The version already resolved (and recursed into) for this slot,
    /// by whichever atom reached it first.
    pub resolved_version: String,
    /// The atom text that does NOT accept `resolved_version`.
    pub conflicting_atom: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphResult {
    pub entries: Vec<GraphEntry>,
    pub slot_conflicts: Vec<SlotConflict>,
}

/// Recursively resolves every atom in `atoms` and -- for packages that
/// would newly merge or upgrade -- its DEPEND+RDEPEND+BDEPEND+PDEPEND+
/// IDEPEND atoms, breadth-first. Returns one `GraphEntry` per distinct
/// category/package/slot combination visited, in discovery order (not
/// topologically sorted): unlike a package name alone, two *different*
/// slots of the same package are both real, independent entries (each
/// gets its own recursion into its own dependency strings) -- mirroring
/// how real portage genuinely allows
/// multiple slots of the same package to coexist in one merge list (the
/// entire point of `SLOT`, e.g. `dev-lang/python:3.11` and
/// `dev-lang/python:3.12` side by side). A *conflict* only exists when
/// two atoms need the identical slot at incompatible versions -- see
/// `SlotConflict`.
///
/// `atoms` seeds the BFS queue together, in the order given, before any
/// dependency is ever pushed -- so all of them are dequeued and resolved
/// first (level-order guarantee), and the existing visited-atom/
/// resolved-slot/blocker bookkeeping below, already keyed by atom text or
/// `(category, package, slot)` rather than by "the one root", handles
/// sharing between them for free: a dependency common to two requested
/// atoms dedupes exactly like a diamond dependency does, and a slot
/// conflict between two top-level atoms (not just between two deps) is
/// now detected too. A top-level atom with no visible candidate is fatal
/// to the whole call (see the `NoVisibleCandidate` check below, matching
/// real portage's own `depgraph.py` "there are no ebuilds to satisfy"
/// behavior) rather than reported-and-continued the way a *dependency's*
/// `NoVisibleCandidate` is -- confirmed with the user before implementing
/// -- and since top-level atoms are always dequeued in argv order before
/// any dependency, the first bad one aborts before any later atom
/// (top-level or not) is even attempted.
///
/// Each package's own `package.use` overrides (see `effective_use_flags`)
/// only affect how *that* package's own dependency strings are flattened --
/// they never leak into a sibling or dependency's resolution, matching
/// real portage's per-package USE. The same already-computed
/// `effective_use_flags` result, filtered down to just this package's own
/// IUSE-declared flags, is also attached to every New/Upgrade entry as
/// `GraphEntry::use_flags_display`, for `--pretend -v`'s USE="..." display
/// -- see pretend.rs.
///
/// KNOWN, DOCUMENTED SCOPE CUTS (all confirmed with the user before
/// implementing):
///   - All five real dependency-string keys are walked (DEPEND, RDEPEND,
///     BDEPEND, PDEPEND, IDEPEND), concatenated and flattened together
///     with no distinction between them: real portage's own merge
///     ordering treats these differently (BDEPEND must be satisfied on
///     the build host before compiling; PDEPEND only after this package
///     itself merges; IDEPEND only at install time) -- meaningless
///     distinctions for a `--pretend`-only pilot with no real merge
///     ordering or phase execution to begin with (see PROMPT.md's
///     "Deferred: ebuild phase execution"), so v1 treats all five as "a
///     dependency this package needs, resolve and report it" uniformly,
///     the same "report, don't enforce" simplification already applied to
///     blockers and slot conflicts below. Atoms whose *exact text*
///     repeats (e.g. a shared dependency, or a cycle) are deduped via a
///     visited-atom-text set purely to guarantee termination -- see below
///     for how repeat visits to the same resolved category/package/slot
///     are actually handled.
///   - `config` (USE, ACCEPT_KEYWORDS, package.mask/.unmask/.accept_keywords)
///     is supplied by the caller (computed via `portage_profile::resolve_config`
///     -- see that crate's doc comment for what real profile/make.conf/
///     package.* mechanics are and aren't implemented) rather than being
///     read here; this crate stays decoupled from profile-parsing logic
///     even though it now depends on portage-profile for the `Config` type.
///   - `||` (any-of) groups: `use_reduce(flat=True)` deliberately discards
///     group boundaries (that's what "flat" means), so there is no
///     reliable way to identify "the first alternative" from its output
///     without reimplementing non-flat structured mode -- a considerably
///     bigger, previously out-of-scope piece of work (see
///     portage-use-reduce's doc comment on why flat-only was chosen).
///     Rather than take that on, v1 resolves *every* atom in an any-of
///     group. This can pull in more than a real resolver would, but is
///     never silently wrong about whether a dependency exists.
///   - A dependency atom with no visible candidate does not fail the
///     whole graph: it still gets a `GraphEntry` with
///     `PretendOutcome::NoVisibleCandidate` (so it's visible in the
///     output, not silently dropped), it's just not recursed into
///     further, matching the "best effort" spirit of the rest of this
///     pilot slice. Repeat atoms resolving to `NoVisibleCandidate` (or
///     `AlreadyInstalled`) for the same category/package are still
///     deduped to a single entry -- these two outcomes carry no slot to
///     usefully distinguish repeats by, unlike New/Upgrade.
///   - Blockers (`!foo/bar`/`!!foo/bar` tokens) are recognized and matched
///     against installed packages and other New/Upgrade entries in this
///     same graph (see `BlockerConflict`), but purely for reporting: v1
///     makes no attempt to resolve a conflict (no merge reordering, no
///     refusing to proceed) or to enforce anything -- a strong (`!!`)
///     match doesn't change the graph's outcome or exit code any
///     differently than a weak (`!`) one. This matches real `--pretend`
///     itself (which only *calculates and shows* what would happen,
///     without touching anything), and stays consistent with the
///     "unresolvable dependency doesn't fail the graph" rule above, and
///     with `SlotConflict` being reported rather than enforced too.
///   - A package's dependencies are only walked if resolving it produced
///     New, Upgrade, or (with `newuse`/`changed_use`) Reinstall; an
///     already-installed package that stays AlreadyInstalled has its
///     own dependencies presumed already satisfied, same as before
///     `--newuse` existed. This also means blockers -- and slot
///     conflicts -- are only ever detected from New/Upgrade/Reinstall
///     packages' own dependency strings; an AlreadyInstalled package's
///     blockers are never inspected, same as the rest of its
///     dependencies. See `reinstall_flags_for_use_change`'s own doc
///     comment for how `newuse`/`changed_use` combine.
///   - `nodeps` (`--nodeps`/`-O`) disables the dependency walk entirely,
///     for every entry, not just top-level atoms: only `atoms` themselves
///     are ever resolved, ported from real `create_depgraph_params.py`
///     popping `"recurse"` out of `myparams` (which depgraph.py's own
///     dependency-walk checks for and returns early without). Each
///     resolved entry's own USE display is still computed (real
///     portage's `-v` output shows a package's own USE regardless of
///     whether its dependencies get walked), but no DEPEND/RDEPEND/etc
///     is ever read, so no dependency atom is ever queued and no blocker
///     is ever collected -- blockers only ever come from a dependency
///     string in this pilot, so this falls out for free rather than
///     needing its own special case.
///   - `update` (`--update`/`-u`) is threaded uniformly to every atom this
///     BFS resolves, top-level and dependency alike, via `resolve_pretend`
///     -- see that function's own doc comment for the real
///     `avoid_update`/`dont_miss_updates` behavior it ports, same
///     whole-graph-uniform application `newuse`/`changed_use` already get
///     above. `avoid_update`/`dont_miss_updates` are themselves plain
///     `myopts` checks inside real `_wrapped_select_pkg_highest_available_imp`,
///     the one package-selection function every atom resolution (args and
///     dependencies alike) already funnels through in real portage too,
///     so this isn't a new pilot-specific simplification beyond the one
///     `newuse`/`changed_use` already made.
// 8 args trips clippy::too_many_arguments; a bundled options struct
// would touch every one of this function's own call sites (production
// and test) for a single-slice-sized addition of one more CLI boolean
// flag alongside three already threaded the same way -- not worth it.
#[allow(clippy::too_many_arguments)]
pub fn resolve_pretend_graph(
    config_root: &Path,
    root: &Path,
    atoms: &[String],
    config: &portage_profile::Config,
    newuse: bool,
    changed_use: bool,
    nodeps: bool,
    update: bool,
) -> Result<GraphResult, String> {
    let repos = find_repos(config_root)?;

    // Only used to tell a top-level atom's own NoVisibleCandidate (fatal
    // to the whole call) apart from a dependency's (reported, not fatal)
    // -- see the doc comment above.
    let top_level: HashSet<&str> = atoms.iter().map(|s| s.as_str()).collect();

    // Guards against infinite requeuing (e.g. a dependency cycle): the
    // exact same atom *text* is only ever resolved once. This is
    // deliberately coarser than the (category, package, slot) dedup
    // below -- it exists purely for termination, not to decide whether a
    // given slot has already been fully resolved (two different atom
    // texts, e.g. a bare "dev-libs/foo" and a slotted "dev-libs/foo:1",
    // can both need to be resolved even though they'd share a visited-atom
    // check keyed any coarser than exact text).
    let mut visited_atoms: HashSet<String> = HashSet::new();
    // (category, package, slot) -> index into `entries`, for New/Upgrade
    // outcomes only. The first atom to resolve a given slot "wins" (its
    // version is what gets recursed into); every later atom landing on
    // the same slot is checked against that already-resolved version
    // (see `SlotConflict`) instead of triggering a second, independent
    // resolution.
    let mut resolved_slots: HashMap<(String, String, String), usize> = HashMap::new();
    // (category, package) -> already added an AlreadyInstalled/
    // NoVisibleCandidate entry for it. Separate from `resolved_slots`
    // since neither outcome carries a slot to usefully key repeats by.
    let mut other_outcomes: HashSet<(String, String)> = HashSet::new();

    let mut entries: Vec<GraphEntry> = Vec::new();
    let mut slot_conflicts: Vec<SlotConflict> = Vec::new();
    let mut queue: VecDeque<String> = VecDeque::new();
    for a in atoms {
        queue.push_back(a.clone());
    }

    let mut pending_blockers: Vec<PendingBlocker> = Vec::new();

    while let Some(current_atom) = queue.pop_front() {
        let Some(atom) = portage_dep::parse_atom(&current_atom) else {
            continue;
        };
        if atom.blocker != portage_dep::Blocker::None {
            continue;
        }
        if !visited_atoms.insert(current_atom.clone()) {
            continue;
        }
        let key = (atom.category.clone(), atom.package.clone());

        let outcome = resolve_pretend(
            &repos,
            root,
            &current_atom,
            config,
            newuse,
            changed_use,
            update,
        )?;

        // A top-level atom (as opposed to a dependency reached while
        // recursing) with no visible candidate aborts the whole call --
        // matching real portage's own depgraph.py behavior for an
        // unsatisfiable target, not the "report and keep going" treatment
        // a dependency's own NoVisibleCandidate gets a few lines down.
        // Top-level atoms are always dequeued (and so reach this point)
        // in argv order, before any dependency, so this also guarantees
        // the *first* bad top-level atom is the one that aborts.
        if top_level.contains(current_atom.as_str())
            && matches!(outcome, PretendOutcome::NoVisibleCandidate)
        {
            return Err(format!("there are no ebuilds to satisfy {current_atom:?}."));
        }

        let resolved_version = match &outcome {
            PretendOutcome::New { version } => Some(version.clone()),
            PretendOutcome::Upgrade { to, .. } => Some(to.clone()),
            PretendOutcome::Reinstall { version, .. } => Some(version.clone()),
            _ => None,
        };

        let Some(version) = resolved_version else {
            // AlreadyInstalled / NoVisibleCandidate: no slot to key a
            // repeat by, so dedup on category/package alone, same as v1
            // always did before slot-aware resolution existed.
            if !other_outcomes.insert(key.clone()) {
                continue;
            }
            entries.push(GraphEntry {
                category: key.0,
                package: key.1,
                outcome,
                blockers: Vec::new(),
                slot: None,
                use_flags_display: Vec::new(),
            });
            continue;
        };

        // The resolved version may have come from any of `repos` (not
        // necessarily the main one), so re-derive which repo it actually
        // lives in -- reusing `list_candidates` rather than threading a
        // repo location back out of `PretendOutcome`, since more than one
        // repo could in principle carry the identical version, tie-broken
        // the same way `resolve_pretend` itself does.
        let Ok(repo_candidates) = list_candidates(&repos, &key.0, &key.1) else {
            continue;
        };
        let Some(resolved) = repo_candidates
            .iter()
            .filter(|c| c.version == version)
            .max_by_key(|c| c.repo_priority)
        else {
            continue;
        };
        let slot = resolved.slot.clone();
        let repo_location = resolved.repo_location.clone();
        let repo_name = resolved.repo_name.clone();

        let slot_key = (key.0.clone(), key.1.clone(), slot.clone());
        if let Some(&existing_idx) = resolved_slots.get(&slot_key) {
            // This exact category/package/slot was already resolved by
            // an earlier atom. If the current atom's own constraint
            // doesn't accept that already-resolved version, it's a real
            // slot conflict -- report it and move on, without a second,
            // independent resolution or any attempt to reconcile the two.
            let existing_version = match &entries[existing_idx].outcome {
                PretendOutcome::New { version } => version.clone(),
                PretendOutcome::Upgrade { to, .. } => to.clone(),
                PretendOutcome::Reinstall { version, .. } => version.clone(),
                _ => unreachable!("resolved_slots only ever indexes New/Upgrade/Reinstall entries"),
            };
            let existing_str = format!("{}/{}-{existing_version}:{slot}", key.0, key.1);
            let satisfied = portage_dep::match_from_list(&current_atom, &[existing_str.as_str()])
                .is_some_and(|m| !m.is_empty());
            if !satisfied {
                slot_conflicts.push(SlotConflict {
                    category: key.0,
                    package: key.1,
                    slot,
                    resolved_version: existing_version,
                    conflicting_atom: current_atom,
                });
            }
            continue;
        }
        let entry_idx = entries.len();
        resolved_slots.insert(slot_key, entry_idx);
        entries.push(GraphEntry {
            category: key.0.clone(),
            package: key.1.clone(),
            outcome,
            blockers: Vec::new(),
            slot: Some(slot.clone()),
            use_flags_display: Vec::new(),
        });

        let pf = format!("{}-{version}", key.1);
        let Ok(metadata) = read_md5_cache(&repo_location, &key.0, &pf) else {
            continue;
        };
        let candidate_str = format!("{}/{}-{version}:{slot}::{repo_name}", key.0, key.1);
        let use_flags = effective_use_flags(
            &config.use_flags,
            &config.package_use,
            &config.package_use_force,
            &config.package_use_mask,
            &candidate_str,
            &key.0,
            &key.1,
        );

        // IUSE's own "+flag"/"-flag" default markers only matter for
        // resolving a flag's default when nothing else decides it --
        // already handled upstream, wherever `use_flags` itself came
        // from -- so display only needs the bare flag name, paired with
        // whatever `use_flags` (the real resolved set) says. Computed
        // (and shown by `--pretend -v`) regardless of `nodeps` below --
        // real portage's own USE display is about the package's own
        // metadata, unrelated to whether its dependencies get walked.
        // REQUIRED_USE (PMS 7.3.4/8.2): checked once, here, right after a
        // candidate is newly resolved -- real depgraph.py's own "NOTE:
        // REQUIRED_USE checks are delayed until after package selection"
        // (it's a genuine *post*-selection check, no part of matching/
        // visibility at all, unlike package.use/package.mask). A
        // violation is FATAL to the whole run regardless of whether this
        // candidate was reached as a top-level atom or a dependency deep
        // in the graph -- real portage's own severity for this (see
        // portage-required-use's own module doc comment for the ported
        // algorithm and where this is grounded), unlike a dependency's
        // own NoVisibleCandidate (report, don't fail the whole call).
        if let Some(required_use) = metadata.get("REQUIRED_USE") {
            if !required_use.trim().is_empty() {
                let iuse_set: HashSet<String> = metadata
                    .get("IUSE")
                    .map(String::as_str)
                    .unwrap_or_default()
                    .split_whitespace()
                    .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
                    .collect();
                match portage_required_use::check_required_use(required_use, &use_flags, &iuse_set)
                {
                    Ok(true) => {}
                    Ok(false) => {
                        let normalized = required_use
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ");
                        return Err(format!(
                            "REQUIRED_USE not satisfied for {}/{}-{version}: \"{normalized}\"",
                            key.0, key.1
                        ));
                    }
                    Err(e) => {
                        return Err(format!(
                            "REQUIRED_USE for {}/{}-{version} is invalid: {e}",
                            key.0, key.1
                        ));
                    }
                }
            }
        }

        if let Some(iuse) = metadata.get("IUSE") {
            let mut display: Vec<(String, bool)> = iuse
                .split_whitespace()
                .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
                .map(|flag| {
                    let enabled = use_flags.contains(&flag);
                    (flag, enabled)
                })
                .collect();
            display.sort_by(|a, b| a.0.cmp(&b.0));
            entries[entry_idx].use_flags_display = display;
        }

        // `--nodeps`: real create_depgraph_params.py pops "recurse" from
        // myparams, and depgraph.py's own dependency-walk returns early
        // when "recurse" isn't in myparams -- ported here as skipping
        // this package's own DEPEND/RDEPEND/etc entirely, so nothing of
        // its is ever parsed, flattened, or queued. This also means no
        // blockers are ever collected from a `--nodeps` run (blockers
        // only ever come from a dependency string, which is never read),
        // matching real portage exactly.
        if nodeps {
            continue;
        }

        let mut depstr = String::new();
        for dep_key in ["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"] {
            if let Some(d) = metadata.get(dep_key) {
                depstr.push_str(d);
                depstr.push(' ');
            }
        }
        let tokens: Vec<String> = depstr.split_whitespace().map(String::from).collect();
        let Ok(flat_deps) = portage_use_reduce::use_reduce_flat(
            &tokens,
            &use_flags,
            portage_use_reduce::MatchMode::Normal,
        ) else {
            continue;
        };
        for tok in flat_deps {
            if tok == "||" {
                continue;
            }
            if let Some(dep_atom) = portage_dep::parse_atom(&tok) {
                if dep_atom.blocker != portage_dep::Blocker::None {
                    pending_blockers.push(PendingBlocker {
                        atom_str: tok,
                        strong: dep_atom.blocker == portage_dep::Blocker::Strong,
                        target_category: dep_atom.category,
                        target_package: dep_atom.package,
                        owner_key: key.clone(),
                        owner_version: version.clone(),
                    });
                    continue;
                }
            }
            queue.push_back(tok);
        }
    }

    resolve_blockers(root, &pending_blockers, &entries)
        .into_iter()
        .for_each(|(owner_key, conflict)| {
            if let Some(entry) = entries
                .iter_mut()
                .find(|e| (e.category.clone(), e.package.clone()) == owner_key)
            {
                entry.blockers.push(conflict);
            }
        });

    Ok(GraphResult {
        entries,
        slot_conflicts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .canonicalize()
            .expect("PORTING/fixtures must exist")
    }

    /// These unit tests exercise portage-repo's own resolution logic in
    /// isolation, independent of real profile/make.conf/package.* parsing
    /// (that's portage-profile's job, tested separately) -- so visibility
    /// is pinned to a fixed "amd64"-only, no-overrides config here,
    /// matching what the fixture's real profile chain also happens to
    /// resolve to (see PORTING/fixtures/repo/profiles and
    /// test_emerge_pretend_contract.py for the end-to-end version of
    /// this). Constructed directly (not via portage_profile::resolve_config)
    /// so these tests don't depend on real file parsing at all.
    fn test_config() -> portage_profile::Config {
        portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            ..Default::default()
        }
    }

    fn resolve(category: &str, package: &str) -> PretendOutcome {
        let root = fixtures_root();
        let repos = find_repos(&root).expect("fixture repos.conf must resolve");
        let atom_str = format!("{category}/{package}");
        resolve_pretend(
            &repos,
            &root,
            &atom_str,
            &test_config(),
            false,
            false,
            false,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
    }

    /// Like `resolve`, but with `--update` enabled -- for the `upgrade`
    /// test below, and anywhere else `--update`'s own "search for a
    /// better version even when the installed one already satisfies the
    /// atom" behavior needs to be exercised directly.
    fn resolve_update(category: &str, package: &str) -> PretendOutcome {
        let root = fixtures_root();
        let repos = find_repos(&root).expect("fixture repos.conf must resolve");
        let atom_str = format!("{category}/{package}");
        resolve_pretend(&repos, &root, &atom_str, &test_config(), false, false, true)
            .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
    }

    #[test]
    fn new_install() {
        assert_eq!(
            resolve("dev-libs", "newpkg"),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn already_installed() {
        assert_eq!(
            resolve("dev-libs", "samepkg"),
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn without_update_an_installed_version_that_still_satisfies_the_atom_is_kept() {
        // dev-libs/upgradepkg is installed at 1.0; a newer 2.0 is visible
        // in the tree too. Real depgraph.py's own `avoid_update`
        // (lines 7814/8448) means plain `emerge dev-libs/upgradepkg`,
        // with no --update, never even looks for a better version --
        // see resolve_pretend's own doc comment. This was, before this
        // slice, this pilot's own (inaccurate) default behavior for
        // `upgrade` below.
        assert_eq!(
            resolve("dev-libs", "upgradepkg"),
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn upgrade() {
        // Same fixture as above, but with --update: now a real Upgrade,
        // matching real depgraph.py's own `dont_miss_updates` branch.
        assert_eq!(
            resolve_update("dev-libs", "upgradepkg"),
            PretendOutcome::Upgrade {
                from: "1.0".to_string(),
                to: "2.0".to_string(),
            }
        );
    }

    #[test]
    fn masked_by_keywords_is_not_visible() {
        assert_eq!(
            resolve("dev-libs", "maskedpkg"),
            PretendOutcome::NoVisibleCandidate
        );
    }

    #[test]
    fn nonexistent_package_is_no_visible_candidate() {
        assert_eq!(
            resolve("dev-libs", "does-not-exist"),
            PretendOutcome::NoVisibleCandidate
        );
    }

    /// Unlike `resolve()`, uses the fixture's *real* resolved config
    /// (package.mask/.unmask/.accept_keywords included), not the
    /// synthetic `test_config()`.
    fn resolve_real(category: &str, package: &str) -> PretendOutcome {
        let root = fixtures_root();
        let repos = find_repos(&root).expect("fixture repos.conf must resolve");
        let config = portage_profile::resolve_config(&root, &root.join("repo"))
            .expect("fixture config resolves");
        let atom_str = format!("{category}/{package}");
        resolve_pretend(&repos, &root, &atom_str, &config, false, false, false)
            .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
    }

    /// Like `resolve_real`, but with `--newuse` enabled -- for the
    /// `PretendOutcome::Reinstall` tests below.
    fn resolve_real_newuse(category: &str, package: &str) -> PretendOutcome {
        let root = fixtures_root();
        let repos = find_repos(&root).expect("fixture repos.conf must resolve");
        let config = portage_profile::resolve_config(&root, &root.join("repo"))
            .expect("fixture config resolves");
        let atom_str = format!("{category}/{package}");
        resolve_pretend(&repos, &root, &atom_str, &config, true, false, false)
            .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
    }

    /// Like `resolve_real`, but with `--changed-use` enabled.
    fn resolve_real_changed_use(category: &str, package: &str) -> PretendOutcome {
        let root = fixtures_root();
        let repos = find_repos(&root).expect("fixture repos.conf must resolve");
        let config = portage_profile::resolve_config(&root, &root.join("repo"))
            .expect("fixture config resolves");
        let atom_str = format!("{category}/{package}");
        resolve_pretend(&repos, &root, &atom_str, &config, false, true, false)
            .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
    }

    #[test]
    fn newuse_reinstalls_when_vdb_use_differs_from_current_use() {
        // dev-libs/reinstallpkg is installed at 1.0 with IUSE="foo" but an
        // empty vdb USE file (foo was off at merge time); the fixture
        // profile chain enables "foo" globally now, so --newuse must
        // report a Reinstall for the changed "foo" flag.
        assert_eq!(
            resolve_real_newuse("dev-libs", "reinstallpkg"),
            PretendOutcome::Reinstall {
                version: "1.0".to_string(),
                changed_flags: vec!["foo".to_string()],
            }
        );
    }

    #[test]
    fn without_newuse_the_same_package_stays_already_installed() {
        // Same fixture as above, but without --newuse: the USE mismatch
        // is real but the flag that would detect it is off, so this must
        // stay the pre-existing AlreadyInstalled outcome.
        assert_eq!(
            resolve_real("dev-libs", "reinstallpkg"),
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn newuse_does_not_reinstall_when_use_has_not_changed() {
        // dev-libs/samepkg has no IUSE at all (declared or in the vdb),
        // so there's nothing for --newuse to detect a change in -- must
        // stay AlreadyInstalled even with --newuse enabled.
        assert_eq!(
            resolve_real_newuse("dev-libs", "samepkg"),
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn newuse_forced_flags_suppresses_a_spurious_reinstall() {
        // dev-libs/usemaskreinstallpkg is installed with an empty vdb
        // IUSE, but its own ebuild now declares
        // IUSE="masked_newly_added_flag" -- a flag
        // PORTING/fixtures/repo/profiles/base/use.mask masks off, so
        // it's never enabled either before or after. Without
        // reinstall_flags_for_use_change's own "flags -= forced_flags"
        // step (real depgraph.py's own line, ported exactly), this
        // would incorrectly report a Reinstall just because the flag
        // now exists in IUSE at all.
        assert_eq!(
            resolve_real_newuse("dev-libs", "usemaskreinstallpkg"),
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn newuse_vs_changed_use_diverge_on_a_newly_added_iuse_flag() {
        // dev-libs/changedusepkg is installed with an empty vdb IUSE,
        // and its current ebuild now declares IUSE="brandnewflag" -- a
        // real, unmasked, not-globally-enabled flag. --newuse's own
        // presence-diff term reacts to the flag simply existing in IUSE
        // now, regardless of enablement; --changed-use's own, narrower
        // formula never looks at IUSE presence at all, only at
        // enablement of flags declared on both sides -- proving the two
        // are genuinely different checks.
        assert_eq!(
            resolve_real_newuse("dev-libs", "changedusepkg"),
            PretendOutcome::Reinstall {
                version: "1.0".to_string(),
                changed_flags: vec!["brandnewflag".to_string()],
            }
        );
        assert_eq!(
            resolve_real_changed_use("dev-libs", "changedusepkg"),
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn changed_use_still_catches_an_enablement_change_on_a_shared_flag() {
        // dev-libs/reinstallpkg's own "foo" flag exists in IUSE on both
        // sides -- only its enablement changed, exactly the shared term
        // both --newuse and --changed-use compute the same way.
        assert_eq!(
            resolve_real_changed_use("dev-libs", "reinstallpkg"),
            PretendOutcome::Reinstall {
                version: "1.0".to_string(),
                changed_flags: vec!["foo".to_string()],
            }
        );
    }

    #[test]
    fn changed_use_reinstall_still_recurses_into_its_own_dependencies() {
        // Same proof as newuse_reinstall_still_recurses_into_its_own_dependencies
        // below, but via --changed-use instead of --newuse.
        assert_eq!(
            graph_real_changed_use("dev-libs/reinstallpkg"),
            vec![
                (
                    "dev-libs/reinstallpkg".to_string(),
                    PretendOutcome::Reinstall {
                        version: "1.0".to_string(),
                        changed_flags: vec!["foo".to_string()],
                    }
                ),
                (
                    "dev-libs/newpkg".to_string(),
                    PretendOutcome::New {
                        version: "1.0".to_string()
                    }
                ),
            ]
        );
    }

    #[test]
    fn fixture_overlay_only_package_is_found() {
        // dev-libs/overlayonlypkg exists only in the fixture's overlay
        // repo (see PORTING/fixtures/etc/portage/repos.conf), not the
        // main repo -- proving the overlay is actually searched, not
        // just present in repos.conf.
        assert_eq!(
            resolve_real("dev-libs", "overlayonlypkg"),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn fixture_repo_constrained_atom_finds_the_named_repo_only() {
        // dev-libs/overlayonlypkg exists only in the fixture's overlay
        // repo (named "overlay" in repos.conf) -- "::overlay" must
        // resolve it, "::testrepo" (the main repo's own name) must not,
        // proving portage-repo's own candidate strings really do carry
        // "::reponame" end to end, not just that portage-dep can parse
        // and match it in isolation.
        let root = fixtures_root();
        let repos = find_repos(&root).expect("fixture repos.conf must resolve");
        let config = portage_profile::resolve_config(&root, &root.join("repo"))
            .expect("fixture config resolves");
        assert_eq!(
            resolve_pretend(
                &repos,
                &root,
                "dev-libs/overlayonlypkg::overlay",
                &config,
                false,
                false,
                false,
            )
            .expect("resolve_pretend must succeed"),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        );
        assert_eq!(
            resolve_pretend(
                &repos,
                &root,
                "dev-libs/overlayonlypkg::testrepo",
                &config,
                false,
                false,
                false,
            )
            .expect("resolve_pretend must succeed"),
            PretendOutcome::NoVisibleCandidate
        );
    }

    #[test]
    fn fixture_best_version_wins_regardless_of_which_repo_has_it() {
        // dev-libs/overlaynewerpkg-1.0 is in the main repo, -2.0 is in
        // the overlay -- the higher version wins even though it isn't
        // in the main (lower-priority) repo.
        assert_eq!(
            resolve_real("dev-libs", "overlaynewerpkg"),
            PretendOutcome::New {
                version: "2.0".to_string()
            }
        );
    }

    #[test]
    fn fixture_same_version_tie_across_repos_is_broken_toward_higher_priority() {
        // dev-libs/overlaytiepkg-1.0 exists identically-versioned in both
        // the main repo (priority -1000, no deps) and the overlay
        // (priority 10, RDEPENDs on dev-libs/newpkg): resolving it must
        // pull in newpkg, proving the overlay's own copy -- not the main
        // repo's -- is the one whose metadata got read.
        let full_names: Vec<String> = graph_entries_real("dev-libs/overlaytiepkg")
            .into_iter()
            .map(|e| format!("{}/{}", e.category, e.package))
            .collect();
        assert_eq!(
            full_names,
            vec!["dev-libs/overlaytiepkg", "dev-libs/newpkg"]
        );
    }

    #[test]
    fn fixture_slot_conflict_is_reported_between_two_incompatible_version_constraints() {
        // dev-libs/slotconflictparent pulls in slotconflictnewconsumer
        // (bare RDEPEND on slotconflicttarget, resolves the best version,
        // 2.0, first) and slotconflictoldconsumer (RDEPEND
        // "<dev-libs/slotconflicttarget-2.0", which 2.0 itself does NOT
        // satisfy) -- both want slot 0 of the same package, at versions
        // that can't both be right, so this must surface as a
        // SlotConflict, not a second, silently-overwriting entry.
        let result = graph_result_real("dev-libs/slotconflictparent");
        let full_names: Vec<String> = result
            .entries
            .iter()
            .map(|e| format!("{}/{}", e.category, e.package))
            .collect();
        assert_eq!(
            full_names,
            vec![
                "dev-libs/slotconflictparent",
                "dev-libs/slotconflictnewconsumer",
                "dev-libs/slotconflictoldconsumer",
                "dev-libs/slotconflicttarget",
            ]
        );
        assert_eq!(
            result.slot_conflicts,
            vec![SlotConflict {
                category: "dev-libs".to_string(),
                package: "slotconflicttarget".to_string(),
                slot: "0".to_string(),
                resolved_version: "2.0".to_string(),
                conflicting_atom: "<dev-libs/slotconflicttarget-2.0".to_string(),
            }]
        );
    }

    #[test]
    fn fixture_different_slots_of_the_same_package_coexist_without_conflict() {
        // dev-libs/multislotparent RDEPENDs on both
        // dev-libs/multislotpkg:0 and dev-libs/multislotpkg:1 -- real,
        // different slots of the same package are normal coexistence
        // (like dev-lang/python:3.11 and :3.12), not a conflict: both
        // must appear as independent entries, and neither is silently
        // dropped by the visited-set the way v1 always did before slot
        // tracking existed.
        let result = graph_result_real("dev-libs/multislotparent");
        let full_names: Vec<String> = result
            .entries
            .iter()
            .map(|e| format!("{}/{}", e.category, e.package))
            .collect();
        assert_eq!(
            full_names,
            vec![
                "dev-libs/multislotparent",
                "dev-libs/multislotpkg",
                "dev-libs/multislotpkg",
            ]
        );
        let slots: Vec<Option<String>> =
            result.entries[1..].iter().map(|e| e.slot.clone()).collect();
        assert_eq!(slots, vec![Some("0".to_string()), Some("1".to_string())]);
        assert!(result.slot_conflicts.is_empty());
    }

    #[test]
    fn fixture_virtual_resolves_through_ordinary_category_and_any_of_machinery() {
        // virtual/texteditor is shaped exactly like a real virtual (e.g.
        // virtual/pager in the real Gentoo tree, confirmed by
        // inspection): an ordinary ebuild whose RDEPEND is a
        // "|| ( ... )" any-of group of real providers -- no PROVIDE
        // mechanism, no dedicated virtuals resolution code anywhere in
        // this pilot. Both alternatives resolve (v1's documented any-of
        // behavior): dev-libs/newpkg as New, dev-libs/samepkg as
        // AlreadyInstalled (multicall's own printing layer is what hides
        // already-installed dependencies from --pretend's stdout, not
        // resolve_pretend_graph itself).
        let entries = graph_entries_real("virtual/texteditor");
        let full_names: Vec<String> = entries
            .iter()
            .map(|e| format!("{}/{}", e.category, e.package))
            .collect();
        assert_eq!(
            full_names,
            vec!["virtual/texteditor", "dev-libs/newpkg", "dev-libs/samepkg"]
        );
        assert_eq!(
            entries[2].outcome,
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn fixture_package_mask_hides_with_no_unmask() {
        assert_eq!(
            resolve_real("dev-libs", "hardmaskedpkg"),
            PretendOutcome::NoVisibleCandidate
        );
    }

    #[test]
    fn fixture_package_unmask_cancels_the_matching_mask() {
        assert_eq!(
            resolve_real("dev-libs", "maskedandunmaskedpkg"),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn fixture_package_mask_minus_atom_removal_is_unaffected() {
        // dev-libs/samepkg is masked then immediately un-masked via
        // "-dev-libs/samepkg" within package.mask itself -- it must
        // resolve completely normally (already installed), not as if it
        // were ever masked.
        assert_eq!(
            resolve_real("dev-libs", "samepkg"),
            PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn fixture_package_accept_keywords_wildcard_extends_visibility() {
        assert_eq!(
            resolve_real("dev-libs", "wildcardkeywordpkg"),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn fixture_package_accept_keywords_double_star_accepts_no_keywords_package() {
        assert_eq!(
            resolve_real("dev-libs", "livekeywordpkg"),
            PretendOutcome::New {
                version: "9999".to_string()
            }
        );
    }

    #[test]
    fn fixture_unrelated_masked_by_keywords_package_is_still_hidden() {
        // Regression guard: the "*/wildcardkeywordpkg" package.accept_keywords
        // entry is scoped to that package name only (not "dev-libs/*"),
        // specifically so it can't accidentally make dev-libs/maskedpkg
        // visible too.
        assert_eq!(
            resolve_real("dev-libs", "maskedpkg"),
            PretendOutcome::NoVisibleCandidate
        );
    }

    /// Regression test: a sibling package sharing a name prefix
    /// ("foo-bar" installed) must not be misread as an installed version
    /// of "foo" when scanning the vdb category directory. Without the
    /// digit-start guard in strip_version_prefix, this would wrongly
    /// report an Upgrade from a bogus "bar-2.0" pseudo-version instead of
    /// a clean New.
    #[test]
    fn sibling_package_prefix_does_not_contaminate_vdb_scan() {
        assert_eq!(
            resolve("dev-libs", "foo"),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        );
        assert_eq!(
            resolve("dev-libs", "foo-bar"),
            PretendOutcome::AlreadyInstalled {
                version: "2.0".to_string()
            }
        );
    }

    fn graph(atom_str: &str) -> Vec<(String, PretendOutcome)> {
        let root = fixtures_root();
        resolve_pretend_graph(
            &root,
            &root,
            &[atom_str.to_string()],
            &test_config(),
            false,
            false,
            false,
            false,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend_graph({atom_str}) failed: {e}"))
        .entries
        .into_iter()
        .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
        .collect()
    }

    /// Like `graph`, but with `--nodeps` enabled.
    fn graph_nodeps(atom_str: &str) -> Vec<(String, PretendOutcome)> {
        let root = fixtures_root();
        resolve_pretend_graph(
            &root,
            &root,
            &[atom_str.to_string()],
            &test_config(),
            false,
            false,
            true,
            false,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend_graph({atom_str}) failed: {e}"))
        .entries
        .into_iter()
        .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
        .collect()
    }

    /// Like `graph`, but with `--update` enabled -- proves `update` threads
    /// through the whole BFS, not just a top-level atom (see
    /// `resolve_pretend_graph`'s own doc comment).
    fn graph_update(atom_str: &str) -> Vec<(String, PretendOutcome)> {
        let root = fixtures_root();
        resolve_pretend_graph(
            &root,
            &root,
            &[atom_str.to_string()],
            &test_config(),
            false,
            false,
            false,
            true,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend_graph({atom_str}) failed: {e}"))
        .entries
        .into_iter()
        .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
        .collect()
    }

    #[test]
    fn recursion_basic_chain() {
        // dev-libs/upgradepkg is installed at 1.0 with a newer 2.0
        // visible in the tree too -- without --update, it stays
        // AlreadyInstalled, same as resolve_pretend's own
        // without_update_an_installed_version_that_still_satisfies_the_atom_is_kept
        // test above, just reached here as a dependency instead of a
        // top-level atom.
        let entries = graph("dev-libs/withdeps");
        assert_eq!(
            entries,
            vec![
                (
                    "dev-libs/withdeps".to_string(),
                    PretendOutcome::New {
                        version: "1.0".to_string()
                    }
                ),
                (
                    "dev-libs/newpkg".to_string(),
                    PretendOutcome::New {
                        version: "1.0".to_string()
                    }
                ),
                (
                    "dev-libs/upgradepkg".to_string(),
                    PretendOutcome::AlreadyInstalled {
                        version: "1.0".to_string()
                    }
                ),
            ]
        );
    }

    #[test]
    fn recursion_basic_chain_with_update_upgrades_the_dependency() {
        // Same fixture as recursion_basic_chain above, but with --update:
        // dev-libs/upgradepkg -- reached only as a *dependency* of
        // withdeps, never a top-level atom -- still upgrades, proving
        // `update` threads uniformly through the whole BFS (see
        // resolve_pretend_graph's own doc comment), not just top-level
        // atoms.
        let entries = graph_update("dev-libs/withdeps");
        assert_eq!(
            entries,
            vec![
                (
                    "dev-libs/withdeps".to_string(),
                    PretendOutcome::New {
                        version: "1.0".to_string()
                    }
                ),
                (
                    "dev-libs/newpkg".to_string(),
                    PretendOutcome::New {
                        version: "1.0".to_string()
                    }
                ),
                (
                    "dev-libs/upgradepkg".to_string(),
                    PretendOutcome::Upgrade {
                        from: "1.0".to_string(),
                        to: "2.0".to_string(),
                    }
                ),
            ]
        );
    }

    #[test]
    fn nodeps_resolves_the_top_level_atom_but_never_recurses() {
        // Same dev-libs/withdeps fixture as recursion_basic_chain above --
        // with --nodeps, only the top-level atom itself is ever resolved;
        // its own DEPEND/RDEPEND (which would otherwise pull in newpkg
        // and upgradepkg) is never even read.
        let entries = graph_nodeps("dev-libs/withdeps");
        assert_eq!(
            entries,
            vec![(
                "dev-libs/withdeps".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            )]
        );
    }

    #[test]
    fn nodeps_still_computes_use_flags_display_for_the_top_level_atom() {
        // Real portage's own -v USE display is about a package's own
        // metadata, unrelated to whether its dependencies get walked --
        // --nodeps must not blank it out. dev-libs/useflagpkg's own
        // foo?-gated RDEPEND on dev-libs/newpkg proves the *dependency*
        // walk really is skipped (no second entry), while its own
        // IUSE="foo missingflag" still shows up correctly.
        let root = fixtures_root();
        let config = portage_profile::resolve_config(&root, &root.join("repo"))
            .expect("fixture config resolves");
        let entries = resolve_pretend_graph(
            &root,
            &root,
            &["dev-libs/useflagpkg".to_string()],
            &config,
            false,
            false,
            true,
            false,
        )
        .expect("resolve_pretend_graph must succeed")
        .entries;
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].use_flags_display,
            vec![
                ("foo".to_string(), true),
                ("missingflag".to_string(), false),
            ]
        );
    }

    #[test]
    fn recursion_walks_bdepend_pdepend_idepend_same_as_depend_rdepend() {
        for pkg in ["bdependpkg", "pdependpkg", "idependpkg"] {
            let entries = graph(&format!("dev-libs/{pkg}"));
            assert_eq!(
                entries,
                vec![
                    (
                        format!("dev-libs/{pkg}"),
                        PretendOutcome::New {
                            version: "1.0".to_string()
                        }
                    ),
                    (
                        "dev-libs/newpkg".to_string(),
                        PretendOutcome::New {
                            version: "1.0".to_string()
                        }
                    ),
                ],
                "{pkg}"
            );
        }
    }

    #[test]
    fn recursion_dedupes_diamond_dependency() {
        let entries = graph("dev-libs/diamond");
        let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "dev-libs/diamond",
                "dev-libs/shared-a",
                "dev-libs/shared-b",
                "dev-libs/common",
            ]
        );
        // "common" must appear exactly once despite being reachable via
        // both shared-a and shared-b.
        assert_eq!(names.iter().filter(|n| **n == "dev-libs/common").count(), 1);
    }

    #[test]
    fn recursion_terminates_on_a_dependency_cycle() {
        let entries = graph("dev-libs/cycle-a");
        let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["dev-libs/cycle-a", "dev-libs/cycle-b"]);
    }

    #[test]
    fn recursion_resolves_every_any_of_alternative() {
        // v1 documented simplification: || resolves every alternative,
        // not just the first (see resolve_pretend_graph's doc comment).
        let entries = graph("dev-libs/anyof");
        let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            vec!["dev-libs/anyof", "dev-libs/newpkg", "dev-libs/samepkg"]
        );
    }

    #[test]
    fn recursion_survives_an_unresolvable_dependency() {
        // The top-level package still resolves, and the unresolvable
        // dependency still shows up in the graph (as NoVisibleCandidate,
        // not silently dropped) -- it just isn't recursed into further.
        let entries = graph("dev-libs/missingdep");
        assert_eq!(
            entries,
            vec![
                (
                    "dev-libs/missingdep".to_string(),
                    PretendOutcome::New {
                        version: "1.0".to_string()
                    }
                ),
                (
                    "dev-libs/doesnotexist-anywhere".to_string(),
                    PretendOutcome::NoVisibleCandidate
                ),
            ]
        );
    }

    #[test]
    fn recursion_dedupes_across_depend_and_rdepend() {
        let entries = graph("dev-libs/dualdep");
        let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["dev-libs/dualdep", "dev-libs/newpkg"]);
    }

    /// End-to-end wiring check: uses the *real* profile-resolved config
    /// (via portage-profile, a dev-dependency only) instead of the fixed
    /// test_accept_keywords()/empty-USE sets every other test in this file
    /// uses, proving real USE flags -- not just a hardcoded empty set --
    /// actually reach use_reduce and change dependency resolution.
    /// PORTING/fixtures/repo/profiles resolves "foo" enabled and
    /// "missingflag" disabled (see portage-profile's own fixture test),
    /// so useflagpkg's `foo? ( dev-libs/newpkg )` must be pulled in and
    /// its `missingflag? ( dev-libs/hiddendep )` must not be.
    #[test]
    fn real_resolved_use_flags_drive_dependency_recursion() {
        let root = fixtures_root();
        let config = portage_profile::resolve_config(&root, &root.join("repo"))
            .expect("fixture config resolves");
        let entries = resolve_pretend_graph(
            &root,
            &root,
            &["dev-libs/useflagpkg".to_string()],
            &config,
            false,
            false,
            false,
            false,
        )
        .expect("resolve_pretend_graph must succeed")
        .entries;
        let full_names: Vec<String> = entries
            .iter()
            .map(|e| format!("{}/{}", e.category, e.package))
            .collect();
        assert_eq!(full_names, vec!["dev-libs/useflagpkg", "dev-libs/newpkg"]);
    }

    fn graph_real(atom_str: &str) -> Vec<(String, PretendOutcome)> {
        let root = fixtures_root();
        let config = portage_profile::resolve_config(&root, &root.join("repo"))
            .expect("fixture config resolves");
        resolve_pretend_graph(
            &root,
            &root,
            &[atom_str.to_string()],
            &config,
            false,
            false,
            false,
            false,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend_graph({atom_str}) failed: {e}"))
        .entries
        .into_iter()
        .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
        .collect()
    }

    /// Like `graph_real`, but for a call expected to fail outright --
    /// returns the error message instead of panicking on one.
    fn graph_real_err(atom_str: &str) -> String {
        let root = fixtures_root();
        let config = portage_profile::resolve_config(&root, &root.join("repo"))
            .expect("fixture config resolves");
        resolve_pretend_graph(
            &root,
            &root,
            &[atom_str.to_string()],
            &config,
            false,
            false,
            false,
            false,
        )
        .expect_err(&format!(
            "resolve_pretend_graph({atom_str}) should have failed"
        ))
    }

    /// Like `graph_real`, but with `--newuse` enabled.
    fn graph_real_newuse(atom_str: &str) -> Vec<(String, PretendOutcome)> {
        let root = fixtures_root();
        let config = portage_profile::resolve_config(&root, &root.join("repo"))
            .expect("fixture config resolves");
        resolve_pretend_graph(
            &root,
            &root,
            &[atom_str.to_string()],
            &config,
            true,
            false,
            false,
            false,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend_graph({atom_str}) failed: {e}"))
        .entries
        .into_iter()
        .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
        .collect()
    }

    /// Like `graph_real`, but with `--changed-use` enabled.
    fn graph_real_changed_use(atom_str: &str) -> Vec<(String, PretendOutcome)> {
        let root = fixtures_root();
        let config = portage_profile::resolve_config(&root, &root.join("repo"))
            .expect("fixture config resolves");
        resolve_pretend_graph(
            &root,
            &root,
            &[atom_str.to_string()],
            &config,
            false,
            true,
            false,
            false,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend_graph({atom_str}) failed: {e}"))
        .entries
        .into_iter()
        .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
        .collect()
    }

    /// Like `graph_real`, but with `--nodeps` enabled.
    fn graph_real_nodeps(atom_str: &str) -> Vec<(String, PretendOutcome)> {
        let root = fixtures_root();
        let config = portage_profile::resolve_config(&root, &root.join("repo"))
            .expect("fixture config resolves");
        resolve_pretend_graph(
            &root,
            &root,
            &[atom_str.to_string()],
            &config,
            false,
            false,
            true,
            false,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend_graph({atom_str}) failed: {e}"))
        .entries
        .into_iter()
        .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
        .collect()
    }

    #[test]
    fn newuse_reinstall_still_recurses_into_its_own_dependencies() {
        // dev-libs/reinstallpkg RDEPENDs on dev-libs/newpkg -- proving a
        // Reinstall entry is walked for dependencies exactly like New/
        // Upgrade, not treated like the AlreadyInstalled dead-end it used
        // to be before --newuse existed.
        assert_eq!(
            graph_real_newuse("dev-libs/reinstallpkg"),
            vec![
                (
                    "dev-libs/reinstallpkg".to_string(),
                    PretendOutcome::Reinstall {
                        version: "1.0".to_string(),
                        changed_flags: vec!["foo".to_string()],
                    }
                ),
                (
                    "dev-libs/newpkg".to_string(),
                    PretendOutcome::New {
                        version: "1.0".to_string()
                    }
                ),
            ]
        );
    }

    #[test]
    fn fixture_package_use_enables_a_flag_not_on_globally() {
        // Neither the profile nor make.conf enables "pkguseflag", but
        // fixtures/etc/portage/package.use has a "*/packageuseenablepkg
        // pkguseflag" wildcard entry, so newpkg's foo?-unrelated,
        // pkguseflag?-gated dependency must be pulled in.
        let full_names: Vec<String> = graph_real("dev-libs/packageuseenablepkg")
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(
            full_names,
            vec!["dev-libs/packageuseenablepkg", "dev-libs/newpkg"]
        );
    }

    #[test]
    fn nodeps_skips_recursion_even_when_package_use_would_otherwise_pull_something_in() {
        // Same fixture as fixture_package_use_enables_a_flag_not_on_globally
        // above, but with --nodeps: the real profile-resolved config still
        // decides packageuseenablepkg's own visibility/USE normally, it's
        // only the *recursion* into its now-enabled pkguseflag?-gated
        // RDEPEND that --nodeps skips -- dev-libs/newpkg must NOT appear.
        let full_names: Vec<String> = graph_real_nodeps("dev-libs/packageuseenablepkg")
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(full_names, vec!["dev-libs/packageuseenablepkg"]);
    }

    #[test]
    fn fixture_package_use_disables_a_flag_that_is_on_globally() {
        // The fixture profile chain enables "foo" globally (see
        // resolves_fixture_profile_chain_and_make_conf in portage-profile,
        // and dev-libs/useflagpkg's own test above, which -- unlike this
        // package -- DOES pull in its foo?-gated dependency), but
        // fixtures/etc/portage/package.use has a "dev-libs/packageusedisablepkg
        // -foo" entry scoped to just this package, so its own foo?-gated
        // dependency must NOT be pulled in.
        let full_names: Vec<String> = graph_real("dev-libs/packageusedisablepkg")
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(full_names, vec!["dev-libs/packageusedisablepkg"]);
    }

    #[test]
    fn use_dep_enforcement_matches_a_declared_and_enabled_flag() {
        // dev-libs/useflagpkg's own IUSE="foo missingflag", "foo"
        // enabled globally by the fixture profile chain -- see
        // resolve_pretend's own doc comment on use_deps_satisfied.
        assert_eq!(
            resolve_real("dev-libs", "useflagpkg[foo]"),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn use_dep_enforcement_rejects_a_negated_but_actually_enabled_flag() {
        assert_eq!(
            resolve_real("dev-libs", "useflagpkg[-foo]"),
            PretendOutcome::NoVisibleCandidate
        );
    }

    #[test]
    fn use_dep_enforcement_rejects_a_declared_but_disabled_flag() {
        assert_eq!(
            resolve_real("dev-libs", "useflagpkg[missingflag]"),
            PretendOutcome::NoVisibleCandidate
        );
    }

    #[test]
    fn use_dep_enforcement_matches_a_negated_and_actually_disabled_flag() {
        assert_eq!(
            resolve_real("dev-libs", "useflagpkg[-missingflag]"),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn use_dep_enforcement_rejects_an_undeclared_flag_with_no_default() {
        assert_eq!(
            resolve_real("dev-libs", "useflagpkg[nonexistentflag]"),
            PretendOutcome::NoVisibleCandidate
        );
    }

    #[test]
    fn use_dep_enforcement_plus_default_rescues_an_undeclared_flag() {
        assert_eq!(
            resolve_real("dev-libs", "useflagpkg[nonexistentflag(+)]"),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        );
    }

    #[test]
    fn use_dep_enforcement_rejects_a_dependency_atom_but_does_not_fail_the_graph() {
        // dev-libs/usedeprejectedpkg's own RDEPEND is
        // "dev-libs/useflagpkg[-foo]", genuinely unsatisfiable -- the
        // parent still resolves, and the dependency gets its own
        // NoVisibleCandidate entry (reported, not silently dropped or
        // failing the whole graph -- same "report, don't fail"
        // precedent an unresolvable dependency atom already gets; see
        // resolve_pretend_graph's own doc comment).
        let entries = graph_real("dev-libs/usedeprejectedpkg");
        assert_eq!(
            entries,
            vec![
                (
                    "dev-libs/usedeprejectedpkg".to_string(),
                    PretendOutcome::New {
                        version: "1.0".to_string()
                    }
                ),
                (
                    "dev-libs/useflagpkg".to_string(),
                    PretendOutcome::NoVisibleCandidate
                ),
            ]
        );
    }

    #[test]
    fn required_use_satisfied_resolves_normally() {
        // dev-libs/requireduseokpkg's own REQUIRED_USE is "foo? ( bar )"
        // -- foo enabled globally, bar forced on by this package's own
        // package.use entry, so genuinely satisfied.
        assert_eq!(
            graph_real("dev-libs/requireduseokpkg"),
            vec![(
                "dev-libs/requireduseokpkg".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            )]
        );
    }

    #[test]
    fn required_use_violated_top_level_fails_the_whole_call() {
        // dev-libs/requiredusebadpkg's own REQUIRED_USE is "foo? ( bar )"
        // -- foo enabled globally, bar never forced on, genuinely
        // violated. Real depgraph.py's own REQUIRED_USE check happens
        // right after package selection and aborts the whole run on
        // failure -- a materially different severity than a merely
        // unresolvable dependency (report, don't fail).
        let err = graph_real_err("dev-libs/requiredusebadpkg");
        assert_eq!(
            err,
            "REQUIRED_USE not satisfied for dev-libs/requiredusebadpkg-1.0: \"foo? ( bar )\""
        );
    }

    #[test]
    fn required_use_violated_dependency_still_fails_the_whole_call() {
        // dev-libs/requiredusebadparentpkg RDEPENDs on
        // dev-libs/requiredusebadpkg -- proving the same fatal severity
        // applies regardless of whether the violating package was
        // reached as a top-level atom or a dependency.
        let err = graph_real_err("dev-libs/requiredusebadparentpkg");
        assert_eq!(
            err,
            "REQUIRED_USE not satisfied for dev-libs/requiredusebadpkg-1.0: \"foo? ( bar )\""
        );
    }

    fn graph_result_real(atom_str: &str) -> GraphResult {
        let root = fixtures_root();
        let config = portage_profile::resolve_config(&root, &root.join("repo"))
            .expect("fixture config resolves");
        resolve_pretend_graph(
            &root,
            &root,
            &[atom_str.to_string()],
            &config,
            false,
            false,
            false,
            false,
        )
        .unwrap_or_else(|e| panic!("resolve_pretend_graph({atom_str}) failed: {e}"))
    }

    fn graph_entries_real(atom_str: &str) -> Vec<GraphEntry> {
        graph_result_real(atom_str).entries
    }

    #[test]
    fn fixture_strong_blocker_matches_an_installed_package() {
        // dev-libs/blockerpkg's RDEPEND is "!!dev-libs/samepkg", and
        // dev-libs/samepkg-1.0 is already installed per the fixture vdb.
        let entries = graph_entries_real("dev-libs/blockerpkg");
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].blockers,
            vec![BlockerConflict {
                atom_str: "!!dev-libs/samepkg".to_string(),
                strong: true,
                matched_category: "dev-libs".to_string(),
                matched_package: "samepkg".to_string(),
                matched_version: "1.0".to_string(),
            }]
        );
    }

    #[test]
    fn fixture_weak_blocker_matches_another_new_package_in_the_same_graph() {
        // dev-libs/graphblockerparent pulls in both dev-libs/blockerpartnerpkg
        // and dev-libs/weakblockerpkg (whose RDEPEND is
        // "!dev-libs/blockerpartnerpkg") as New in the same run, so the
        // weak blocker must be reported against blockerpartnerpkg's
        // graph-resolved version, not just against the (empty) vdb.
        let entries = graph_entries_real("dev-libs/graphblockerparent");
        let full_names: Vec<String> = entries
            .iter()
            .map(|e| format!("{}/{}", e.category, e.package))
            .collect();
        assert_eq!(
            full_names,
            vec![
                "dev-libs/graphblockerparent",
                "dev-libs/blockerpartnerpkg",
                "dev-libs/weakblockerpkg",
            ]
        );
        assert!(entries[0].blockers.is_empty());
        assert!(entries[1].blockers.is_empty());
        assert_eq!(
            entries[2].blockers,
            vec![BlockerConflict {
                atom_str: "!dev-libs/blockerpartnerpkg".to_string(),
                strong: false,
                matched_category: "dev-libs".to_string(),
                matched_package: "blockerpartnerpkg".to_string(),
                matched_version: "1.0".to_string(),
            }]
        );
    }

    #[test]
    fn fixture_unrelated_packages_report_no_blockers() {
        // Regression guard: the diamond fixture has no blockers at all, so
        // none of its entries should gain a spurious one.
        let entries = graph_entries_real("dev-libs/diamond");
        assert!(entries.iter().all(|e| e.blockers.is_empty()));
    }

    fn candidate(version: &str, keywords: &[&str]) -> Candidate {
        Candidate {
            version: version.to_string(),
            keywords: keywords.iter().map(|s| s.to_string()).collect(),
            slot: "0".to_string(),
            repo_location: PathBuf::new(),
            repo_priority: 0,
            repo_name: "test".to_string(),
        }
    }

    #[test]
    fn package_mask_hides_a_matching_candidate() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            package_mask: vec!["dev-libs/foo".to_string()],
            ..Default::default()
        };
        assert!(!is_visible(
            &candidate("1.0", &["amd64"]),
            "dev-libs",
            "foo",
            &config
        ));
    }

    #[test]
    fn package_unmask_cancels_a_matching_mask() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            package_mask: vec!["dev-libs/foo".to_string()],
            package_unmask: vec!["dev-libs/foo".to_string()],
            ..Default::default()
        };
        assert!(is_visible(
            &candidate("1.0", &["amd64"]),
            "dev-libs",
            "foo",
            &config
        ));
    }

    #[test]
    fn package_mask_wildcard_matches_whole_category() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            package_mask: vec!["dev-libs/*".to_string()],
            ..Default::default()
        };
        assert!(!is_visible(
            &candidate("1.0", &["amd64"]),
            "dev-libs",
            "anything",
            &config
        ));
        assert!(is_visible(
            &candidate("1.0", &["amd64"]),
            "app-misc",
            "anything",
            &config
        ));
    }

    #[test]
    fn package_accept_keywords_wildcard_extends_visibility() {
        // Globally only "amd64" is accepted, but a package.accept_keywords
        // wildcard entry additionally accepts "~amd64" for dev-qt/*.
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            package_accept_keywords: vec![("dev-qt/*".to_string(), vec!["~amd64".to_string()])],
            ..Default::default()
        };
        assert!(is_visible(
            &candidate("1.0", &["~amd64"]),
            "dev-qt",
            "qtcore",
            &config
        ));
        // A package outside the wildcard doesn't get the extra keyword.
        assert!(!is_visible(
            &candidate("1.0", &["~amd64"]),
            "dev-libs",
            "foo",
            &config
        ));
    }

    #[test]
    fn package_accept_keywords_double_star_accepts_unconditionally() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            package_accept_keywords: vec![("dev-libs/live".to_string(), vec!["**".to_string()])],
            ..Default::default()
        };
        // No KEYWORDS at all (e.g. a live/9999 ebuild) is still visible.
        assert!(is_visible(
            &candidate("9999", &[]),
            "dev-libs",
            "live",
            &config
        ));
    }

    #[test]
    fn effective_use_flags_layers_a_matching_package_use_entry_on_top_of_base() {
        let base = HashSet::from(["foo".to_string()]);
        let package_use = vec![(
            "dev-libs/bar".to_string(),
            vec!["baz".to_string(), "-foo".to_string()],
        )];
        let use_flags = effective_use_flags(
            &base,
            &package_use,
            &[],
            &[],
            "dev-libs/bar-1.0:0",
            "dev-libs",
            "bar",
        );
        assert_eq!(use_flags, HashSet::from(["baz".to_string()]));
    }

    #[test]
    fn effective_use_flags_does_not_affect_a_non_matching_package() {
        let base = HashSet::from(["foo".to_string()]);
        let package_use = vec![("dev-libs/bar".to_string(), vec!["baz".to_string()])];
        let use_flags = effective_use_flags(
            &base,
            &package_use,
            &[],
            &[],
            "dev-libs/unrelated-1.0:0",
            "dev-libs",
            "unrelated",
        );
        assert_eq!(use_flags, base);
    }

    #[test]
    fn effective_use_flags_matches_a_wildcard_package_use_entry() {
        let base = HashSet::new();
        let package_use = vec![("*/bar".to_string(), vec!["baz".to_string()])];
        let use_flags = effective_use_flags(
            &base,
            &package_use,
            &[],
            &[],
            "dev-libs/bar-1.0:0",
            "dev-libs",
            "bar",
        );
        assert_eq!(use_flags, HashSet::from(["baz".to_string()]));
    }

    #[test]
    fn effective_use_flags_applies_package_use_force_and_mask() {
        let base = HashSet::new();
        let package_use_force = vec![("dev-libs/bar".to_string(), vec!["forceflag".to_string()])];
        let package_use_mask = vec![("dev-libs/bar".to_string(), vec!["maskflag".to_string()])];
        let use_flags = effective_use_flags(
            &base,
            &[],
            &package_use_force,
            &package_use_mask,
            "dev-libs/bar-1.0:0",
            "dev-libs",
            "bar",
        );
        assert_eq!(use_flags, HashSet::from(["forceflag".to_string()]));
    }

    #[test]
    fn effective_use_flags_package_use_mask_wins_over_force_on_conflict() {
        // Same flag both forced and masked by two entries at the SAME
        // specificity tier -- mask must win, matching real portage's own
        // force-then-mask application order (see effective_use_flags's
        // own doc comment).
        let base = HashSet::new();
        let package_use_force = vec![("dev-libs/bar".to_string(), vec!["bothflag".to_string()])];
        let package_use_mask = vec![("dev-libs/bar".to_string(), vec!["bothflag".to_string()])];
        let use_flags = effective_use_flags(
            &base,
            &[],
            &package_use_force,
            &package_use_mask,
            "dev-libs/bar-1.0:0",
            "dev-libs",
            "bar",
        );
        assert!(!use_flags.contains("bothflag"));
    }

    #[test]
    fn specificity_ordered_flags_lets_a_more_specific_entry_override_a_less_specific_one() {
        // A bare atom masks "flag", a more specific exact-version atom
        // un-masks it again ("-flag") -- the more specific entry must
        // win regardless of which order the two entries appear in the
        // input list, proving this is genuine specificity-based
        // reordering, not just "last entry wins".
        let entries = vec![
            ("=dev-libs/bar-1.0".to_string(), vec!["-flag".to_string()]),
            ("dev-libs/bar".to_string(), vec!["flag".to_string()]),
        ];
        let flags = specificity_ordered_flags(&entries, "dev-libs/bar-1.0:0", "dev-libs", "bar");
        assert!(!flags.contains("flag"));
    }

    #[test]
    fn atom_specificity_ranks_exact_version_above_bare_above_wildcard() {
        assert!(atom_specificity("=dev-libs/bar-1.0") > atom_specificity("dev-libs/bar:0"));
        assert!(atom_specificity("dev-libs/bar:0") > atom_specificity("dev-libs/bar"));
        assert!(atom_specificity("dev-libs/bar") > atom_specificity("*/*"));
    }

    fn graph_entry(category: &str, package: &str, version: &str) -> GraphEntry {
        GraphEntry {
            category: category.to_string(),
            package: package.to_string(),
            outcome: PretendOutcome::New {
                version: version.to_string(),
            },
            blockers: Vec::new(),
            slot: Some("0".to_string()),
            use_flags_display: Vec::new(),
        }
    }

    #[test]
    fn resolve_blockers_matches_a_graph_resolved_package_with_no_installed_candidates() {
        let entries = vec![
            graph_entry("dev-libs", "owner", "1.0"),
            graph_entry("dev-libs", "target", "2.0"),
        ];
        let pending = vec![PendingBlocker {
            atom_str: "!!dev-libs/target".to_string(),
            strong: true,
            target_category: "dev-libs".to_string(),
            target_package: "target".to_string(),
            owner_key: ("dev-libs".to_string(), "owner".to_string()),
            owner_version: "1.0".to_string(),
        }];
        let conflicts = resolve_blockers(
            Path::new("/nonexistent-root-for-this-test"),
            &pending,
            &entries,
        );
        assert_eq!(
            conflicts,
            vec![(
                ("dev-libs".to_string(), "owner".to_string()),
                BlockerConflict {
                    atom_str: "!!dev-libs/target".to_string(),
                    strong: true,
                    matched_category: "dev-libs".to_string(),
                    matched_package: "target".to_string(),
                    matched_version: "2.0".to_string(),
                }
            )]
        );
    }

    #[test]
    fn resolve_blockers_skips_a_blocker_matching_its_own_owner() {
        let entries = vec![graph_entry("dev-libs", "owner", "1.0")];
        let pending = vec![PendingBlocker {
            atom_str: "!dev-libs/owner".to_string(),
            strong: false,
            target_category: "dev-libs".to_string(),
            target_package: "owner".to_string(),
            owner_key: ("dev-libs".to_string(), "owner".to_string()),
            owner_version: "1.0".to_string(),
        }];
        let conflicts = resolve_blockers(
            Path::new("/nonexistent-root-for-this-test"),
            &pending,
            &entries,
        );
        assert!(conflicts.is_empty());
    }

    #[test]
    fn resolve_blockers_returns_nothing_when_the_target_matches_nothing() {
        let entries = vec![graph_entry("dev-libs", "owner", "1.0")];
        let pending = vec![PendingBlocker {
            atom_str: "!dev-libs/nonexistent".to_string(),
            strong: false,
            target_category: "dev-libs".to_string(),
            target_package: "nonexistent".to_string(),
            owner_key: ("dev-libs".to_string(), "owner".to_string()),
            owner_version: "1.0".to_string(),
        }];
        let conflicts = resolve_blockers(
            Path::new("/nonexistent-root-for-this-test"),
            &pending,
            &entries,
        );
        assert!(conflicts.is_empty());
    }
}
