//! The embedded `make.globals` (backlog #384).

use super::*;

/// A `make.globals` under the config root is not Portage's file: Portage
/// reads `GLOBAL_CONFIG_PATH` only (`config.py:467-474`,
/// `LocationsManager.py:413-417`), so a root carrying its own copy resolves
/// exactly like a root with none.
#[test]
fn a_make_globals_under_the_config_root_is_ignored() {
    let root = portage_util::TempDir::new("portage-profile-test-globals-ignored").keep();
    let prof = root.join("repo/profiles/default");
    fs::create_dir_all(&prof).unwrap();
    fs::create_dir_all(root.join("etc/portage")).unwrap();
    fs::write(prof.join("make.defaults"), "ARCH=\"amd64\"\n").unwrap();
    std::os::unix::fs::symlink(&prof, root.join("etc/portage/make.profile")).unwrap();
    let resolve = || {
        resolve_config(ResolveConfigArgs {
            config_root: &root,
            main_repo_location: &root.join("repo"),
            overlay_repos: &[],
            repo_aliases: &[],
            main_repo_name: "testrepo",
            repo_masters: &HashMap::new(),
            eroot: &root,
        })
        .expect("resolves")
    };
    let features = |c: &Config| c.resolved_incremental("FEATURES").unwrap_or_default();
    let without = with_test_env(&[], || features(&resolve()));
    fs::create_dir_all(root.join("usr/share/portage/config")).unwrap();
    fs::write(
        root.join("usr/share/portage/config/make.globals"),
        "FEATURES=\"decoy\"\nBINPKG_FORMAT=\"xpak\"\n",
    )
    .unwrap();
    let (with, format) = with_test_env(&[], || {
        let c = resolve();
        (features(&c), c.other_vars.get("BINPKG_FORMAT").cloned())
    });
    assert_eq!(with, without);
    assert!(without.iter().any(|t| t == "preserve-libs"), "{without:?}");
    assert!(
        without.iter().any(|t| t == "unmerge-orphans"),
        "{without:?}"
    );
    assert!(!with.iter().any(|t| t == "decoy"));
    assert_eq!(format.as_deref(), Some("gpkg"));
}

/// The embedded copy is upstream's `cnf/make.globals` plus the one line the
/// Gentoo ebuild appends, so a Portage bump (`3rdparty/portage`) is noticed.
#[test]
fn the_embedded_make_globals_is_upstream_plus_the_xattr_line() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let upstream = fs::read_to_string(repo.join("3rdparty/portage/cnf/make.globals")).unwrap();
    let embedded = fs::read_to_string(repo.join("cnf/make.globals")).unwrap();
    assert_eq!(
        embedded,
        format!("{upstream}FEATURES=\"${{FEATURES}} xattr\"\n")
    );
}

/// A `<config root>/etc/make.globals` draws Portage's obsolete-file notice.
#[test]
fn an_obsolete_etc_make_globals_is_reported() {
    let root = portage_util::TempDir::new("portage-profile-test-globals-obsolete").keep();
    let prof = root.join("repo/profiles/default");
    fs::create_dir_all(&prof).unwrap();
    fs::create_dir_all(root.join("etc/portage")).unwrap();
    fs::write(prof.join("make.defaults"), "ARCH=\"amd64\"\n").unwrap();
    fs::write(root.join("etc/make.globals"), "FEATURES=\"decoy\"\n").unwrap();
    std::os::unix::fs::symlink(&prof, root.join("etc/portage/make.profile")).unwrap();
    let config = with_test_env(&[], || {
        resolve_config(ResolveConfigArgs {
            config_root: &root,
            main_repo_location: &root.join("repo"),
            overlay_repos: &[],
            repo_aliases: &[],
            main_repo_name: "testrepo",
            repo_masters: &HashMap::new(),
            eroot: &root,
        })
        .expect("resolves")
    });
    let expected = format!(
        "!!! Found obsolete make.globals file: '{}', (using '/usr/share/portage/config/make.globals' instead)",
        root.join("etc/make.globals").display()
    );
    assert!(
        config.config_warnings.contains(&expected),
        "{:?}",
        config.config_warnings
    );
    assert!(
        !config
            .resolved_incremental("FEATURES")
            .unwrap_or_default()
            .contains(&"decoy".to_string())
    );
}
