use super::*;

/// A source merge with a resolved config threads the full effective `USE`, the
/// resolved (folded) `FEATURES`, and the entry's `SLOT` into the phase, under
/// **both** backends. Expected value from Portage source.
#[allow(clippy::field_reassign_with_default)]
#[test]
fn source_merge_with_resolved_config_threads_use_and_slot_into_the_phase() {
    for (shell, label) in [
        (ebuild_phases::ShellBackend::Bash, "bash"),
        (ebuild_phases::ShellBackend::Brush, "brush"),
    ] {
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        let root = tempdir();
        let portage_tmpdir = tempdir();
        let mut config = portage_profile::Config::default();
        config.use_tokens = vec!["abi_x86_64 amd64 elibc_glibc kernel_linux".to_string()];
        config.iuse_effective = ["abi_x86_64", "amd64", "elibc_glibc", "kernel_linux"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut config_env = config.clone();
        config_env.other_vars.insert(
            "FEATURES".to_string(),
            "resolved features token".to_string(),
        );
        let mut options = ebuild_merge::MergeOptions {
            distdir: tempdir(),
            config_root: config_root.clone(),
            shell,
            ..ebuild_merge::MergeOptions::default()
        };
        options.build_env = portage_profile::phase_environ(&config_env, None);
        options.resolved_config = Some(std::sync::Arc::new(config));
        // The `${T}`/`build-info` files asserted below are the subject; keep
        // the builddir via `FEATURES=noclean` (the default post-merge clean
        // is pinned separately).
        options.features = "noclean".to_string();

        let entries = vec![source_entry(
            "phaseenvpkg",
            PretendOutcome::New {
                version: "1.0".into(),
            },
        )];
        run_source_merge(RunSourceMergeArgs {
            entries: &entries,
            repos: &repos,
            root: &root,
            portage_tmpdir: &portage_tmpdir,
            options: &options,
            keep_going: false,
            buildpkg: None,
            buildpkg_exclude: &[],
            jobs: 1,
            load_average: None,
            capture_log: false,
            mode: StatusMode::for_tests(),
        })
        .unwrap_or_else(|e| panic!("{label}: source merge succeeds: {e}"));

        let keep = root.join("var/lib/phaseenvtest/.keep_dev-libs_phaseenvpkg-0");
        assert!(
            keep.exists(),
            "{label}: keepdir marker missing: {}",
            keep.display()
        );
        let t_dir = portage_tmpdir.join("portage/dev-libs/phaseenvpkg-1.0/temp");
        assert_eq!(
            fs::read_to_string(t_dir.join("phase-env-use.txt")).unwrap(),
            "USE=abi_x86_64 amd64 elibc_glibc kernel_linux\n",
            "{label}: USE"
        );
        assert_eq!(
            fs::read_to_string(t_dir.join("phase-env-slot.txt")).unwrap(),
            "SLOT=0\n",
            "{label}: SLOT"
        );
        assert_eq!(
            fs::read_to_string(t_dir.join("phase-env-features.txt")).unwrap(),
            "FEATURES=resolved features token\n",
            "{label}: the resolved FEATURES must win over the raw process-env base"
        );
        let build_use = fs::read_to_string(
            portage_tmpdir.join("portage/dev-libs/phaseenvpkg-1.0/build-info/USE"),
        )
        .expect("build-info/USE should be written by the real __dyn_install");
        assert_eq!(
            build_use.trim(),
            "abi_x86_64 amd64 elibc_glibc kernel_linux",
            "{label}: build-info/USE"
        );

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&portage_tmpdir);
    }
}

