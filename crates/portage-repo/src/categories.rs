//! The repo-traversal category allow-list (`settings.categories`).

use crate::*;

/// Portage's `settings.categories`: the incremental union of every repo's
/// `profiles/categories` (main repo first, then overlays) plus
/// `<config_root>/etc/portage/categories`, keeping only names matching
/// Portage's `_category_re`. Regen traversal drops on-disk cps outside it.
// Portage: config.py (settings.categories), LocationsManager.py (profile_and_user_locations).
pub fn repo_categories(repos: &[RepoConfig], config_root: &Path) -> BTreeSet<String> {
    let mains_first = repos
        .iter()
        .filter(|r| r.is_main)
        .chain(repos.iter().filter(|r| !r.is_main));
    let mut sources: Vec<Vec<String>> = Vec::new();
    for repo in mains_first {
        sources.push(read_categories_file(
            &repo.location.join("profiles").join("categories"),
        ));
    }
    sources.push(read_categories_file(
        &config_root.join("etc").join("portage").join("categories"),
    ));
    stack_categories(&sources)
}

/// Portage's `settings.categories` for one run, built once and shared by
/// every lookup: the allow-list plus the once-per-cp record behind the
/// unlisted-category `!!!`. Portage memoises `cp_list` once frozen
/// (`xcache`), so the notice fires once per cp per process; sharing one
/// `Categories` across walks is this side's equivalent.
// Portage: dbapi/porttree.py, cp_list
#[derive(Debug)]
pub struct Categories {
    /// The resolved allow-list (repo files stacked, then the user file).
    pub set: BTreeSet<String>,
    /// `(category, package)` pairs the `!!!` already fired for.
    pub warned: RefCell<HashSet<(String, String)>>,
}

impl Categories {
    /// Read the allow-list from `repos` and `config_root` once.
    pub fn load(repos: &[RepoConfig], config_root: &Path) -> Self {
        Self {
            set: repo_categories(repos, config_root),
            warned: RefCell::new(HashSet::new()),
        }
    }
}

/// One `categories` file's tokens. A missing file contributes nothing.
// Portage: util/__init__.py, grabfile (comment/blank handling).
fn read_categories_file(path: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines() {
        for token in line.split_whitespace() {
            if token.starts_with('#') {
                break;
            }
            out.push(token.to_string());
        }
    }
    out
}

/// Fold the per-source token lists with `-` removal (`-*` resets).
// Portage: util/__init__.py, stack_lists(incremental=1).
fn stack_categories(sources: &[Vec<String>]) -> BTreeSet<String> {
    let mut stacked: BTreeSet<String> = BTreeSet::new();
    for lines in sources {
        for token in lines {
            if token == "-*" {
                stacked.clear();
            } else if let Some(removed) = token.strip_prefix('-') {
                stacked.remove(removed);
            } else {
                stacked.insert(token.clone());
            }
        }
    }
    stacked.retain(|name: &String| category_name_ok(name));
    stacked
}

