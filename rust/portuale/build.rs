// Embeds the vendored ebuild phase runtime (`bin/`) and the package-set
// definitions (`cnf/sets/portage.conf`) into the binary (backlog #322 S2),
// so a `portuale` copied out of its build tree -- a container, a minimal
// host -- still has them. The output is a table, `ENTRIES`, that
// `src/embedded_runtime.rs` extracts at run time.
//
// No dependencies: a directory walk and `include_bytes!` per file. The table
// is sorted (reproducible), records no mtimes and normalises modes to
// 0o755 (any exec bit) / 0o644 so the build host's umask never leaks in.
// The absolute paths in the generated `include_bytes!` calls exist only at
// compile time; they are not stored in the binary.

use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let repo = manifest.join("../..");
    let out =
        PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR")).join("embedded_runtime.rs");

    // A directory path makes cargo rescan the whole tree, so an added or
    // removed file reruns this script, not only an edited one.
    println!("cargo:rerun-if-changed=../../bin");
    println!("cargo:rerun-if-changed=../../cnf");
    println!("cargo:rerun-if-changed=build.rs");

    let mut entries: Vec<(String, String)> = Vec::new();
    walk(&repo.join("bin"), "bin", &mut entries);
    walk(&repo.join("cnf"), "cnf", &mut entries);
    if !entries.iter().any(|(p, _)| p == "bin/ebuild.sh") {
        panic!(
            "build.rs: {} has no bin/ebuild.sh -- the vendored phase runtime must be in the \
             build context (a container build needs `COPY bin/ bin/` and `COPY cnf/ cnf/`)",
            repo.display()
        );
    }
    entries.sort();
    let mut src = String::from("pub(crate) static ENTRIES: &[Entry] = &[\n");
    for (_, line) in &entries {
        src.push_str(line);
        src.push('\n');
    }
    src.push_str("];\n");
    fs::write(&out, src).expect("write embedded_runtime.rs");
}

fn walk(dir: &Path, rel: &str, out: &mut Vec<(String, String)>) {
    out.push((
        rel.to_string(),
        format!("    Entry {{ path: {rel:?}, mode: 0o755, kind: Kind::Dir }},"),
    ));
    let mut names: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("build.rs: {}: {e}", dir.display()))
        .map(|e| e.expect("dir entry").file_name())
        .collect();
    names.sort();
    for name in names {
        let path = dir.join(&name);
        let name = name.to_string_lossy();
        let rel = format!("{rel}/{name}");
        let meta = fs::symlink_metadata(&path).expect("stat");
        let ft = meta.file_type();
        if ft.is_dir() {
            walk(&path, &rel, out);
        } else if ft.is_symlink() {
            let target = fs::read_link(&path).expect("readlink");
            out.push((
                rel.clone(),
                format!(
                    "    Entry {{ path: {rel:?}, mode: 0o777, kind: Kind::Symlink({:?}) }},",
                    target.to_string_lossy()
                ),
            ));
        } else {
            let mode = if meta.permissions().mode() & 0o111 != 0 {
                0o755
            } else {
                0o644
            };
            let abs = fs::canonicalize(&path).expect("canonicalize");
            let mut line = String::new();
            write!(
                line,
                "    Entry {{ path: {rel:?}, mode: {mode:#o}, kind: Kind::File(include_bytes!({:?})) }},",
                abs.to_string_lossy()
            )
            .unwrap();
            out.push((rel, line));
        }
    }
}
