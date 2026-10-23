//! Source lint: a candidate match string `cat/pkg-ver:slot/sub::repo` is built in
//! one place, `Candidate::match_str` (`candidates.rs`). Any other line that
//! formats that shape must carry a `// match-str-ok: <reason>` marker on the
//! same or the preceding line (a record that is not a `Candidate`, or a
//! display string). Otherwise a hand-built string can drop a field again, as
//! the sub-slot was once dropped (what-this-proves.md, sub-slot history).

use std::{fs, path::Path};

const MARKER: &str = "match-str-ok:";
/// The one file allowed to format the shape without a marker.
const CHOKE_FILE: &str = "candidates.rs";

/// True when `line` carries a `match-str-ok:` marker with a non-empty reason.
fn has_marker(line: &str) -> bool {
    line.split(MARKER)
        .nth(1)
        .is_some_and(|reason| !reason.trim().is_empty())
}

/// Skip the `{...}` group starting at `s` (which begins with `{`); the rest after `}`.
fn skip_group(s: &str) -> Option<&str> {
    s.strip_prefix('{')?.split_once('}').map(|(_, rest)| rest)
}

/// True when `line` formats a candidate string: `:{..}/{..}::{..}` (positional
/// or named arguments), or an `::__installed__` repo sentinel after a `}`.
fn formats_candidate_shape(line: &str) -> bool {
    if line.contains("}::__installed__") {
        return true;
    }
    line.match_indices(":{").any(|(i, _)| {
        skip_group(&line[i + 1..])
            .and_then(|r| r.strip_prefix('/'))
            .and_then(skip_group)
            .is_some_and(|r| r.starts_with("::{"))
    })
}

/// Scan `text`, appending `label:line` for every unmarked candidate-shape line.
/// Test modules (`#[cfg(test)]` followed by `mod`) end the scan.
fn scan_text(label: &str, text: &str, out: &mut Vec<String>) {
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("#[cfg(test)]")
            && lines
                .get(i + 1)
                .is_some_and(|n| n.contains("mod ") && !n.trim_start().starts_with("//"))
        {
            break;
        }
        if line.trim_start().starts_with("//") || !formats_candidate_shape(line) {
            continue;
        }
        if !(has_marker(line) || (i > 0 && has_marker(lines[i - 1]))) {
            out.push(format!("{label}:{}", i + 1));
        }
    }
}

/// Recurse into `path`; `tests/` directories, files whose name contains `tests`
/// and the choke file are skipped.
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
    } else if path.extension().is_some_and(|x| x == "rs") {
        let name = path.file_name().unwrap().to_str().unwrap();
        if name != CHOKE_FILE && !name.contains("tests") {
            scan_text(
                &path.display().to_string(),
                &fs::read_to_string(path).unwrap(),
                out,
            );
        }
    }
}

#[test]
fn every_candidate_match_string_goes_through_match_str_or_is_justified() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut bad = Vec::new();
    // `portuale` consumes `Candidate`s too; its display strings are marked.
    for root in [manifest.join("src"), manifest.join("../portuale/src")] {
        scan_path(&root, &mut bad);
    }
    assert!(
        bad.is_empty(),
        "hand-built candidate strings (use Candidate::match_str or add `// match-str-ok: <reason>`):\n{}",
        bad.join("\n")
    );
}

#[test]
fn scanner_flags_unmarked_candidate_string() {
    let mut out = Vec::new();
    let src = "let s = format!(\n    \"{}/{}-{}:{}/{}::{}\",\n    a, b, c, d, e, f\n);\n";
    scan_text("x.rs", src, &mut out);
    assert_eq!(out, vec!["x.rs:2"]);
    out.clear();
    scan_text(
        "x.rs",
        "format!(\"{category}/{package}-{}:{s}/{ss}::{r}\", v)\n",
        &mut out,
    );
    assert_eq!(out.len(), 1);
    out.clear();
    scan_text(
        "x.rs",
        "format!(\"{cat}/{pkg}-{v}:{s}/{ss}::__installed__\")\n",
        &mut out,
    );
    assert_eq!(out.len(), 1);
}

#[test]
fn scanner_accepts_justified_and_ignores_partial_or_test_code() {
    let mut out = Vec::new();
    let same = "format!(\"{}/{}-{}:{}/{}::{}\") // match-str-ok: display only\n";
    let prev = "// match-str-ok: installed record\nformat!(\"{}/{}-{}:{}/{}::{}\")\n";
    let empty = "format!(\"{}/{}-{}:{}/{}::{}\") // match-str-ok:\n";
    let partial = "format!(\"{}/{}-{}:{}/{}\")\n";
    let in_tests = "#[cfg(test)]\nmod tests {\n    format!(\"{}/{}-{}:{}/{}::{}\")\n}\n";
    for ok in [same, prev, partial, in_tests] {
        scan_text("x.rs", ok, &mut out);
    }
    assert!(out.is_empty(), "{out:?}");
    scan_text("x.rs", empty, &mut out);
    assert_eq!(out.len(), 1, "an empty reason is not a justification");
}

#[test]
fn choke_file_is_not_scanned() {
    // `candidates.rs` formats the shape in `match_str`; the walker skips it by name.
    let mut out = Vec::new();
    scan_path(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src/candidates.rs"),
        &mut out,
    );
    assert!(out.is_empty());
}
