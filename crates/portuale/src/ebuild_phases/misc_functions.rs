//! `misc-functions.sh` steps.

use crate::ebuild_phases::*;

/// Arguments of [`run_misc_functions`].
pub(crate) struct RunMiscFunctionsArgs<'a> {
    pub(crate) env: &'a Environment,
    pub(crate) root: &'a Path,
    pub(crate) ebuild_phase_value: &'a str,
    pub(crate) dyn_command: &'a str,
    pub(crate) extra_env: &'a [(String, String)],
    pub(crate) debug: bool,
    pub(crate) config_root: &'a Path,
    pub(crate) shell: ShellBackend,
    pub(crate) log_file: Option<&'a Path>,
}
/// Arguments of [`run_misc_functions_brush`].
pub(crate) struct RunMiscFunctionsBrushArgs<'a> {
    pub(crate) env: &'a Environment,
    pub(crate) root: &'a Path,
    pub(crate) ebuild_phase_value: &'a str,
    pub(crate) dyn_command: &'a str,
    pub(crate) extra_env: &'a [(String, String)],
    pub(crate) debug: bool,
    pub(crate) bin_dir: &'a Path,
    pub(crate) helpers_dir: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) log_file: Option<&'a Path>,
}

/// Arguments of [`run_misc_functions_bash`].
pub(crate) struct RunMiscFunctionsBashArgs<'a> {
    pub(crate) env: &'a Environment,
    pub(crate) root: &'a Path,
    pub(crate) ebuild_phase_value: &'a str,
    pub(crate) dyn_command: &'a str,
    pub(crate) extra_env: &'a [(String, String)],
    pub(crate) debug: bool,
    pub(crate) bin_dir: &'a Path,
    pub(crate) helpers_dir: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) log_file: Option<&'a Path>,
    pub(crate) fs_sandbox: bool,
}

/// One process-wide multi-threaded tokio runtime (Portage runs a whole invocation
/// on a single asyncio loop; per-phase runtimes paid setup/teardown per package).
/// `OnceLock`-shared: none of the entry points is called from inside an existing
/// tokio context, so lazy reuse is safe. Never torn down (CLI process).
pub(crate) fn shared_runtime() -> Result<&'static tokio::runtime::Runtime, String> {
    use std::sync::OnceLock;
    static RUNTIME: OnceLock<Result<tokio::runtime::Runtime, String>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|e| format!("failed to start async runtime: {e}"))
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Arguments of [`run_misc_function`].
pub(crate) struct RunMiscFunctionArgs<'a> {
    pub(crate) ebuild_path: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) root: &'a Path,
    pub(crate) ebuild_phase_value: &'a str,
    pub(crate) dyn_command: &'a str,
    pub(crate) extra_env: &'a [(String, String)],
    pub(crate) debug: bool,
    pub(crate) config_root: &'a Path,
    pub(crate) shell: ShellBackend,
}
