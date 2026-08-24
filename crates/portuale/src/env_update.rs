//! Portage's `env_update()` (`lib/portage/util/env_update.py`), run after a
//! merge that installed something: collate `/etc/env.d/*` into
//! `profile.env`, `csh.env`, the systemd `environment.d` file and
//! `ld.so.conf`, running the target root's own `ldconfig` when a
//! candidate lib dir exists on disk.
//!
//! Scope cuts a caller must know: no cross-process mtime cache, so every
//! run is treated as a first run (`ldconfig` may run redundantly but never
//! wrongly); no `ld.so.conf.d` includes, `CHOST`/`CBUILD`, `EPREFIX`,
//! extra cumulative keys, or shlex parsing; `/etc/ld.so.conf` is always
//! rewritten.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const SPACE_SEPARATED: [&str; 2] = ["CONFIG_PROTECT", "CONFIG_PROTECT_MASK"];
const COLON_SEPARATED: [&str; 12] = [
    "ADA_INCLUDE_PATH",
    "ADA_OBJECTS_PATH",
    "CLASSPATH",
    "INFODIR",
    "INFOPATH",
    "KDEDIRS",
    "LDPATH",
    "MANPATH",
    "PATH",
    "PKG_CONFIG_PATH",
    "PYTHONPATH",
    "ROOTPATH",
];

/// One `KEY="value"`/`KEY='value'`/`KEY=value` assignment per line,
/// trailing `# comment` allowed. No shlex (multi-line values, escapes).
fn parse_env_d_line(line: &str) -> Option<(String, String)> {
    let re = regex::Regex::new(
        r#"^[ \t]*([A-Za-z_][A-Za-z0-9_]*)=(?:"([^"]*)"|'([^']*)'|(\S*))[ \t]*(#.*)?$"#,
    )
    .expect("static regex is valid");
    let caps = re.captures(line)?;
    let key = caps.get(1)?.as_str().to_string();
    let value = caps
        .get(2)
        .or_else(|| caps.get(3))
        .or_else(|| caps.get(4))
        .map(|m| m.as_str())
        .unwrap_or("");
    Some((key, value.to_string()))
}

/// At least 3 chars, starting with two digits, not a dotfile/backup.
// Portage: lib/portage/util/env_update.py, env_update
fn is_env_d_filename(name: &str) -> bool {
    let bytes = name.as_bytes();
    name.len() >= 3
        && bytes[0].is_ascii_digit()
        && bytes[1].is_ascii_digit()
        && !name.starts_with('.')
        && !name.ends_with('~')
        && !name.ends_with(".bak")
}

/// `INFOPATH` entries first, then `INFODIR` entries; empty entries
/// dropped, duplicates kept. Shared by `info_dirs_inodes` and `info_files`
/// so there is exactly one env.d parser for both.
// Portage: lib/_emerge/post_emerge.py, post_emerge
pub fn info_dir_values(root: &Path) -> Vec<String> {
    let mut infopath: Vec<String> = Vec::new();
    let mut infodir: Vec<String> = Vec::new();
    let envd_dir = root.join("etc/env.d");
    if let Ok(entries) = portage_util::read_dir_entries(&envd_dir) {
        let filenames: Vec<String> = entries
            .into_iter()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|name| is_env_d_filename(name))
            .collect();
        for fname in &filenames {
            let Ok(text) = std::fs::read_to_string(envd_dir.join(fname)) else {
                continue;
            };
            for line in text.lines() {
                let Some((key, value)) = parse_env_d_line(line) else {
                    continue;
                };
                if key == "INFOPATH" {
                    infopath.extend(value.split(':').filter(|s| !s.is_empty()).map(String::from));
                } else if key == "INFODIR" {
                    infodir.extend(value.split(':').filter(|s| !s.is_empty()).map(String::from));
                }
            }
        }
    }
    infopath.extend(infodir);
    infopath
}

fn write_generated(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use portage_util::TempDir;
    use std::os::unix::fs::MetadataExt;

    fn tempdir() -> std::path::PathBuf {
        TempDir::new("portuale-env-update-test").keep()
    }
}
