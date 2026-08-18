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

/// Arguments of [`run_single_phase`].
pub(crate) struct RunSinglePhaseArgs<'a> {
    pub(crate) ebuild_path: &'a Path,
    pub(crate) phase: &'a str,
    pub(crate) root: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) debug: bool,
    pub(crate) config_root: &'a Path,
    pub(crate) shell: ShellBackend,
    /// Resolved `USE` for an `emerge` merge's own hooks (see
    /// `run_commands_async`); `&[]` otherwise.
    pub(crate) build_env: &'a [(String, String)],
    /// Background-mode `PORTAGE_LOG_FILE`, extended to this hook (see
    /// `ebuild_merge::MergeOptions::log_file`); `None` for foreground.
    pub(crate) log_file: Option<&'a Path>,
}

/// Arguments of [`run_clean`].
pub(crate) struct RunCleanArgs<'a> {
    pub(crate) ebuild_path: &'a Path,
    pub(crate) root: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) build_env: &'a [(String, String)],
    pub(crate) debug: bool,
    pub(crate) config_root: &'a Path,
    pub(crate) shell: ShellBackend,
    pub(crate) log_file: Option<&'a Path>,
}

/// Seeds `${T}/environment` from a binpkg's saved env, plus the `.raw` marker.
/// Portage: _emerge/BinpkgEnvExtractor.
pub(crate) fn seed_saved_environment(
    env: &Environment,
    saved_env_bz2: &Path,
) -> Result<(), String> {
    let dest_env = env.t().join("environment");
    let out =
        std::fs::File::create(&dest_env).map_err(|e| format!("{}: {e}", dest_env.display()))?;
    let status = std::process::Command::new("bzip2")
        .args(["-d", "-c", "--"])
        .arg(saved_env_bz2)
        .stdout(out)
        .status()
        .map_err(|e| format!("failed to spawn bzip2: {e}"))?;
    if !status.success() {
        let _ = std::fs::remove_file(&dest_env);
        return Err(format!(
            "bzip2 failed to decompress {} ({status})",
            saved_env_bz2.display()
        ));
    }
    std::fs::write(env.t().join("environment.raw"), [])
        .map_err(|e| format!("{}: {e}", env.t().join("environment.raw").display()))
}

/// `EMERGE_FROM`/`MERGE_TYPE=binary` for a binary merge: eclasses gate
/// build-time work on `MERGE_TYPE`, which `EMERGE_FROM` alone doesn't cover.
pub(crate) fn binary_merge_env() -> Vec<(String, String)> {
    vec![
        ("EMERGE_FROM".to_string(), "binary".to_string()),
        ("MERGE_TYPE".to_string(), "binary".to_string()),
    ]
}

/// Arguments of [`run_instprep`].
pub(crate) struct RunInstprepArgs<'a> {
    pub(crate) ebuild_path: &'a Path,
    pub(crate) saved_env_bz2: Option<&'a Path>,
    /// Extract `${T}/environment` first: `true` for the first phase of a
    /// binary merge, `false` once an earlier hook seeded it.
    pub(crate) seed: bool,
    pub(crate) root: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) build_env: &'a [(String, String)],
    pub(crate) debug: bool,
    pub(crate) config_root: &'a Path,
    pub(crate) shell: ShellBackend,
    pub(crate) log_file: Option<&'a Path>,
}

/// Arguments of [`run_phase_from_saved_env`].
pub(crate) struct RunPhaseFromSavedEnvArgs<'a> {
    pub(crate) ebuild_path: &'a Path,
    pub(crate) saved_env_bz2: &'a Path,
    /// Extract `${T}/environment` first: `true` for the first hook of a
    /// package or an unmerge, `false` for the following hooks of one merge.
    pub(crate) seed: bool,
    pub(crate) phase: &'a str,
    pub(crate) root: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) debug: bool,
    pub(crate) config_root: &'a Path,
    pub(crate) shell: ShellBackend,
    /// Background-mode `PORTAGE_LOG_FILE`, extended to this hook (see
    /// `ebuild_merge::MergeOptions::log_file`); `None` for foreground.
    pub(crate) log_file: Option<&'a Path>,
    /// Regenerates the vdb env from the live phase environment when `Some`;
    /// non-empty `refresh_features` overrides `FEATURES` for that regeneration.
    pub(crate) update_env: Option<&'a Path>,
    pub(crate) refresh_features: Option<&'a str>,
    /// Threaded into the hook env for binary-merge hooks; `None` for unmerge
    /// hooks (see `ebuild_merge::replacing_versions` for the source).
    pub(crate) replacing_versions: Option<&'a str>,
}
