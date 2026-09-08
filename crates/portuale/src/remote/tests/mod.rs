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
mod preflight;
mod ssh;
