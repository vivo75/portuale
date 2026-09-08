use super::*;

use portage_util::TempDir;

fn ctx_with_args(hostname: &str, ssh_args: Option<&str>) -> RemoteContext {
    RemoteContext {
        hostname: hostname.to_string(),
        user: None,
        port: 22222,
        key_file: None,
        timeout_secs: 10,
        ssh_args: ssh_args.map(String::from),
        strict_host_key_checking: StrictHostKeyChecking::AcceptNew,
        max_clock_skew_secs: 900,
        root: "/".to_string(),
        workdir: "/var/tmp/portage-remote".to_string(),
        transport: RemoteTransport::Ssh,
        binpkg: None,
        config_protect: "/etc".to_string(),
        config_protect_explicit: false,
        config_protect_mask: "/etc/env.d".to_string(),
        config_protect_mask_explicit: false,
        etc_portage: ConfigPlacement::Client("/etc/portage".to_string()),
        vdb: ConfigPlacement::Client("/var/db/pkg".to_string()),
        edb: ConfigPlacement::Server("/var/cache/edb".to_string()),
        ledger_dir: None,
        require_ledger_match: false,
        portuale_binary: None,
        portuale_dir: None,
        bin_plan: ClientBinPlan::Unresolved,
    }
}

fn binary_entry(package: &str, binary: bool) -> portage_repo::GraphEntry {
    use portage_repo::{CandidateSource, PretendOutcome, VisibilityProvenance};
    portage_repo::GraphEntry {
        discovery: 0,
        category: "dev-libs".to_string(),
        package: package.to_string(),
        outcome: PretendOutcome::New {
            version: "1.0".to_string(),
        },
        blockers: Vec::new(),
        slot: Some("0".to_string()),
        sub_slot: Some("0".to_string()),
        repo_name: Some("testrepo".to_string()),
        oldbest: Vec::new(),
        use_flags_display: Vec::new(),
        use_expand_display: Vec::new(),
        use_expand_display_p: Vec::new(),
        keyword_mask: None,
        new_slot: false,
        interactive: false,
        fetch_restrict: false,
        fetch_restrict_satisfied: false,
        download_files: Vec::new(),
        required_by: Vec::new(),
        source: if binary {
            CandidateSource::Binary
        } else {
            CandidateSource::Ebuild
        },
        provenance: VisibilityProvenance::default(),
        keyword_suggestion: None,
        use_suggestion: None,
        parent_use_suggestion: None,
        targets_running_root: false,
        remote_binary: false,
        build_id: None,
        deps: Vec::new(),
    }
}

