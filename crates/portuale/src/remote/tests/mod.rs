use super::*;

use portage_util::TempDir;

// --- Client-binary gates: digest, architecture, preflight facts ------------------

/// Fake (but well-formed) full SHA-256 hex for gate tests: any 64-hex
/// string decides the same way a genuine digest would.
const S8_DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

const S8_SHORT: &str = "0123456789abcdef";

const S8_WRONG: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

fn s8_pre(dirs: &[&str], machine: u16) -> BinPreflight {
    let dirs: Vec<String> = dirs.iter().map(|s| s.to_string()).collect();
    BinPreflight {
        candidates: bin_candidates(&dirs, S8_SHORT),
        digest: S8_DIGEST.to_string(),
        bin_path: "/server/portuale".to_string(),
        machine,
        dirs,
    }
}

/// Parsed preflight values for the binary gates: `cands` are the
/// `CAND_<i>_SHA` facts in D5 order, `dir_states` the
/// `(EXISTS, WRITABLE, MODE)` triple per directory.
fn s8_values(
    cands: &[&str],
    dir_states: &[(&str, &str, &str)],
    uname: &str,
) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for (i, sha) in cands.iter().enumerate() {
        map.insert(format!("CAND_{i}_SHA"), sha.to_string());
    }
    for (j, (exists, writable, mode)) in dir_states.iter().enumerate() {
        map.insert(format!("DIR_{j}_EXISTS"), exists.to_string());
        map.insert(format!("DIR_{j}_WRITABLE"), writable.to_string());
        map.insert(format!("DIR_{j}_MODE"), mode.to_string());
    }
    map.insert("UNAME_M".to_string(), uname.to_string());
    map
}

/// `portuale` must never be invoked by bare name in generated client
/// scripts -- only through `$PORTUALE_BIN`, an absolute path, or a
/// non-command token. Returns the offending lines.
fn bare_portuale_hits(script: &str) -> Vec<String> {
    let mut scrubbed = script.to_string();
    for token in [
        "portuale-python",
        "portuale-remote",
        ".portuale.",
        "portuale-helper-ping-1",
        "(portuale #",
    ] {
        scrubbed = scrubbed.replace(token, "########");
    }
    // `portuale-<hex>` install names.
    while let Some(start) = scrubbed.find("portuale-") {
        let rest = &scrubbed[start + "portuale-".len()..];
        let hex_len = rest.chars().take_while(|c| c.is_ascii_hexdigit()).count();
        if hex_len == 0 {
            break;
        }
        scrubbed.replace_range(start..start + "portuale-".len() + hex_len, "########");
    }
    scrubbed
        .lines()
        .filter(|line| line.contains("portuale"))
        .map(str::to_string)
        .collect()
}

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
        build_time: None,
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

/// Serialises the tests that pin `PORTAGE_CONFIGROOT`: it is
/// process-global, and `place_config_root` reads it (the server
/// `make.globals` lookup) while `PlacedConfig` overwrites it, so
/// two such tests racing would seed from each other's server root. The
/// `FEATURES`-mutating tests elsewhere (`pretend::tests::unmerge`) share it.
pub(crate) static PLACED_CONFIG_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

/// Synthetic server root: `etc/portage/repos.conf` + the server's own
/// `make.globals`.
fn l171e_server_root(tmp: &std::path::Path, repos_conf: &str) -> std::path::PathBuf {
    let server_root = tmp.join("serverroot");
    std::fs::create_dir_all(server_root.join("etc/portage/repos.conf")).unwrap();
    std::fs::create_dir_all(server_root.join("usr/share/portage/config")).unwrap();
    std::fs::write(
        server_root.join("etc/portage/repos.conf/l171e.conf"),
        repos_conf,
    )
    .unwrap();
    std::fs::write(
        server_root.join("usr/share/portage/config/make.globals"),
        "FEATURES=\"sandbox news sign\"\n",
    )
    .unwrap();
    server_root
}

