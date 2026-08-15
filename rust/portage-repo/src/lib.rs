// Repo/metadata/vdb access for the single-atom `emerge --pretend` pilot
// slice (see PORTING/PROMPT.md's depgraph/config-resolution follow-up
// work, and PORTING/README.md for the full scope writeup).
//
// KNOWN, DOCUMENTED SCOPE CUTS for v1 (all confirmed with the user before
// implementing):
//   - Only the main repo (`repos.conf`'s `[DEFAULT] main-repo`) is
//     consulted; overlays are ignored.
//   - Ebuild metadata comes from `metadata/md5-cache/<cat>/<pf>` (plain
//     `KEY=value` text -- confirmed against a real vendored tree), never
//     from executing the ebuild in bash. This is deliberate: it lets
//     `--pretend` work without the bash dependency that real phase
//     execution will eventually require (see PROMPT.md's "Deferred:
//     ebuild phase execution").
//   - No package.mask/.use/.accept_keywords, no slot conflicts, no
//     virtuals, no backtracking.
//
// USE and ACCEPT_KEYWORDS are no longer hardcoded: they're computed by
// `portage_profile::resolve_config` (real profile chain + make.conf, with
// its own documented scope cuts -- see that crate's doc comment) and
// threaded through `resolve_pretend`/`resolve_pretend_graph` as plain
// parameters, keeping this crate decoupled from profile-parsing concerns.
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
/// returns the `[DEFAULT] main-repo`'s location. Overlays are ignored.
pub fn find_main_repo(config_root: &Path) -> Result<RepoConfig, String> {
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

    let location = sections
        .get(&main_repo)
        .and_then(|s| s.get("location"))
        .ok_or_else(|| format!("no location for repo {main_repo:?} in repos.conf"))?
        .clone();
    let location = PathBuf::from(location);
    // Real repos.conf always uses absolute locations; relative ones are a
    // pilot/testing convenience so the fixture tree under PORTING/fixtures
    // can be committed without a machine-specific absolute path baked in.
    let location = if location.is_absolute() {
        location
    } else {
        config_root.join(location)
    };

    Ok(RepoConfig {
        name: main_repo,
        location,
    })
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

/// Lists every version of `category/package` that has an ebuild in the
/// repo, with metadata (KEYWORDS, SLOT) from the md5-cache. A candidate
/// whose cache entry is missing or unreadable is silently skipped (v1
/// doesn't distinguish "stale cache" from "doesn't exist" -- both just
/// mean "not visible").
pub fn list_candidates(
    repo_location: &Path,
    category: &str,
    package: &str,
) -> Result<Vec<Candidate>, String> {
    let pkg_dir = repo_location.join(category).join(package);
    if !pkg_dir.is_dir() {
        return Ok(Vec::new());
    }
    let entries =
        fs::read_dir(&pkg_dir).map_err(|e| format!("reading {}: {e}", pkg_dir.display()))?;

    let mut candidates = Vec::new();
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
        let Ok(metadata) = read_md5_cache(repo_location, category, stem) else {
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
        });
    }
    Ok(candidates)
}

pub fn is_visible(candidate: &Candidate, accept_keywords: &HashSet<String>) -> bool {
    candidate
        .keywords
        .iter()
        .any(|k| accept_keywords.contains(k))
}

fn vercmp_ordering(a: &str, b: &str) -> Ordering {
    match vercmp(a, b) {
        Some(n) if n > 0 => Ordering::Greater,
        Some(n) if n < 0 => Ordering::Less,
        _ => Ordering::Equal,
    }
}

