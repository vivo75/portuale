//! Placing the client's config root on the server.

use crate::remote::*;

// --- Resolve-path support ------------------------------------------------------

/// Temporarily point `PORTAGE_CONFIGROOT` at `dir` for a resolve run,
/// restoring the previous value on drop. `unsafe` because
/// `std::env::set_var` is (edition 2024):
/// SAFETY: the guarded region is `mrg`'s synchronous resolve path --
/// binary-only, so no build threads spawn, and nothing else in the
/// process writes this variable concurrently.
pub(crate) struct ConfigRootOverride {
    pub(crate) previous: Option<std::ffi::OsString>,
}

impl Drop for ConfigRootOverride {
    fn drop(&mut self) {
        // SAFETY: see the struct doc comment.
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var("PORTAGE_CONFIGROOT", value),
                None => std::env::remove_var("PORTAGE_CONFIGROOT"),
            }
        }
    }
}

/// A placed `/etc/portage` config root for a resolve run: `Server`
/// paths are used directly, `Client` trees are pulled once into a temp
/// dir re-rooted as `<tmp>/etc/portage` (a valid `PORTAGE_CONFIGROOT`)
/// with `ConfigRootOverride` held for the whole resolve. The pulled temp
/// dir is removed best-effort on drop; the pulled tree additionally
/// carries the server's own `make.globals` seed plus a remapped
/// `make.profile` (see `place_config_root`, `remap_client_make_profile`).
/// Shared by `run_remote_resolve` and `run_bundle_stage`.
pub(crate) struct PlacedConfig {
    /// The dir to resolve from (the server path, or the pulled temp root).
    pub(crate) dir: std::path::PathBuf,
    pub(crate) _config_guard: ConfigRootOverride,
    /// Temp dir holding a pulled client tree (`None` for `Server`).
    pub(crate) tmp: Option<std::path::PathBuf>,
}

impl Drop for PlacedConfig {
    fn drop(&mut self) {
        if let Some(tmp) = &self.tmp {
            let _ = std::fs::remove_dir_all(tmp);
        }
    }
}
