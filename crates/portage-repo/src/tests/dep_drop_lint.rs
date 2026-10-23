//! Source lint: every dependency-token `portage_dep::parse_atom(` call in the
//! scanned resolver code must either be counted (`parse_dep_atom`, which records
//! unparseable tokens per site) or carry a `// drop-ok: <reason>` marker on the
//! same or the preceding line. Otherwise an unparseable token could be dropped
//! silently and the "unparsed dependency tokens" invariant would not see it.

use std::{fs, path::Path};

/// Directories and files (relative to `src/`) whose parse sites are checked.
const SCANNED: &[&str] = &[
    "resolve",
    "slot_operator",
    "merge_order",
    "solver_bridge.rs",
];

const NEEDLE: &str = "portage_dep::parse_atom(";
const MARKER: &str = "drop-ok:";

/// True when `line` carries a `drop-ok:` marker with a non-empty reason.
fn has_marker(line: &str) -> bool {
    line.split(MARKER)
        .nth(1)
        .is_some_and(|reason| !reason.trim().is_empty())
}

/// Scan one file, appending `path:line` for every unmarked parse site. Test code
/// (everything from the first `#[cfg(test)]` line on) is not checked.
fn scan_file(path: &Path, out: &mut Vec<String>) {
    let text = fs::read_to_string(path).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("#[cfg(test)]") {
            break;
        }
        if line.trim_start().starts_with("//") {
            continue;
        }
        // A call split as `portage_dep::` / `parse_atom(` on the next line.
        let split = line.trim_end().ends_with("portage_dep::")
            && lines
                .get(i + 1)
                .is_some_and(|next| next.trim_start().starts_with("parse_atom("));
        if !(line.contains(NEEDLE) || split) {
            continue;
        }
        let marked = has_marker(line) || (i > 0 && has_marker(lines[i - 1]));
        if !marked {
            out.push(format!("{}:{}", path.display(), i + 1));
        }
    }
}

/// Recurse into `path`: `tests/` directories and files whose name contains
/// `tests` are skipped; other `.rs` files are scanned.
fn scan_path(path: &Path, out: &mut Vec<String>) {
    if path.is_dir() {
        if path.file_name().is_some_and(|n| n == "tests") {
            return;
        }
        let mut entries: Vec<_> = fs::read_dir(path)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for entry in entries {
            scan_path(&entry, out);
        }
    } else if path.extension().is_some_and(|x| x == "rs")
        && !path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .contains("tests")
    {
        scan_file(path, out);
    }
}

#[test]
fn every_dep_atom_parse_is_counted_or_justified() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut bad = Vec::new();
    for rel in SCANNED {
        scan_path(&root.join(rel), &mut bad);
    }
    assert!(
        bad.is_empty(),
        "unmarked portage_dep::parse_atom sites (use parse_dep_atom or add `// drop-ok: <reason>`):\n{}",
        bad.join("\n")
    );
}
