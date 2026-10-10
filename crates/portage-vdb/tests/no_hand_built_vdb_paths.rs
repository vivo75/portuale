//! Test that production code doesn't build installed-database paths by hand.
//!
//! The only correct way to get a VDB path is through the public API
//! (portage_vdb::for_root, FilesDb::open_vdb_dir, etc.), never by hand.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Check if a line matches any of the database path patterns
fn matches_pattern(line: &str) -> bool {
    line.contains("var/db/pkg")
        || (line.contains(r#""db")"#) && line.contains(r#"join("pkg")"#))
        || line.contains("VDB_DIR")
        || (line.contains("vdb_dir(") && !line.contains(".vdb_dir()"))
        || (line.contains("var/lib/portage/world") && !line.contains("world_sets"))
        || line.contains(r#""world")"#)
        || line.contains("var/lib/portage/world_sets")
        || line.contains(r#""world_sets")"#)
        || line.contains("preserved_libs_registry")
        || line.contains("var/lib/portage/config")
        || line.contains("cache/edb/counter")
}

/// Find spans of #[cfg(test)] annotations to exclude them
fn find_test_spans(lines: &[&str]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim_start().starts_with("#[cfg(test)]") {
            let start = i;
            let mut depth = 0;
            let mut seen = false;
            let mut j = i + 1;

            while j < lines.len() {
                // Strip comments and strings from the line
                let code = strip_comments_and_strings(lines[j]);

                for ch in code.chars() {
                    if ch == '{' {
                        depth += 1;
                        seen = true;
                    } else if ch == '}' {
                        depth -= 1;
                    }
                }

                if !seen && code.trim_end().ends_with(';') {
                    break;
                }
                if seen && depth <= 0 {
                    break;
                }
                j += 1;
            }
            spans.push((start + 1, j + 1)); // Convert to 1-indexed line numbers
            i = j + 1;
        } else {
            i += 1;
        }
    }
    spans
}

/// Strip comments and string/char literals from a line of Rust code, the
/// same rough way `docs/evidence/305-s0-inventory.py` does: cut at the
/// first `//`, then blank `"..."` strings and real char literals (`'x'`,
/// `'\\n'`). A lone `'` is a lifetime (`'a`, `'_`, `'static`) and is kept.
fn strip_comments_and_strings(line: &str) -> String {
    let code = match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    };
    let chars: Vec<char> = code.chars().collect();
    let mut result = String::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '"' => {
                result.push_str("\"\"");
                i += 1;
                while i < chars.len() {
                    if chars[i] == '\\' {
                        i += 2;
                        continue;
                    }
                    if chars[i] == '"' {
                        break;
                    }
                    i += 1;
                }
                i += 1;
            }
            '\'' if i + 2 < chars.len() && chars[i + 1] != '\\' && chars[i + 2] == '\'' => {
                result.push_str("''");
                i += 3;
            }
            '\'' if i + 3 < chars.len() && chars[i + 1] == '\\' && chars[i + 3] == '\'' => {
                result.push_str("''");
                i += 4;
            }
            c => {
                result.push(c);
                i += 1;
            }
        }
    }
    result
}

/// Check if a line is a pure comment (only comment content, no code)
fn is_pure_comment(line: &str) -> bool {
    let stripped = line.trim_start();
    if stripped.starts_with("///") || stripped.starts_with("//!") || stripped.starts_with("//") {
        // It's a comment - check if there's any code part
        if stripped.starts_with("///") || stripped.starts_with("//!") {
            // Doc comments are always pure
            return true;
        }
        // For //, check if the part before // has code
        let code_part = strip_comments_and_strings(line);
        return code_part.trim().is_empty();
    }
    false
}

