// Repo/metadata/vdb access for the single-atom `emerge --pretend` pilot
// slice (see PORTING/PROMPT.md's depgraph/config-resolution follow-up
// work, and PORTING/README.md for the full scope writeup).
//
// KNOWN, DOCUMENTED SCOPE CUTS for v1 (all confirmed with the user before
// implementing):
//   - ACCEPT_KEYWORDS is hardcoded to "amd64" (`ACCEPT_KEYWORDS` const
//     below); make.conf/profile stacking is not read at all.
//   - Only the main repo (`repos.conf`'s `[DEFAULT] main-repo`) is
//     consulted; overlays are ignored.
//   - Ebuild metadata comes from `metadata/md5-cache/<cat>/<pf>` (plain
//     `KEY=value` text -- confirmed against a real vendored tree), never
//     from executing the ebuild in bash. This is deliberate: it lets
//     `--pretend` work without the bash dependency that real phase
//     execution will eventually require (see PROMPT.md's "Deferred:
//     ebuild phase execution").
//   - No dependency recursion (DEPEND/RDEPEND are read into the metadata
//     map but never walked), no package.mask/.use/.accept_keywords, no
//     slot conflicts, no blockers, no virtuals, no backtracking.
//
// Config/target roots are read from the real `PORTAGE_CONFIGROOT` and
// `ROOT` environment variables (portage's own mechanism for relocating
// `/etc/portage` and the install target -- see lib/portage/const.py),
// defaulting to `/` when unset. This is not a pilot-only convention: it's
// what lets tests point at a fixture tree without needing anything
// pilot-specific.

use portage_versions::vercmp;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// v1 hardcodes visibility to stable amd64 only -- see the module doc
/// comment.
pub const ACCEPT_KEYWORDS: &str = "amd64";

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

pub fn is_visible(candidate: &Candidate) -> bool {
    candidate.keywords.iter().any(|k| k == ACCEPT_KEYWORDS)
}

fn vercmp_ordering(a: &str, b: &str) -> Ordering {
    match vercmp(a, b) {
        Some(n) if n > 0 => Ordering::Greater,
        Some(n) if n < 0 => Ordering::Less,
        _ => Ordering::Equal,
    }
}

pub fn select_best_visible(candidates: &[Candidate]) -> Option<&Candidate> {
    candidates
        .iter()
        .filter(|c| is_visible(c))
        .max_by(|a, b| vercmp_ordering(&a.version, &b.version))
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

/// The whole v1 `emerge --pretend category/package` decision: find the
/// best visible candidate in the main repo, compare it against what's
/// installed. No dependency recursion -- see the module doc comment.
pub fn resolve_pretend(
    config_root: &Path,
    root: &Path,
    category: &str,
    package: &str,
) -> Result<PretendOutcome, String> {
    let repo = find_main_repo(config_root)?;
    let candidates = list_candidates(&repo.location, category, package)?;
    let Some(best) = select_best_visible(&candidates) else {
        return Ok(PretendOutcome::NoVisibleCandidate);
    };

    let installed = installed_versions(root, category, package);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .canonicalize()
            .expect("PORTING/fixtures must exist")
    }

    fn resolve(category: &str, package: &str) -> PretendOutcome {
        let root = fixtures_root();
        resolve_pretend(&root, &root, category, package)
            .unwrap_or_else(|e| panic!("resolve_pretend({category}/{package}) failed: {e}"))
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
}
