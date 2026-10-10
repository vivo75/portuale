use super::*;

/// #326 S8.3: every generated script kind exports the resolved
/// absolute `PORTUALE_BIN` (and `PORTAGE_PYM_PATH=/`, unsetting
/// `PORTAGE_IPC_DAEMON`), resolves `PORTAGE_PYTHON` through the
/// bundle's own `$UNITBIN/portuale-python`, and invokes no
/// `portuale` command by bare name.
#[test]
fn generated_scripts_use_absolute_portuale_bin() {
    const ABS: &str = "/opt/bin/portuale-0123456789abcdef";
    let staged = render_test_staged();
    let phase = phase_script(PhaseScriptArgs {
        unit_dir: "/work/probe-1.0",
        staged: &staged,
        phase: "setup",
        root: "/",
        workdir_parent: "/work",
        colormap: "",
        portuale_bin: ABS,
    });
    let regen = postinst_regen_script(PostinstRegenScriptArgs {
        unit_dir: "/work/probe-1.0",
        staged: &staged,
        root: "/",
        workdir_parent: "/work",
        colormap: "",
        features: None,
        locale: &[],
        portuale_bin: ABS,
    });
    let merge = merge_script(MergeScriptArgs {
        unit_dir: "/work/probe-1.0",
        staged: &staged,
        root: "/",
        vdb: "/var/db/pkg",
        stateless: false,
        protect_list: "/etc",
        mask_list: "/etc/env.d",
        ledger: None,
        portuale_bin: ABS,
    })
    .expect("merge script renders");
    let helpers_flow = format!("{MERGE_HELPERS}\n{MERGE_FLOW}");
    let unpack = unpack_script("/work", "probe-1.0", 1234);
    let preflight = preflight_script("/", "/work", None);
    // The three rendered kinds export the absolute path (the old
    // hooks inherit the merge header's export at runtime); the
    // read-only unpack/preflight drivers invoke no binary at all.
    for (name, script) in [
        ("phase", phase.as_str()),
        ("regen", regen.as_str()),
        ("merge", merge.as_str()),
    ] {
        assert!(
            script.contains(&format!("export PORTUALE_BIN='{ABS}'")),
            "{name} must export the absolute PORTUALE_BIN:\n{script}"
        );
    }
    for (name, script) in [
        ("phase", phase.as_str()),
        ("regen", regen.as_str()),
        ("merge", merge.as_str()),
        ("old-hook", helpers_flow.as_str()),
    ] {
        assert!(
            script.contains("export PORTAGE_PYM_PATH=/"),
            "{name} must pin PORTAGE_PYM_PATH=/:\n{script}"
        );
        assert!(
            script.contains("unset PORTAGE_IPC_DAEMON"),
            "{name} must drop PORTAGE_IPC_DAEMON (no IPC daemon):\n{script}"
        );
        assert!(
            !script.contains("PORTAGE_IPC_DAEMON="),
            "{name} must not export PORTAGE_IPC_DAEMON:\n{script}"
        );
        assert!(
            script.contains("PORTAGE_PYTHON=\"$UNITBIN/portuale-python\""),
            "{name} must resolve PORTAGE_PYTHON through $UNITBIN:\n{script}"
        );
        assert!(
            !script.contains("/usr/bin/python"),
            "{name} must not point at a system python:\n{script}"
        );
    }
    assert!(
        !merge.contains(":-portuale"),
        "portageq-wrapper's PATH fallback must be gone:\n{merge}"
    );
    for (name, script) in [
        ("phase", phase.as_str()),
        ("regen", regen.as_str()),
        ("merge", merge.as_str()),
        ("old-hook", helpers_flow.as_str()),
        ("unpack", unpack.as_str()),
        ("preflight", preflight.as_str()),
    ] {
        assert_eq!(
            bare_portuale_hits(script),
            Vec::<String>::new(),
            "{name} invokes portuale by bare name"
        );
    }
}

