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
