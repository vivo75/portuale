use super::*;

#[test]
fn info_scalar_value_prefers_the_calling_env_over_the_config() {
    // Backlog #219: `--info`'s scalar dump reads the calling env
    // over the config files (real `settings.get(k)`, env-first
    // `lookuplist`), not `other_vars` with an env fallback.
    // Hermetic without `with_test_env` (that hook lives in
    // portage-profile's own `cfg(test)`): a probe key no other code
    // reads, mutated under a lock, ambient value saved/restored via
    // a `Drop` guard so an assertion panic cannot leak it.
    let _guard = INFO_SCALAR_ENV_LOCK.lock().unwrap();
    let key = "PORTUALE_INFO_SCALAR_PROBE_219";
    struct RestoreProbe {
        key: &'static str,
        saved: Option<std::ffi::OsString>,
    }
    impl Drop for RestoreProbe {
        fn drop(&mut self) {
            // SAFETY: held `INFO_SCALAR_ENV_LOCK`; no other test in
            // this binary touches this probe key.
            unsafe {
                match self.saved.take() {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }
    let _restore = RestoreProbe {
        key,
        saved: std::env::var_os(key),
    };
    // SAFETY: held `INFO_SCALAR_ENV_LOCK`; no other test in this
    // binary touches this probe key.
    unsafe {
        std::env::set_var(key, "env-value-219");
    }
    let config = portage_profile::Config {
        other_vars: [(key.to_string(), "conf-value-219".to_string())]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    assert_eq!(
        info_scalar_value(&config, key).as_deref(),
        Some("env-value-219"),
        "the calling env must win over make.conf, as real prints it"
    );
    std::mem::drop(_restore);
    assert_eq!(
        info_scalar_value(&config, key).as_deref(),
        Some("conf-value-219"),
        "without the env layer the config value still shows"
    );
}