/// Portage `dbapi._category_re` (`^\w[-.+\w]*$`, ASCII): a leading word
/// char, then word chars, dots, dashes or pluses.
fn category_name_ok(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphanumeric() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | '+'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use portage_util::TempDir;

    fn repo_config(name: &str, location: PathBuf, is_main: bool) -> RepoConfig {
        RepoConfig {
            name: name.to_string(),
            location,
            priority: if is_main { -1000 } else { 0 },
            is_main,
            masters: Vec::new(),
            profile_formats: Vec::new(),
            cache_formats: Vec::new(),
            aliases: Vec::new(),
            sync_type: None,
            sync_uri: None,
            volatile: false,
            module_specific_options: Vec::new(),
        }
    }

    fn write_categories(dir: &Path, names: &str) {
        std::fs::create_dir_all(dir.join("profiles")).unwrap();
        std::fs::write(dir.join("profiles").join("categories"), names).unwrap();
    }

    /// Union of the main repo, the overlay and the user file, in that
    /// precedence order; comments and blank lines contribute nothing.
    /// Portage: config.py:948-960.
    #[test]
    fn repo_categories_unions_repo_and_user_files() {
        let dir = TempDir::new("portuale-repo-categories").keep();
        let _ = std::fs::remove_dir_all(&dir);
        let main = dir.join("main");
        let overlay = dir.join("overlay");
        let cfg = dir.join("cfg");
        write_categories(&main, "# main repo\ndev-libs\nkde-base\n\n");
        write_categories(&overlay, "dev-python\n");
        std::fs::create_dir_all(cfg.join("etc/portage")).unwrap();
        std::fs::write(cfg.join("etc/portage/categories"), "media-libs\n").unwrap();
        let repos = vec![
            // Overlays sort before the main repo by name here on purpose:
            // the main repo still stacks first.
            repo_config("aaa-overlay", overlay, false),
            repo_config("testrepo", main, true),
        ];
        let got = repo_categories(&repos, &cfg);
        assert_eq!(
            got,
            ["dev-libs", "dev-python", "kde-base", "media-libs"]
                .into_iter()
                .map(str::to_string)
                .collect()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `-` removals and `-*` apply across the stack; unmatched removals
    /// are silent. Portage: stack_lists(incremental=1).
    #[test]
    fn repo_categories_stacks_removals_incrementally() {
        let dir = TempDir::new("portuale-repo-categories-removal").keep();
        let _ = std::fs::remove_dir_all(&dir);
        let main = dir.join("main");
        let overlay = dir.join("overlay");
        let cfg = dir.join("cfg");
        write_categories(&main, "dev-libs\ndev-python\n");
        write_categories(&overlay, "-dev-python\n-nothing-listed\n");
        std::fs::create_dir_all(cfg.join("etc/portage")).unwrap();
        std::fs::write(cfg.join("etc/portage/categories"), "-*\nnet-nds\n").unwrap();
        let repos = vec![
            repo_config("testrepo", main, true),
            repo_config("overlay", overlay, false),
        ];
        let got = repo_categories(&repos, &cfg);
        assert_eq!(got, ["net-nds".to_string()].into_iter().collect());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Missing files contribute nothing; invalid names are filtered.
    /// Portage: grabfile on absent files, dbapi._category_re.
    #[test]
    fn repo_categories_tolerates_missing_files_and_filters_names() {
        let dir = TempDir::new("portuale-repo-categories-names").keep();
        let _ = std::fs::remove_dir_all(&dir);
        let main = dir.join("main");
        let cfg = dir.join("cfg");
        // No profiles/categories under main, no user file either.
        std::fs::create_dir_all(main.join("profiles")).unwrap();
        let repos = vec![repo_config("testrepo", main.clone(), true)];
        assert!(repo_categories(&repos, &cfg).is_empty());
        // Names: leading alnum/underscore ok (digits included); a leading
        // dot never matches; inline comments cut the line.
        write_categories(&main, "9lives\n_under\n.dot\nok+plus # trailing\n");
        let got = repo_categories(&repos, &cfg);
        assert_eq!(
            got,
            ["9lives", "_under", "ok+plus"]
                .into_iter()
                .map(str::to_string)
                .collect()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The user file always stacks: with only the main repo listing
    /// `dev-libs`, the user file's `media-libs` joins the set.
    /// Portage: config.py:948-960.
    #[test]
    fn repo_categories_stacks_the_user_file() {
        let dir = TempDir::new("portuale-repo-categories-local").keep();
        let _ = std::fs::remove_dir_all(&dir);
        let main = dir.join("main");
        let cfg = dir.join("cfg");
        write_categories(&main, "dev-libs\n");
        std::fs::create_dir_all(cfg.join("etc/portage")).unwrap();
        std::fs::write(cfg.join("etc/portage/categories"), "media-libs\n").unwrap();
        let repos = vec![repo_config("testrepo", main, true)];
        assert_eq!(
            repo_categories(&repos, &cfg),
            ["dev-libs", "media-libs"]
                .into_iter()
                .map(str::to_string)
                .collect()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
