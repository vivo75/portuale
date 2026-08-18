//! `misc-functions.sh` steps.

use crate::ebuild_phases::*;

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
