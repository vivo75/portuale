use super::*;

/// #37 S2 end-to-end: a real source merge with a resolved config
/// threads the full effective `USE`, the resolved (folded)
/// `FEATURES` -- which must win over `phase_env_vars`' raw process-env
/// base -- and the entry's `SLOT` into the phase, under **both**
/// backends. `build-info/USE` is what real `__dyn_install` writes from
/// the phase `${USE}`; the `.keep_*` marker is written by the external
/// `ebuild-helpers/keepdir` subprocess, which is why an unexported
/// `SLOT` used to produce a bare `-` (S0's `l2-env-*` findings).
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
        // The `${T}`/`build-info` files asserted below are the
        // subject; keep the builddir the way real `FEATURES=noclean`
        // does (the default post-merge clean is pinned separately).
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

/// Backlog #332 end to end: `envdreload-a` installs
/// `/etc/env.d/99envdreload` (`ENVDRELOAD_VAR=1`); the merge's
/// `env-update` rewrites `<root>/etc/profile.env`; `envdreload-b`,
/// built afterwards in the same run, must see the variable in its
/// `src_install` (real reloads config before every package task,
/// `Scheduler._allocate_config`), as the bed's `helper-doins` /
/// `PT_HELPER_DOINS` pair showed.
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

/// #333: a two-entry source merge with `CONFIG_PROTECT` resolved
/// from the config (`/etc /usr/share/cfgprotme`), none in the
/// process env. `cfgprotlate-a` installs `/etc/env.d/99cfgprotlate`
/// (`CONFIG_PROTECT="/usr/share/cfgprotlate"`) and its merge runs
/// env-update; `cfgprotlate-b` then installs one file under each path
/// over a locally modified copy. Real (Portage 3.0.82.2 in the bed
/// image, `env -i emerge`, same shape: pmtest
/// `differential-test-bed/scripts/333-config-protect.sh`) keeps both
/// local files and writes `._cfg0000_x` /
/// `._cfg0000_y`: the config's fold protects `x`
/// (`dblink._get_protect_obj`, `vartree.py:2063-2072`), and the
/// per-task `config.reload()` (`Scheduler.py:1899-1913`) protects
/// `y`.
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

/// #160: real `config.environ()`'s `filter_calling_env`
/// (`lib/portage/package/ebuild/config.py:3275-3305`, bug #189417) is
/// what keeps a variable the ebuild `unset` in an earlier phase unset:
/// every phase after `${T}/environment` exists spawns with only the
/// `special_env_vars.environ_whitelist` keys from the config/calling
/// env, and the saved environment supplies the rest. `unsetcflagspkg`'s
/// `src_compile` is `dev-build/ninja-1.13.2-r1`'s own `unset CFLAGS`
/// shape, so real's `__dyn_install` writes no `build-info/CFLAGS` and
/// the vdb gets no `CFLAGS` row (the l3-20260925T074707Z Class 4 row).
/// `CXXFLAGS` (never unset) is the control: its row must stay.
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
    // Real `bin/phase-functions.sh:1257` chgrps `${T}/environment` to
    // `${PORTAGE_GRPNAME:-portage}`; unprivileged, the default
    // `portage` group makes that fail and bash prints a `chgrp: …
    // Operation not permitted` line per phase straight to the harness
    // stderr. Point it at this process's own gid -- the group the file
    // already has -- so the chgrp is a permitted no-op. Ownership is
    // unchanged, so the aux-file assertion below is not weakened.
    let gid = unsafe { libc::getegid() };
    options.build_env = vec![
        ("CFLAGS".to_string(), "-O2 -pipe".to_string()),
        ("CXXFLAGS".to_string(), "-O2 -pipe".to_string()),
        ("PORTAGE_GRPNAME".to_string(), gid.to_string()),
    ];
    // Keep the builddir: the observed `${T}` file is copied into the
    // merged image, and the vdb read below happens on the final tree.
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