/// Backlog #172: `phase_exports` carries `MERGE_TYPE=binary` exactly
/// when real's `eapi_exports_merge_type` holds.
#[test]
fn phase_exports_gates_merge_type_on_eapi() {
    // Real `_emerge/Binpkg.py:92` sets `MERGE_TYPE=binary` for a binpkg
    // merge, and real `config.environ()` (`config.py:3335-3336`) exports
    // it only when `eapi_exports_merge_type` holds (EAPI >= 4,
    // `eapi.py:300`). Bed `l31-20260926T061556Z` shows real appending
    // `merge_type=binary` x4 (322 B) where mrg wrote `?` x4 (302 B).
    // `mrg` only merges binpkgs, so `binary` is the only value.
    fn exports_for(eapi: &str) -> String {
        let staged = crate::remote_bundle::StagedBundle {
            tarball: std::path::PathBuf::from("/tmp/bundle.tar"),
            byte_count: 0,
            manifest: crate::remote_bundle::BundleManifest {
                format: 1,
                cpv: "dev-libs/probe-1.0".to_string(),
                slot: "0".to_string(),
                repo: "test".to_string(),
                has_environment: false,
            },
            eapi: eapi.to_string(),
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
        phase_exports(PhaseExportsArgs {
            ebuild: "/tmp/work/build-info/probe-1.0.ebuild",
            build_dir: "/tmp/work",
            root: "/",
            image_dir: "/tmp/work/image",
            temp_dir: "/tmp/work/temp",
            work_subdir: "/tmp/work/work",
            home_dir: "/tmp/work/homedir",
            files_dir: "/tmp/work/files",
            bin_dir: "/tmp/work/bin",
            tmpdir: "/tmp",
            colormap: "",
            staged: &staged,
            phase: "setup",
            portuale_bin: "/opt/bin/portuale-0123456789abcdef",
        })
    }
    assert!(eapi_exports_merge_type("4"));
    assert!(eapi_exports_merge_type("8"));
    assert!(!eapi_exports_merge_type("0"));
    assert!(!eapi_exports_merge_type("3"));
    assert!(!eapi_exports_merge_type("2"));
    assert!(eapi_exports_merge_type("5"));
    assert!(eapi_exports_merge_type("9"));
    assert!(eapi_exports_merge_type("3-foo"));
    assert!(eapi_exports_merge_type("bogus"));
    let exporting = exports_for("8");
    assert!(
        exporting.contains("export EBUILD_PHASE=setup EMERGE_FROM=binary MERGE_TYPE=binary\n"),
        "EAPI 8 must export MERGE_TYPE=binary:\n{exporting}"
    );
    let gated = exports_for("3");
    assert!(
        !gated.contains("MERGE_TYPE"),
        "EAPI 3 must not export MERGE_TYPE:\n{gated}"
    );
    assert!(
        gated.contains("export EBUILD_PHASE=setup EMERGE_FROM=binary\n"),
        "EAPI 3 keeps the bare EMERGE_FROM line:\n{gated}"
    );
}

/// Positive pretend dispatch through the real stack: a synthetic unit
/// (hand-written ebuild + environment carrying `pkg_pretend`, the real
/// shipped `bin/`) runs the generated phase script under local bash.
/// Proves template + `ebuild.sh` + DEFINED_PHASES-agnostic dispatch;
/// the ebuild/env being synthetic is the only unreal part.
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
    // The real runtime, as shipped in bundles.
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
        // which execs `$PORTUALE_BIN`: a real binary, never the
        // libtest harness (#326 D8).
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

/// Backlog #171b task 3: the server's set locale variables reach
/// the client phase env, and only those -- a fake getter pins the
/// selection (`LANG` set + empty-but-set `LANGUAGE` forwarded in
/// `REGEN_LOCALE_VARS` order; unset `LC_ALL` skipped; a
/// non-locale variable never consulted).
#[test]
fn collect_server_locale_forwards_only_set_vars() {
    use std::collections::HashMap;
    let env: HashMap<&str, &str> = [("LANG", "C.UTF-8"), ("LANGUAGE", ""), ("TERM", "xterm")]
        .into_iter()
        .collect();
    let got = collect_server_locale(|name| env.get(name).map(|v| v.to_string()));
    let names: Vec<&str> = got.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["LANG", "LANGUAGE"]);
    assert_eq!(got[0].1, "C.UTF-8");
    assert_eq!(got[1].1, "");
}

