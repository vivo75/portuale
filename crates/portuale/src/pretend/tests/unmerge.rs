use super::*;

#[test]
fn execute_unmerge_forwards_quiet_and_noinfo_to_the_info_regen() {
    // Portage: post_emerge.py. `--quiet` and the resolved `noinfo` reach the info regen.
    // Hermetic: an empty removal and a `dir`-only info dir need no host binary.
    fn scratch_info_root(tag: &str) -> (std::path::PathBuf, String) {
        let root = TempDir::new(&format!("unmerge_info_wiring_{tag}")).keep();
        std::fs::create_dir_all(root.join("etc/env.d")).unwrap();
        std::fs::write(
            root.join("etc/env.d/50-test"),
            "INFOPATH=\"/usr/share/info\"\n",
        )
        .unwrap();
        let info_dir = root.join("usr/share/info");
        std::fs::create_dir_all(&info_dir).unwrap();
        std::fs::write(info_dir.join("dir"), "stale index\n").unwrap();
        let inforoot = format!("{}/usr/share/info", root.display());
        (root, inforoot)
    }
    let color = crate::color::Colorizer::new(false);

    // `ExitCode` has no `PartialEq`, so success is asserted via `Debug`.
    let success = format!("{:?}", ExitCode::SUCCESS);
    let (root, inforoot) = scratch_info_root("record");
    let rc = execute_unmerge(ExecuteUnmergeArgs {
        removal_list: &[],
        root: &root,
        shell: crate::ebuild_phases::ShellBackend::default(),
        debug: false,
        color: &color,
        quiet: true,
        noinfo: false,
        repos: &[],
        config: &portage_profile::Config::default(),
        autoclean: false,
    });
    assert_eq!(format!("{rc:?}"), success);
    assert!(
        crate::mtimedb::read_info_mtimes(&root).contains_key(&inforoot),
        "noinfo=false must let the post-unmerge regen record the memo"
    );
    let _ = std::fs::remove_dir_all(&root);

    let (root, _) = scratch_info_root("suppressed");
    let rc = execute_unmerge(ExecuteUnmergeArgs {
        removal_list: &[],
        root: &root,
        shell: crate::ebuild_phases::ShellBackend::default(),
        debug: false,
        color: &color,
        quiet: true,
        noinfo: true,
        repos: &[],
        config: &portage_profile::Config::default(),
        autoclean: false,
    });
    assert_eq!(format!("{rc:?}"), success);
    assert!(crate::mtimedb::read_info_mtimes(&root).is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn unmerge_end_notice_skips_an_empty_removal_list() {
    // Portage: post_emerge.py. An empty removal changes nothing, so no notice and no evaluation.
    let color = crate::color::Colorizer::new(false);
    let (base, root, repos, config) = news_notice_test_tree("unmerge_empty");
    let rc = execute_unmerge(ExecuteUnmergeArgs {
        removal_list: &[],
        root: &root,
        shell: crate::ebuild_phases::ShellBackend::default(),
        debug: false,
        color: &color,
        quiet: true,
        noinfo: true,
        repos: &repos,
        config: &config,
        autoclean: false,
    });
    assert_eq!(format!("{rc:?}"), format!("{:?}", ExitCode::SUCCESS));
    assert!(
        !root.join("var/lib/gentoo/news").exists(),
        "an empty removal list must not evaluate news at all"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn still_listed_parents_applies_the_higher_slot_refinement() {
    // A higher different slot still matching the set atom suppresses the parent warning.
    let root = fixtures_root();
    let sets = vec![(
        "dualslotset".to_string(),
        vec!["dev-libs/dualslotpkg".to_string()],
    )];

    assert!(still_listed_parents(&root, &sets, "dev-libs", "dualslotpkg", "1.0").is_empty());

    assert_eq!(
        still_listed_parents(&root, &sets, "dev-libs", "dualslotpkg", "2.0"),
        vec!["dualslotset"]
    );
}

#[test]
fn still_listed_parents_is_empty_when_no_set_lists_the_cp() {
    let root = fixtures_root();
    let sets = vec![("someset".to_string(), vec!["dev-libs/other".to_string()])];
    assert!(still_listed_parents(&root, &sets, "dev-libs", "dualslotpkg", "1.0").is_empty());
}

// `FEATURES` is process-global and the remote tests mutate it too: share
// their lock, so no two `FEATURES` writers overlap.
use crate::remote::tests::PLACED_CONFIG_ENV_LOCK as FEATURES_ENV_LOCK;

/// Saves `FEATURES` on entry and restores it on drop, so an assertion panic
/// cannot leak a probe value into another test.
struct RestoreFeatures {
    saved: Option<std::ffi::OsString>,
}

impl RestoreFeatures {
    fn set(value: Option<&str>) -> Self {
        let saved = std::env::var_os("FEATURES");
        // SAFETY: the caller holds `FEATURES_ENV_LOCK`; no other
        // test in this binary touches `FEATURES` while it is held.
        unsafe {
            match value {
                Some(v) => std::env::set_var("FEATURES", v),
                None => std::env::remove_var("FEATURES"),
            }
        }
        Self { saved }
    }
}

impl Drop for RestoreFeatures {
    fn drop(&mut self) {
        // SAFETY: same locked env mutation as `set` (restoring the value).
        unsafe {
            match self.saved.take() {
                Some(value) => std::env::set_var("FEATURES", value),
                None => std::env::remove_var("FEATURES"),
            }
        }
    }
}

/// A scratch config root resolving `FEATURES` from a `make.globals` carrying
/// `binpkg-multi-instance` plus `make_conf` as `make.conf`.
fn unmerge_features_probe_root(tag: &str, make_conf: &str) -> std::path::PathBuf {
    let root = TempDir::new(&format!("unmerge_features_{tag}")).keep();
    let _ = std::fs::remove_dir_all(&root);
    let repo = root.join("repo");
    let prof = repo.join("profiles/default");
    let portage_dir = root.join("etc/portage");
    let globals_dir = root.join("usr/share/portage/config");
    std::fs::create_dir_all(&prof).unwrap();
    std::fs::create_dir_all(&portage_dir).unwrap();
    std::fs::create_dir_all(&globals_dir).unwrap();
    std::fs::write(
        prof.join("make.defaults"),
        "ARCH=\"amd64\"\nACCEPT_KEYWORDS=\"${ARCH}\"\n",
    )
    .unwrap();
    std::fs::write(portage_dir.join("make.conf"), make_conf).unwrap();
    std::fs::write(
        portage_dir.join("repos.conf"),
        "[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = repo\n",
    )
    .unwrap();
    std::fs::write(
        globals_dir.join("make.globals"),
        "FEATURES=\"binpkg-multi-instance\"\n",
    )
    .unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&prof, portage_dir.join("make.profile")).unwrap();
    root
}

/// Resolves the probe root's config the way the unmerge path does.
fn unmerge_features_probe_config(root: &std::path::Path) -> portage_profile::Config {
    crate::ebuild_phases::resolve_unmerge_config(root, root).expect("scratch chain resolves")
}

#[test]
fn unmerge_backup_gate_reads_a_make_conf_only_token() {
    // Portage: lib/portage/dbapi/vartree.py:6288 (`_pre_unmerge_backup`
    // gates on `settings.features`, the folded chain, not the calling
    // env); `make.globals`' own `FEATURES` line carries
    // `binpkg-multi-instance` (host `/usr/share/portage/config/make.globals`).
    let _env = FEATURES_ENV_LOCK.lock().unwrap();
    let _restore = RestoreFeatures::set(None);
    let root = unmerge_features_probe_root("make_conf_only", "FEATURES=\"unmerge-backup\"\n");
    let config = unmerge_features_probe_config(&root);
    let (backup, resolved) = unmerge_backup_features(&config);
    assert!(
        backup,
        "a make.conf-only unmerge-backup must back up, got {resolved:?}"
    );
    assert!(
        resolved
            .split_whitespace()
            .any(|t| t == "binpkg-multi-instance"),
        "the make.globals token must ride along, got {resolved:?}"
    );
    // The handed-on package options lay the backup out multi-instance.
    let package_options = package_options_from_env(
        crate::ebuild_phases::ShellBackend::default(),
        false,
        Some(&resolved),
        Some(&config),
    );
    assert!(
        package_options.binpkg_multi_instance,
        "the resolved features must switch the backup to the multi-instance layout"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn unmerge_backup_gate_honours_an_env_negation_over_make_conf() {
    // Portage: lib/portage/config.py, regenerate (incremental stacking: a
    // calling-env `-token` removes a lower layer's token);
    // lib/portage/dbapi/vartree.py:6288 (the gate itself).
    let _env = FEATURES_ENV_LOCK.lock().unwrap();
    let _restore = RestoreFeatures::set(Some("-unmerge-backup"));
    let root = unmerge_features_probe_root("env_negation", "FEATURES=\"unmerge-backup\"\n");
    let config = unmerge_features_probe_config(&root);
    let (backup, resolved) = unmerge_backup_features(&config);
    assert!(
        !backup,
        "an env -unmerge-backup must win over make.conf, got {resolved:?}"
    );
    assert!(
        resolved
            .split_whitespace()
            .any(|t| t == "binpkg-multi-instance"),
        "the negation must only drop its own token, got {resolved:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn unmerge_backup_gate_stays_on_for_an_env_only_token() {
    // Portage: lib/portage/dbapi/vartree.py:6288 (the gate reads the folded
    // `settings.features`, whose top layer is the calling env). This is the
    // pre-fix behaviour, kept: previously the only source.
    let _env = FEATURES_ENV_LOCK.lock().unwrap();
    let _restore = RestoreFeatures::set(Some("unmerge-backup"));
    let root = unmerge_features_probe_root("env_only", "# no FEATURES here\n");
    let config = unmerge_features_probe_config(&root);
    let (backup, resolved) = unmerge_backup_features(&config);
    assert!(
        backup,
        "an env-only unmerge-backup must still back up, got {resolved:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// The removal display opens with the non-`/` ROOT notice and,
/// under `--pretend`, the unmerged header — each with its own leading blank
/// line, `--quiet` dropping the blanks (`_unmerge_display`).
// Portage: _emerge/unmerge.py, _unmerge_display
#[test]
fn unmerge_display_header_prints_the_root_notice_and_the_header() {
    let color = Colorizer::new(false);
    let root = std::path::Path::new("/tmp/some-root");
    // A non-`/` ROOT under `--pretend`: the ROOT notice, then the header.
    assert_eq!(
        unmerge_display_header_text(root, true, false, &color),
        "\n>>> Using system located in ROOT tree /tmp/some-root/\n\
         \n>>> These are the packages that would be unmerged:\n"
    );
    // `ROOT=/` drops the notice, keeping the header's blank line.
    assert_eq!(
        unmerge_display_header_text(std::path::Path::new("/"), true, false, &color),
        "\n>>> These are the packages that would be unmerged:\n"
    );
    // Not `--pretend`: the notice only.
    assert_eq!(
        unmerge_display_header_text(root, false, false, &color),
        "\n>>> Using system located in ROOT tree /tmp/some-root/\n"
    );
    // `--quiet`: no leading blanks anywhere.
    assert_eq!(
        unmerge_display_header_text(root, true, true, &color),
        ">>> Using system located in ROOT tree /tmp/some-root/\n"
    );
}
