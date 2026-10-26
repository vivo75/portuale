use super::*;

#[test]
fn clock_gate_bounds_skew_and_disables_at_zero() {
    assert_eq!(clock_gate(Some("1000"), 1000, 900), None);
    assert_eq!(clock_gate(Some("100"), 1000, 900), None);
    assert_eq!(
        clock_gate(Some("99"), 1000, 900),
        Some("client/server clocks differ by 901s (limit 900s)".to_string())
    );
    // `0` disables the check entirely.
    assert_eq!(clock_gate(Some("1"), 1_000_000, 0), None);
    // Unparsable client time fails open here (the bash gate already
    // failed -- no CLIENT_TIME means the script never ran right).
    assert_eq!(clock_gate(None, 1000, 900), None);
    assert_eq!(clock_gate(Some("soon"), 1000, 900), None);
}

#[test]
fn preflight_gates_catch_old_bash_missing_tools_and_roots() {
    let mut ok = HashMap::new();
    for (k, v) in [
        ("BASH_MAJOR", "5"),
        ("BASH_MINOR", "3"),
        ("TOOL_tar", "yes"),
        ("TOOL_mkdir", "yes"),
        ("TOOL_rm", "yes"),
        ("TOOL_cat", "yes"),
        ("TOOL_chmod", "yes"),
        ("TOOL_ln", "yes"),
        ("TOOL_find", "yes"),
        ("TOOL_grep", "yes"),
        ("TOOL_sed", "yes"),
        ("TOOL_cmp", "yes"),
        ("TOOL_stat", "yes"),
        ("TOOL_readlink", "yes"),
        ("TOOL_id", "yes"),
        ("TOOL_tail", "yes"),
        ("TOOL_sha256sum", "yes"),
        ("TOOL_mv", "yes"),
        ("TOOL_uname", "yes"),
        ("TOOL_mktemp", "yes"),
        ("ROOT_WRITABLE", "yes"),
        ("VDB_DIR", "yes"),
        ("WORKDIR", "writable"),
    ] {
        ok.insert(k.to_string(), v.to_string());
    }
    assert_eq!(preflight_gates(&ok), (Vec::new(), Vec::new()));

    let mut old = ok.clone();
    old.insert("BASH_MINOR".to_string(), "2".to_string());
    let (failures, _) = preflight_gates(&old);
    assert!(failures.iter().any(|f| f.contains("5.3")), "{failures:?}");

    let mut missing = ok.clone();
    missing.insert("TOOL_tar".to_string(), "no".to_string());
    missing.insert("TOOL_tail".to_string(), "no".to_string());
    missing.insert("TOOL_sha256sum".to_string(), "no".to_string());
    missing.insert("ROOT_WRITABLE".to_string(), "no".to_string());
    missing.insert("WORKDIR".to_string(), "uncreatable".to_string());
    let (failures, _) = preflight_gates(&missing);
    assert_eq!(failures.len(), 5, "{failures:?}");

    // A missing vdb warns, never fails.
    let mut novdb = ok.clone();
    novdb.insert("VDB_DIR".to_string(), "no".to_string());
    let (failures, warnings) = preflight_gates(&novdb);
    assert!(failures.is_empty());
    assert_eq!(warnings.len(), 1);
}