fn fixture(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

/// Synthetic merge through the driver: protect rename and fail-closed
/// collision, no binpkg needed.
fn synthetic_unit(
    tmp: &std::path::Path,
    files: &[(&str, &str)],
) -> (String, crate::remote_bundle::StagedBundle) {
    let bytes: Vec<(&str, &[u8])> = files.iter().map(|(r, c)| (*r, c.as_bytes())).collect();
    synthetic_unit_bytes(tmp, &bytes)
}

/// [`synthetic_unit`] with binary payloads.
fn synthetic_unit_bytes(
    tmp: &std::path::Path,
    files: &[(&str, &[u8])],
) -> (String, crate::remote_bundle::StagedBundle) {
    let unit = tmp.join("work/probe-1.0");
    let image = unit.join("image");
    let build_info = unit.join("build-info");
    std::fs::create_dir_all(&build_info).unwrap();
    for (rel, content) in files {
        let path = image.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
    }
    for (name, content) in [
        ("PF", "probe-1.0"),
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
    (unit.to_str().unwrap().to_string(), staged)
}

fn local_ctx(root: &str, workdir: &str) -> RemoteContext {
    // Direct stage runs (no preflight) still need a resolved absolute
    // path in the fixture context: the built `portuale` binary.
    let abs = crate::self_exe::built_portuale_bin().display().to_string();
    RemoteContext {
        hostname: "localtest".to_string(),
        user: None,
        port: 22,
        key_file: None,
        timeout_secs: 10,
        ssh_args: None,
        strict_host_key_checking: StrictHostKeyChecking::AcceptNew,
        max_clock_skew_secs: 0,
        root: root.to_string(),
        workdir: workdir.to_string(),
        transport: RemoteTransport::Local,
        binpkg: None,
        config_protect: "/etc".to_string(),
        config_protect_explicit: false,
        config_protect_mask: "/etc/env.d".to_string(),
        config_protect_mask_explicit: false,
        etc_portage: ConfigPlacement::Client("/etc/portage".to_string()),
        vdb: ConfigPlacement::Client(format!("{root}/var/db/pkg")),
        edb: ConfigPlacement::Server(format!("{root}/var/cache/edb")),
        ledger_dir: None,
        require_ledger_match: false,
        portuale_binary: None,
        portuale_dir: None,
        bin_plan: ClientBinPlan::Local(abs),
    }
}

/// Build-time hook env mimicking `__save_ebuild_env` output, with a stray
/// build-host local and build-time `FEATURES` (with `buildpkg`).
fn build_time_environment(with_postinst: bool) -> String {
    let mut lines = vec![
        "declare -x EAPI=\"8\"",
        "declare -x CATEGORY=\"dev-libs\"",
        "declare -x PN=\"regen\"",
        "declare -x PV=\"1.0\"",
        "declare -x PR=\"r0\"",
        "declare -x PVR=\"1.0\"",
        "declare -x P=\"regen-1.0\"",
        "declare -x PF=\"regen-1.0\"",
        "declare -- x=\"\"",
        "declare -x FEATURES=\"buildpkg sandbox\"",
        "declare -x PORTAGE_FEATURES=\"buildpkg sandbox\"",
        "declare -x USE=\"amd64\"",
    ];
    if with_postinst {
        lines.push("pkg_postinst() {\n\texport PT_MERGE_MARKER=\"merge-time\"\n}");
    }
    lines.join("\n") + "\n"
}

/// A full client unit for the regen tests: image + build-info (with the
/// given ebuild text and `DEFINED_PHASES`) + build-time `$UNIT/environment`
/// + runtime `bin/` + `filemeta`.
fn regen_unit(
    tmp: &std::path::Path,
    ebuild: &str,
    defined_phases: &str,
) -> (String, crate::remote_bundle::StagedBundle) {
    let unit = tmp.join("work/regen-1.0");
    let image = unit.join("image");
    let build_info = unit.join("build-info");
    std::fs::create_dir_all(&build_info).unwrap();
    let path = image.join("usr/share/regen/payload.txt");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "payload\n").unwrap();
    for (name, content) in [
        ("PF", "regen-1.0"),
        ("CATEGORY", "dev-libs"),
        ("SLOT", "0"),
        ("DEFINED_PHASES", defined_phases),
    ] {
        std::fs::write(build_info.join(name), content).unwrap();
    }
    std::fs::write(build_info.join("regen-1.0.ebuild"), ebuild).unwrap();
    // The saved env carries the hooks the phase runs (an ebuild without
    // `pkg_postinst` yields an env without one).
    let with_postinst = ebuild.contains("pkg_postinst()");
    std::fs::write(
        unit.join("environment"),
        build_time_environment(with_postinst),
    )
    .unwrap();
    // Like a binpkg, `build-info/` carries the build-time
    // `environment.bz2` (the vdb fallback the regen later overwrites).
    {
        let compressed = bzip2_compress(build_time_environment(with_postinst).as_bytes())
            .expect("server bzip2 compresses");
        std::fs::write(build_info.join("environment.bz2"), &compressed).unwrap();
    }
    let status = std::process::Command::new("cp")
        .args(["-a"])
        .arg(crate::ebuild_phases::bin_dir().unwrap())
        .arg(unit.join("bin"))
        .status()
        .expect("cp -a bin");
    assert!(status.success());
    // Mirror `build_bundle`: the regen run's
    // `${PORTAGE_BZIP2_COMMAND}` stand-in rides `bin/`.
    {
        use std::os::unix::fs::PermissionsExt as _;
        let passthrough = unit.join("bin").join("bzip2-passthrough");
        std::fs::write(&passthrough, crate::remote_bundle::BZIP2_PASSTHROUGH).unwrap();
        std::fs::set_permissions(&passthrough, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let entries = crate::remote_bundle::collect_filemeta(&image).unwrap();
    let filemeta = crate::remote_bundle::render_filemeta(&entries).unwrap();
    std::fs::write(unit.join("filemeta"), &filemeta).unwrap();
    let staged = crate::remote_bundle::StagedBundle {
        tarball: tmp.join("bundle.tar"),
        byte_count: 0,
        manifest: crate::remote_bundle::BundleManifest {
            format: 1,
            cpv: "dev-libs/regen-1.0".to_string(),
            slot: "0".to_string(),
            repo: "test".to_string(),
            has_environment: true,
        },
        eapi: "8".to_string(),
        category: "dev-libs".to_string(),
        pn: "regen".to_string(),
        pv: "1.0".to_string(),
        pr: "r0".to_string(),
        pvr: "1.0".to_string(),
        p: "regen-1.0".to_string(),
        pf: "regen-1.0".to_string(),
        phases: Vec::new(),
        postinst_defined: defined_phases.split_whitespace().any(|w| w == "postinst"),
    };
    (unit.to_str().unwrap().to_string(), staged)
}

fn regen_tmp(tag: &str) -> std::path::PathBuf {
    TempDir::new(&format!("portuale-remote-regen-{tag}")).keep()
}

/// Decompress a vdb `environment.bz2` for content assertions.
fn read_vdb_env(vdb_env_bz2: &std::path::Path) -> String {
    let bytes = std::fs::read(vdb_env_bz2).unwrap();
    let output = std::process::Command::new("bzip2")
        .args(["-d", "-c", "--"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write as _;
            child.stdin.take().unwrap().write_all(&bytes)?;
            child.wait_with_output()
        })
        .expect("server bzip2 decompresses");
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap()
}

/// A minimal staged bundle for the render-level regen script test
/// (the script only needs the identity fields).
fn render_test_staged() -> crate::remote_bundle::StagedBundle {
    crate::remote_bundle::StagedBundle {
        tarball: std::path::PathBuf::from("/tmp/bundle.tar"),
        byte_count: 0,
        manifest: crate::remote_bundle::BundleManifest {
            format: 1,
            cpv: "dev-libs/regen-1.0".to_string(),
            slot: "0".to_string(),
            repo: "test".to_string(),
            has_environment: true,
        },
        eapi: "8".to_string(),
        category: "dev-libs".to_string(),
        pn: "regen".to_string(),
        pv: "1.0".to_string(),
        pr: "r0".to_string(),
        pvr: "1.0".to_string(),
        p: "regen-1.0".to_string(),
        pf: "regen-1.0".to_string(),
        phases: Vec::new(),
        postinst_defined: true,
    }
}

/// Saved env for an old instance: `declare -x` lines (the
/// `env_val` form `run_old_hook` greps) plus hook functions writing
/// to a client log.
fn old_hook_environment() -> String {
    [
        "declare -x EAPI=\"8\"",
        "declare -x CATEGORY=\"dev-libs\"",
        "declare -x PN=\"oldhook\"",
        "declare -x PV=\"1.0\"",
        "declare -x PR=\"r0\"",
        "declare -x PVR=\"1.0\"",
        "declare -x P=\"oldhook-1.0\"",
        "declare -x PF=\"oldhook-1.0\"",
        "pkg_prerm() {",
        "\techo prerm-ok >> \"${EROOT}/var/lib/oldhook.log\"",
        "}",
        "pkg_postrm() {",
        "\techo postrm-ok >> \"${EROOT}/var/lib/oldhook.log\"",
        "}",
    ]
    .join("\n")
        + "\n"
}

/// PATH farm: symlinks to every `/usr/bin` + `/bin` entry except
/// `bzip2`, so `command -v bzip2` fails exactly like a bzip2-less client.
fn farm_path_without_bzip2(dir: &std::path::Path) -> String {
    let farm = dir.join("farm");
    std::fs::create_dir_all(&farm).unwrap();
    for bindir in ["/usr/bin", "/bin"] {
        let Ok(entries) = std::fs::read_dir(bindir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == "bzip2" {
                continue;
            }
            let link = farm.join(&name);
            if !link.exists() {
                let _ = std::os::unix::fs::symlink(entry.path(), &link);
            }
        }
    }
    assert!(!farm.join("bzip2").exists());
    assert!(farm.join("bash").exists(), "the farm must resolve bash");
    farm.to_str().unwrap().to_string()
}

/// Drive `run_old_hook` straight through local bash under a PATH with no
/// `bzip2`. Returns stdout.
fn run_old_hook_snippet(
    unit: &str,
    root: &str,
    work: &str,
    farm_path: &str,
    oldvdb: &str,
) -> String {
    // The old hooks resolve `PORTAGE_PYTHON` through
    // `$UNITBIN/portuale-python`, whose shim execs `$PORTUALE_BIN` --
    // export the built binary directly (never the libtest harness).
    let exe = crate::self_exe::built_portuale_bin();
    let script = format!(
        concat!(
            "export PATH={farm}\n",
            "export PORTUALE_BIN={bin}\n",
            "command -v bzip2 >/dev/null 2>&1 && echo BZIP2-PRESENT || echo BZIP2-ABSENT\n",
            "UNIT={unit}\n",
            "ROOT={root}\n",
            "WORKDIR={work}\n",
            "UNITBIN=\"$UNIT/bin\"\n",
            "COLORMAP=''\n",
            "{helpers}\n",
            "run_old_hook {oldvdb} prerm\n",
            "echo \"RC=$?\"\n",
            "run_old_hook {oldvdb} postrm\n",
            "echo \"RC=$?\"\n",
        ),
        farm = sh_quote(farm_path),
        bin = sh_quote(exe.to_str().unwrap()),
        unit = sh_quote(unit),
        root = sh_quote(root),
        work = sh_quote(work),
        helpers = MERGE_HELPERS,
        oldvdb = sh_quote(oldvdb),
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
        .expect("local bash runs run_old_hook");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stdout.contains("BZIP2-ABSENT"),
        "bzip2 must be shadowed away:\n{stdout}\n{stderr}"
    );
    stdout
}

// --- Client-binary install e2e over local transport --------------------------------
//
// Local transport installs nothing by design -- except through
// `--remote-portuale-dir`, which names local paths the test owns, so
// the ssh-shaped install path (mktemp, stream, chmod, digest, mv,
// ping) runs without sshd.

/// The `portuale` binary next to the current test executable.
fn s8_portuale_bin() -> std::path::PathBuf {
    crate::self_exe::built_portuale_bin()
}

fn s8_fixtures() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

/// Full SHA-256 hex of a file.
fn s8_sha256(path: &std::path::Path) -> String {
    use sha2::Digest as _;
    format!(
        "{:x}",
        sha2::Sha256::digest(std::fs::read(path).expect("test file reads"))
    )
}
mod context;
mod host_keys;
mod merge_script;
mod preflight;
mod ssh;
mod stages;
