use super::*;

#[test]
fn merge_driver_protects_a_modified_config() {
    let tmp = TempDir::new("portuale-remote-protect").keep();
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

    let tmp = TempDir::new("portuale-remote-atomic-fd").keep();
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

    let tmp = TempDir::new("portuale-remote-atomic-exec").keep();
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
    let tmp = TempDir::new("portuale-remote-collide").keep();
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
