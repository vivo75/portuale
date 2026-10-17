use super::*;

/// The merge's protect lists are the resolved `CONFIG_PROTECT[_MASK]` fold,
/// not the raw process-env values; a fold resolving to nothing protects nothing.
/// Portage: `vartree.py`, `_get_protect_obj`.
#[test]
fn apply_config_protect_takes_the_resolved_fold() {
    let layers = |v: &[&[&str]]| -> Vec<Vec<String>> {
        v.iter()
            .map(|l| l.iter().map(|t| t.to_string()).collect())
            .collect()
    };
    let mut config = portage_profile::Config::default();
    config.incremental_sources.insert(
        "CONFIG_PROTECT".to_string(),
        layers(&[
            &["/etc", "/usr/share/gnupg/qualified.txt"],
            &["/usr/share/cfgprotme"],
        ]),
    );
    let mut options = MergeOptions {
        config_protect: "/from-env".to_string(),
        config_protect_mask: "/from-env-mask".to_string(),
        ..MergeOptions::default()
    };
    options.apply_config_protect(&config);
    assert_eq!(
        options.config_protect,
        "/etc /usr/share/cfgprotme /usr/share/gnupg/qualified.txt"
    );
    assert_eq!(options.config_protect_mask, "/from-env-mask");

    config
        .incremental_sources
        .insert("CONFIG_PROTECT".to_string(), layers(&[&["/etc"], &["-*"]]));
    options.apply_config_protect(&config);
    assert_eq!(options.config_protect, "");
}

#[test]
fn set_resolved_features_rederives_the_merge_tokens() {
    // The emerge paths replace the raw-FEATURES fields with the resolved
    // list, so make.conf tokens take effect.
    let mut options = MergeOptions::default();
    options.set_resolved_features("collision-protect binpkg-request-signature");
    assert!(options.collision_protect);
    assert!(!options.protect_owned);
    assert!(!options.protect_if_modified);
    assert!(!options.unmerge_orphans);
    assert_eq!(
        options.features,
        "collision-protect binpkg-request-signature"
    );
    assert!(options.gpg_verify.request_signature);
    options.set_resolved_features("protect-owned config-protect-if-modified");
    assert!(options.protect_owned);
    assert!(options.protect_if_modified);
    assert!(!options.collision_protect);
    assert!(!options.unmerge_orphans);
    assert!(!options.gpg_verify.request_signature);
    // The resolved list carries the make.globals default (or the Q3
    // synthetic base when make.globals was not read).
    options.set_resolved_features("unmerge-orphans");
    assert!(options.unmerge_orphans);
}

// `FEATURES` is process-global: share the remote tests' lock, so no two
// `FEATURES` writers overlap.
use crate::remote::tests::PLACED_CONFIG_ENV_LOCK as FEATURES_ENV_LOCK;

/// Saves `FEATURES` on entry and restores it on drop, so an assertion panic
/// cannot leak a probe value into another test.
struct RestoreFeatures {
    saved: Option<std::ffi::OsString>,
}

impl RestoreFeatures {
    fn set(value: Option<&str>) -> Self {
        let saved = std::env::var_os("FEATURES");
        // SAFETY: the caller holds `FEATURES_ENV_LOCK`; no other
        // test in this binary touches `FEATURES` while it is held.
        unsafe {
            match value {
                Some(v) => std::env::set_var("FEATURES", v),
                None => std::env::remove_var("FEATURES"),
            }
        }
        Self { saved }
    }
}

impl Drop for RestoreFeatures {
    fn drop(&mut self) {
        // SAFETY: same locked env mutation as `set` (restoring the value).
        unsafe {
            match self.saved.take() {
                Some(value) => std::env::set_var("FEATURES", value),
                None => std::env::remove_var("FEATURES"),
            }
        }
    }
}

#[test]
fn from_env_folds_the_calling_env_over_the_default_on_unmerge_orphans() {
    // Portage const.py:130: `FEATURES` is incremental, and
    // `unmerge-orphans` is a make.globals default. The old literal test
    // read `FEATURES=buildpkg` as "orphans off"; the fold keeps it on.
    let _env = FEATURES_ENV_LOCK.lock().unwrap();
    let shell = crate::ebuild_phases::ShellBackend::default();
    let _restore = RestoreFeatures::set(None);
    assert!(MergeOptions::from_env(shell, false).unmerge_orphans);
    let _restore = RestoreFeatures::set(Some("buildpkg"));
    assert!(MergeOptions::from_env(shell, false).unmerge_orphans);
    let _restore = RestoreFeatures::set(Some("-unmerge-orphans"));
    assert!(!MergeOptions::from_env(shell, false).unmerge_orphans);
    let _restore = RestoreFeatures::set(Some("-*"));
    assert!(!MergeOptions::from_env(shell, false).unmerge_orphans);
}

#[test]
fn uninstall_ignore_defaults_to_the_make_globals_value() {
    // Portage cnf/make.globals:93: `UNINSTALL_IGNORE=/lib/modules/*`.
    assert_eq!(MergeOptions::default().uninstall_ignore, "/lib/modules/*");
}

#[test]
fn apply_uninstall_ignore_takes_the_resolved_scalar() {
    // A literal assignment replaces the seeded default; the append form
    // arrives already expanded (see the Q3 base-layer test).
    let mut config = portage_profile::Config::default();
    let mut options = MergeOptions::default();
    options.apply_uninstall_ignore(&config);
    assert_eq!(options.uninstall_ignore, "/lib/modules/*");
    config
        .other_vars
        .insert("UNINSTALL_IGNORE".to_string(), "/x/*".to_string());
    options.apply_uninstall_ignore(&config);
    assert_eq!(options.uninstall_ignore, "/x/*");
}

#[test]
fn set_resolved_features_folds_packdebug_over_uninstall_ignore() {
    // Portage vartree.py:2926-2931: under `FEATURES=packdebug` the old
    // debuginfo tarballs are kept for binhost clients.
    let mut options = MergeOptions::default();
    options.set_resolved_features("packdebug buildpkg");
    assert_eq!(
        options.uninstall_ignore,
        "/lib/modules/* /usr/lib/debug/.tarball/*"
    );
    // Re-resolving never stacks the pattern.
    options.set_resolved_features("packdebug");
    assert_eq!(
        options.uninstall_ignore,
        "/lib/modules/* /usr/lib/debug/.tarball/*"
    );
    options.set_resolved_features("buildpkg");
    assert_eq!(options.uninstall_ignore, "/lib/modules/*");
}

#[test]
fn from_env_reads_uninstall_ignore_from_the_calling_env() {
    let _env = FEATURES_ENV_LOCK.lock().unwrap();
    let shell = crate::ebuild_phases::ShellBackend::default();
    let _restore = RestoreFeatures::set(Some("packdebug"));
    let before = std::env::var("UNINSTALL_IGNORE").ok();
    unsafe {
        std::env::set_var("UNINSTALL_IGNORE", "/x/*");
    }
    let options = MergeOptions::from_env(shell, false);
    match before {
        Some(v) => unsafe {
            std::env::set_var("UNINSTALL_IGNORE", v);
        },
        None => unsafe {
            std::env::remove_var("UNINSTALL_IGNORE");
        },
    }
    // The env value replaces the default; packdebug appends its tarball dir.
    assert_eq!(options.uninstall_ignore, "/x/* /usr/lib/debug/.tarball/*");
}