/// Add a server repo with one profile `<location>/profiles/<rel>`
/// carrying `make.defaults` `FEATURES="<features>"` (plus a
/// `repo_name` file so `find_repos` keeps the section name).
fn l171e_server_repo(
    server_root: &std::path::Path,
    name: &str,
    rel: &str,
    features: &str,
) -> std::path::PathBuf {
    let location = server_root.join("repos").join(name);
    let profile = location.join("profiles").join(rel);
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(
        profile.join("make.defaults"),
        format!("FEATURES=\"{features}\"\n"),
    )
    .unwrap();
    std::fs::write(location.join("profiles/repo_name"), format!("{name}\n")).unwrap();
    location
}

/// Pin `PORTAGE_CONFIGROOT` at a synthetic server root with ambient
/// `FEATURES`/`INSTALL_MASK` cleared (all process-global; shares
/// `PLACED_CONFIG_ENV_LOCK`), restoring everything on drop while the
/// lock is still held.
struct ServerEnvPin {
    saved_config_root: Option<std::ffi::OsString>,
    saved_features: Option<std::ffi::OsString>,
    saved_mask: Option<std::ffi::OsString>,
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl ServerEnvPin {
    fn pin(server_root: &std::path::Path) -> Self {
        let guard = PLACED_CONFIG_ENV_LOCK.lock().unwrap();
        let saved_config_root = std::env::var_os("PORTAGE_CONFIGROOT");
        let saved_features = std::env::var_os("FEATURES");
        let saved_mask = std::env::var_os("INSTALL_MASK");
        // SAFETY: held `PLACED_CONFIG_ENV_LOCK`; no other test in
        // this binary pins these without it.
        unsafe {
            std::env::set_var("PORTAGE_CONFIGROOT", server_root);
            std::env::remove_var("FEATURES");
            std::env::remove_var("INSTALL_MASK");
        }
        Self {
            saved_config_root,
            saved_features,
            saved_mask,
            _guard: guard,
        }
    }
}

impl Drop for ServerEnvPin {
    fn drop(&mut self) {
        // SAFETY: same as above.
        unsafe {
            match self.saved_config_root.take() {
                Some(value) => std::env::set_var("PORTAGE_CONFIGROOT", value),
                None => std::env::remove_var("PORTAGE_CONFIGROOT"),
            }
            match self.saved_features.take() {
                Some(value) => std::env::set_var("FEATURES", value),
                None => std::env::remove_var("FEATURES"),
            }
            match self.saved_mask.take() {
                Some(value) => std::env::set_var("INSTALL_MASK", value),
                None => std::env::remove_var("INSTALL_MASK"),
            }
        }
    }
}

/// Resolve a placed client tree's config against explicit server repos
/// (kept for the remap tests; the production path uses the shared
/// `load_repos_and_config`).
fn l171e_resolve_with_server_repos(
    placed_dir: &std::path::Path,
    server_root: &std::path::Path,
    eroot: &std::path::Path,
) -> portage_profile::Config {
    let server_repos = portage_repo::find_repos(server_root).expect("server repos resolve");
    let main = server_repos
        .iter()
        .find(|repo| repo.is_main)
        .expect("a main repo");
    let overlays: Vec<(String, std::path::PathBuf)> = server_repos
        .iter()
        .filter(|repo| !repo.is_main)
        .map(|repo| (repo.name.clone(), repo.location.clone()))
        .collect();
    let aliases: Vec<(String, std::path::PathBuf)> = server_repos
        .iter()
        .flat_map(|repo| {
            repo.aliases
                .iter()
                .map(|alias| (alias.clone(), repo.location.clone()))
        })
        .collect();
    let masters: std::collections::HashMap<String, Vec<std::path::PathBuf>> = server_repos
        .iter()
        .map(|repo| (repo.name.clone(), repo.masters.clone()))
        .collect();
    portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: placed_dir,
        main_repo_location: &main.location,
        overlay_repos: &overlays,
        repo_aliases: &aliases,
        main_repo_name: &main.name,
        repo_masters: &masters,
        eroot,
    })
    .expect("placed client config resolves")
}