/// Lists every installed version of `category/package` found in the vdb
/// under `root` (`<root>/var/db/pkg/<category>/<package>-<version>/`).
pub fn installed_versions(root: &Path, category: &str, package: &str) -> Vec<String> {
    let cat_dir = root.join("var/db/pkg").join(category);
    let Ok(entries) = fs::read_dir(&cat_dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            strip_version_prefix(&name, package).map(|v| v.to_string())
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PretendOutcome {
    NoVisibleCandidate,
    New { version: String },
    Upgrade { from: String, to: String },
    AlreadyInstalled { version: String },
}

/// The single-atom v1 `emerge --pretend` decision: find the best visible
/// candidate matching `atom_str` in the given repo, compare it against
/// what's installed. `atom_str` may be a full atom (operator, slot --
/// anything portage-dep's v1 grammar supports), not just a bare
/// category/package: this is what lets dependency atoms extracted from
/// DEPEND/RDEPEND (see `resolve_pretend_graph`) reuse the exact same
/// resolution logic as the top-level CLI atom.
pub fn resolve_pretend(
    repo_location: &Path,
    root: &Path,
    atom_str: &str,
    accept_keywords: &HashSet<String>,
) -> Result<PretendOutcome, String> {
    let atom =
        portage_dep::parse_atom(atom_str).ok_or_else(|| format!("invalid atom {atom_str:?}"))?;

    let candidates = list_candidates(repo_location, &atom.category, &atom.package)?;
    let visible: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| is_visible(c, accept_keywords))
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
                "{}/{}-{}:{}",
                atom.category, atom.package, c.version, c.slot
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
    let Some(best) = matched
        .iter()
        .filter_map(|m| by_str.get(m).copied())
        .max_by(|a, b| vercmp_ordering(&a.version, &b.version))
    else {
        return Ok(PretendOutcome::NoVisibleCandidate);
    };

    let installed = installed_versions(root, &atom.category, &atom.package);
    if installed.iter().any(|v| v == &best.version) {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEntry {
    pub category: String,
    pub package: String,
    pub outcome: PretendOutcome,
}

/// Recursively resolves `atom_str` and -- for packages that would newly
/// merge or upgrade -- its DEPEND+RDEPEND atoms, breadth-first. Returns
/// one `GraphEntry` per distinct category/package visited, in discovery
/// order (not topologically sorted).
///
/// KNOWN, DOCUMENTED SCOPE CUTS (all confirmed with the user before
/// implementing):
///   - Only DEPEND and RDEPEND are walked, not BDEPEND/IDEPEND/PDEPEND;
///     atoms are deduped across both fields (and across packages) via the
///     shared visited set.
///   - `use_flags`/`accept_keywords` are supplied by the caller (computed
///     via `portage_profile::resolve_config` -- see that crate's doc
///     comment for what real profile/make.conf mechanics are and aren't
///     implemented) rather than being read here; this crate stays
///     decoupled from profile-parsing concerns.
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
///     pilot slice.
///   - Blockers (`!foo/bar` tokens) are recognized and skipped, not
///     resolved or enforced.
///   - A package's dependencies are only walked if resolving it produced
///     New or Upgrade; an already-installed package's own dependencies
///     are presumed already satisfied (v1 has no --newuse/--changed-use
///     equivalent).
pub fn resolve_pretend_graph(
    config_root: &Path,
    root: &Path,
    atom_str: &str,
    use_flags: &HashSet<String>,
    accept_keywords: &HashSet<String>,
) -> Result<Vec<GraphEntry>, String> {
    let repo = find_main_repo(config_root)?;

    let mut visited: HashSet<(String, String)> = HashSet::new();
    let mut entries = Vec::new();
    let mut queue: VecDeque<String> = VecDeque::new();
    queue.push_back(atom_str.to_string());

    while let Some(current_atom) = queue.pop_front() {
        let Some(atom) = portage_dep::parse_atom(&current_atom) else {
            continue;
        };
        if atom.blocker != portage_dep::Blocker::None {
            continue;
        }
        let key = (atom.category.clone(), atom.package.clone());
        if !visited.insert(key.clone()) {
            continue;
        }

        let outcome = resolve_pretend(&repo.location, root, &current_atom, accept_keywords)?;
        let resolved_version = match &outcome {
            PretendOutcome::New { version } => Some(version.clone()),
            PretendOutcome::Upgrade { to, .. } => Some(to.clone()),
            _ => None,
        };

        entries.push(GraphEntry {
            category: key.0.clone(),
            package: key.1.clone(),
            outcome,
        });

        let Some(version) = resolved_version else {
            continue;
        };

        let pf = format!("{}-{version}", key.1);
        let Ok(metadata) = read_md5_cache(&repo.location, &key.0, &pf) else {
            continue;
        };
        let mut depstr = String::new();
        for dep_key in ["DEPEND", "RDEPEND"] {
            if let Some(d) = metadata.get(dep_key) {
                depstr.push_str(d);
                depstr.push(' ');
            }
        }
        let tokens: Vec<String> = depstr.split_whitespace().map(String::from).collect();
        let Ok(flat_deps) = portage_use_reduce::use_reduce_flat(
            &tokens,
            use_flags,
            portage_use_reduce::MatchMode::Normal,
        ) else {
            continue;
        };
        for tok in flat_deps {
            if tok == "||" {
                continue;
            }
            queue.push_back(tok);
        }
    }

    Ok(entries)
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
    /// isolation, independent of real profile/make.conf parsing (that's
    /// portage-profile's job, tested separately) -- so visibility is
    /// pinned to a fixed "amd64" set here, matching what the fixture's
    /// real profile chain also happens to resolve to (see
    /// PORTING/fixtures/repo/profiles and test_emerge_pretend_contract.py
    /// for the end-to-end version of this).
    fn test_accept_keywords() -> HashSet<String> {
        HashSet::from(["amd64".to_string()])
    }

    fn resolve(category: &str, package: &str) -> PretendOutcome {
        let root = fixtures_root();
        let repo = find_main_repo(&root).expect("fixture repos.conf must resolve");
        let atom_str = format!("{category}/{package}");
        resolve_pretend(&repo.location, &root, &atom_str, &test_accept_keywords())
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
    fn upgrade() {
        assert_eq!(
            resolve("dev-libs", "upgradepkg"),
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
            atom_str,
            &HashSet::new(),
            &test_accept_keywords(),
        )
        .unwrap_or_else(|e| panic!("resolve_pretend_graph({atom_str}) failed: {e}"))
        .into_iter()
        .map(|e| (format!("{}/{}", e.category, e.package), e.outcome))
        .collect()
    }

    #[test]
    fn recursion_basic_chain() {
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
                    PretendOutcome::Upgrade {
                        from: "1.0".to_string(),
                        to: "2.0".to_string(),
                    }
                ),
            ]
        );
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
        let config = portage_profile::resolve_config(&root).expect("fixture config resolves");
        let entries = resolve_pretend_graph(
            &root,
            &root,
            "dev-libs/useflagpkg",
            &config.use_flags,
            &config.accept_keywords,
        )
        .expect("resolve_pretend_graph must succeed");
        let full_names: Vec<String> = entries
            .iter()
            .map(|e| format!("{}/{}", e.category, e.package))
            .collect();
        assert_eq!(full_names, vec!["dev-libs/useflagpkg", "dev-libs/newpkg"]);
    }
}
