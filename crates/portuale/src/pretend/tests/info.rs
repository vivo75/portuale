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

#[test]
fn repo_timestamp_is_the_first_grabfile_entry() {
    // Backlog #504: `timestamp:` takes `portage.grabfile(...)[0].strip()`:
    // comments and blank lines are skipped, trailing blanks stripped.
    let dir = TempDir::new("info_504");
    std::fs::create_dir(dir.path().join("metadata")).unwrap();
    assert_eq!(repo_timestamp(dir.path()), None);
    std::fs::write(
        dir.path().join("metadata/timestamp.chk"),
        "# c\n\nTue, 02 Feb 2026 01:02:03 +0000  \nsecond\n",
    )
    .unwrap();
    assert_eq!(
        repo_timestamp(dir.path()).as_deref(),
        Some("Tue, 02 Feb 2026 01:02:03 +0000")
    );
}

#[test]
fn repo_head_commit_is_git_only() {
    // Backlog #504: only a `sync-type = git` repo reports `head commit:`
    // (the git sync module's `retrieve_head`); rsync raises
    // NotImplementedError in Portage and prints nothing.
    let dir = TempDir::new("info_504");
    let git = |args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(["-c", "user.name=a", "-c", "user.email=a@b"])
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    };
    git(&["init", "-q"]);
    git(&["commit", "-q", "--allow-empty", "-m", "x"]);
    assert_eq!(repo_head_commit(None, dir.path()), None);
    assert_eq!(repo_head_commit(Some("rsync"), dir.path()), None);
    let head = repo_head_commit(Some("git"), dir.path()).unwrap();
    assert_eq!(head.len(), 40);
    assert!(head.bytes().all(|b| b.is_ascii_hexdigit()));
}

#[test]
fn info_masks_url_passwords_like_hide_url_passwd() {
    // Backlog #504: `sync-uri` of repos and binrepos goes through
    // `_hide_url_passwd`; a URI without a password is untouched.
    use crate::emerge_getbinpkg::hide_binhost_passwd as h;
    assert_eq!(h("https://u:pw@h/x.git"), "https://u:*password*@h/x.git");
    assert_eq!(h("https://h/x.git"), "https://h/x.git");
}
