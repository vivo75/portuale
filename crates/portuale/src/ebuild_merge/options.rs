//! Merge options and the xattr policy.

use crate::ebuild_merge::*;

/// Options for `run_merge`, bundled into a struct rather than more
/// positional parameters -- portuale already relearned the
/// "positional-parameter pain" lesson once, in `--newrepo`'s own
/// bulk-fix saga. `config_protect`/`config_protect_mask` are env-var-
/// sourced at the `ebuild.rs` CLI boundary, the same "env var, not full
/// config resolution" shortcut `PORTAGE_TMPDIR`/`ROOT` already use;
/// `Default` matches real `make.globals`'s own values exactly.
#[derive(Clone)]
pub struct MergeOptions {
    pub debug: bool,
    pub config_protect: String,
    pub config_protect_mask: String,
    pub distdir: PathBuf,
    pub shell: ebuild_phases::ShellBackend,
    /// Real `"collision-protect" in self.settings.features` -- `FEATURES`
    /// itself isn't in `FEATURES` by default (real `make.globals` never
    /// sets it), so `Default` matches that: `false`.
    pub collision_protect: bool,
    /// Real `"protect-owned" in self.settings.features` (`lib/portage/
    /// dbapi/vartree.py:4718`): a separate abort condition from
    /// `collision_protect` -- see `run_merge`'s own doc comment for the
    /// exact real logic. **Unlike `collision_protect`**, real
    /// `protect-owned` *is* one of real `make.globals`'s own default
    /// `FEATURES` tokens (`cnf/make.globals:77-84`) -- confirmed by
    /// reading it directly (a real, previously-undiscovered mismatch:
    /// this field's own `Default` used to be `false` with a doc comment
    /// incorrectly claiming the same "not in FEATURES by default"
    /// reasoning `collision_protect` genuinely has). `Default` is now
    /// `true`, matching real portage's own actual out-of-the-box
    /// behavior. Portuale's own env-var read still only checks
    /// whether the literal `FEATURES` value (when set at all) contains
    /// the `"protect-owned"` token -- it doesn't *accumulate* onto the
    /// real default set the way real portage's own `+`/`-`-prefixed
    /// `make.conf` `FEATURES` merging does, so setting `FEATURES` to
    /// any *other* token still reads as `protect_owned: false` here,
    /// unlike real portage (which would keep it enabled unless `-
    /// protect-owned` was explicitly given) -- a pre-existing
    /// simplification this fix doesn't attempt to also resolve.
    pub protect_owned: bool,
    /// Real `--noconfmem`/`settings["NOCONFMEM"]` (`lib/_emerge/
    /// actions.py:2790`, `vartree.py:4949`'s own `cfgfiledict["IGNORE"]`):
    /// an `emerge`-only CLI flag with no real `bin/ebuild` equivalent, so
    /// portuale reads the `NOCONFMEM` env var directly (presence-based,
    /// matching real `"NOCONFMEM" in self.settings`) rather than adding a
    /// CLI flag real `ebuild` doesn't have. Forces every already-offered,
    /// unmodified-since CONFIG_PROTECT update to be re-protected into a
    /// fresh `._cfgNNNN_` file instead of silently reused/applied.
    /// `Default` matches real portage's own default: unset, `false`.
    pub noconfmem: bool,
    /// Real `"config-protect-if-modified" in self.settings.features`
    /// (`vartree.py:5376-5379`): gates `_protect()`'s own `protect_if_
    /// modified` behavior (see `protect_decision`'s own doc comment) --
    /// real `config-protect-if-modified` *is* one of real `make.globals`'s
    /// own default `FEATURES` tokens (`cnf/make.globals:79`), confirmed
    /// by reading it directly, the same category of previously-
    /// undiscovered default-`FEATURES` mismatch `protect_owned`'s own doc
    /// comment already found. `Default` is `true`, matching real
    /// portage's own actual out-of-the-box behavior. Same env-var-not-
    /// full-config-resolution shortcut (and the same "doesn't accumulate
    /// onto the real default set" simplification) `protect_owned`
    /// already uses.
    pub protect_if_modified: bool,
    /// Real `PORTAGE_CONFIGROOT` (`portage_repo::config_root_from_env`'s
    /// own real default: `/` when unset) -- consulted only by
    /// `blocked_installed_packages`'s own real `repos.conf`/profile/USE
    /// resolution (see its own doc comment). Deliberately an explicit
    /// field, not an ambient env read inside this module -- the same
    /// "explicit parameter, not an ambient env read inside library code"
    /// reasoning `portage_fetch::FetchOptions::gentoo_mirrors` already
    /// established, load-bearing here for a genuinely different reason:
    /// portuale's own dev/test machine has a real, populated
    /// `/etc/portage/repos.conf` (a real Gentoo system), so silently
    /// defaulting to real `/` the way `ebuild.rs`'s own CLI boundary
    /// does would make every test that doesn't override this field read
    /// real host config -- `Default` below uses a deliberately
    /// impossible path instead, so `blocked_installed_packages` always
    /// degrades to an empty blocked set unless a test opts in explicitly.
    pub config_root: PathBuf,
    /// Extra environment for every ebuild phase this merge runs
    /// (`install` via `run_merge`, `pkg_preinst`/`pkg_postinst` via
    /// `merge_after_install`). The `emerge <atom>` source path sets
    /// `[("USE", "<resolved enabled IUSE flags>")]` here -- computed per
    /// entry by `merge_one_source_entry`, so `bin/ebuild.sh`'s own
    /// `use()` sees the real flags instead of the `""` `phase_env_vars`
    /// leaves. Empty (`Default`) for a standalone `ebuild <file> merge`
    /// / `qmerge` and every test. See `ebuild_phases::run_commands_async`.
    pub build_env: Vec<(String, String)>,
    /// The resolved config this merge's `build_env` came from, when one
    /// is in scope (`emerge <atom>` and friends; #37 S2). `None` for a
    /// standalone `ebuild <file> merge`/`qmerge` and for tests -- those
    /// keep the curated base env. `emerge_build::entry_build_env` uses
    /// it to compute each entry's own resolved `USE` (`portage_profile::
    /// portage_use` over `portage_repo::candidate_effective_use_flags`)
    /// and the per-package rows of real `config.environ()` (`phase_
    /// environ_pkg`), instead of the enabled-IUSE-only `USE` this field
    /// used to carry. An `Arc` so the per-entry `options.clone()` is a
    /// pointer copy.
    pub resolved_config: Option<std::sync::Arc<portage_profile::Config>>,
    /// `/etc/portage/package.env`'s non-`USE` scalar half
    /// (`Config::package_env_vars`): `(atom, [(KEY, value)])` pairs.
    /// `emerge_build::entry_build_env` matches a build-bound entry's cpv
    /// against each atom and layers the matching vars over `build_env`
    /// (scalars lose to the calling environment, incrementals fold in
    /// `[base, pkg, calling-env]` order) -- real `_grab_pkg_env` into
    /// `configdict["pkg"]`. Empty (`Default`) everywhere else.
    pub package_env_vars: Vec<(String, Vec<(String, String)>)>,
    /// Real `Scheduler._background_mode`'s own `PORTAGE_LOG_FILE`
    /// redirection, extended to this merge's own `pkg_preinst`/
    /// `pkg_postinst`/`pkg_prerm`/`pkg_postrm` hooks -- previously only
    /// the `install` phase chain (`emerge_build.rs`'s own
    /// `build_one_source_entry`/`run_build_scheduler`) had a log path at
    /// all, so every hook this module runs (`merge_after_install`'s
    /// `preinst`/`postinst`, and the same-slot-replace/binary-merge
    /// `prerm`/`postrm`/`setup` hooks reached through `unmerge_one_
    /// installed`/`run_binary_merge`'s own `run_hook` closures) printed
    /// straight to the terminal even under `-jN`/`--quiet-build`,
    /// interleaving with -- or outliving -- the per-job build log real
    /// portage keeps genuinely separate. `Some` only when the caller
    /// already derived a `build.log` path for this same package's
    /// `install` phase (`emerge_build.rs`'s own `build_log_path`) --
    /// `run_one_phase`'s own log file is opened in append mode, so a
    /// hook's own output lands after that phase's, in the same file,
    /// exactly the way real portage's own single `PORTAGE_LOG_FILE`
    /// spans a whole package's build-then-merge lifecycle. `None`
    /// (`Default`) for a standalone `ebuild <file> merge`/`qmerge`/
    /// `unmerge`, `emerge -C`, and every test -- terminal output there
    /// already matches what real portage's own foreground run does.
    pub log_file: Option<PathBuf>,
    /// Real `INSTALL_MASK` as `preinst_mask()` (`bin/misc-functions.sh`)
    /// resolves it: the configured `INSTALL_MASK` (`make.conf`/profile/
    /// bashrc) with the `no{man,info,doc}` `FEATURES` tokens folded in
    /// (each appends `/usr/share/<man|info|doc>`). Real `dblink.
    /// treewalk()` applies this to the staged image -- deleting every
    /// matched path -- *before* `CONTENTS` is recorded and before
    /// collision-protect, so a masked file never lands in `${ROOT}` nor
    /// in the vdb `CONTENTS`. Written verbatim into the vdb entry's own
    /// `INSTALL_MASK` file too (real copies `build-info/INSTALL_MASK`
    /// wholesale). Empty (`Default`) = nothing masked, no `INSTALL_MASK`
    /// file written. See [`crate::install_mask`].
    pub install_mask: String,
    /// Whether any of `nodoc`/`noman`/`noinfo` is in `FEATURES` -- real
    /// `dblink.treewalk()` then also `rmdir`s a now-empty `ED/usr/share`
    /// after applying the mask. `false` (`Default`) leaves an emptied
    /// `usr/share` in place, exactly as real does when the mask emptied
    /// it for some other reason.
    pub install_mask_prunes_usr_share: bool,
    /// Merge-time gpkg signature policy (real `gpkg._verify_binpkg`'s
    /// own GPG layer -- see `crate::binpkg::GpgVerify`): enforced by
    /// `merge_binpkg` (via `extract_binpkg`) before anything is
    /// unpacked. `from_env` resolves it from `FEATURES` /
    /// `BINPKG_GPG_VERIFY_*`; `Default` is the same resolution (these
    /// are env reads either way -- the struct carries no other state).
    pub gpg_verify: crate::binpkg::GpgVerify,
    /// The resolved, merge-time `FEATURES` incremental list (space-
    /// joined), for `merge_binpkg`'s `PORTAGE_UPDATE_ENV` vdb-environment
    /// regeneration -- real portage's stored vdb env carries the fully-
    /// resolved list, not the binpkg's build-time one. The `emerge`
    /// production paths set this from `Config::resolved_incremental`
    /// ("FEATURES"); `from_env` / `Default` fall back to the raw
    /// `$FEATURES` string (all `ebuild <file> merge` has), and an empty
    /// value leaves the phase env's own `FEATURES` in place.
    pub features: String,
    /// The calling environment's own `PORTAGE_TMPDIR`, `Some` iff the
    /// process environment carries the key (a CLI-boundary read).
    /// Real's `env` layer outranks the `pkg` layer, so a per-package
    /// `package.env` `PORTAGE_TMPDIR` only re-derives the build directory
    /// when this is `None` (backlog #99, resolved per entry by
    /// `ebuild_phases::resolve_entry_portage_tmpdir`). `from_env` reads
    /// it from the process env; `Default` (tests) carries none.
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
            // "/dev/null" is a real character device, never a directory
            // -- joining anything under it can never exist on any real
            // filesystem, guaranteeing find_repos always fails cleanly
            // here regardless of what happens to exist on the host.
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
    /// Real portage's own `settings`-derived merge configuration, but via
    /// the same "read the env var, fall back to `make.globals`'s own
    /// default" shortcut every other real-execution CLI boundary in this
    /// portuale already takes (`PORTAGE_TMPDIR`/`PKGDIR`/... -- no full
    /// profile+`make.conf` resolution): `CONFIG_PROTECT`/
    /// `CONFIG_PROTECT_MASK`, `DISTDIR`, the `FEATURES` tokens
    /// `collision-protect`/`protect-owned`/`config-protect-if-modified`,
    /// `NOCONFMEM` (presence), and `PORTAGE_CONFIGROOT` (real default
    /// `"/"`). Shared by `ebuild <file> merge`/`qmerge` (`ebuild.rs`) and
    /// `emerge <atom>` (`emerge_build::run_source_merge`).
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
            // Real `preinst_mask()`: configured `INSTALL_MASK` + the
            // `no{man,info,doc}` `FEATURES` fold. The `emerge <atom>`
            // production path overrides both fields from the resolved
            // `Config` (make.conf `INSTALL_MASK`/`FEATURES`) right after
            // this; the env read is the `ebuild <file> merge`/`qmerge`
            // fallback, matching every other var here.
            install_mask,
            install_mask_prunes_usr_share,
            gpg_verify: crate::binpkg::GpgVerify::from_env(),
            // `ebuild <file> merge` fallback: the raw `$FEATURES` string
            // (the `emerge` paths overwrite this with the resolved
            // incremental list right after, as with `install_mask`).
            features: std::env::var("FEATURES").unwrap_or_default(),
            // The calling env's own `PORTAGE_TMPDIR` (presence is the
            // `env`-over-`pkg` precedence input for #99); the `emerge`
            // paths keep `from_env`'s value like every other var here.
            process_tmpdir: std::env::var_os("PORTAGE_TMPDIR").map(PathBuf::from),
        }
    }
}