/// `envdreload-a` installs `/etc/env.d/99envdreload`; `envdreload-b`, built
/// afterwards in the same run, must see the variable in its `src_install`.
/// Expected value from Portage source.
#[allow(clippy::field_reassign_with_default)]
#[test]
fn source_merge_second_package_sees_env_d_installed_by_the_first() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let mut config = portage_profile::Config::default();
    config.use_tokens = vec!["amd64".to_string()];
    config.iuse_effective = ["amd64"].iter().map(|s| s.to_string()).collect();
    config.envd_eroot = root.clone();
    let mut options = ebuild_merge::MergeOptions {
        distdir: tempdir(),
        config_root: config_root.clone(),
        ..ebuild_merge::MergeOptions::default()
    };
    options.build_env = run_wide_phase_env(&config);
    options.resolved_config = Some(std::sync::Arc::new(config));
    options.features = "noclean".to_string();
    let entries = vec![
        source_entry(
            "envdreload-a",
            PretendOutcome::New {
                version: "1.0".into(),
            },
        ),
        source_entry(
            "envdreload-b",
            PretendOutcome::New {
                version: "1.0".into(),
            },
        ),
    ];
    run_source_merge(RunSourceMergeArgs {
        entries: &entries,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &options,
        keep_going: false,
        buildpkg: None,
        buildpkg_exclude: &[],
        jobs: 1,
        load_average: None,
        capture_log: false,
        mode: StatusMode::for_tests(),
    })
    .expect("source merge succeeds");
    let seen = portage_tmpdir.join("portage/dev-libs/envdreload-b-1.0/temp/envdreload-seen.txt");
    assert_eq!(fs::read_to_string(seen).unwrap(), "ENVDRELOAD_VAR=1\n");
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&portage_tmpdir);
}

/// A two-entry merge with `CONFIG_PROTECT` from the config: `cfgprotlate-a`
/// installs an `env.d` entry adding a `CONFIG_PROTECT` path and `cfgprotlate-b`
/// installs over locally modified copies. Both local files must survive with
/// `._cfg0000_*` siblings. Expected value from Portage in the bed image.
#[allow(clippy::field_reassign_with_default)]
#[test]
fn source_merge_protects_the_config_fold_and_env_d_added_paths() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    for (dir, name) in [("usr/share/cfgprotme", "x"), ("usr/share/cfgprotlate", "y")] {
        fs::create_dir_all(root.join(dir)).unwrap();
        fs::write(root.join(dir).join(name), "local\n").unwrap();
    }
    let mut config = portage_profile::Config::default();
    config.use_tokens = vec!["amd64".to_string()];
    config.iuse_effective = ["amd64"].iter().map(|s| s.to_string()).collect();
    config.envd_eroot = root.clone();
    config.incremental_sources.insert(
        "CONFIG_PROTECT".to_string(),
        vec![vec!["/etc".to_string(), "/usr/share/cfgprotme".to_string()]],
    );
    let mut options = ebuild_merge::MergeOptions {
        distdir: tempdir(),
        config_root: config_root.clone(),
        ..ebuild_merge::MergeOptions::default()
    };
    options.apply_config_protect(&config);
    options.build_env = run_wide_phase_env(&config);
    options.resolved_config = Some(std::sync::Arc::new(config));
    options.features = "noclean".to_string();
    let entries = ["cfgprotlate-a", "cfgprotlate-b"].map(|p| {
        source_entry(
            p,
            PretendOutcome::New {
                version: "1.0".into(),
            },
        )
    });
    run_source_merge(RunSourceMergeArgs {
        entries: &entries,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &options,
        keep_going: false,
        buildpkg: None,
        buildpkg_exclude: &[],
        jobs: 1,
        load_average: None,
        capture_log: false,
        mode: StatusMode::for_tests(),
    })
    .expect("source merge succeeds");
    let read = |p: &str| fs::read_to_string(root.join(p)).unwrap_or_else(|e| format!("<{e}>"));
    assert_eq!(read("usr/share/cfgprotme/x"), "local\n");
    assert_eq!(read("usr/share/cfgprotme/._cfg0000_x"), "from-package\n");
    assert_eq!(read("usr/share/cfgprotlate/y"), "local\n");
    assert_eq!(read("usr/share/cfgprotlate/._cfg0000_y"), "from-package\n");
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&portage_tmpdir);
}