/// Allowlist of (distinctive substring, reason)
fn get_allowlist() -> Vec<(&'static str, &'static str)> {
    vec![
        // Doc comments and help/message strings that mention paths but don't construct them
        (
            "client:<root>/var/db/pkg",
            "help text for --remote-vdb option",
        ),
        // Bash script strings embedded in code - these are executed as scripts, not Rust paths
        ("$ROOT/var/db/pkg", "bash script string checking directory"),
        (
            "$ROOT/var/cache/edb/counter",
            "bash script writing counter file",
        ),
        ("$VDBROOT", "bash script constructing VDBROOT path"),
        // API usage that correctly uses portage_vdb functions
        ("portage_vdb::for_root(root).vdb_dir()", "correct API usage"),
        (
            "portage_vdb::FilesDb::open_vdb_dir(dir)",
            "correct API usage",
        ),
        // Production code that constructs paths for remote configuration defaults
        // (not actual VDB access, just passing default config to remote client)
        (
            "format!(\"{}/var/db/pkg\", root",
            "remote client default VDB path config",
        ),
        (
            "format!(\"{}/var/db/pkg\", ctx.root",
            "remote client default VDB path config",
        ),
        // Functions that take VDB paths as arguments (not hand-constructing them)
        (
            "delete_vdb_dir(root",
            "function call passing root, not constructing path",
        ),
        // Tag/identifier strings that happen to match patterns but aren't paths
        ("(s.clone(), \"world\")", "tag identifier, not a path"),
        (
            "(format!(\"@{name}\"), \"world_sets\")",
            "tag identifier, not a path",
        ),
        (
            ".filter(|(_, f)| *f == \"world\")",
            "tag filter, not a path",
        ),
        (
            ".filter(|(_, f)| *f == \"world_sets\")",
            "tag filter, not a path",
        ),
        // Configuration value checks (not path construction)
        (
            "values.get(\"VDB_DIR\")",
            "checking bash-provided config value, not constructing path",
        ),
    ]
}

/// Recursively walk directories and collect .rs files
fn collect_rs_files(dir: &Path, files: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                collect_rs_files(&path, files);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }
}

#[test]
fn no_hand_built_vdb_paths() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = PathBuf::from(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let crates_root = workspace_root.join("crates");

    let allowlist = get_allowlist();
    let allowlist_set: HashSet<&str> = allowlist.iter().map(|(s, _)| *s).collect();

    let mut violations = Vec::new();

    // Collect all .rs files
    let mut rs_files = Vec::new();
    collect_rs_files(&crates_root, &mut rs_files);
    // A wrong root yields no files, and an empty walk would pass vacuously.
    assert!(
        !rs_files.is_empty(),
        "no .rs files under {}",
        crates_root.display()
    );

    for path in rs_files {
        // Skip target/, portage-vdb/ itself and tests/ directories.
        if path.components().any(|c| {
            if let std::path::Component::Normal(n) = c {
                let s = n.to_string_lossy();
                s == "target" || s == "tests" || s == "portage-vdb"
            } else {
                false
            }
        }) {
            continue;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let lines: Vec<&str> = content.lines().collect();
        let test_spans = find_test_spans(&lines);
        let in_tests_dir = path.components().any(|c| {
            if let std::path::Component::Normal(n) = c {
                n.to_string_lossy() == "tests"
            } else {
                false
            }
        });

        for (line_no, line) in lines.iter().enumerate() {
            let line_num = line_no + 1;

            // Check if this line is in a #[cfg(test)] span
            let in_test_span = test_spans
                .iter()
                .any(|(start, end)| start <= &line_num && &line_num <= end);
            if in_test_span || in_tests_dir {
                continue; // Skip test code
            }

            // Check if this line matches any pattern
            if matches_pattern(line) {
                // Skip pure comment lines
                if is_pure_comment(line) {
                    continue;
                }

                // Check if this line is in the allowlist
                let is_allowed = allowlist_set.iter().any(|substr| line.contains(substr));
                if !is_allowed {
                    let rel_path = path.strip_prefix(&crates_root).unwrap_or(&path);
                    violations.push(format!(
                        "{}:{}: {}",
                        rel_path.display(),
                        line_num,
                        line.trim()
                    ));
                }
            }
        }
    }

    if !violations.is_empty() {
        panic!(
            "Found code outside crates/portage-vdb/ that builds installed-database paths by hand.\n\
             Use portage_vdb::for_root(root) instead.\n\n{}",
            violations.join("\n")
        );
    }
}