const L171E_REL: &str = "default/linux/amd64/23.0";

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

/// One end-to-end tree: a client root + workdir, a `--remote-portuale-dir`
/// bin dir, and a minimal `server:` config.
struct S8Tree {
    tmp: std::path::PathBuf,
    root: std::path::PathBuf,
    work: std::path::PathBuf,
    bindir: std::path::PathBuf,
    clientetc: std::path::PathBuf,
}

impl S8Tree {
    fn fresh(tag: &str) -> Self {
        let tmp = TempDir::new(&format!("portuale-remote-s8-{tag}")).keep();
        let root = tmp.join("root");
        let work = tmp.join("work");
        let bindir = tmp.join("bin");
        std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
        std::fs::create_dir_all(&work).unwrap();
        std::fs::create_dir_all(&bindir).unwrap();
        let clientetc = tmp.join("clientetc");
        let repo = tmp.join("repo");
        std::fs::create_dir_all(clientetc.join("etc/portage/repos.conf")).unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(
            clientetc.join("etc/portage/repos.conf/testrepo.conf"),
            format!(
                "[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = {}\n",
                repo.display()
            ),
        )
        .unwrap();
        std::fs::write(
            clientetc.join("etc/portage/make.conf"),
            "FEATURES=\"sandbox\"\n",
        )
        .unwrap();
        Self {
            tmp,
            root,
            work,
            bindir,
            clientetc,
        }
    }
}

impl Drop for S8Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.tmp);
    }
}

/// Run the child `portuale mrg` trial flow against the tree.
/// `extra` appends argv (e.g. `--pretend`).
fn s8_run(tree: &S8Tree, extra: &[&str]) -> std::process::Output {
    let binpkg = s8_fixtures().join("pkgdir/dev-libs/binpkgrmpkg-1.0.tbz2");
    assert!(
        binpkg.is_file(),
        "fixture binpkg missing: {}",
        binpkg.display()
    );
    let shipped = s8_portuale_bin();
    let etc = format!("server:{}", tree.clientetc.display());
    let mut args = vec![
        "mrg",
        "--getbinpkgonly",
        "--remote-hostname",
        "localtest",
        "--remote-transport",
        "local",
        "--remote-root",
        tree.root.to_str().unwrap(),
        "--remote-workdir",
        tree.work.to_str().unwrap(),
        "--remote-etc-portage",
        etc.as_str(),
        "--remote-portuale-dir",
        tree.bindir.to_str().unwrap(),
        "--remote-portuale-binary",
        shipped.to_str().unwrap(),
        "--remote-binpkg",
        binpkg.to_str().unwrap(),
    ];
    args.extend(extra.iter().copied());
    std::process::Command::new(s8_portuale_bin())
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .env("PORTAGE_CONFIGROOT", &tree.clientetc)
        .env("ROOT", &tree.root)
        .env("DISTDIR", s8_fixtures().join("distfiles"))
        .env("PORTAGE_TMPDIR", tree.tmp.join("pt"))
        .env("FEATURES", "sandbox")
        .output()
        .expect("portuale mrg spawns")
}

fn s8_stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

mod binpkg_flow;
mod client_binary;
mod config_place;
mod context;
mod host_keys;
mod ledger;
mod merge_script;
mod plan;
mod preflight;
mod ssh;
mod stages;

/// The resolved `FEATURES` hold every token of `tokens` plus the embedded
/// `make.globals` base (`preserve-libs` stands for it). The server's
/// `usr/share/portage/config/make.globals` is no longer read.
pub(crate) fn assert_features_include(
    config: &portage_profile::Config,
    tokens: &[&str],
    why: &str,
) {
    let features = crate::pretend::config_features_string(config);
    let have: Vec<&str> = features.split_whitespace().collect();
    for token in tokens.iter().chain(["preserve-libs"].iter()) {
        assert!(
            have.contains(token),
            "{why}: {token} missing from {features}"
        );
    }
}