/// A variable the ebuild `unset` in an earlier phase stays unset in later
/// phases: `unsetcflagspkg`'s `src_compile` unsets `CFLAGS`, so no vdb `CFLAGS`
/// row is written while `CXXFLAGS` (never unset) keeps its row as the control.
/// Expected value from Portage source.
#[allow(clippy::field_reassign_with_default)]
#[test]
fn source_merge_omits_a_vdb_aux_file_the_ebuild_unset_in_an_earlier_phase() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let mut options = ebuild_merge::MergeOptions {
        distdir: tempdir(),
        config_root: config_root.clone(),
        shell: ebuild_phases::ShellBackend::Bash,
        ..ebuild_merge::MergeOptions::default()
    };
    // Point `PORTAGE_GRPNAME` at this process's own gid, so the per-phase
    // `chgrp` of `${T}/environment` is a permitted no-op instead of a
    // per-phase "Operation not permitted" line on harness stderr.
    let gid = unsafe { libc::getegid() };
    options.build_env = vec![
        ("CFLAGS".to_string(), "-O2 -pipe".to_string()),
        ("CXXFLAGS".to_string(), "-O2 -pipe".to_string()),
        ("PORTAGE_GRPNAME".to_string(), gid.to_string()),
    ];
    // Keep the builddir (`FEATURES=noclean`): the observed `${T}` file is
    // copied into the merged image, and the vdb read below happens on the
    // final tree.
    options.features = "noclean".to_string();

    let entries = vec![source_entry(
        "unsetcflagspkg",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    )];
    run_source_merge(RunSourceMergeArgs {
        entries: &entries,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &options,
        keep_going: false,
        buildpkg: None,
        buildpkg_exclude: &[],
        jobs: 1,
        load_average: None,
        capture_log: false,
        mode: StatusMode::for_tests(),
    })
    .expect("source merge succeeds");

    let observed = fs::read_to_string(root.join("usr/share/unsetcflagspkg/unset-observed.txt"))
        .expect("the install phase ran and merged its observation");
    assert_eq!(
        observed, "CFLAGS=<unset>\nCXXFLAGS=-O2 -pipe\n",
        "src_install must still see the `unset CFLAGS` from src_compile (bug #189417)"
    );

    let vdb = root.join("var/db/pkg/dev-libs/unsetcflagspkg-1.0");
    assert!(
        !vdb.join("CFLAGS").exists(),
        "#160: real omits the vdb CFLAGS row its ebuild unset"
    );
    assert_eq!(
        fs::read_to_string(vdb.join("CXXFLAGS")).unwrap().trim(),
        "-O2 -pipe",
        "an aux file the package still has a value for must stay"
    );

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&portage_tmpdir);
}

/// A byte-identical installed file survives a re-merge when the ebuild's
/// `pkg_preinst` helper is gated on `REPLACING_VERSIONS`: the preinst returns
/// early and the destination keeps its inode. Expected value from Portage source.
#[test]
fn reinstall_leaves_a_byte_identical_file_a_preinst_helper_would_remove() {
    use std::os::unix::fs::MetadataExt;

    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let options = ebuild_merge::MergeOptions {
        distdir: tempdir(),
        config_root: config_root.clone(),
        ..ebuild_merge::MergeOptions::default()
    };
    let merge = |outcome: PretendOutcome| {
        run_source_merge(RunSourceMergeArgs {
            entries: &[source_entry("identicalownerpkg", outcome)],
            repos: &repos,
            root: &root,
            portage_tmpdir: &portage_tmpdir,
            options: &options,
            keep_going: false,
            buildpkg: None,
            buildpkg_exclude: &[],
            jobs: 1,
            load_average: None,
            capture_log: false,
            mode: StatusMode::for_tests(),
        })
        .expect("merge succeeds");
    };

    // First merge: no same-slot instance yet, so the fixture's preinst
    // `rm` is a no-op and the image's file lands.
    merge(PretendOutcome::New {
        version: "4".into(),
    });
    let dest = root.join("usr/share/identicalownerpkg/identical.txt");
    let before = fs::symlink_metadata(&dest)
        .expect("the fixture installed its file")
        .ino();

    // Re-merge the same version: `REPLACING_VERSIONS=4` is set for the
    // preinst, the fixture returns early, and the byte-identical
    // destination is left in place (same inode, same owner).
    merge(PretendOutcome::Reinstall {
        version: "4".into(),
        changed_flags: vec![],
        deps_changed: false,
        slot_changed: false,
        rebuilt_binary: true,
        new_repo: false,
        slot_operator_rebuild: false,
    });
    let after = fs::symlink_metadata(&dest).expect("still installed").ino();
    assert_eq!(
        before, after,
        "#158: a byte-identical file must keep its inode (and owner) through a re-merge"
    );

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&portage_tmpdir);
}

