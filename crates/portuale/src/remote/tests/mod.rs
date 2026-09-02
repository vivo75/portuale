use super::*;

use portage_util::TempDir;

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
mod ssh;
