//! Running one phase on brush or bash.

use crate::ebuild_phases::*;

/// The host `bash` binary, as a phase's `$BASH` (brush never sets it, which broke
/// `__filter_readonly_variables`). First `bash` on `PATH`, resolved once.
pub(crate) fn real_bash_path() -> String {
    use std::sync::OnceLock;
    static RESOLVED: OnceLock<String> = OnceLock::new();
    RESOLVED
        .get_or_init(|| {
            if let Some(path) = std::env::var_os("PATH") {
                for dir in std::env::split_paths(&path) {
                    let candidate = dir.join("bash");
                    if candidate.is_file() {
                        return candidate.to_string_lossy().into_owned();
                    }
                }
            }
            "bash".to_owned()
        })
        .clone()
}
