use super::*;

use portage_util::TempDir;

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .canonicalize()
        .expect("fixtures/ does not resolve: it is a symlink to ../pmtest/fixtures, so the sibling pmtest repo has to be checked out next to this one")
}

type PackageFlags = Vec<(String, Vec<String>)>;

/// Writes `package.use{infix}.mask`/`.force` into a main repo
/// (`dev-libs/a`) and an overlay (`dev-libs/b`) and checks the resolved
/// lists that `fields` selects: Portage's UseManager never merges an
/// overlay's own lists with its master's, so the main repo's entry stays
/// unscoped and the overlay's gets `::overlay`.
fn check_overlay_package_use_scoping(
    infix: &str,
    fields: impl Fn(&Config) -> (&PackageFlags, &PackageFlags),
) {
    let root = TempDir::new("portage-profile-test-overlay-package-use-scoping").keep();
    let repo = root.join("repo");
    let overlay = root.join("overlay");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(overlay.join("profiles")).unwrap();
    for (dir, atom, kind, flag) in [
        (&repo, "dev-libs/a", "mask", "maska"),
        (&overlay, "dev-libs/b", "mask", "maskb"),
        (&repo, "dev-libs/a", "force", "forcea"),
        (&overlay, "dev-libs/b", "force", "forceb"),
    ] {
        let file = dir.join(format!("profiles/package.use{infix}.{kind}"));
        fs::write(file, format!("{atom} {flag}\n")).unwrap();
    }

    let overlay_repos = [("overlay".to_string(), overlay.to_path_buf())];
    let config = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &repo,
        overlay_repos: &overlay_repos,
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("config must resolve");
    let entry = |atom: &str, flag: &str| (atom.to_string(), vec![flag.to_string()]);
    let (mask, force) = fields(&config);
    assert_eq!(
        *mask,
        vec![
            entry("dev-libs/a", "maska"),
            entry("dev-libs/b::overlay", "maskb")
        ]
    );
    assert_eq!(
        *force,
        vec![
            entry("dev-libs/a", "forcea"),
            entry("dev-libs/b::overlay", "forceb")
        ]
    );
}

mod binrepo;
mod graph;
mod incrementals;
mod package_files;
mod resolve;
