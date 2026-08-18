//! Running one phase on brush or bash.

use crate::ebuild_phases::*;

/// Arguments of [`run_one_phase`].
pub(crate) struct RunOnePhaseArgs<'a> {
    pub(crate) env: &'a Environment,
    pub(crate) root: &'a Path,
    pub(crate) phase: &'a str,
    pub(crate) debug: bool,
    pub(crate) extra_env: &'a [(String, String)],
    pub(crate) config_root: &'a Path,
    pub(crate) shell: ShellBackend,
    pub(crate) log_file: Option<&'a Path>,
}

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

/// A fresh embedded brush shell with `$BASH` set to the host bash (a shell-state
/// assignment, not an exported variable).
pub(crate) async fn new_brush_phase_shell() -> Result<brush_core::Shell, String> {
    let mut shell = brush_core::Shell::builder()
        .default_builtins(brush_builtins::BuiltinSet::BashMode)
        .build()
        .await
        .map_err(|e| format!("brush shell failed to start: {e}"))?;
    shell
        .env_mut()
        .set_global("BASH", brush_core::ShellVariable::new(real_bash_path()))
        .map_err(|e| format!("setting $BASH failed: {e}"))?;
    Ok(shell)
}

/// Restores the process umask on drop: the embedded shell's `umask` builtin is
/// process-wide, while Portage runs every phase in its own process.
pub(crate) struct UmaskGuard(pub(crate) libc::mode_t);

impl UmaskGuard {
    pub(crate) fn save() -> Self {
        // No `umask(0)` dance: that would briefly make other threads' files world-writable.
        let from_proc = std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find_map(|l| l.strip_prefix("Umask:"))
                    .and_then(|v| libc::mode_t::from_str_radix(v.trim(), 8).ok())
            });
        Self(from_proc.unwrap_or_else(|| {
            // SAFETY: `umask(2)` cannot fail.
            let old = unsafe { libc::umask(0) };
            unsafe { libc::umask(old) };
            old
        }))
    }
}

impl Drop for UmaskGuard {
    fn drop(&mut self) {
        // SAFETY: `umask(2)` cannot fail.
        unsafe { libc::umask(self.0) };
    }
}
/// Arguments of [`run_one_phase_brush`].
pub(crate) struct RunOnePhaseBrushArgs<'a> {
    pub(crate) env: &'a Environment,
    pub(crate) root: &'a Path,
    pub(crate) phase: &'a str,
    pub(crate) debug: bool,
    pub(crate) extra_env: &'a [(String, String)],
    pub(crate) bin_dir: &'a Path,
    pub(crate) helpers_dir: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) log_file: Option<&'a Path>,
}

/// Arguments of [`run_one_phase_bash`].
pub(crate) struct RunOnePhaseBashArgs<'a> {
    pub(crate) env: &'a Environment,
    pub(crate) root: &'a Path,
    pub(crate) phase: &'a str,
    pub(crate) debug: bool,
    pub(crate) extra_env: &'a [(String, String)],
    pub(crate) bin_dir: &'a Path,
    pub(crate) helpers_dir: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) log_file: Option<&'a Path>,
    pub(crate) iso: Isolation,
}
