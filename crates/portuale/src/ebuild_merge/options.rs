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

impl MergeOptions {
    /// Merge configuration from the process environment, falling back
    /// to `make.globals` defaults. Shared by `ebuild <file> merge`,
    /// `qmerge` and `emerge <atom>`.
    pub fn from_env(shell: ebuild_phases::ShellBackend, debug: bool) -> Self {
        let d = Self::default();
        let has_feature = |tok: &str| {
            std::env::var("FEATURES")
                .map(|f| f.split_whitespace().any(|t| t == tok))
                .unwrap_or(false)
        };
        let env_features: Vec<String> = std::env::var("FEATURES")
            .map(|f| f.split_whitespace().map(String::from).collect())
            .unwrap_or_default();
        let (install_mask, install_mask_prunes_usr_share) = crate::install_mask::resolve(
            &std::env::var("INSTALL_MASK").unwrap_or_default(),
            &env_features,
        );
        Self {
            debug,
            shell,
            resolved_config: None,
            config_protect: std::env::var("CONFIG_PROTECT").unwrap_or(d.config_protect),
            config_protect_mask: std::env::var("CONFIG_PROTECT_MASK")
                .unwrap_or(d.config_protect_mask),
            distdir: std::env::var_os("DISTDIR")
                .map(PathBuf::from)
                .unwrap_or(d.distdir),
            collision_protect: has_feature("collision-protect"),
            protect_owned: std::env::var("FEATURES")
                .map(|f| f.split_whitespace().any(|t| t == "protect-owned"))
                .unwrap_or(d.protect_owned),
            protect_if_modified: std::env::var("FEATURES")
                .map(|f| {
                    f.split_whitespace()
                        .any(|t| t == "config-protect-if-modified")
                })
                .unwrap_or(d.protect_if_modified),
            noconfmem: std::env::var_os("NOCONFMEM").is_some(),
            config_root: portage_repo::config_root_from_env(),
            build_env: Vec::new(),
            package_env_vars: Vec::new(),
            log_file: d.log_file,
            install_mask,
            install_mask_prunes_usr_share,
            gpg_verify: crate::binpkg::GpgVerify::from_env(),
            features: std::env::var("FEATURES").unwrap_or_default(),
            process_tmpdir: std::env::var_os("PORTAGE_TMPDIR").map(PathBuf::from),
        }
    }
}

impl MergeOptions {
    /// Re-derive every `FEATURES`-following token from the resolved list.
    /// The `emerge` paths call this right after `from_env`.
    pub fn set_resolved_features(&mut self, features: &str) {
        let has = |token: &str| features.split_whitespace().any(|t| t == token);
        self.collision_protect = has("collision-protect");
        self.protect_owned = has("protect-owned");
        self.protect_if_modified = has("config-protect-if-modified");
        self.gpg_verify = crate::binpkg::GpgVerify::from_features(features);
        self.features = features.to_string();
    }

    /// Replace the protect lists from the resolved incremental fold,
    /// when some layer assigns the key; an empty fold protects nothing.
    /// Portage: lib/portage/dbapi/vartree.py, dblink._get_protect_obj
    pub fn apply_config_protect(&mut self, config: &portage_profile::Config) {
        if let Some(v) = config.resolved_incremental("CONFIG_PROTECT") {
            self.config_protect = v.join(" ");
        }
        if let Some(v) = config.resolved_incremental("CONFIG_PROTECT_MASK") {
            self.config_protect_mask = v.join(" ");
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

impl XattrPolicy {
    /// From the merge's resolved `FEATURES` and `PORTAGE_XATTR_EXCLUDE`.
    pub(crate) fn from_options(options: &MergeOptions) -> Self {
        let features = if options.features.is_empty() {
            std::env::var("FEATURES").unwrap_or_default()
        } else {
            options.features.clone()
        };
        let exclude = options
            .resolved_config
            .as_deref()
            .and_then(|c| c.other_vars.get("PORTAGE_XATTR_EXCLUDE").cloned())
            .or_else(|| std::env::var("PORTAGE_XATTR_EXCLUDE").ok())
            .unwrap_or_default();
        Self {
            enabled: features.split_whitespace().any(|t| t == "xattr"),
            exclude,
        }
    }
}