#[test]
fn run_source_merge_builds_and_merges_a_new_package_end_to_end() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();

    // `samepkg` is already installed, so it is skipped silently.
    let entries = vec![
        source_entry(
            "samepkg",
            PretendOutcome::AlreadyInstalled {
                version: "1.0".into(),
            },
        ),
        source_entry(
            "packagepkg",
            PretendOutcome::New {
                version: "1.0".into(),
            },
        ),
    ];

    let options = ebuild_merge::MergeOptions {
        distdir: tempdir(),
        config_root: config_root.clone(),
        ..ebuild_merge::MergeOptions::default()
    };
    run_source_merge(RunSourceMergeArgs {
        entries: &entries,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &options,
        keep_going: false,
        buildpkg: None,
        buildpkg_exclude: &[],
        jobs: 1,
        load_average: None,
        capture_log: false,
        mode: StatusMode::for_tests(),
    })
    .expect("source merge succeeds");

    assert_eq!(
        fs::read_to_string(root.join("usr/share/packagepkg/hello.txt"))
            .unwrap()
            .trim(),
        "hello from packagepkg"
    );
    let vdb = root.join("var/db/pkg/dev-libs/packagepkg-1.0");
    assert!(vdb.join("CONTENTS").is_file());
    assert_eq!(
        fs::read_to_string(vdb.join("RDEPEND")).unwrap().trim(),
        "dev-libs/samepkg"
    );
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&portage_tmpdir);
}

/// `merge_one_source_entry` pre-cleans before every build: with a stale
/// `.installed` marker and a tampered `${D}` left by an earlier build (kept via
/// `noclean`), the next run must discard them and rebuild. Expected value from
/// Portage source.
#[test]
fn source_merge_pre_cleans_a_stale_installed_image() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let entry = source_entry(
        "packagepkg",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );

    // First merge with noclean: the build state survives.
    let options = ebuild_merge::MergeOptions {
        features: "noclean".to_string(),
        ..ebuild_merge::MergeOptions::default()
    };
    merge_one_source_entry(MergeOneSourceEntryArgs {
        entry: &entry,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &options,
        buildpkg: None,
        progress: mrg_director::MergeProgress::single(),
        display: &StatusDisplay::for_tests(),
    })
    .expect("first merge succeeds");
    let builddir = portage_tmpdir.join("portage/dev-libs/packagepkg-1.0");
    assert!(
        builddir.join(".installed").exists(),
        "noclean must keep the first build's state"
    );

    // Tamper: replace the image file with a stale one, keeping the
    // `.installed` marker -- the state the missing pre-clean treated as
    // "already built".
    let image = builddir.join("image/usr/share/packagepkg");
    std::fs::remove_file(image.join("hello.txt")).unwrap();
    std::fs::write(image.join("stale.txt"), "stale\n").unwrap();

    // Second merge, no noclean: the pre-clean drops the stale state, so
    // `install` really re-runs.
    let options = ebuild_merge::MergeOptions {
        features: "sandbox".to_string(),
        ..ebuild_merge::MergeOptions::default()
    };
    merge_one_source_entry(MergeOneSourceEntryArgs {
        entry: &entry,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &options,
        buildpkg: None,
        progress: mrg_director::MergeProgress::single(),
        display: &StatusDisplay::for_tests(),
    })
    .expect("second merge succeeds");
    assert!(root.join("usr/share/packagepkg/hello.txt").is_file());
    assert!(
        !root.join("usr/share/packagepkg/stale.txt").exists(),
        "the stale image must not be merged"
    );

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&portage_tmpdir);
}