/// #158: a byte-identical installed file survives a re-merge when the
/// ebuild's own `pkg_preinst` helper is gated on `REPLACING_VERSIONS`
/// (real `dblink.treewalk`, `vartree.py:4768-4771`). Without the var
/// the fixture's preinst `rm`s the file; with it the preinst returns
/// early and `merge_tree`'s `needs_move` (real `_needs_move`,
/// `vartree.py:6363`) leaves the existing inode -- the property that
/// keeps real's OWNER (`/usr/share/man/man1/awk.1`'s `1:1`, #158)
/// instead of recreating the file from the image.
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
    // `rm` is a no-op and the image's `identical.txt` lands.
    merge(PretendOutcome::New {
        version: "4".into(),
    });
    let dest = root.join("usr/share/identicalownerpkg/identical.txt");
    let before = fs::symlink_metadata(&dest)
        .expect("the fixture installed its file")
        .ino();

    // Re-merge the same version: `installed_instance_pf` sees the
    // just-written vdb entry, `REPLACING_VERSIONS=4` is set for the
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

    // `samepkg` is packagepkg's RDEPEND -- already installed, so it's
    // an AlreadyInstalled entry that must be skipped silently.
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

/// Backlog #42 regression: `merge_one_source_entry` (the serial
/// `emerge` source path) pre-cleans before every build, exactly like
/// real `_emerge/EbuildBuild._start_pre_clean`. With a stale
/// `.installed` marker and a tampered `${D}` left by an earlier build
/// in the same `${PORTAGE_BUILDDIR}` (kept here via `noclean`), the
/// next run must discard them and rebuild -- previously `install`
/// printed "already installed; skipping" and merged the stale image,
/// which is how a `-B` after a source merge shipped stripped
/// binaries (#38 S4).
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

    // Tamper: replace the real image file with a stale one, keeping
    // the `.installed` marker -- the exact state the missing
    // pre-clean treated as "already built".
    let image = builddir.join("image/usr/share/packagepkg");
    std::fs::remove_file(image.join("hello.txt")).unwrap();
    std::fs::write(image.join("stale.txt"), "stale\n").unwrap();

    // Second merge, no noclean: the pre-clean drops the stale
    // `.installed`/image, so `install` really re-runs.
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

    // schedparent RDEPENDs schedleaf-a + schedleaf-b (both leaves,
    // buildable, not installed). required_by wires the DAG so the
    // scheduler builds the leaves first (concurrently under jobs=2)
    // and schedparent only after both have merged.
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
    // A load average of 1e9 can never be exceeded -> the throttle is
    // a no-op and every package still builds and merges.
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

    // schedbad's src_install dies; schedbaddep RDEPENDs it (so it must
    // be skipped); schedok is independent and must still merge.
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
    assert!(err.contains("schedbad-1.0"), "{err}");
    assert!(err.contains("schedbaddep"), "{err}");

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
    // Real `Scheduler._keep_scheduling`/`_terminate_tasks`
    // (`PollScheduler.py:106-126`): once any package fails without
    // `--keep-going`, real portage sends kill signals to whatever's
    // still running rather than letting it finish -- its own result
    // would be discarded regardless. `schedbad` (fails almost
    // instantly, in `install`, the last phase) and `schedslow`
    // (sleeps 120s in `compile`, well before `install`) are
    // independent leaves under `jobs=2`, so both start together;
    // `schedbad` fails long before `schedslow`'s own sleep would
    // ever finish on its own.
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
    assert!(err.contains("schedbad-1.0"), "{err}");
    // Real generously bounded (loaded machines, #308): well under schedslow's own 120s sleep,
    // proving the scheduler didn't just wait it out.
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
    // The error is raised before any real execution, so a bogus
    // ROOT/tmpdir that would fail loudly if touched is safe here.
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
    let err = run_merge_loop(&[a, b], false, &tempdir(), |_, e| {
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
    let err = run_merge_loop(&[dep, mid, top, other], true, &tempdir(), |_, e| {
        if e.package == "dep" {
            return Err("dep boom".into());
        }
        merged.push(e.package.clone());
        Ok(())
    })
    .unwrap_err();

    assert_eq!(merged, vec!["other".to_string()]);
    assert!(
        err.contains("1 package(s) failed to merge (--keep-going):"),
        "{err}"
    );
    assert!(err.contains("  dep boom"), "{err}");
    assert!(err.contains("2 dependent package(s) not merged:"), "{err}");
    assert!(err.contains("  dev-libs/mid"), "{err}");
    assert!(err.contains("  dev-libs/top"), "{err}");
}

#[test]
fn run_source_merge_upgrade_replaces_the_installed_version() {
    // Merge binpkgrmpkg-1.0 (New), then 2.0 (Upgrade) -- 2.0's files
    // land, 1.0's own file is unmerged, 1.0's vdb entry is gone, and
    // 1.0's pkg_prerm/pkg_postrm run from its own saved vdb env (the
    // fixture's five hooks each append `<phase>-<PVR>` to a ROOT log).
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