impl MergeOptions {
    /// Apply the **resolved** `FEATURES` list (real `settings.features`
    /// from `config.environ()`) and re-derive every merge-time token that
    /// follows it. The `emerge` paths call this right after `from_env`
    /// (which reads the raw process env as the `ebuild <file>` fallback);
    /// `ebuild <file> merge`/`qmerge` keep `from_env`'s values. This is
    /// what makes a `make.conf` `FEATURES=collision-protect` (or
    /// `-protect-owned`) take effect on the `emerge` paths -- real reads
    /// `settings.features`, never the calling env, for all three.
    pub fn set_resolved_features(&mut self, features: &str) {
        let has = |token: &str| features.split_whitespace().any(|t| t == token);
        self.collision_protect = has("collision-protect");
        self.protect_owned = has("protect-owned");
        self.protect_if_modified = has("config-protect-if-modified");
        self.gpg_verify = crate::binpkg::GpgVerify::from_features(features);
        self.features = features.to_string();
    }

    /// Apply the **resolved** `CONFIG_PROTECT` / `CONFIG_PROTECT_MASK`
    /// (backlog #333). Real `dblink._get_protect_obj` (`vartree.py:2063-
    /// 2072`) and quickpkg's `include_config` filter (`vartree.py:2340-
    /// 2350`) read `settings["CONFIG_PROTECT"]`: the incremental fold of
    /// env.d, `make.globals`, the profile chain, `make.conf` and the
    /// calling env (`config.py:2735-2736`), which
    /// `Config::resolved_incremental` already computes (`emerge --info`
    /// prints it byte-identical to real). `from_env` read only the
    /// process env, so a merge started without the login shell's exported
    /// `CONFIG_PROTECT` protected `/etc` alone. A fold that resolves to
    /// nothing (`-*`) protects nothing, as in real; a config where no
    /// layer assigns the key at all keeps the `from_env` value.
    pub fn apply_config_protect(&mut self, config: &portage_profile::Config) {
        if let Some(v) = config.resolved_incremental("CONFIG_PROTECT") {
            self.config_protect = v.join(" ");
        }
        if let Some(v) = config.resolved_incremental("CONFIG_PROTECT_MASK") {
            self.config_protect_mask = v.join(" ");
        }
    }
}

/// What real `movefile()` does with a merged file's extended attributes
/// (#329). Real renames the image file into place when both sides share
/// a device (`movefile.py:326-330`), so every xattr survives whatever
/// `FEATURES` says. Across devices it copies, then `_copyxattr`s
/// (`movefile.py:351-358`) only under `FEATURES=xattr`, skipping
/// `PORTAGE_XATTR_EXCLUDE` patterns. `_needs_move` (`vartree.py:6376-6379`)
/// also compares xattrs (`_cmpxattr`) under `FEATURES=xattr`. portuale
/// always copies (see [`replace_file_atomic`]), so it reproduces both
/// outcomes explicitly.
#[derive(Debug, Clone, Default)]
pub(crate) struct XattrPolicy {
    /// `"xattr" in settings.features`.
    pub enabled: bool,
    /// `settings.get("PORTAGE_XATTR_EXCLUDE", "")`.
    pub exclude: String,
}

impl XattrPolicy {
    /// From the merge's resolved `FEATURES` (the raw `$FEATURES` when the
    /// options carry none, as everywhere else in this module) and the
    /// resolved config's `PORTAGE_XATTR_EXCLUDE` (`make.globals` sets
    /// it), else the process env's.
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