#[test]
fn run_build_scheduler_builds_two_leaves_in_parallel_then_the_parent() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();

    // schedparent depends on both leaves; the scheduler builds the leaves
    // first (concurrently under jobs=2) and the parent only after both merge.
    let leaf_a = source_entry(
        "schedleaf-a",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    let mut leaf_a = leaf_a;
    leaf_a.required_by = vec![("dev-libs".into(), "schedparent".into())];
    let mut leaf_b = source_entry(
        "schedleaf-b",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    leaf_b.required_by = vec![("dev-libs".into(), "schedparent".into())];
    let parent = source_entry(
        "schedparent",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    let entries = vec![leaf_a, leaf_b, parent];

    let options = ebuild_merge::MergeOptions {
        distdir: tempdir(),
        config_root: config_root.clone(),
        ..ebuild_merge::MergeOptions::default()
    };
    run_source_merge(RunSourceMergeArgs {
        entries: &entries,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &options,
        keep_going: false,
        buildpkg: None,
        buildpkg_exclude: &[],
        jobs: 2,
        load_average: None,
        capture_log: false,
        mode: StatusMode::for_tests(),
    })
    .expect("parallel source merge succeeds");

    for pkg in ["schedleaf-a-1.0", "schedleaf-b-1.0", "schedparent-1.0"] {
        assert!(
            root.join(format!("var/db/pkg/dev-libs/{pkg}/CONTENTS"))
                .is_file(),
            "{pkg} should be merged"
        );
    }
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&portage_tmpdir);
}

#[test]
fn run_build_scheduler_with_a_high_load_average_never_throttles() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();

    let mut leaf_a = source_entry(
        "schedleaf-a",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    leaf_a.required_by = vec![("dev-libs".into(), "schedparent".into())];
    let mut leaf_b = source_entry(
        "schedleaf-b",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    leaf_b.required_by = vec![("dev-libs".into(), "schedparent".into())];
    let parent = source_entry(
        "schedparent",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    let entries = vec![leaf_a, leaf_b, parent];

    let options = ebuild_merge::MergeOptions {
        distdir: tempdir(),
        config_root: config_root.clone(),
        ..ebuild_merge::MergeOptions::default()
    };
    // A load average of 1e9 can never be exceeded, so the throttle is a
    // no-op and every package still builds and merges.
    run_source_merge(RunSourceMergeArgs {
        entries: &entries,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &options,
        keep_going: false,
        buildpkg: None,
        buildpkg_exclude: &[],
        jobs: 2,
        load_average: Some(1e9),
        capture_log: false,
        mode: StatusMode::for_tests(),
    })
    .expect("high --load-average must not stall the scheduler");
    for pkg in ["schedleaf-a-1.0", "schedleaf-b-1.0", "schedparent-1.0"] {
        assert!(
            root.join(format!("var/db/pkg/dev-libs/{pkg}/CONTENTS"))
                .is_file()
        );
    }
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&portage_tmpdir);
}

#[test]
fn run_build_scheduler_keep_going_skips_a_failed_builds_dependents() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();

    // schedbad's install fails; schedbaddep depends on it (must be skipped);
    // schedok is independent (must still merge).
    let mut bad = source_entry(
        "schedbad",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    bad.required_by = vec![("dev-libs".into(), "schedbaddep".into())];
    let baddep = source_entry(
        "schedbaddep",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    let ok = source_entry(
        "schedok",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    let entries = vec![bad, baddep, ok];

    let options = ebuild_merge::MergeOptions {
        distdir: tempdir(),
        config_root: config_root.clone(),
        ..ebuild_merge::MergeOptions::default()
    };
    let err = run_source_merge(RunSourceMergeArgs {
        entries: &entries,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &options,
        keep_going: true,
        buildpkg: None,
        buildpkg_exclude: &[],
        jobs: 2,
        load_average: None,
        capture_log: false,
        mode: StatusMode::for_tests(),
    })
    .expect_err("a failed build must make the whole run fail under --keep-going");
    assert!(is_reported_failure(&err), "{err}");

    assert!(
        root.join("var/db/pkg/dev-libs/schedok-1.0/CONTENTS")
            .is_file(),
        "the independent schedok must still merge"
    );
    assert!(
        !root.join("var/db/pkg/dev-libs/schedbaddep-1.0").exists(),
        "schedbaddep depends on the failed schedbad and must be skipped"
    );
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&portage_tmpdir);
}

#[test]
fn a_hard_failure_kills_still_running_builds_instead_of_waiting_them_out() {
    // Once any package fails without `--keep-going`, still-running builds are
    // killed rather than waited out. `schedbad` fails almost instantly while
    // `schedslow` sleeps 120s; both start together as independent leaves
    // under `jobs=2`, so the run must finish well under 120s.
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();

    let bad = source_entry(
        "schedbad",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    let slow = source_entry(
        "schedslow",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    let entries = vec![bad, slow];

    let options = ebuild_merge::MergeOptions {
        distdir: tempdir(),
        config_root: config_root.clone(),
        ..ebuild_merge::MergeOptions::default()
    };
    let started = std::time::Instant::now();
    let err = run_source_merge(RunSourceMergeArgs {
        entries: &entries,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &options,
        keep_going: false, // keep_going
        buildpkg: None,
        buildpkg_exclude: &[],
        jobs: 2,
        load_average: None,
        capture_log: false,
        mode: StatusMode::for_tests(),
    })
    .expect_err("schedbad's own failure must fail the whole run");
    assert!(is_reported_failure(&err), "{err}");
    // Generously bounded for loaded machines: well under schedslow's 120s
    // sleep, proving the scheduler didn't just wait it out.
    assert!(
        started.elapsed() < std::time::Duration::from_secs(60),
        "run_source_merge took {:?}, schedslow's sleep should have been killed",
        started.elapsed()
    );

    let marker =
        portage_tmpdir.join("portage/dev-libs/schedslow-1.0/temp/schedslow-slept-to-completion");
    assert!(
        !marker.exists(),
        "schedslow's own sleep must have been killed, not left to finish"
    );

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&portage_tmpdir);
}

#[test]
fn run_source_merge_rejects_a_binary_entry() {
    // The error is raised before any execution, so nonexistent paths are
    // safe here.
    let bogus = PathBuf::from("/nonexistent/does/not/exist");
    let options = ebuild_merge::MergeOptions::default();

    let mut binary = source_entry(
        "packagepkg",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    binary.source = CandidateSource::Binary;
    let err = run_source_merge(RunSourceMergeArgs {
        entries: &[binary],
        repos: &[],
        root: &bogus,
        portage_tmpdir: &bogus,
        options: &options,
        keep_going: false,
        buildpkg: None,
        buildpkg_exclude: &[],
        jobs: 1,
        load_average: None,
        capture_log: false,
        mode: StatusMode::for_tests(),
    })
    .unwrap_err();
    assert!(err.contains("binary package"), "{err}");
}

#[test]
fn run_merge_loop_without_keep_going_stops_at_the_first_failure() {
    let a = source_entry(
        "aaa",
        PretendOutcome::New {
            version: "1".into(),
        },
    );
    let b = source_entry(
        "bbb",
        PretendOutcome::New {
            version: "1".into(),
        },
    );
    let mut seen: Vec<String> = Vec::new();
    // No resume list under this fresh root, so the per-merge shrink
    // is a silent no-op.
    let root = tempdir();
    let no_paths = |_: &GraphEntry| None;
    let report = FailReport {
        root: &root,
        source_paths: &no_paths,
        features: String::new(),
    };
    let err = run_merge_loop(&[a, b], false, &report, |_, e, _| {
        seen.push(e.package.clone());
        Err(format!("{} boom", e.package))
    })
    .unwrap_err();
    assert_eq!(err, "aaa boom");
    assert_eq!(seen, vec!["aaa".to_string()]);
}

#[test]
fn run_merge_loop_keep_going_skips_the_failed_packages_transitive_dependents() {
    // dep <- mid <- top   (top depends on mid depends on dep);
    // `other` is independent. `dep` fails, so `mid` and `top` are
    // dropped, `other` still merges, and the combined Err names all.
    let mut dep = source_entry(
        "dep",
        PretendOutcome::New {
            version: "1".into(),
        },
    );
    dep.required_by = vec![("dev-libs".into(), "mid".into())];
    let mut mid = source_entry(
        "mid",
        PretendOutcome::New {
            version: "1".into(),
        },
    );
    mid.required_by = vec![("dev-libs".into(), "top".into())];
    let top = source_entry(
        "top",
        PretendOutcome::New {
            version: "1".into(),
        },
    );
    let other = source_entry(
        "other",
        PretendOutcome::New {
            version: "1".into(),
        },
    );

    let mut merged: Vec<String> = Vec::new();
    // No resume list under this fresh root, so the per-merge shrink
    // is a silent no-op.
    let root = tempdir();
    let no_paths = |_: &GraphEntry| None;
    let report = FailReport {
        root: &root,
        source_paths: &no_paths,
        features: String::new(),
    };
    let err = run_merge_loop(&[dep, mid, top, other], true, &report, |_, e, _| {
        if e.package == "dep" {
            return Err("dep boom".into());
        }
        merged.push(e.package.clone());
        Ok(())
    })
    .unwrap_err();

    assert_eq!(merged, vec!["other".to_string()]);
    // The failure is not a phase failure, so it is returned for the `emerge:`
    // line; the dropped dependents are announced, not folded into the message.
    assert_eq!(err, "dep boom");
}

#[test]
fn run_merge_loop_keep_going_restarts_the_progress_counter() {
    // bad <- top; `good` is independent. After `bad` fails and `top` is dropped,
    // `good` is the only package left: Portage prints `(1 of 1)`, not `(2 of 3)`.
    let mut bad = source_entry(
        "bad",
        PretendOutcome::New {
            version: "1".into(),
        },
    );
    bad.required_by = vec![("dev-libs".into(), "top".into())];
    let good = source_entry(
        "good",
        PretendOutcome::New {
            version: "1".into(),
        },
    );
    let top = source_entry(
        "top",
        PretendOutcome::New {
            version: "1".into(),
        },
    );
    let root = tempdir();
    let no_paths = |_: &GraphEntry| None;
    let report = FailReport {
        root: &root,
        source_paths: &no_paths,
        features: String::new(),
    };
    let mut seen: Vec<(String, usize, usize)> = Vec::new();
    let _ = run_merge_loop(&[bad, good, top], true, &report, |_, e, p| {
        seen.push((e.package.clone(), p.cur, p.max));
        if e.package == "bad" {
            return Err("bad boom".into());
        }
        Ok(())
    });
    assert_eq!(
        seen,
        vec![("bad".to_string(), 1, 3), ("good".to_string(), 1, 1),]
    );
}

#[test]
fn wrap_words_breaks_like_textwrap() {
    // Expected lines are Python 3's `textwrap.wrap(text, width)` (which splits
    // after a hyphen that follows two letters and cuts words longer than a line).
    let cases: [(&str, usize, &[&str]); 5] = [
        (
            "emerge --keep-going: dev-libs/schedbaddep-1.0 for /var/tmp/w0/bt-final1/test_emerge_keep_going_endgame0/root dropped because it requires dev-libs/schedbad",
            75,
            &[
                "emerge --keep-going: dev-libs/schedbaddep-1.0 for /var/tmp/w0/bt-",
                "final1/test_emerge_keep_going_endgame0/root dropped because it requires",
                "dev-libs/schedbad",
            ],
        ),
        (
            "emerge --keep-going: dev-util/top-1 for /var/tmp/b423/root/ dropped because it requires dev-util/bad",
            75,
            &[
                "emerge --keep-going: dev-util/top-1 for /var/tmp/b423/root/ dropped because",
                "it requires dev-util/bad",
            ],
        ),
        (
            "The following 2 packages have failed to build, install, or execute postinst:",
            72,
            &[
                "The following 2 packages have failed to build, install, or execute",
                "postinst:",
            ],
        ),
        (
            "aaaa-bbbb-cccc dddd-eeee-ffff-gggg-hhhh-iiii-jjjj-kkkk-llll-mmmm-nnnn-oooo-pppp-qqqq-rrrr-ssss-tttt-uuuu-vvvv-wwww-xxxx-yyyy zzzz",
            40,
            &[
                "aaaa-bbbb-cccc dddd-eeee-ffff-gggg-hhhh-",
                "iiii-jjjj-kkkk-llll-mmmm-nnnn-oooo-pppp-",
                "qqqq-rrrr-ssss-tttt-uuuu-vvvv-wwww-xxxx-",
                "yyyy zzzz",
            ],
        ),
        (
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx tail",
            30,
            &[
                "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
                "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
                "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
                "xxxxxxxxxx tail",
            ],
        ),
    ];
    for (text, width, expected) in cases {
        assert_eq!(wrap_words(text, width), expected, "{text}");
    }
}

#[test]
fn run_source_merge_upgrade_replaces_the_installed_version() {
    // Merges 1.0 (`New`), then 2.0 (`Upgrade`): 2.0's files land, 1.0's file
    // is unmerged, 1.0's vdb entry is gone, and 1.0's `pkg_prerm`/`pkg_postrm`
    // run from its saved vdb env (each fixture hook appends `<phase>-<PVR>`).
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let options = ebuild_merge::MergeOptions {
        distdir: tempdir(),
        config_root: config_root.clone(),
        ..ebuild_merge::MergeOptions::default()
    };

    run_source_merge(RunSourceMergeArgs {
        entries: &[source_entry(
            "binpkgrmpkg",
            PretendOutcome::New {
                version: "1.0".into(),
            },
        )],
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &options,
        keep_going: false,
        buildpkg: None,
        buildpkg_exclude: &[],
        jobs: 1,
        load_average: None,
        capture_log: false,
        mode: StatusMode::for_tests(),
    })
    .expect("1.0 merges");
    run_source_merge(RunSourceMergeArgs {
        entries: &[source_entry(
            "binpkgrmpkg",
            PretendOutcome::Upgrade {
                from: "1.0".into(),
                to: "2.0".into(),
            },
        )],
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &options,
        keep_going: false,
        buildpkg: None,
        buildpkg_exclude: &[],
        jobs: 1,
        load_average: None,
        capture_log: false,
        mode: StatusMode::for_tests(),
    })
    .expect("2.0 upgrade merges");

    assert!(
        root.join("var/db/pkg/dev-libs/binpkgrmpkg-2.0/CONTENTS")
            .is_file()
    );
    assert!(!root.join("var/db/pkg/dev-libs/binpkgrmpkg-1.0").exists());
    assert!(root.join("usr/share/binpkgrmpkg/payload-2.0.txt").is_file());
    assert!(!root.join("usr/share/binpkgrmpkg/payload-1.0.txt").exists());
    assert_eq!(
        fs::read_to_string(root.join("var/lib/binpkgrmpkg.log")).unwrap(),
        "setup-1.0\npreinst-1.0\npostinst-1.0\n\
         setup-2.0\npreinst-2.0\nprerm-1.0\npostrm-1.0\npostinst-2.0\n"
    );
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&portage_tmpdir);
}