/// Backlog #171b tasks 2-3, render level: the regen script unsets
/// `O`, unsets `SHELL`, unsets `LC_ALL` (EAPI 8 is posixish), and
/// re-exports exactly the given locale pairs (quoting values like
/// any other export).
#[test]
fn postinst_regen_script_matches_portage_shell_shape() {
    let staged = render_test_staged();
    let locale = vec![
        ("LANG".to_string(), "C.UTF-8".to_string()),
        ("LC_NUMERIC".to_string(), "a b".to_string()),
    ];
    let script = postinst_regen_script(PostinstRegenScriptArgs {
        unit_dir: "/work/regen-1.0",
        staged: &staged,
        root: "/",
        workdir_parent: "/work",
        colormap: "never",
        features: Some("sandbox merge-time"),
        locale: &locale,
        portuale_bin: "/opt/bin/portuale-0123456789abcdef",
    });
    assert!(
        script.contains("unset O\n"),
        "O must go (real environ_filter):\n{script}"
    );
    assert!(
        script.contains("unset SHELL\n"),
        "SHELL must be unset so bash self-inits it (real declare --):\n{script}"
    );
    assert!(
        script.contains("unset LC_ALL\n"),
        "LC_ALL must go on posixish EAPIs (real split_LC_ALL):\n{script}"
    );
    assert!(
        script.contains("export LANG='C.UTF-8'\n"),
        "server LANG missing:\n{script}"
    );
    assert!(
        script.contains("export LC_NUMERIC='a b'\n"),
        "server LC_* missing or misquoted:\n{script}"
    );
    assert!(
        !script.contains("LANGUAGE"),
        "uninjected locale must not appear:\n{script}"
    );
}

/// Backlog #169, site (a): `chown(2)` on a regular file clears
/// `S_ISUID`/`S_ISGID` even when owner and group are unchanged, so
/// the merge copy driver must apply ownership *before* restoring
/// the mode -- real `lib/portage/util/movefile.py::_apply_stat`
/// does `os.chown` then `os.chmod` for exactly this reason. That
/// needs root to observe behaviourally (no sudo on this host), so
/// this pins the order structurally in `MERGE_FLOW` instead: the
/// `chown --reference` line must precede the `chmod --reference`
/// line in both the `obj` (`$tmp`) and `dir` (`$dest`) branches.
/// Fails while chmod runs first.
#[test]
fn merge_flow_applies_chown_before_chmod() {
    for (what, chown, chmod) in [
        (
            "obj",
            "chown --reference \"$src\" \"$tmp\"",
            "chmod --reference \"$src\" \"$tmp\"",
        ),
        (
            "dir",
            "chown --reference \"$src\" \"$dest\"",
            "chmod --reference \"$src\" \"$dest\"",
        ),
    ] {
        for pat in [chown, chmod] {
            assert_eq!(
                MERGE_FLOW.matches(pat).count(),
                1,
                "{what} branch: `{pat}` must occur exactly once for the order check"
            );
        }
        let chown_pos = MERGE_FLOW
            .find(chown)
            .unwrap_or_else(|| panic!("{what} branch lost its chown --reference line"));
        let chmod_pos = MERGE_FLOW
            .find(chmod)
            .unwrap_or_else(|| panic!("{what} branch lost its chmod --reference line"));
        assert!(
            chown_pos < chmod_pos,
            "{what} branch must chown before chmod (chown(2) clears setuid/setgid)"
        );
    }
}
