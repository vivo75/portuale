//! The phase shell backend, the calling-environment filter, and sandbox/isolation.

use crate::ebuild_phases::*;

/// The `src_*` phases sandboxing applies to. Portage sandboxes every phase outside
/// `_unsandboxed_phases` and unshares every phase outside `_ipc_phases`; for the
/// phases run here both come out to this list. `nofetch` is excluded (its own
/// private tmpdir, neither unshared nor wrapped).
/// Portage: doebuild.py, _doebuild_spawn
pub(crate) const SANDBOXED_SRC_PHASES: &str = include_str!("sandboxed_src_phases.txt");

/// Whether `name` is a line of the one-name-per-line `table`.
pub(crate) fn listed(table: &str, name: &str) -> bool {
    table.lines().any(|line| line == name)
}

/// A `FEATURES` token check over `features_string`'s output. CLI fallbacks elsewhere
/// keep their own process-env reads.
pub(crate) fn feature_token_present(features: &str, token: &str) -> bool {
    features.split_whitespace().any(|t| t == token)
}

/// `FEATURES=sandbox` or `usersandbox` present (equivalent here: no `userpriv`).
/// Portage: process.py, _spawn
pub(crate) fn fs_sandbox_requested(features: &str) -> bool {
    feature_token_present(features, "sandbox") || feature_token_present(features, "usersandbox")
}

/// `/usr/bin/sandbox` when executable (Portage's `sandbox_capable`). `None`
/// degrades to an unsandboxed run.
/// Portage: const.py, process.py
pub(crate) fn sandbox_binary() -> Option<&'static Path> {
    use std::sync::OnceLock;
    static BIN: OnceLock<Option<PathBuf>> = OnceLock::new();
    BIN.get_or_init(|| {
        let p = PathBuf::from("/usr/bin/sandbox");
        // Matches Portage's `os.access(_, os.X_OK)`.
        if p.is_file()
            && std::fs::metadata(&p)
                .map(|m| {
                    use std::os::unix::fs::PermissionsExt as _;
                    m.permissions().mode() & 0o111 != 0
                })
                .unwrap_or(false)
        {
            Some(p)
        } else {
            None
        }
    })
    .as_deref()
}

/// True when sandboxing is requested, the phase is a `src_*` phase, and the binary
/// exists (else a one-shot warning and an unsandboxed run).
pub(crate) fn fs_sandbox_for_phase(phase: &str, features: &str) -> bool {
    use std::sync::OnceLock;
    if !fs_sandbox_requested(features) || !listed(SANDBOXED_SRC_PHASES, phase) {
        return false;
    }
    if sandbox_binary().is_some() {
        return true;
    }
    static WARNED: OnceLock<()> = OnceLock::new();
    WARNED.get_or_init(|| {
        eprintln!(
            "!!! /usr/bin/sandbox not found (for FEATURES=\"sandbox\"); \
             src_* phases run without filesystem confinement"
        );
    });
    false
}
