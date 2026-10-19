//! The merge-time refusal of image paths that contain a line ending.
//!
//! A `\n` or `\r` in a path cannot be written to `CONTENTS` and read back, so
//! Portage scans the image first and refuses the whole package. Only regular
//! files and symlinks count, and a symlink also counts when its target holds
//! the character; directories (and a symlink to a directory, which `os.walk`
//! files under `dirs`) are not checked.
//! Portage: lib/portage/dbapi/vartree.py, `dblink.treewalk` (`line_ending_re`).

use std::os::unix::ffi::OsStrExt;
use std::path::Path;

fn has_line_ending(bytes: &[u8]) -> bool {
    bytes.iter().any(|b| *b == b'\n' || *b == b'\r')
}

/// The sorted, `\n`/`\r`-escaped image-relative paths (each with a leading
/// `/`) that Portage's scan reports.
///
/// # Errors
/// An unreadable directory in the image.
pub(crate) fn image_paths_with_line_endings(image: &Path) -> Result<Vec<String>, String> {
    let mut found = Vec::new();
    let mut stack = vec![image.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)
            .map_err(|e| format!("!!! scanning the image {}: {e}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("!!! scanning the image: {e}"))?;
            let path = entry.path();
            let meta = std::fs::symlink_metadata(&path)
                .map_err(|e| format!("!!! scanning the image {}: {e}", path.display()))?;
            if meta.is_dir() {
                stack.push(path);
                continue;
            }
            let relative = path.strip_prefix(image).unwrap_or(&path);
            let mut flagged = has_line_ending(relative.as_os_str().as_bytes());
            if meta.file_type().is_symlink() {
                // A link to a directory is a "dir" to `os.walk`: never checked.
                if path.is_dir() {
                    continue;
                }
                if let Ok(target) = std::fs::read_link(&path) {
                    flagged |= has_line_ending(target.as_os_str().as_bytes());
                }
            }
            if flagged {
                found.push(format!(
                    "/{}",
                    relative
                        .to_string_lossy()
                        .replace('\n', "\\n")
                        .replace('\r', "\\r")
                ));
            }
        }
    }
    found.sort();
    Ok(found)
}

/// The `eerror` lines of the refusal, without the ` * ` prefix.
pub(crate) fn refusal_lines(cpv: &str, paths: &[String]) -> Vec<String> {
    let mut lines = vec![
        "This package installs one or more files containing line ending characters:".to_string(),
        String::new(),
    ];
    lines.extend(paths.iter().map(|p| format!("\t{p}")));
    lines.push(String::new());
    lines.push(format!("package {cpv} NOT merged"));
    lines.push(String::new());
    lines
}
