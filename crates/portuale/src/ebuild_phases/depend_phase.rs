//! The depend phase and its metadata cache.

use crate::ebuild_phases::*;

/// Runs one ebuild's `depend` phase and returns the raw metadata keys it emits.
///
/// `bin/ebuild.sh depend` writes `KEY=value` lines to `${PORTAGE_PIPE_FD}` (wired
/// to a `${T}` temp file). Never sandboxed; `Bash` only, since Brush takes no raw fd.
// Portage: EbuildMetadataPhase, bin/ebuild.sh, _doebuild_spawn
/// A `run_depend_phase` failure carrying the exit code, so `regen.rs` can replay
/// Portage's retry decision (`returncode != 1` is retried).
pub(crate) struct DependError {
    pub(crate) code: i32,
    pub(crate) message: String,
}

/// Splits the `KEY=value` lines from the phase pipe (or a depcachedir entry)
/// into the map `read_md5_cache` also returns. Lines without `=` are skipped.
pub(crate) fn parse_aux_entry(text: &str) -> std::collections::HashMap<String, String> {
    let mut md = std::collections::HashMap::new();
    for line in text.lines() {
        if let Some((k, v)) = line.split_once('=') {
            md.insert(k.to_string(), v.to_string());
        }
    }
    md
}

/// Portage's flat depcachedir layout (`cache/flat_hash.py`):
/// `<depcachedir>/<repo path without '/'>/<cat>/<pf>`.
pub(crate) fn depcache_entry_path(
    repo_location: &Path,
    category: &str,
    pf: &str,
) -> std::path::PathBuf {
    let location = repo_location.to_string_lossy();
    depcachedir()
        .join(location.trim_start_matches('/').trim_end_matches('/'))
        .join(category)
        .join(pf)
}

/// `$PORTAGE_DEPCACHEDIR` when set, else `/var/cache/edb/dep`; process env only.
/// Portage: config.py, settings.depcachedir
pub(crate) fn depcachedir() -> std::path::PathBuf {
    std::env::var_os("PORTAGE_DEPCACHEDIR")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/var/cache/edb/dep"))
}