#[test]
fn preflight_script_reports_candidates_dirs_and_uname() {
    // Render level with temp dirs: the script's candidate digests,
    // writability and mode facts must match `sha256sum` / `stat` run
    // directly.
    let tmp = TempDir::new("portuale-remote-binscript").keep();
    let dir = tmp.join("bin");
    std::fs::create_dir_all(&dir).unwrap();
    let present = dir.join("portuale");
    std::fs::write(&present, b"client binary bytes").unwrap();
    let missing = dir.join("portuale-0123456789abcdef");
    assert!(!missing.exists());
    let dirs = vec![dir.to_string_lossy().into_owned()];
    let candidates = vec![
        present.to_string_lossy().into_owned(),
        missing.to_string_lossy().into_owned(),
    ];
    let script = preflight_script(
        tmp.join("root").to_str().unwrap(),
        tmp.join("work").to_str().unwrap(),
        Some((&dirs, &candidates)),
    );
    assert!(script.contains("sha256sum"), "tool floor:\n{script}");
    assert!(script.contains("UNAME_M"), "uname fact:\n{script}");
    let output = std::process::Command::new("bash")
        .args(["-s"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write as _;
            child.stdin.take().unwrap().write_all(script.as_bytes())?;
            child.wait_with_output()
        })
        .expect("local bash runs the preflight script");
    assert!(output.status.success());
    let values = parse_kv(&String::from_utf8_lossy(&output.stdout));
    use sha2::Digest as _;
    let expected = portage_util::hex_lower(&sha2::Sha256::digest(b"client binary bytes"));
    assert_eq!(
        values.get("CAND_0_SHA").map(String::as_str),
        Some(expected.as_str())
    );
    assert_eq!(
        values.get("CAND_1_SHA").map(String::as_str),
        Some("missing")
    );
    assert!(
        values.get("UNAME_M").is_some_and(|m| !m.is_empty()),
        "{values:?}"
    );
    assert_eq!(values.get("DIR_0_EXISTS").map(String::as_str), Some("yes"));
    assert_eq!(
        values.get("DIR_0_WRITABLE").map(String::as_str),
        Some("yes")
    );
    let mode = values.get("DIR_0_MODE").map(String::as_str).unwrap_or("?");
    assert!(
        u32::from_str_radix(mode.trim(), 8).is_ok(),
        "DIR_0_MODE must be octal: {values:?}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// The unpack driver rejects a truncated stream before touching
/// anything: byte-count gate first, tar never runs on short input.
#[test]
fn unpack_script_rejects_a_truncated_bundle() {
    let tmp = TempDir::new("portuale-remote-trunc").keep();
    let staging = tmp.join("staging");
    std::fs::create_dir_all(&staging).unwrap();
    let staged = crate::remote_bundle::build_bundle(crate::remote_bundle::BuildBundleArgs {
        binpkg_path: &fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2"),
        staging_tmp: &staging,
        repo_override: None,
        gpg: &crate::binpkg::GpgVerify::default(),
        install_mask: "",
        install_mask_prunes_usr_share: false,
    })
    .expect("fixture tbz2 stages");
    // Simulate the streamed file, truncated to half its bytes.
    let unit = tmp.join("work/packagepkg-1.0");
    std::fs::create_dir_all(&unit).unwrap();
    let bytes = std::fs::read(&staged.tarball).unwrap();
    std::fs::write(unit.join("bundle.tar"), &bytes[..bytes.len() / 2]).unwrap();

    let script = unpack_script(
        tmp.join("work").to_str().unwrap(),
        "packagepkg-1.0",
        staged.byte_count,
    );
    let output = std::process::Command::new("bash")
        .args(["-s"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write as _;
            child.stdin.take().unwrap().write_all(script.as_bytes())?;
            child.wait_with_output()
        })
        .expect("local bash runs the driver");
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        stdout.contains("UNPACK=byte-count-mismatch"),
        "unexpected driver output:\n{stdout}"
    );
    // Nothing unpacked: the gate runs before tar.
    assert!(!unit.join("image").exists());
    let _ = std::fs::remove_dir_all(&tmp);
}

/// End to end through the merge driver (expected value from Portage): a
/// bundle built with a non-empty mask merges without the masked files
/// (not on the root, not in CONTENTS), while the merge-time aux files
/// land in the vdb entry.
#[test]
fn remote_merge_masks_image_and_lands_aux_files_in_vdb() {
    use md5::Digest as _;

    let tmp = TempDir::new("portuale-remote-mask").keep();
    let binpkg = fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2");
    let staging = tmp.join("staging");
    std::fs::create_dir_all(&staging).unwrap();
    let staged = crate::remote_bundle::build_bundle(crate::remote_bundle::BuildBundleArgs {
        binpkg_path: &binpkg,
        staging_tmp: &staging,
        repo_override: None,
        gpg: &crate::binpkg::GpgVerify::default(),
        install_mask: "/usr/share/packagepkg",
        install_mask_prunes_usr_share: false,
    })
    .expect("fixture tbz2 stages");
    // Deliver the bundle the way the stream would, then run the unpack
    // driver.
    let work = tmp.join("work");
    let unit = work.join("packagepkg-1.0");
    std::fs::create_dir_all(&unit).unwrap();
    std::fs::copy(&staged.tarball, unit.join("bundle.tar")).unwrap();
    let script = unpack_script(work.to_str().unwrap(), "packagepkg-1.0", staged.byte_count);
    let output = std::process::Command::new("bash")
        .args(["-s"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write as _;
            child.stdin.take().unwrap().write_all(script.as_bytes())?;
            child.wait_with_output()
        })
        .expect("local bash runs the driver");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success() && stdout.contains("UNPACK=ok"),
        "unpack driver failed:\n{stdout}"
    );
    // Run the merge driver against a scratch root.
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
    let ctx = local_ctx(root.to_str().unwrap(), work.to_str().unwrap());
    let markers =
        run_merge_stage(&ctx, None, unit.to_str().unwrap(), &staged, None).expect("merge succeeds");
    assert!(markers.iter().any(|m| m == "MERGE_COPY=ok"), "{markers:?}");
    assert!(markers.iter().any(|m| m == "MERGE_VDB=ok"), "{markers:?}");
    // The masked file merged nowhere: not on the root, not in CONTENTS.
    assert!(
        !root.join("usr/share/packagepkg/hello.txt").exists(),
        "masked file must not reach the merged root"
    );
    let vdb = root.join("var/db/pkg/dev-libs/packagepkg-1.0");
    let contents = std::fs::read_to_string(vdb.join("CONTENTS")).unwrap();
    assert!(
        !contents.contains("hello.txt"),
        "masked file must not reach CONTENTS:\n{contents}"
    );
    // The merge-time aux files land in the vdb entry verbatim.
    let bytes = std::fs::read(&binpkg).unwrap();
    assert_eq!(
        std::fs::read_to_string(vdb.join("BINPKGMD5")).unwrap(),
        format!("{}\n", portage_util::hex_lower(&md5::Md5::digest(&bytes))),
    );
    assert_eq!(
        std::fs::read_to_string(vdb.join("INSTALL_MASK")).unwrap(),
        "/usr/share/packagepkg\n",
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// End to end through both shell fragments: a staged unit with modes
/// 4711/2755/1750 plus a 2750 directory must keep those modes on the
/// merged root after unpack + merge.
#[test]
fn unpack_then_merge_preserves_special_mode_bits() {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    let tmp = TempDir::new("portuale-remote-setuid").keep();
    let pf = "probe-1.0";
    // Stage the unit the way `build_bundle` lays it out (`<pf>/` with
    // image/, build-info/, remote-manifest, filemeta).
    let staging = tmp.join("staging").join(pf);
    let image = staging.join("image");
    for (rel, mode) in [
        ("usr/bin/pt-setuid", 0o4711u32),
        ("usr/bin/pt-setgid", 0o2755u32),
        ("usr/bin/pt-sticky", 0o1750u32),
    ] {
        let path = image.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"probe payload\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    }
    let sgiddir = image.join("usr/lib/pt-sgiddir");
    std::fs::create_dir_all(&sgiddir).unwrap();
    std::fs::set_permissions(&sgiddir, std::fs::Permissions::from_mode(0o2750)).unwrap();
    let build_info = staging.join("build-info");
    std::fs::create_dir_all(&build_info).unwrap();
    for (name, content) in [
        ("PF", "probe-1.0"),
        ("CATEGORY", "dev-libs"),
        ("SLOT", "0"),
        ("DEFINED_PHASES", "-"),
    ] {
        std::fs::write(build_info.join(name), content).unwrap();
    }
    // The unpack driver only gates `FORMAT=` in the manifest.
    std::fs::write(staging.join("remote-manifest"), "FORMAT=1\n").unwrap();
    let entries = crate::remote_bundle::collect_filemeta(&image).unwrap();
    let filemeta = crate::remote_bundle::render_filemeta(&entries).unwrap();
    std::fs::write(staging.join("filemeta"), &filemeta).unwrap();
    // Tar `<pf>/` like `build_bundle` (`tar -cf`, `-C` the staging dir),
    // then deliver it as the streamed bundle.
    let tarball = tmp.join("bundle.tar");
    let status = std::process::Command::new("tar")
        .args(["-cf"])
        .arg(&tarball)
        .args(["-C"])
        .arg(tmp.join("staging"))
        .arg(pf)
        .status()
        .expect("tar -cf stages the bundle");
    assert!(status.success());
    let byte_count = std::fs::metadata(&tarball).unwrap().len();
    let work = tmp.join("work");
    let unit = work.join(pf);
    std::fs::create_dir_all(&unit).unwrap();
    std::fs::copy(&tarball, unit.join("bundle.tar")).unwrap();

    let script = unpack_script(work.to_str().unwrap(), pf, byte_count);
    let output = std::process::Command::new("bash")
        .args(["-s"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write as _;
            child.stdin.take().unwrap().write_all(script.as_bytes())?;
            child.wait_with_output()
        })
        .expect("local bash runs the driver");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success() && stdout.contains("UNPACK=ok"),
        "unpack driver failed:\n{stdout}"
    );

    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
    let staged = crate::remote_bundle::StagedBundle {
        tarball: tmp.join("bundle.tar"),
        byte_count: 0,
        manifest: crate::remote_bundle::BundleManifest {
            format: 1,
            cpv: "dev-libs/probe-1.0".to_string(),
            slot: "0".to_string(),
            repo: "test".to_string(),
            has_environment: false,
        },
        eapi: "8".to_string(),
        category: "dev-libs".to_string(),
        pn: "probe".to_string(),
        pv: "1.0".to_string(),
        pr: "r0".to_string(),
        pvr: "1.0".to_string(),
        p: "probe-1.0".to_string(),
        pf: "probe-1.0".to_string(),
        phases: Vec::new(),
        postinst_defined: false,
    };
    let ctx = local_ctx(root.to_str().unwrap(), work.to_str().unwrap());
    let markers =
        run_merge_stage(&ctx, None, unit.to_str().unwrap(), &staged, None).expect("merge succeeds");
    assert!(markers.iter().any(|m| m == "MERGE_COPY=ok"), "{markers:?}");

    for (rel, mode) in [
        ("usr/bin/pt-setuid", 0o4711u32),
        ("usr/bin/pt-setgid", 0o2755u32),
        ("usr/bin/pt-sticky", 0o1750u32),
        ("usr/lib/pt-sgiddir", 0o2750u32),
    ] {
        let got = std::fs::metadata(root.join(rel)).unwrap().mode() & 0o7777;
        assert_eq!(
            got, mode,
            "{rel}: special mode bits must survive unpack + merge"
        );
    }
    let _ = std::fs::remove_dir_all(&tmp);
}
