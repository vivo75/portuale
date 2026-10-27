//! Only allowlisted calls may relinquish a scratch guard.
//!
//! Every other `keep` on a temp guard leaks a directory under the temp
//! root, because the guard's cleanup runs on drop. Test helpers return the
//! guard and each test binds it instead.

use std::path::{Path, PathBuf};

/// (crate-relative file, enclosing function) pairs allowed to keep:
/// the guard's own self-test, two `tempfile` staging keeps removed after
/// use, and four remote-path keeps cleaned up explicitly.
const ALLOWLIST: &[(&str, &str)] = &[
    (
        "portage-util/src/temp_dir.rs",
        "temp_dir_keep_relinquishes_cleanup",
    ),
    (
        "portuale/src/emerge_build/pretend_pass.rs",
        "stage_pretend_capture",
    ),
    (
        "portuale/src/emerge_build/pretend_pass.rs",
        "compose_pretend_log",
    ),
    ("portuale/src/remote/binpkg_flow.rs", "load_vdb_shadow"),
    ("portuale/src/remote/binpkg_flow.rs", "run_binpkg_flow"),
    ("portuale/src/remote/config_place.rs", "place_config_root"),
    ("portuale/src/remote/stages.rs", "install_file_atomic"),
];

/// The `keep()` call split apart so this file's own source never matches
/// the scan.
fn keep_needle() -> String {
    [".keep", "()"].concat()
}

fn workspace_crates() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("crates")
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).expect("read crates dir");
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Line indexes starting a `fn` item, in order.
fn fn_defs(lines: &[&str]) -> Vec<(usize, String)> {
    let mut defs = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let mut rest = line.trim_start();
        if rest.starts_with("pub") {
            rest = rest["pub".len()..].trim_start();
            if rest.starts_with('(') {
                let Some(end) = rest.find(')') else { continue };
                rest = rest[end + 1..].trim_start();
            }
        }
        for prefix in ["unsafe ", "async ", "extern "] {
            if let Some(stripped) = rest.strip_prefix(prefix) {
                rest = stripped;
            }
        }
        if let Some(name) = rest.strip_prefix("fn ") {
            let name: String = name
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                defs.push((idx, name));
            }
        }
    }
    defs
}

#[test]
fn only_allowlisted_calls_may_keep_a_scratch_guard() {
    let root = workspace_crates();
    let needle = keep_needle();
    let mut files = Vec::new();
    collect_rs(&root, &mut files);
    assert!(!files.is_empty(), "no rust sources found under {root:?}");
    let mut offenders = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).expect("read rust source");
        let lines: Vec<&str> = text.lines().collect();
        let defs = fn_defs(&lines);
        for (idx, line) in lines.iter().enumerate() {
            if !line.contains(&needle) {
                continue;
            }
            let enclosing = defs
                .iter()
                .rev()
                .find(|(def_idx, _)| *def_idx < idx)
                .map(|(_, name)| name.clone())
                .unwrap_or_default();
            let rel = file
                .strip_prefix(&root)
                .expect("source under crates")
                .to_string_lossy()
                .into_owned();
            if !ALLOWLIST.contains(&(rel.as_str(), enclosing.as_str())) {
                offenders.push(format!("{rel}:{enclosing}: {}", line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "non-allowlisted keep calls (return the guard and bind it instead):\n{}",
        offenders.join("\n")
    );
}
