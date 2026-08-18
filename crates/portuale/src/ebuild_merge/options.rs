//! Merge options and the xattr policy.

use crate::ebuild_merge::*;

/// Options for `run_merge`, bundled into a struct rather than more
/// positional parameters. Protect lists and `FEATURES` tokens are
/// env-var-sourced at the CLI boundary; `Default` matches `make.globals`.
#[derive(Clone)]
pub struct MergeOptions {
    pub debug: bool,
    pub config_protect: String,
    pub config_protect_mask: String,
    pub distdir: PathBuf,
    pub shell: ebuild_phases::ShellBackend,
    /// True when `FEATURES` carries `collision-protect` (off by default).
    pub collision_protect: bool,
    /// True when `FEATURES` carries `protect-owned` (on by default), a
    /// separate abort condition from `collision_protect`.
    pub protect_owned: bool,
    /// Re-protect every already-offered update instead of reusing it
    /// (`NOCONFMEM` presence, off by default).
    pub noconfmem: bool,
    /// Apply the new content directly when the live file still matches
    /// the installed instance (on by default).
    pub protect_if_modified: bool,
    /// Root used for blocker config/USE resolution. An explicit field,
    /// not an ambient read, so tests never see host config: `Default`
    /// is an impossible path that degrades to an empty blocked set.
    pub config_root: PathBuf,
    /// Extra environment for every ebuild phase this merge runs.
    /// The `emerge <atom>` path sets `USE` per entry; empty otherwise.
    pub build_env: Vec<(String, String)>,
    /// The resolved config this merge's `build_env` came from, when one
    /// is in scope (`None` for standalone merges and tests).
    pub resolved_config: Option<std::sync::Arc<portage_profile::Config>>,
    /// Per-package extra env from `package.env`, matched by atom.
    pub package_env_vars: Vec<(String, Vec<(String, String)>)>,
    /// Build log receiving the merge hooks' output (`None` = terminal).
    pub log_file: Option<PathBuf>,
    /// `INSTALL_MASK` with the `no{man,info,doc}` `FEATURES` tokens
    /// folded in; empty masks nothing. See [`crate::install_mask`].
    /// Portage: bin/misc-functions.sh, preinst_mask
    pub install_mask: String,
    /// Also remove an emptied `ED/usr/share` after applying the mask.
    pub install_mask_prunes_usr_share: bool,
    /// Merge-time binpkg signature policy, resolved from `FEATURES`.
    pub gpg_verify: crate::binpkg::GpgVerify,
    /// Resolved merge-time `FEATURES` list, for the vdb env regeneration.
    /// `from_env`/`Default` carry the raw `$FEATURES`; empty keeps the
    /// phase env's own value.
    pub features: String,
    /// The calling environment's own `PORTAGE_TMPDIR`, when set.
    /// Outranks a per-package `package.env` value; `None` in tests.
    pub process_tmpdir: Option<PathBuf>,
}

impl Default for MergeOptions {
    fn default() -> Self {
        Self {
            debug: false,
            config_protect: "/etc".to_string(),
            config_protect_mask: "/etc/env.d".to_string(),
            distdir: PathBuf::from("/var/cache/distfiles"),
            shell: ebuild_phases::ShellBackend::default(),
            collision_protect: false,
            resolved_config: None,
            protect_owned: true,
            noconfmem: false,
            protect_if_modified: true,
            // Impossible path: blocker resolution then degrades to empty
            // instead of reading host config.
            config_root: PathBuf::from("/dev/null/no-config-root-configured"),
            build_env: Vec::new(),
            package_env_vars: Vec::new(),
            log_file: None,
            install_mask: String::new(),
            install_mask_prunes_usr_share: false,
            gpg_verify: crate::binpkg::GpgVerify::default(),
            features: String::new(),
            process_tmpdir: None,
        }
    }
}

/// A merged file's extended attributes: same-device rename keeps every
/// xattr; cross-device copy carries them only under `FEATURES=xattr`,
/// minus `PORTAGE_XATTR_EXCLUDE`.
/// Portage: lib/portage/util/movefile.py, movefile
#[derive(Debug, Clone, Default)]
pub(crate) struct XattrPolicy {
    /// Whether `FEATURES` carries `xattr`.
    pub enabled: bool,
    /// `PORTAGE_XATTR_EXCLUDE` patterns.
    pub exclude: String,
}
