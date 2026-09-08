use super::*;

/// A synthetic unit (hand-written ebuild + environment carrying
/// `pkg_pretend`, the shipped `bin/`) runs the generated phase script
/// under local bash; the ebuild/env being synthetic is the only gap.
#[test]
fn phase_script_runs_pkg_pretend_from_saved_env() {
    let tmp = TempDir::new("portuale-remote-pretend").keep();
    let unit = tmp.join("work/probe-1.0");
    let build_info = unit.join("build-info");
    std::fs::create_dir_all(&build_info).unwrap();
    std::fs::write(
        build_info.join("probe-1.0.ebuild"),
        "EAPI=8\nDESCRIPTION=\"synthetic pretend probe\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\n",
    )
    .unwrap();
    std::fs::write(
        unit.join("environment"),
        "EAPI=8\npkg_pretend() {\n\techo pretend-ok >> \"${EROOT}/var/lib/probe.log\"\n}\n",
    )
    .unwrap();
    // The runtime, as shipped in bundles.
    let status = std::process::Command::new("cp")
        .args(["-a"])
        .arg(crate::ebuild_phases::bin_dir().unwrap())
        .arg(unit.join("bin"))
        .status()
        .expect("cp -a bin");
    assert!(status.success());
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("var/lib")).unwrap();

    let staged = crate::remote_bundle::StagedBundle {
        tarball: tmp.join("bundle.tar"),
        byte_count: 0,
        manifest: crate::remote_bundle::BundleManifest {
            format: 1,
            cpv: "dev-libs/probe-1.0".to_string(),
            slot: "0".to_string(),
            repo: "test".to_string(),
            has_environment: true,
        },
        eapi: "8".to_string(),
        category: "dev-libs".to_string(),
        pn: "probe".to_string(),
        pv: "1.0".to_string(),
        pr: "r0".to_string(),
        pvr: "1.0".to_string(),
        p: "probe-1.0".to_string(),
        pf: "probe-1.0".to_string(),
        phases: vec!["pretend".to_string()],
        postinst_defined: false,
    };
    let script = phase_script(PhaseScriptArgs {
        unit_dir: unit.to_str().unwrap(),
        staged: &staged,
        phase: "pretend",
        root: root.to_str().unwrap(),
        workdir_parent: tmp.to_str().unwrap(),
        colormap: "", // The phase may invoke the shipped `portuale-python` shim,
        // which execs `$PORTUALE_BIN`: a genuine binary, never the
        // libtest harness.
        portuale_bin: s8_portuale_bin().to_str().unwrap(),
    });
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
        .expect("local bash runs the phase script");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        stdout.contains("PHASE_pretend=0"),
        "phase marker missing:\n{stdout}"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("var/lib/probe.log")).unwrap(),
        "pretend-ok\n"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}
