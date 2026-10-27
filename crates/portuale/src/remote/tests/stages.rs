use super::*;

#[test]
fn merge_driver_protects_a_modified_config() {
    let tmp = TempDir::new("portuale-remote-protect");
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("etc")).unwrap();
    std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
    // Live config differs from the incoming image version.
    std::fs::write(
        root.join("etc/probe.conf"),
        "live
",
    )
    .unwrap();
    let (unit, staged) = synthetic_unit(
        &tmp,
        &[(
            "etc/probe.conf",
            "incoming
",
        )],
    );
    let ctx = local_ctx(root.to_str().unwrap(), tmp.join("work").to_str().unwrap());
    let markers = run_merge_stage(&ctx, None, &unit, &staged, None).expect("merge succeeds");
    assert!(markers.iter().any(|m| m == "MERGE_COPY=ok"), "{markers:?}");
    // Original untouched, update diverted to a `._cfg` sibling...
    assert_eq!(
        std::fs::read_to_string(root.join("etc/probe.conf")).unwrap(),
        "live
"
    );
    let cfg = root.join("etc/._cfg0000_probe.conf");
    assert_eq!(
        std::fs::read_to_string(&cfg).unwrap(),
        "incoming
"
    );
    // ...while CONTENTS records the logical path (like the local merge).
    let contents =
        std::fs::read_to_string(root.join("var/db/pkg/dev-libs/probe-1.0/CONTENTS")).unwrap();
    assert!(
        contents.contains("obj /etc/probe.conf "),
        "unexpected CONTENTS:\n{contents}"
    );
    // Identical content merges in place (no `._cfg` spam): the
    // live file now matches the image, so reinstalling overwrites.
    std::fs::write(root.join("etc/probe.conf"), "incoming\n").unwrap();
    let markers = run_merge_stage(&ctx, None, &unit, &staged, None).expect("remerge succeeds");
    assert!(markers.iter().any(|m| m == "MERGE_COPY=ok"), "{markers:?}");
    assert!(!root.join("etc/._cfg0001_probe.conf").exists());
    assert_eq!(
        std::fs::read_to_string(root.join("etc/probe.conf")).unwrap(),
        "incoming\n"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Replacing a file must never write the existing inode: an open fd keeps
/// the old bytes while the path carries the new content on a fresh inode.
#[test]
fn merge_driver_never_writes_the_existing_inode() {
    use std::io::Read;
    use std::os::unix::fs::MetadataExt as _;

    let tmp = TempDir::new("portuale-remote-atomic-fd");
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("lib")).unwrap();
    std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
    std::fs::write(root.join("lib/libx.so.1"), b"old library bytes").unwrap();
    let old_inode = std::fs::metadata(root.join("lib/libx.so.1")).unwrap().ino();
    let mut old_fd = std::fs::File::open(root.join("lib/libx.so.1")).unwrap();

    let (unit, staged) =
        synthetic_unit_bytes(&tmp, &[("lib/libx.so.1", b"new library bytes, longer")]);
    let ctx = local_ctx(root.to_str().unwrap(), tmp.join("work").to_str().unwrap());
    let markers = run_merge_stage(&ctx, None, &unit, &staged, None).expect("merge succeeds");
    assert!(markers.iter().any(|m| m == "MERGE_COPY=ok"), "{markers:?}");

    let mut still_open = Vec::new();
    old_fd.read_to_end(&mut still_open).unwrap();
    assert_eq!(
        still_open, b"old library bytes",
        "the existing inode must never be written to"
    );
    assert_eq!(
        std::fs::read(root.join("lib/libx.so.1")).unwrap(),
        b"new library bytes, longer"
    );
    assert_ne!(
        std::fs::metadata(root.join("lib/libx.so.1")).unwrap().ino(),
        old_inode,
        "the destination must be a fresh inode"
    );
    let leftovers: Vec<String> = std::fs::read_dir(root.join("lib"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.contains("_portage_merge_"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "no merge temporary may remain: {leftovers:?}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Re-merging a currently-executing binary succeeds, and the running
/// process keeps its old inode while the path carries the new binary.
#[test]
fn merge_driver_replaces_a_running_executable_atomically() {
    use std::os::unix::fs::MetadataExt as _;

    let sleep = ["/bin/sleep", "/usr/bin/sleep"]
        .iter()
        .map(std::path::Path::new)
        .find(|p| p.is_file())
        .expect("a system sleep binary");
    let true_bin = ["/bin/true", "/usr/bin/true"]
        .iter()
        .map(std::path::Path::new)
        .find(|p| p.is_file())
        .expect("a system true binary");

    let tmp = TempDir::new("portuale-remote-atomic-exec");
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("bin")).unwrap();
    std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
    std::fs::copy(sleep, root.join("bin/prog")).unwrap();
    let old_inode = std::fs::metadata(root.join("bin/prog")).unwrap().ino();
    let new_bytes = std::fs::read(true_bin).unwrap();
    let (unit, staged) = synthetic_unit_bytes(&tmp, &[("bin/prog", new_bytes.as_slice())]);

    let spawn = || {
        std::process::Command::new(root.join("bin/prog"))
            .arg("60")
            .spawn()
    };
    let mut child = match spawn() {
        Ok(child) => child,
        Err(first) => {
            // A loaded test machine can transiently fail a fork/exec
            // with EAGAIN; retry once before giving up.
            std::thread::sleep(std::time::Duration::from_millis(500));
            spawn().unwrap_or_else(|e| panic!("spawn the old binary ({first}, {e})"))
        }
    };
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(
        child.try_wait().expect("try_wait").is_none(),
        "the old binary must still be executing before the merge"
    );

    let ctx = local_ctx(root.to_str().unwrap(), tmp.join("work").to_str().unwrap());
    let outcome = run_merge_stage(&ctx, None, &unit, &staged, None);

    let alive = child.try_wait().expect("try_wait").is_none();
    let _ = child.kill();
    let _ = child.wait();
    let markers = outcome.expect(
        "replacing a running executable must not write its inode (ETXTBSY would fail the copy)",
    );
    assert!(markers.iter().any(|m| m == "MERGE_COPY=ok"), "{markers:?}");
    assert!(alive, "the running process must survive the merge");
    assert_eq!(
        std::fs::read(root.join("bin/prog")).unwrap(),
        new_bytes,
        "the path must carry the new binary"
    );
    assert_ne!(
        std::fs::metadata(root.join("bin/prog")).unwrap().ino(),
        old_inode,
        "the destination must be a fresh inode"
    );
    let leftovers: Vec<String> = std::fs::read_dir(root.join("bin"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.contains("_portage_merge_"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "no merge temporary may remain: {leftovers:?}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn merge_driver_aborts_on_unowned_collision() {
    let tmp = TempDir::new("portuale-remote-collide");
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("usr/share")).unwrap();
    std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
    // A file another installed package owns: fail-closed. An unowned
    // orphan is simply overwritten -- only foreign ownership aborts.
    std::fs::write(
        root.join("usr/share/foreign.txt"),
        "mine
",
    )
    .unwrap();
    let (unit, staged) = synthetic_unit(
        &tmp,
        &[(
            "usr/share/foreign.txt",
            "theirs
",
        )],
    );
    let owner = root.join("var/db/pkg/dev-libs/owner-1.0");
    std::fs::create_dir_all(&owner).unwrap();
    std::fs::write(owner.join("SLOT"), "0\n").unwrap();
    std::fs::write(
        owner.join("CONTENTS"),
        "obj /usr/share/foreign.txt deadbeef 100\n",
    )
    .unwrap();
    let ctx = local_ctx(root.to_str().unwrap(), tmp.join("work").to_str().unwrap());
    let err = run_merge_stage(&ctx, None, &unit, &staged, None).unwrap_err();
    assert!(err.contains("collision"), "{err}");
    // Nothing was written: no vdb for the new package (the other
    // owner's entry stays), foreign file untouched.
    assert!(!root.join("var/db/pkg/dev-libs/probe-1.0").exists());
    assert!(root.join("var/db/pkg/dev-libs/owner-1.0").exists());
    assert_eq!(
        std::fs::read_to_string(root.join("usr/share/foreign.txt")).unwrap(),
        "mine
"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A foreign package owning `/usr/share/a b.txt` does not own `/usr/share/a`:
/// the ownership scan used to cut the CONTENTS path at the space, so shipping
/// the unowned `usr/share/a` over an orphan file aborted as a collision
/// (backlog #419; Portage's `_contents_re` keeps the whole path).
#[test]
fn merge_driver_ownership_scan_keeps_spaces_in_foreign_paths() {
    let tmp = TempDir::new("portuale-remote-space-collide");
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("usr/share")).unwrap();
    std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
    std::fs::write(root.join("usr/share/a"), "orphan\n").unwrap();
    let (unit, staged) = synthetic_unit(&tmp, &[("usr/share/a", "new\n")]);
    let owner = root.join("var/db/pkg/dev-libs/owner-1.0");
    std::fs::create_dir_all(&owner).unwrap();
    std::fs::write(owner.join("SLOT"), "0\n").unwrap();
    std::fs::write(
        owner.join("CONTENTS"),
        "obj /usr/share/a b.txt deadbeef 100\nsym /usr/share/c d -> a 100\n",
    )
    .unwrap();
    let ctx = local_ctx(root.to_str().unwrap(), tmp.join("work").to_str().unwrap());
    let markers = run_merge_stage(&ctx, None, &unit, &staged, None).expect("no false collision");
    assert!(markers.iter().any(|m| m == "MERGE_COPY=ok"), "{markers:?}");
    assert_eq!(
        std::fs::read_to_string(root.join("usr/share/a")).unwrap(),
        "new\n"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A merge plus the regen postinst run leaves `environment.bz2` holding
/// the postinst-time env, and no plain `environment` file.
#[test]
fn regen_postinst_rewrites_vdb_env_from_merge_time_env() {
    let tmp = regen_tmp("merge-time");
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
    let (unit, staged) = regen_unit(
        &tmp,
        "EAPI=8\nDESCRIPTION=\"synthetic regen probe\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\npkg_postinst() {\n\texport PT_MERGE_MARKER=\"merge-time\"\n}\n",
        "postinst",
    );
    let ctx = local_ctx(root.to_str().unwrap(), tmp.join("work").to_str().unwrap());
    let markers = run_merge_stage(&ctx, None, &unit, &staged, None).expect("merge succeeds");
    assert!(markers.iter().any(|m| m == "MERGE_VDB=ok"), "{markers:?}");
    let vdb = root.join("var/db/pkg/dev-libs/regen-1.0");
    // No plain `environment` in the vdb (Portage's shape).
    assert!(
        !vdb.join("environment").exists(),
        "MERGE_FLOW must not write a plain environment file"
    );
    // The vdb still holds the build-time env before the regen.
    let before = read_vdb_env(&vdb.join("environment.bz2"));
    assert!(
        before.contains("declare -- x=\"\""),
        "pre-regen env:\n{before}"
    );
    assert!(before.contains("buildpkg"), "pre-regen env:\n{before}");

    let report = run_postinst_regen_stage(RunPostinstRegenStageArgs {
        ctx: &ctx,
        control: None,
        unit_dir: &unit,
        staged: &staged,
        features: Some("sandbox merge-time"),
        locale: &[],
    })
    .expect("regen phase runs");
    assert_eq!(report.phase_rc, 0);
    assert!(report.regen_present);
    assert!(!report.skipped_no_hooks);
    install_regenerated_env(
        &ctx,
        None,
        &unit,
        vdb.join("environment.bz2").to_str().unwrap(),
        Some("bzip2"),
    )
    .expect("install succeeds");

    let after = read_vdb_env(&vdb.join("environment.bz2"));
    assert!(
        after.contains("declare -x PT_MERGE_MARKER=\"merge-time\""),
        "postinst-time var missing:\n{after}"
    );
    assert!(
        !after.contains("x=\"\""),
        "stray build-time x survived:\n{after}"
    );
    // Token-wise: the embedded `make.globals` carries `buildpkg-live`.
    assert!(
        !after
            .lines()
            .filter(|l| l.contains("FEATURES=\""))
            .any(|l| l
                .split(|c: char| !(c.is_alphanumeric() || c == '-'))
                .any(|t| t == "buildpkg")),
        "build-time FEATURES survived:\n{after}"
    );
    assert!(
        after.contains("declare -x FEATURES=\"sandbox merge-time\""),
        "resolved FEATURES missing:\n{after}"
    );
    assert!(
        after.contains("declare -x PORTAGE_FEATURES=\"sandbox merge-time\""),
        "resolved PORTAGE_FEATURES missing:\n{after}"
    );
    assert!(
        after.contains("declare -x PORTAGE_BZIP2_COMMAND=\"bzip2\""),
        "passthrough path leaked into the vdb env:\n{after}"
    );
    assert!(
        !vdb.join("environment").exists(),
        "install must not write a plain environment file"
    );
    // Atomic install leaves no temp behind.
    assert!(!vdb.join("environment.bz2.portuale-regen-tmp").exists());
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A package without `pkg_postinst` still gets a regenerated env: an
/// undefined function is a no-op that still re-saves.
#[test]
fn regen_runs_without_pkg_postinst() {
    let tmp = regen_tmp("no-postinst");
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
    let (unit, staged) = regen_unit(
        &tmp,
        "EAPI=8\nDESCRIPTION=\"synthetic regen probe, no postinst\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\n",
        "-",
    );
    assert!(!staged.postinst_defined);
    let ctx = local_ctx(root.to_str().unwrap(), tmp.join("work").to_str().unwrap());
    run_merge_stage(&ctx, None, &unit, &staged, None).expect("merge succeeds");
    let report = run_postinst_regen_stage(RunPostinstRegenStageArgs {
        ctx: &ctx,
        control: None,
        unit_dir: &unit,
        staged: &staged,
        features: Some("sandbox"),
        locale: &[],
    })
    .expect("regen phase runs");
    assert_eq!(report.phase_rc, 0, "undefined pkg_postinst is a no-op");
    assert!(report.regen_present);
    let vdb = root.join("var/db/pkg/dev-libs/regen-1.0");
    install_regenerated_env(
        &ctx,
        None,
        &unit,
        vdb.join("environment.bz2").to_str().unwrap(),
        Some("bzip2"),
    )
    .expect("install succeeds");
    let after = read_vdb_env(&vdb.join("environment.bz2"));
    assert!(
        !after.contains("x=\"\""),
        "stray build-time x survived:\n{after}"
    );
    assert!(
        after.contains("declare -x FEATURES=\"sandbox\""),
        "resolved FEATURES missing:\n{after}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A regen run leaves no `O` line, records `SHELL` unexported, and
/// carries the injected locale values.
#[test]
fn regen_matches_portage_o_shell_and_locale() {
    let tmp = regen_tmp("o-shell-locale");
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
    let (unit, staged) = regen_unit(
        &tmp,
        "EAPI=8\nDESCRIPTION=\"synthetic regen probe\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\npkg_postinst() {\n\texport PT_MERGE_MARKER=\"merge-time\"\n}\n",
        "postinst",
    );
    let ctx = local_ctx(root.to_str().unwrap(), tmp.join("work").to_str().unwrap());
    let locale = vec![
        ("LANG".to_string(), "C.UTF-8".to_string()),
        ("LC_MESSAGES".to_string(), "C.UTF-8".to_string()),
    ];
    let report = run_postinst_regen_stage(RunPostinstRegenStageArgs {
        ctx: &ctx,
        control: None,
        unit_dir: &unit,
        staged: &staged,
        features: Some("sandbox"),
        locale: &locale,
    })
    .expect("regen phase runs");
    assert_eq!(report.phase_rc, 0);
    assert!(report.regen_present);
    let regen = std::fs::read_to_string(format!("{unit}/environment.regen")).expect("regen file");
    for line in regen.lines() {
        let Some(rest) = line.strip_prefix("declare ") else {
            continue;
        };
        let rest = rest
            .strip_prefix("-x ")
            .or_else(|| rest.strip_prefix("-- "))
            .unwrap_or(rest);
        let name = rest.split(['=', ' ']).next().unwrap_or("");
        assert_ne!(
            name, "O",
            "unit-local O leaked into the regen'd env:\n{regen}"
        );
    }
    assert!(
        regen.contains("declare -- SHELL="),
        "SHELL must be unexported like real's:\n{regen}"
    );
    assert!(
        !regen.contains("declare -x SHELL="),
        "exported SHELL leaked into the regen'd env:\n{regen}"
    );
    assert!(
        regen.contains("declare -x LANG=\"C.UTF-8\""),
        "server LANG missing:\n{regen}"
    );
    assert!(
        regen.contains("declare -x LC_MESSAGES=\"C.UTF-8\""),
        "server LC_* missing:\n{regen}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A regen run with only `LC_ALL=C.UTF-8` forwarded records `LANG` plus
/// all twelve categories as `C.UTF-8`, and no `LC_ALL`.
#[test]
fn regen_splits_lc_all_like_portage() {
    let tmp = regen_tmp("lc-all-split");
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
    let (unit, staged) = regen_unit(
        &tmp,
        "EAPI=8\nDESCRIPTION=\"synthetic regen probe\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\npkg_postinst() {\n\texport PT_MERGE_MARKER=\"merge-time\"\n}\n",
        "postinst",
    );
    let ctx = local_ctx(root.to_str().unwrap(), tmp.join("work").to_str().unwrap());
    let locale = vec![("LC_ALL".to_string(), "C.UTF-8".to_string())];
    let report = run_postinst_regen_stage(RunPostinstRegenStageArgs {
        ctx: &ctx,
        control: None,
        unit_dir: &unit,
        staged: &staged,
        features: Some("sandbox"),
        locale: &locale,
    })
    .expect("regen phase runs");
    assert_eq!(report.phase_rc, 0);
    assert!(report.regen_present);
    let regen = std::fs::read_to_string(format!("{unit}/environment.regen")).expect("regen file");
    assert!(
        regen.contains("declare -x LANG=\"C.UTF-8\""),
        "split LANG missing:\n{regen}"
    );
    for name in [
        "LC_COLLATE",
        "LC_CTYPE",
        "LC_MESSAGES",
        "LC_MONETARY",
        "LC_NUMERIC",
        "LC_TIME",
        "LC_ADDRESS",
        "LC_IDENTIFICATION",
        "LC_MEASUREMENT",
        "LC_NAME",
        "LC_PAPER",
        "LC_TELEPHONE",
    ] {
        assert!(
            regen.contains(&format!("declare -x {name}=\"C.UTF-8\"")),
            "split {name} missing:\n{regen}"
        );
    }
    assert!(
        !regen.lines().any(|line| line.contains("LC_ALL")),
        "LC_ALL leaked into the regen'd env:\n{regen}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A `make.globals` on the server's config root is not staged under the
/// pulled client root: Portage reads it from its own install only, so the
/// embedded copy is the bottom layer (the client's `-news sign` folds over
/// it).
#[test]
fn remote_client_config_stacks_server_make_globals() {
    let _env_guard = PLACED_CONFIG_ENV_LOCK.lock().unwrap();
    let tmp = regen_tmp("server-globals");
    let server_root = tmp.join("serverroot");
    std::fs::create_dir_all(server_root.join("usr/share/portage/config")).unwrap();
    std::fs::write(
        server_root.join("usr/share/portage/config/make.globals"),
        "FEATURES=\"news sandbox\"\n",
    )
    .unwrap();
    let client_etc = tmp.join("clientroot/etc/portage");
    let repo = tmp.join("repo");
    std::fs::create_dir_all(client_etc.join("repos.conf")).unwrap();
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(
        client_etc.join("repos.conf/testrepo.conf"),
        format!(
            "[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = {}\n",
            repo.display()
        ),
    )
    .unwrap();
    std::fs::write(client_etc.join("make.conf"), "FEATURES=\"-news sign\"\n").unwrap();
    let saved_config_root = std::env::var_os("PORTAGE_CONFIGROOT");
    let saved_features = std::env::var_os("FEATURES");
    let saved_mask = std::env::var_os("INSTALL_MASK");
    // SAFETY: held `PLACED_CONFIG_ENV_LOCK`; no other test in this
    // binary pins these without it.
    unsafe {
        std::env::set_var("PORTAGE_CONFIGROOT", &server_root);
        std::env::remove_var("FEATURES");
        std::env::remove_var("INSTALL_MASK");
    }
    let mut ctx = local_ctx(
        tmp.join("root").to_str().unwrap(),
        tmp.join("work").to_str().unwrap(),
    );
    ctx.etc_portage = ConfigPlacement::Client(client_etc.to_string_lossy().into_owned());
    let placed = place_config_root(&ctx, None).expect("client placement resolves");
    assert!(
        !placed
            .dir
            .join("usr/share/portage/config/make.globals")
            .exists(),
        "no make.globals is staged under the pulled root"
    );
    let eroot = tmp.join("eroot");
    let (repos, config) = crate::pretend::load_repos_and_config(&placed.dir, &eroot)
        .expect("placed client config resolves");
    assert!(repos.iter().any(|r| r.is_main));
    let resolved = crate::pretend::config_features_string(&config);
    assert!(
        resolved.split_whitespace().any(|t| t == "sign")
            && !resolved.split_whitespace().any(|t| t == "news")
            && resolved.split_whitespace().any(|t| t == "preserve-libs"),
        "the embedded base folds under the client make.conf: {resolved}"
    );
    drop(placed);
    // SAFETY: same as above.
    unsafe {
        match saved_config_root {
            Some(value) => std::env::set_var("PORTAGE_CONFIGROOT", value),
            None => std::env::remove_var("PORTAGE_CONFIGROOT"),
        }
        match saved_features {
            Some(value) => std::env::set_var("FEATURES", value),
            None => std::env::remove_var("FEATURES"),
        }
        match saved_mask {
            Some(value) => std::env::set_var("INSTALL_MASK", value),
            None => std::env::remove_var("INSTALL_MASK"),
        }
    }
    // The regenerated vdb env carries the resolved list: merge the
    // synthetic unit, run the regen postinst with the resolved
    // `FEATURES`, and read the installed `environment.bz2`.
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
    let (unit, staged) = regen_unit(
        &tmp,
        "EAPI=8\nDESCRIPTION=\"synthetic globals probe\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\n",
        "-",
    );
    let ctx = local_ctx(root.to_str().unwrap(), tmp.join("work").to_str().unwrap());
    run_merge_stage(&ctx, None, &unit, &staged, None).expect("merge succeeds");
    let report = run_postinst_regen_stage(RunPostinstRegenStageArgs {
        ctx: &ctx,
        control: None,
        unit_dir: &unit,
        staged: &staged,
        features: Some(&resolved),
        locale: &[],
    })
    .expect("regen phase runs");
    assert_eq!(report.phase_rc, 0);
    assert!(report.regen_present);
    let vdb = root.join("var/db/pkg/dev-libs/regen-1.0");
    install_regenerated_env(
        &ctx,
        None,
        &unit,
        vdb.join("environment.bz2").to_str().unwrap(),
        Some("bzip2"),
    )
    .expect("install succeeds");
    let after = read_vdb_env(&vdb.join("environment.bz2"));
    assert!(
        after
            .lines()
            .any(|l| l.starts_with("declare -x FEATURES=\"")
                && l.contains(" sign")
                && !l.contains(" news ")),
        "resolved FEATURES missing:\n{after}"
    );
    assert!(
        after
            .lines()
            .any(|l| l.starts_with("declare -x PORTAGE_FEATURES=\"")
                && l.contains(" sign")
                && !l.contains(" news ")),
        "resolved PORTAGE_FEATURES missing:\n{after}"
    );
    // Token-wise: the embedded `make.globals` carries `buildpkg-live`.
    assert!(
        !after
            .lines()
            .filter(|l| l.contains("FEATURES=\""))
            .any(|l| l
                .split_whitespace()
                .any(|t| t.trim_matches('"') == "buildpkg")),
        "build-time FEATURES survived:\n{after}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A failed pull keeps the build-time `environment.bz2` byte-identical
/// and reports the step.
#[test]
fn install_keeps_build_time_env_when_pull_fails() {
    let tmp = regen_tmp("failure");
    let root = tmp.join("root");
    let vdb = root.join("var/db/pkg/dev-libs/regen-1.0");
    std::fs::create_dir_all(&vdb).unwrap();
    // A build-time env already in the vdb (the MERGE_FLOW fallback).
    let build_time =
        bzip2_compress(build_time_environment(false).as_bytes()).expect("server bzip2 compresses");
    std::fs::write(vdb.join("environment.bz2"), &build_time).unwrap();
    let unit = tmp.join("work/regen-1.0");
    std::fs::create_dir_all(&unit).unwrap();
    let ctx = local_ctx(root.to_str().unwrap(), tmp.join("work").to_str().unwrap());
    let err = install_regenerated_env(
        &ctx,
        None,
        unit.to_str().unwrap(),
        vdb.join("environment.bz2").to_str().unwrap(),
        Some("bzip2"),
    )
    .expect_err("pull must fail without environment.regen");
    assert_eq!(err.0, "pull", "unexpected failing step: {err:?}");
    assert_eq!(
        std::fs::read(vdb.join("environment.bz2")).unwrap(),
        build_time,
        "the build-time env must survive byte-identical"
    );
    assert_eq!(
        regen_warn_message("dev-libs/regen-1.0", "pull"),
        "!!! Remote dev-libs/regen-1.0: merge-time environment regen failed at pull, keeping build-time environment.bz2",
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A failed atomic install leaves no client tmp litter: the remote temp
/// name is removed best-effort.
#[test]
fn install_file_atomic_removes_tmp_when_mv_fails() {
    let tmp = regen_tmp("install-tmp");
    let root = tmp.join("root");
    let vdb = root.join("var/db/pkg/dev-libs/regen-1.0");
    std::fs::create_dir_all(&vdb).unwrap();
    // A directory at the dest path makes the `rename` fail.
    let dest = vdb.join("environment.bz2");
    std::fs::create_dir_all(&dest).unwrap();
    let ctx = local_ctx(root.to_str().unwrap(), tmp.join("work").to_str().unwrap());
    let err = install_file_atomic(&ctx, None, b"regen", dest.to_str().unwrap())
        .expect_err("install into a directory must fail");
    assert!(err.contains("installing"), "unexpected error text: {err}");
    assert!(
        !vdb.join("environment.bz2.portuale-regen-tmp").exists(),
        "the remote temp name must not litter the client"
    );
    assert!(dest.is_dir(), "the blocking directory is untouched");
    let _ = std::fs::remove_dir_all(&tmp);
}

/// The ssh leg stages the bytes into a file before shipping: against an
/// unreachable host the install still fails, but past staging -- at
/// transport.
#[test]
fn install_file_atomic_ssh_stages_bytes_before_transport() {
    let ctx = ctx_with_args("unreachable.invalid", None);
    let err = install_file_atomic(
        &ctx,
        None,
        b"regen",
        "/var/db/pkg/dev-libs/regen-1.0/environment.bz2",
    )
    .expect_err("no ssh server: the install must fail");
    assert!(
        !err.contains("staging the regen install failed"),
        "ssh staging must succeed (transport may fail): {err}"
    );
}

/// The scrub rewrites only the unit-local passthrough stand-in; every
/// other value passes verbatim and a missing line stays missing.
#[test]
fn scrub_bzip2_command_rewrites_only_the_passthrough() {
    let regen = b"declare -x FEATURES=\"sandbox\"\ndeclare -x PORTAGE_BZIP2_COMMAND=\"'/tmp/w/bin/bzip2-passthrough'\"\ndeclare -x USE=\"amd64\"\n";
    // Configured client value (e.g. `lbzip2` in `make.conf`) is written.
    let scrubbed = scrub_bzip2_command(regen, "lbzip2");
    assert_eq!(
        String::from_utf8(scrubbed).unwrap(),
        "declare -x FEATURES=\"sandbox\"\ndeclare -x PORTAGE_BZIP2_COMMAND=\"lbzip2\"\ndeclare -x USE=\"amd64\"\n",
    );
    // Unset (the `make.globals` default) restores `bzip2`.
    let scrubbed = scrub_bzip2_command(regen, "bzip2");
    assert_eq!(
        String::from_utf8(scrubbed).unwrap(),
        "declare -x FEATURES=\"sandbox\"\ndeclare -x PORTAGE_BZIP2_COMMAND=\"bzip2\"\ndeclare -x USE=\"amd64\"\n",
    );
    // An unrelated value -- even a foreign compressor path -- is
    // copied verbatim, never forced to the configured one ...
    let foreign = b"declare -x PORTAGE_BZIP2_COMMAND=\"/opt/bin/pbzip2\"\n";
    assert_eq!(scrub_bzip2_command(foreign, "lbzip2"), foreign);
    // ... Portage's own spelling passes through byte-identical ...
    let real = b"declare -x PORTAGE_BZIP2_COMMAND=\"bzip2\"\n";
    assert_eq!(scrub_bzip2_command(real, "lbzip2"), real);
    // ... and a missing line stays missing (never invent it).
    let bare = b"declare -x FEATURES=\"sandbox\"\n";
    assert_eq!(scrub_bzip2_command(bare, "lbzip2"), bare);
    // Trailing line without a newline is preserved.
    let noeol =
        b"declare -x FEATURES=\"sandbox\"\ndeclare -x PORTAGE_BZIP2_COMMAND=\"'/tmp/w/bin/bzip2-passthrough'\"";
    assert_eq!(
        String::from_utf8(scrub_bzip2_command(noeol, "lbzip2")).unwrap(),
        "declare -x FEATURES=\"sandbox\"\ndeclare -x PORTAGE_BZIP2_COMMAND=\"lbzip2\"\n",
    );
}

/// The old-version probe parser takes only `OLDPF=` lines apart into
/// `(pf, slot)`.
#[test]
fn parse_oldpf_probe_parses_only_probe_lines() {
    assert_eq!(
        parse_oldpf_probe("OLDPF=old-1.0 SLOT=0\nnoise\nOLDPF=old-0.9 SLOT=unknown\n"),
        vec![
            ("old-1.0".to_string(), "0".to_string()),
            ("old-0.9".to_string(), "unknown".to_string()),
        ]
    );
    assert!(parse_oldpf_probe("UNPACK=ok\n").is_empty());
    assert!(parse_oldpf_probe("OLDPF= SLOT=0\n").is_empty());
}

/// When both the `environment.bz2` pull and the plain fallback pull fail,
/// the staging warning carries the original bz2 error.
#[test]
fn ship_old_hook_envs_warns_when_both_pulls_fail() {
    let tmp = regen_tmp("old-noenv");
    let root = tmp.join("root");
    let work = tmp.join("work");
    let oldvdb = root.join("var/db/pkg/dev-libs/oldhook-1.0");
    std::fs::create_dir_all(&oldvdb).unwrap();
    std::fs::write(oldvdb.join("SLOT"), "0\n").unwrap();
    // Neither `environment.bz2` nor `environment` on the client.
    assert!(!oldvdb.join("environment.bz2").exists());
    assert!(!oldvdb.join("environment").exists());

    let unit = work.join("oldhook-2.0");
    std::fs::create_dir_all(&unit).unwrap();
    let ctx = local_ctx(root.to_str().unwrap(), work.to_str().unwrap());
    let shipment = ship_old_hook_envs(ShipOldHookEnvsArgs {
        ctx: &ctx,
        control: None,
        unit_dir: unit.to_str().unwrap(),
        vdb: root.join("var/db/pkg").to_str().unwrap(),
        category: "dev-libs",
        pn: "oldhook",
        new_pf: "oldhook-2.0",
        new_mainslot: "0",
    });
    assert!(shipment.shipped.is_empty(), "{:?}", shipment.shipped);
    assert_eq!(shipment.warnings.len(), 1, "{:?}", shipment.warnings);
    assert!(
        shipment.warnings[0].contains("oldhook-1.0"),
        "warning must name the pf: {:?}",
        shipment.warnings
    );
    assert!(
        shipment.warnings[0].contains("environment.bz2"),
        "warning must carry the original bz2 error: {:?}",
        shipment.warnings
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// With only `environment.bz2` in the old entry and no client `bzip2` on
/// `PATH`, the hook still gets a sourceable env via the staged
/// server-side decompress.
#[test]
fn old_hook_uses_server_decompressed_env_without_client_bzip2() {
    let tmp = regen_tmp("old-shipped");
    let root = tmp.join("root");
    let work = tmp.join("work");
    std::fs::create_dir_all(root.join("var/lib")).unwrap();
    let oldvdb = root.join("var/db/pkg/dev-libs/oldhook-1.0");
    std::fs::create_dir_all(&oldvdb).unwrap();
    std::fs::write(oldvdb.join("SLOT"), "0\n").unwrap();
    std::fs::write(
        oldvdb.join("oldhook-1.0.ebuild"),
        "EAPI=8\nDESCRIPTION=\"synthetic old-hook probe\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\n",
    )
    .unwrap();
    // Only `environment.bz2` -- no plain file (Portage's shape).
    let compressed =
        bzip2_compress(old_hook_environment().as_bytes()).expect("server bzip2 compresses");
    std::fs::write(oldvdb.join("environment.bz2"), &compressed).unwrap();

    // The new unit (hook runtime for the old phases).
    let unit = work.join("oldhook-2.0");
    std::fs::create_dir_all(&unit).unwrap();
    let status = std::process::Command::new("cp")
        .args(["-a"])
        .arg(crate::ebuild_phases::bin_dir().unwrap())
        .arg(unit.join("bin"))
        .status()
        .expect("cp -a bin");
    assert!(status.success());

    // Server stages the decompressed env (local transport = same
    // paths the client sees).
    let ctx = local_ctx(root.to_str().unwrap(), work.to_str().unwrap());
    let shipment = ship_old_hook_envs(ShipOldHookEnvsArgs {
        ctx: &ctx,
        control: None,
        unit_dir: unit.to_str().unwrap(),
        vdb: root.join("var/db/pkg").to_str().unwrap(),
        category: "dev-libs",
        pn: "oldhook",
        new_pf: "oldhook-2.0",
        new_mainslot: "0",
    });
    assert!(shipment.warnings.is_empty(), "{:?}", shipment.warnings);
    assert_eq!(shipment.shipped, vec!["oldhook-1.0".to_string()]);
    assert_eq!(
        std::fs::read_to_string(unit.join("old-env/oldhook-1.0")).unwrap(),
        old_hook_environment(),
    );

    let farm = farm_path_without_bzip2(&tmp);
    let stdout = run_old_hook_snippet(
        unit.to_str().unwrap(),
        root.to_str().unwrap(),
        work.to_str().unwrap(),
        &farm,
        oldvdb.to_str().unwrap(),
    );
    assert!(stdout.contains("OLDHOOK_prerm=0"), "stdout:\n{stdout}");
    assert!(stdout.contains("OLDHOOK_postrm=0"), "stdout:\n{stdout}");
    assert_eq!(
        std::fs::read_to_string(root.join("var/lib/oldhook.log")).unwrap(),
        "prerm-ok\npostrm-ok\n"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Replacing an old instance whose CONTENTS records a path with a space
/// removes that file. The shell removal cut the path at the space (`set --
/// $line` takes `$2`, the ownership scan `awk '{print $2}'`), so the file
/// survived every remote replace; the shipped Rust unmerge parses CONTENTS
/// space-safely (backlog #382).
#[test]
fn merge_driver_removes_an_old_file_with_a_space_in_its_path() {
    use std::os::unix::fs::MetadataExt as _;
    let tmp = TempDir::new("portuale-remote-space-unmerge");
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
    // Old instance owning a spaced path, with matching md5+mtime.
    let spaced_rel = "usr/share/probe/a b.txt";
    let spaced = root.join(spaced_rel);
    std::fs::create_dir_all(spaced.parent().unwrap()).unwrap();
    std::fs::write(&spaced, "old spaced bytes\n").unwrap();
    let md5 = crate::ebuild_merge::md5_hex(&spaced).unwrap();
    let mtime = std::fs::metadata(&spaced).unwrap().mtime();
    let oldvdb = root.join("var/db/pkg/dev-libs/probe-1.0");
    std::fs::create_dir_all(&oldvdb).unwrap();
    std::fs::write(oldvdb.join("SLOT"), "0\n").unwrap();
    std::fs::write(oldvdb.join("COUNTER"), "3\n").unwrap();
    std::fs::write(
        oldvdb.join("probe-1.0.ebuild"),
        "EAPI=8\nDESCRIPTION=\"synthetic space probe\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\n",
    )
    .unwrap();
    std::fs::write(
        oldvdb.join("CONTENTS"),
        format!("obj /{spaced_rel} {md5} {mtime}\ndir /usr/share/probe\n"),
    )
    .unwrap();
    // New instance, same slot, disjoint files.
    let (unit, staged) = spaced_replacement_unit(&tmp);
    let ctx = local_ctx(root.to_str().unwrap(), tmp.join("work").to_str().unwrap());
    let markers = run_merge_stage(&ctx, None, &unit, &staged, None).expect("merge succeeds");
    assert!(markers.iter().any(|m| m == "MERGE_COPY=ok"), "{markers:?}");
    assert!(
        !spaced.exists(),
        "the spaced old file must be removed, not cut at the space"
    );
    assert!(!oldvdb.exists(), "the replaced vdb entry must be retired");
    assert!(root.join("usr/share/probe/new.txt").is_file());
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A synthetic replacement unit with a caller-chosen pf (the shared
/// helper hardcodes `probe-1.0`, which the old instance already takes).
fn spaced_replacement_unit(tmp: &std::path::Path) -> (String, crate::remote_bundle::StagedBundle) {
    let unit = tmp.join("work/probe-2.0");
    let image = unit.join("image");
    let build_info = unit.join("build-info");
    std::fs::create_dir_all(&build_info).unwrap();
    let path = image.join("usr/share/probe/new.txt");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "new\n").unwrap();
    for (name, content) in [
        ("PF", "probe-2.0"),
        ("CATEGORY", "dev-libs"),
        ("SLOT", "0"),
        ("DEFINED_PHASES", "-"),
    ] {
        std::fs::write(build_info.join(name), content).unwrap();
    }
    let entries = crate::remote_bundle::collect_filemeta(&image).unwrap();
    let filemeta = crate::remote_bundle::render_filemeta(&entries).unwrap();
    std::fs::write(unit.join("filemeta"), &filemeta).unwrap();
    let staged = crate::remote_bundle::StagedBundle {
        tarball: tmp.join("bundle.tar"),
        byte_count: 0,
        manifest: crate::remote_bundle::BundleManifest {
            format: 1,
            cpv: "dev-libs/probe-2.0".to_string(),
            slot: "0".to_string(),
            repo: "test".to_string(),
            has_environment: false,
        },
        eapi: "8".to_string(),
        category: "dev-libs".to_string(),
        pn: "probe".to_string(),
        pv: "2.0".to_string(),
        pr: "r0".to_string(),
        pvr: "2.0".to_string(),
        p: "probe-2.0".to_string(),
        pf: "probe-2.0".to_string(),
        phases: Vec::new(),
        postinst_defined: false,
    };
    (unit.to_str().unwrap().to_string(), staged)
}

/// An entry with both files whose staging failed still runs its hooks
/// from the plain `environment` final fallback.
#[test]
fn old_hook_falls_back_to_plain_when_bz2_present_without_client_bzip2() {
    let tmp = regen_tmp("old-both");
    let root = tmp.join("root");
    let work = tmp.join("work");
    std::fs::create_dir_all(root.join("var/lib")).unwrap();
    let oldvdb = root.join("var/db/pkg/dev-libs/oldhook-1.0");
    std::fs::create_dir_all(&oldvdb).unwrap();
    std::fs::write(oldvdb.join("SLOT"), "0\n").unwrap();
    std::fs::write(
        oldvdb.join("oldhook-1.0.ebuild"),
        "EAPI=8\nDESCRIPTION=\"synthetic old-hook probe\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\n",
    )
    .unwrap();
    // Both files (older `mrg`), nothing staged: the bz2 is
    // unreadable without a client bzip2, so the plain file wins.
    let compressed =
        bzip2_compress(old_hook_environment().as_bytes()).expect("server bzip2 compresses");
    std::fs::write(oldvdb.join("environment.bz2"), &compressed).unwrap();
    std::fs::write(oldvdb.join("environment"), old_hook_environment()).unwrap();

    let unit = work.join("oldhook-2.0");
    std::fs::create_dir_all(&unit).unwrap();
    let status = std::process::Command::new("cp")
        .args(["-a"])
        .arg(crate::ebuild_phases::bin_dir().unwrap())
        .arg(unit.join("bin"))
        .status()
        .expect("cp -a bin");
    assert!(status.success());

    let farm = farm_path_without_bzip2(&tmp);
    let stdout = run_old_hook_snippet(
        unit.to_str().unwrap(),
        root.to_str().unwrap(),
        work.to_str().unwrap(),
        &farm,
        oldvdb.to_str().unwrap(),
    );
    assert!(stdout.contains("OLDHOOK_prerm=0"), "stdout:\n{stdout}");
    assert!(stdout.contains("OLDHOOK_postrm=0"), "stdout:\n{stdout}");
    assert_eq!(
        std::fs::read_to_string(root.join("var/lib/oldhook.log")).unwrap(),
        "prerm-ok\npostrm-ok\n"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A legacy entry (plain `environment`, no `environment.bz2`) still
/// sources with no client `bzip2` and nothing staged.
#[test]
fn old_hook_falls_back_to_legacy_plain_environment() {
    let tmp = regen_tmp("old-legacy");
    let root = tmp.join("root");
    let work = tmp.join("work");
    std::fs::create_dir_all(root.join("var/lib")).unwrap();
    let oldvdb = root.join("var/db/pkg/dev-libs/oldhook-1.0");
    std::fs::create_dir_all(&oldvdb).unwrap();
    std::fs::write(oldvdb.join("SLOT"), "0\n").unwrap();
    std::fs::write(
        oldvdb.join("oldhook-1.0.ebuild"),
        "EAPI=8\nDESCRIPTION=\"synthetic old-hook probe\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\n",
    )
    .unwrap();
    std::fs::write(oldvdb.join("environment"), old_hook_environment()).unwrap();
    assert!(!oldvdb.join("environment.bz2").exists());

    let unit = work.join("oldhook-2.0");
    std::fs::create_dir_all(&unit).unwrap();
    let status = std::process::Command::new("cp")
        .args(["-a"])
        .arg(crate::ebuild_phases::bin_dir().unwrap())
        .arg(unit.join("bin"))
        .status()
        .expect("cp -a bin");
    assert!(status.success());

    let farm = farm_path_without_bzip2(&tmp);
    let stdout = run_old_hook_snippet(
        unit.to_str().unwrap(),
        root.to_str().unwrap(),
        work.to_str().unwrap(),
        &farm,
        oldvdb.to_str().unwrap(),
    );
    assert!(stdout.contains("OLDHOOK_prerm=0"), "stdout:\n{stdout}");
    assert!(stdout.contains("OLDHOOK_postrm=0"), "stdout:\n{stdout}");
    assert_eq!(
        std::fs::read_to_string(root.join("var/lib/oldhook.log")).unwrap(),
        "prerm-ok\npostrm-ok\n"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}
