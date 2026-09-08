use super::*;

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
