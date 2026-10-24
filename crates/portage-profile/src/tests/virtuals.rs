//! Backlog #513: deprecated `virtuals` files. Oracle: host Portage 3.0.83
//! captures in `LLM/evidence/2026-10-24-513/` (shapes 01-29): the profile
//! warning is a `UserWarning` on stderr for every non-empty profile
//! `virtuals` (chain order, `/etc/portage/profile` last); the user block is
//! printed by the CLI, not by config load.

use super::*;

use crate::virtuals::grabdict_nonempty;
use portage_util::TempDir;

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// `grabdict` drops comments and one-token lines; a single line with a key
/// and a value is enough (Portage `util/__init__.py`, `grabdict`).
#[test]
fn grabdict_nonempty_needs_a_key_and_a_value() {
    let root = TempDir::new("portage-profile-test-virtuals-grabdict").keep();
    let file = root.join("virtuals");
    let check = |content: &str| {
        write(&file, content);
        grabdict_nonempty(&file)
    };
    assert!(!grabdict_nonempty(&root.join("missing")));
    assert!(check("virtual/editor app-editors/vim\n"));
    assert!(check("# note\nvirtual/editor app-editors/vim # trailing\n"));
    assert!(check("virtual/editor app-editors/vim\r\n"));
    assert!(!check(""));
    assert!(!check("# only a comment\n"));
    assert!(!check("virtual/editor\n"));
    assert!(!check("virtual/editor # no provider\n"));
    assert!(!check("   # indented comment is a key\n"));
}

/// One warning per non-empty profile, in the order given (parent first).
#[test]
fn profile_virtuals_warn_once_per_non_empty_profile_in_chain_order() {
    let root = TempDir::new("portage-profile-test-virtuals-chain").keep();
    let parent = root.join("parent");
    let leaf = root.join("leaf");
    let empty = root.join("empty");
    write(&parent.join("virtuals"), "virtual/x app-misc/a\n");
    write(&leaf.join("virtuals"), "virtual/y app-misc/b\n");
    write(&empty.join("virtuals"), "# nothing\n");
    let warnings =
        crate::virtuals::profile_virtuals_warnings(&[parent.clone(), empty.clone(), leaf.clone()]);
    let texts: Vec<&str> = warnings.iter().map(|w| w.text.as_str()).collect();
    assert_eq!(texts.len(), 2, "{texts:?}");
    assert!(texts[0].contains(&format!("{}/virtuals", parent.display())));
    assert!(texts[1].contains(&format!("{}/virtuals", leaf.display())));
    assert_eq!(
        texts[0],
        format!(
            "portage/package/ebuild/_config/VirtualsManager.py:54: UserWarning: {}/virtuals is deprecated, support for PROVIDE virtuals was removed in portage-2.3.25, use GLEP 37 virtual packages instead\n  warnings.warn(",
            parent.display()
        )
    );
}

/// A non-empty profile `virtuals` and a non-empty `/etc/portage/profile`
/// `virtuals` both warn, and an existing `/etc/portage/virtuals` (even empty)
/// sets the user flag; the user block itself is not a config warning.
#[test]
fn resolve_config_reports_virtuals_files() {
    let root = TempDir::new("portage-profile-test-virtuals-resolve").keep();
    let prof = root.join("repo/profiles/default");
    fs::create_dir_all(&prof).unwrap();
    fs::write(prof.join("make.defaults"), "ARCH=\"amd64\"\n").unwrap();
    write(&prof.join("virtuals"), "virtual/editor app-editors/vim\n");
    write(
        &root.join("etc/portage/profile/virtuals"),
        "virtual/x app-misc/a\n",
    );
    write(&root.join("etc/portage/virtuals"), "");
    std::os::unix::fs::symlink(&prof, root.join("etc/portage/make.profile")).unwrap();
    let config = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("resolves");
    assert!(config.user_virtuals_deprecated);
    let texts: Vec<&str> = config.config_warnings.iter().map(String::as_str).collect();
    assert_eq!(texts.len(), 2, "{texts:?}");
    assert!(texts[0].contains("/repo/profiles/default/virtuals is deprecated"));
    assert!(texts[1].contains("/etc/portage/profile/virtuals is deprecated"));
    assert_eq!(
        crate::USER_VIRTUALS_NOTICE,
        "\n!!! /etc/portage/virtuals is deprecated and ignored.\n\
!!! Support for PROVIDE virtuals was removed in\n\
!!! portage-2.3.25; use GLEP 37 virtual packages\n\
!!! instead. Please remove this file.\n\n"
    );
}

/// Without a profile chain there is no user profile node, so its
/// `virtuals` is not read (Portage appends `custom_prof` only when
/// `self.profiles` is non-empty), and no user file means no flag.
#[test]
fn user_profile_virtuals_need_a_profile_chain() {
    let root = TempDir::new("portage-profile-test-virtuals-no-chain").keep();
    write(
        &root.join("etc/portage/profile/virtuals"),
        "virtual/x app-misc/a\n",
    );
    let config = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("resolves");
    assert!(!config.user_virtuals_deprecated);
    assert!(
        config.config_warnings.is_empty(),
        "{:?}",
        config.config_warnings
    );
}
