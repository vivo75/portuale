//! Task 12 (#571): short-name expansion reads the allow-listed tree.
//!
//! A bare name (`emerge p571-shared`) qualifies against `all_cp` over
//! `settings.categories` (Portage `cpv_expand.py` / `depgraph.py:4948`):
//! the overlay-listed `p571-shared` expands, the unlisted `p571-hidden`
//! is a plain miss with no `!!!` line. Oracle: Portage 3.0.83 host
//! (`LLM/evidence/2026-10-26-571/`, `portage-{shared,hidden}-short.*`).

use super::*;

fn p571_repos() -> (std::path::PathBuf, Vec<portage_repo::RepoConfig>) {
    let fixtures = fixtures_root();
    let repo = |name: &str, dir: &str, priority: i32, is_main: bool| portage_repo::RepoConfig {
        name: name.to_string(),
        location: fixtures.join(dir),
        priority,
        is_main,
        masters: Vec::new(),
        profile_formats: Vec::new(),
        cache_formats: Vec::new(),
        aliases: Vec::new(),
        sync_type: None,
        sync_uri: None,
        volatile: false,
        module_specific_options: Vec::new(),
    };
    let cfg = TempDir::new("portuale-571-expand").keep();
    let _ = std::fs::remove_dir_all(&cfg);
    std::fs::create_dir_all(&cfg).unwrap();
    (
        cfg,
        vec![
            repo("p571main", "repo-p571", -1000, true),
            repo("p571overlay", "overlay-p571", 60, false),
        ],
    )
}

fn expand_p571(atoms: &[&str]) -> Result<ExpandTargetsOut, std::process::ExitCode> {
    let (cfg, repos) = p571_repos();
    let opts = EmergeOpts {
        atom_args: atoms.to_vec(),
        pretend: true,
        ..Default::default()
    };
    let color = Colorizer::new(false);
    let config = portage_profile::Config::default();
    let out = expand_targets(ExpandTargetsArgs {
        opts: &opts,
        color: &color,
        config: &config,
        root: &cfg,
        config_root: &cfg,
        repos: &repos,
    });
    let _ = std::fs::remove_dir_all(&cfg);
    out
}

#[test]
fn short_names_expand_only_within_listed_categories() {
    // Listed by the overlay alone: expands to the main repo's ebuild.
    let out = expand_p571(&["p571-shared"]).expect("overlay-listed short name expands");
    assert_eq!(
        out.expanded_atoms,
        vec!["p571-shared/p571-shared".to_string()]
    );
    // Listed by the main repo: expands.
    let out = expand_p571(&["p571-listed"]).expect("listed short name expands");
    assert_eq!(
        out.expanded_atoms,
        vec!["p571-listed/p571-listed".to_string()]
    );
    // Unlisted anywhere: no match (and no `!!!` — expansion iterates the
    // allowed categories, never querying the unlisted cp).
    assert!(expand_p571(&["p571-hidden"]).is_err());
    // A qualified atom passes expansion untouched; the allow-list bites
    // at resolution, not here.
    let out = expand_p571(&["p571-hidden/p571-hidden"]).expect("qualified passes expansion");
    assert_eq!(
        out.expanded_atoms,
        vec!["p571-hidden/p571-hidden".to_string()]
    );
}
