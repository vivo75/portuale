//! Single phases, clean, instprep, and phases run from a saved environment.

use crate::ebuild_phases::*;

/// Builds the `ExecutionParameters` for a brush phase shell, redirecting
/// its stdout+stderr to `log_file` when given. Captured parallel builds
/// run through `bash` instead; this brush path is only for a forced backend.
pub(crate) fn brush_phase_params(
    shell: &mut brush_core::Shell,
    log_file: Option<&Path>,
) -> Result<(brush_core::ExecutionParameters, Option<LogPump>), String> {
    let mut pump = None;
    if let Some(path) = log_file {
        let sink = open_log_file(path)?;
        shell
            .open_files_mut()
            .set_fd(brush_core::openfiles::OpenFiles::STDOUT_FD, sink.out.into());
        shell
            .open_files_mut()
            .set_fd(brush_core::openfiles::OpenFiles::STDERR_FD, sink.err.into());
        // The shell owns the pipe write ends now; the caller holds the
        // guard across execution and drops the shell first.
        pump = sink.pump;
    }
    Ok((shell.default_exec_params(), pump))
}

/// `EMERGE_FROM`/`MERGE_TYPE=binary` for a binary merge: eclasses gate
/// build-time work on `MERGE_TYPE`, which `EMERGE_FROM` alone doesn't cover.
pub(crate) fn binary_merge_env() -> Vec<(String, String)> {
    vec![
        ("EMERGE_FROM".to_string(), "binary".to_string()),
        ("MERGE_TYPE".to_string(), "binary".to_string()),
    ]
}
