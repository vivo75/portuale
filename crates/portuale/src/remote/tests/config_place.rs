use super::*;

/// Both placements resolve through the shared helper: `Server` used
/// directly, `Client` pulled over local transport and re-rooted, with the
/// override held for the resolve. The pulled temp dir is gone afterwards.
#[test]
fn placed_config_root_covers_both_placements() {
    // `PORTAGE_CONFIGROOT` is process-global (see
    // `PLACED_CONFIG_ENV_LOCK`); pin it to an empty dir so the
    // host's own `/usr/share/portage/config/make.globals` -- when
    // one is installed -- cannot leak into this hermetic resolve.
    let _env_guard = PLACED_CONFIG_ENV_LOCK.lock().unwrap();
    let tmp = regen_tmp("placed-config");
    let pinned_server = tmp.join("pinned-server");
    std::fs::create_dir_all(&pinned_server).unwrap();
    let saved_config_root = std::env::var_os("PORTAGE_CONFIGROOT");
    // SAFETY: held `PLACED_CONFIG_ENV_LOCK`; no other test in this
    // binary pins `PORTAGE_CONFIGROOT` without it.
    unsafe {
        std::env::set_var("PORTAGE_CONFIGROOT", &pinned_server);
    }
    let config_root = tmp.join("clientroot");
    let client_etc = config_root.join("etc/portage");
    let repo = tmp.join("repo");
    std::fs::create_dir_all(client_etc.join("repos.conf")).unwrap();
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(
        client_etc.join("repos.conf/testrepo.conf"),
        format!(
            "[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = {}\n",
            repo.display()
        ),
    )
    .unwrap();
    std::fs::write(
        client_etc.join("make.conf"),
        "INSTALL_MASK=\"/usr/share/porttest/im/drop.txt *.la\"\nFEATURES=\"sandbox merge-time\"\nPORTAGE_BZIP2_COMMAND=\"lbzip2\"\n",
    )
    .unwrap();
    // `config_install_mask` prefers the process `INSTALL_MASK` env
    // var; clear it so the test pins the placed file.
    let saved_mask = std::env::var_os("INSTALL_MASK");
    // SAFETY: no other test in this binary sets `INSTALL_MASK`.
    unsafe {
        std::env::remove_var("INSTALL_MASK");
    }
    let eroot = tmp.join("eroot");
    let mut client_tmp: Option<std::path::PathBuf> = None;
    for placement in [
        ConfigPlacement::Client(client_etc.to_string_lossy().into_owned()),
        ConfigPlacement::Server(config_root.to_string_lossy().into_owned()),
    ] {
        let mut ctx = local_ctx(
            tmp.join("root").to_str().unwrap(),
            tmp.join("work").to_str().unwrap(),
        );
        ctx.etc_portage = placement.clone();
        let placed = place_config_root(&ctx, None).expect("placement resolves");
        // Both shapes resolve to a root layout: the `Client`
        // pull is re-rooted as `<tmp>/etc/portage`, the `Server`
        // root is used directly.
        assert!(
            placed.dir.join("etc/portage/make.conf").is_file(),
            "placed root lacks the client make.conf: {}",
            placed.dir.display()
        );
        assert_eq!(
            std::env::var_os("PORTAGE_CONFIGROOT"),
            Some(placed.dir.as_os_str().to_os_string()),
            "config-root override not held for the resolve"
        );
        // The same shared resolver + getters `run_bundle_stage`
        // feeds into `run_binpkg_flow`.
        let (repos, config) = crate::pretend::load_repos_and_config(&placed.dir, &eroot)
            .expect("placed client config resolves");
        assert!(repos.iter().any(|r| r.is_main));
        let (mask, _) = crate::pretend::config_install_mask(&config);
        assert_eq!(mask, "/usr/share/porttest/im/drop.txt *.la");
        // The embedded `make.globals` base plus the client's
        // `make.conf` tokens (`merge-time`, `sandbox`).
        assert_eq!(
            crate::pretend::config_features_string(&config),
            "assume-digests binpkg-docompress binpkg-dostrip binpkg-logs binpkg-multi-instance buildpkg-live compress-index config-protect-if-modified distlocks ebuild-locks fixlafiles ipc-sandbox merge-sync merge-time merge-wait multilib-strict network-sandbox news parallel-fetch pid-sandbox pkgdir-index-trusted preserve-libs protect-owned qa-unresolved-soname-deps sandbox strict unknown-features-warn unmerge-logs unmerge-orphans userfetch userpriv usersandbox usersync xattr"
        );
        assert_eq!(crate::pretend::config_bzip2_command(&config), "lbzip2");
        if matches!(placement, ConfigPlacement::Client(_)) {
            client_tmp = Some(placed.dir.to_path_buf());
        }
        drop(placed);
    }
    // SAFETY: same as above.
    unsafe {
        match saved_mask {
            Some(value) => std::env::set_var("INSTALL_MASK", value),
            None => std::env::remove_var("INSTALL_MASK"),
        }
        match saved_config_root {
            Some(value) => std::env::set_var("PORTAGE_CONFIGROOT", value),
            None => std::env::remove_var("PORTAGE_CONFIGROOT"),
        }
    }
    // The pulled temp copy is removed best-effort with the
    // resolve; the server source tree is untouched.
    assert!(
        client_tmp.as_ref().is_some_and(|dir| !dir.exists()),
        "pulled client config temp dir leaked: {client_tmp:?}"
    );
    assert!(
        client_etc.join("make.conf").is_file(),
        "server placement must not remove the source tree"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A dangling `make.profile` into the client's repo path resolves to the
/// same-named server repo's profile, even when the main repo provides the
/// same relative profile with different content.
#[test]
fn remote_client_make_profile_resolves_against_same_named_server_repo() {
    let tmp = regen_tmp("make-profile-name");
    let server_root = l171e_server_root(
        &tmp,
        &format!(
            "[DEFAULT]\nmain-repo = gentoo\n\n[gentoo]\nlocation = {}\n\n[custom]\nlocation = {}\n",
            tmp.join("serverroot/repos/gentoo").display(),
            tmp.join("serverroot/repos/custom").display(),
        ),
    );
    l171e_server_repo(&server_root, "gentoo", L171E_REL, "otherfeat -sign");
    let custom_profile = l171e_server_repo(&server_root, "custom", L171E_REL, "filecaps -sign");
    // The client's repo path models a tree the client holds elsewhere;
    // the remap only matches on its text, but discovery keeps a repo
    // solely when its location exists (Portage drops a missing directory),
    // so the path lives under the test tmpdir, created.
    let prefix = tmp.join("client-repos/custom").display().to_string();
    std::fs::create_dir_all(&prefix).unwrap();
    let client_etc = tmp.join("clientroot/etc/portage");
    std::fs::create_dir_all(client_etc.join("repos.conf")).unwrap();
    std::fs::write(
        client_etc.join("repos.conf/l171e.conf"),
        format!("[DEFAULT]\nmain-repo = custom\n\n[custom]\nlocation = {prefix}\n"),
    )
    .unwrap();
    std::fs::write(client_etc.join("make.conf"), "FEATURES=\"-news custom\"\n").unwrap();
    std::os::unix::fs::symlink(
        format!("{prefix}/profiles/{L171E_REL}"),
        client_etc.join("make.profile"),
    )
    .unwrap();
    let _env = ServerEnvPin::pin(&server_root);
    let mut ctx = local_ctx(
        tmp.join("root").to_str().unwrap(),
        tmp.join("work").to_str().unwrap(),
    );
    ctx.etc_portage = ConfigPlacement::Client(client_etc.to_string_lossy().into_owned());
    let placed = place_config_root(&ctx, None).expect("client placement resolves");
    // Remapped onto the same-named server repo, not the main one.
    assert_eq!(
        std::fs::read_link(placed.dir.join("etc/portage/make.profile")).unwrap(),
        custom_profile.join("profiles").join(L171E_REL),
        "the pulled link must point at the same-named server repo's profile"
    );
    let eroot = tmp.join("eroot");
    let (repos, config) = crate::pretend::load_repos_and_config(&placed.dir, &eroot)
        .expect("placed client config resolves");
    assert!(repos.iter().any(|repo| repo.is_main));
    assert_features_include(
        &config,
        &["custom", "filecaps", "sandbox"],
        "server make.globals + same-named profile + client make.conf",
    );
    drop(placed);
    drop(_env);
    let _ = std::fs::remove_dir_all(&tmp);
}

/// With no client `repos.conf`, the dangling link resolves by
/// `profiles/<rel>` against the server repos.
#[test]
fn remote_client_make_profile_resolves_without_client_repos_conf() {
    let tmp = regen_tmp("make-profile-norepos");
    let server_root = l171e_server_root(
        &tmp,
        &format!(
            "[DEFAULT]\nmain-repo = gentoo\n\n[gentoo]\nlocation = {}\n",
            tmp.join("serverroot/repos/gentoo").display(),
        ),
    );
    let server_profile = l171e_server_repo(&server_root, "gentoo", L171E_REL, "filecaps -sign");
    let target = format!("/var/db/repos-client-l171e/gentoo/profiles/{L171E_REL}");
    let client_etc = tmp.join("clientroot/etc/portage");
    std::fs::create_dir_all(&client_etc).unwrap();
    std::fs::write(client_etc.join("make.conf"), "FEATURES=\"-news custom\"\n").unwrap();
    std::os::unix::fs::symlink(&target, client_etc.join("make.profile")).unwrap();
    let _env = ServerEnvPin::pin(&server_root);
    let mut ctx = local_ctx(
        tmp.join("root").to_str().unwrap(),
        tmp.join("work").to_str().unwrap(),
    );
    ctx.etc_portage = ConfigPlacement::Client(client_etc.to_string_lossy().into_owned());
    let placed = place_config_root(&ctx, None).expect("client placement resolves");
    assert_eq!(
        std::fs::read_link(placed.dir.join("etc/portage/make.profile")).unwrap(),
        server_profile.join("profiles").join(L171E_REL),
    );
    let eroot = tmp.join("eroot");
    let config = l171e_resolve_with_server_repos(&placed.dir, &server_root, &eroot);
    assert_features_include(
        &config,
        &["custom", "filecaps", "sandbox"],
        "server make.globals + profile + client make.conf",
    );
    drop(placed);
    drop(_env);
    let _ = std::fs::remove_dir_all(&tmp);
}

/// An unresolvable target fails loudly, naming the target -- never a
/// silent empty chain.
#[test]
fn remote_client_make_profile_unresolvable_fails_loudly() {
    let tmp = regen_tmp("make-profile-loud");
    let server_root = l171e_server_root(
        &tmp,
        &format!(
            "[DEFAULT]\nmain-repo = gentoo\n\n[gentoo]\nlocation = {}\n",
            tmp.join("serverroot/repos/gentoo").display(),
        ),
    );
    l171e_server_repo(
        &server_root,
        "gentoo",
        "default/linux/amd64/22.0",
        "filecaps -sign",
    );
    let target = "/var/db/repos-client-l171e/gentoo/profiles/default/linux/amd64/99.9";
    let client_etc = tmp.join("clientroot/etc/portage");
    std::fs::create_dir_all(client_etc.join("repos.conf")).unwrap();
    std::fs::write(
        client_etc.join("repos.conf/l171e.conf"),
        "[DEFAULT]\nmain-repo = gentoo\n\n[gentoo]\nlocation = /var/db/repos-client-l171e/gentoo\n",
    )
    .unwrap();
    std::fs::write(client_etc.join("make.conf"), "FEATURES=\"custom\"\n").unwrap();
    std::os::unix::fs::symlink(target, client_etc.join("make.profile")).unwrap();
    let _env = ServerEnvPin::pin(&server_root);
    let mut ctx = local_ctx(
        tmp.join("root").to_str().unwrap(),
        tmp.join("work").to_str().unwrap(),
    );
    ctx.etc_portage = ConfigPlacement::Client(client_etc.to_string_lossy().into_owned());
    let error = match place_config_root(&ctx, None) {
        Ok(_) => panic!("unresolvable target must fail"),
        Err(message) => message,
    };
    assert!(error.contains("!!!"), "failure must be loud, got: {error}");
    assert!(
        error.contains(target),
        "failure must name the target, got: {error}"
    );
    drop(_env);
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A directory `make.profile` keeps working through the normal machinery:
/// its `parent` entries resolve against the configured repos.
#[test]
fn remote_client_make_profile_dir_with_cross_repo_parent_resolves() {
    let tmp = regen_tmp("make-profile-dir");
    let server_root = l171e_server_root(
        &tmp,
        &format!(
            "[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = {}\n",
            tmp.join("serverroot/repos/testrepo").display(),
        ),
    );
    // The client repo lives client-side (a directory profile's
    // parents resolve through the normal machinery, no remap).
    let client_repo = tmp.join("crepo");
    let base = client_repo.join("profiles/base");
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("make.defaults"), "FEATURES=\"filecaps -sign\"\n").unwrap();
    let client_etc = tmp.join("clientroot/etc/portage");
    std::fs::create_dir_all(client_etc.join("repos.conf")).unwrap();
    std::fs::create_dir_all(client_etc.join("make.profile")).unwrap();
    std::fs::write(
        client_etc.join("repos.conf/l171e.conf"),
        format!(
            "[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = {}\n",
            client_repo.display(),
        ),
    )
    .unwrap();
    std::fs::write(client_etc.join("make.profile/parent"), "testrepo:base\n").unwrap();
    std::fs::write(client_etc.join("make.conf"), "FEATURES=\"-news custom\"\n").unwrap();
    let _env = ServerEnvPin::pin(&server_root);
    let mut ctx = local_ctx(
        tmp.join("root").to_str().unwrap(),
        tmp.join("work").to_str().unwrap(),
    );
    ctx.etc_portage = ConfigPlacement::Client(client_etc.to_string_lossy().into_owned());
    let placed = place_config_root(&ctx, None).expect("client placement resolves");
    assert!(
        placed.dir.join("etc/portage/make.profile").is_dir()
            && placed
                .dir
                .join("etc/portage/make.profile")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_dir(),
        "a directory make.profile must pass through untouched"
    );
    let eroot = tmp.join("eroot");
    let (repos, config) = crate::pretend::load_repos_and_config(&placed.dir, &eroot)
        .expect("placed client config resolves");
    assert!(repos.iter().any(|repo| repo.is_main));
    assert_features_include(
        &config,
        &["custom", "filecaps", "sandbox"],
        "server make.globals + parent profile + client make.conf",
    );
    drop(placed);
    drop(_env);
    let _ = std::fs::remove_dir_all(&tmp);
}

/// With no client `repos.conf` and two server repos providing
/// `profiles/<rel>`, the main repo wins; the link may be client-relative
/// (resolved against the client's `/etc/portage`, never followed there).
#[test]
fn remote_client_make_profile_prefers_main_repo_and_relative_targets() {
    let tmp = regen_tmp("make-profile-order");
    let server_root = l171e_server_root(
        &tmp,
        &format!(
            "[DEFAULT]\nmain-repo = gentoo\n\n[gentoo]\nlocation = {}\n\n[ovl]\nlocation = {}\n",
            tmp.join("serverroot/repos/gentoo").display(),
            tmp.join("serverroot/repos/ovl").display(),
        ),
    );
    let main_profile = l171e_server_repo(&server_root, "gentoo", L171E_REL, "mainfeat -sign");
    l171e_server_repo(&server_root, "ovl", L171E_REL, "otherfeat -sign");
    let client_etc = tmp.join("clientroot/etc/portage");
    std::fs::create_dir_all(&client_etc).unwrap();
    std::fs::write(client_etc.join("make.conf"), "FEATURES=\"-news custom\"\n").unwrap();
    // A client-relative target: climb from the client's
    // `/etc/portage` to `/`, then the dangling repo path.
    let depth = client_etc
        .components()
        .filter(|component| matches!(component, std::path::Component::Normal(_)))
        .count();
    let target = format!(
        "{}var/db/repos-client-l171e/gentoo/profiles/{L171E_REL}",
        "../".repeat(depth)
    );
    std::os::unix::fs::symlink(&target, client_etc.join("make.profile")).unwrap();
    let _env = ServerEnvPin::pin(&server_root);
    let mut ctx = local_ctx(
        tmp.join("root").to_str().unwrap(),
        tmp.join("work").to_str().unwrap(),
    );
    ctx.etc_portage = ConfigPlacement::Client(client_etc.to_string_lossy().into_owned());
    let placed = place_config_root(&ctx, None).expect("client placement resolves");
    assert_eq!(
        std::fs::read_link(placed.dir.join("etc/portage/make.profile")).unwrap(),
        main_profile.join("profiles").join(L171E_REL),
        "the main repo must win the server scan"
    );
    let eroot = tmp.join("eroot");
    let config = l171e_resolve_with_server_repos(&placed.dir, &server_root, &eroot);
    assert_features_include(
        &config,
        &["custom", "mainfeat", "sandbox"],
        "server make.globals + main profile + client make.conf",
    );
    drop(placed);
    drop(_env);
    let _ = std::fs::remove_dir_all(&tmp);
}

/// `--remote-binpkg` loads the placed client config through the same
/// shared resolver the plan path uses, so `build_bundle` and the env regen
/// see the same mask, merge-time `FEATURES` and `PORTAGE_BZIP2_COMMAND`.
#[test]
fn remote_binpkg_path_resolves_the_placed_client_config() {
    // `INSTALL_MASK` is process-global (see `PLACED_CONFIG_ENV_LOCK`);
    // hold the lock across the remove/restore below so a parallel
    // test pinning a different value cannot interleave.
    let _env_guard = PLACED_CONFIG_ENV_LOCK.lock().unwrap();
    let tmp = regen_tmp("binpkg-config");
    let config_root = tmp.join("configroot");
    let repo = tmp.join("repo");
    std::fs::create_dir_all(config_root.join("etc/portage/repos.conf")).unwrap();
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(
        config_root.join("etc/portage/repos.conf/testrepo.conf"),
        format!(
            "[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = {}\n",
            repo.display()
        ),
    )
    .unwrap();
    std::fs::write(
        config_root.join("etc/portage/make.conf"),
        "INSTALL_MASK=\"/usr/share/porttest/im/drop.txt *.la\"\nFEATURES=\"sandbox merge-time\"\nPORTAGE_BZIP2_COMMAND=\"lbzip2\"\n",
    )
    .unwrap();
    // `config_install_mask` prefers the process `INSTALL_MASK` env
    // var (the same rule the plan path runs under); clear it so
    // the test pins the placed file, restoring before asserting.
    let saved_mask = std::env::var_os("INSTALL_MASK");
    // SAFETY: no other test in this binary sets `INSTALL_MASK`;
    // readers elsewhere only consume it.
    unsafe {
        std::env::remove_var("INSTALL_MASK");
    }
    let eroot = tmp.join("eroot");
    let resolved = crate::pretend::load_repos_and_config(&config_root, &eroot);
    // SAFETY: same as above.
    unsafe {
        match saved_mask {
            Some(value) => std::env::set_var("INSTALL_MASK", value),
            None => std::env::remove_var("INSTALL_MASK"),
        }
    }
    let (repos, config) = resolved.expect("synthetic client config resolves");
    assert!(repos.iter().any(|r| r.is_main));
    let (mask, prunes_usr_share) = crate::pretend::config_install_mask(&config);
    assert_eq!(mask, "/usr/share/porttest/im/drop.txt *.la");
    assert!(!prunes_usr_share);
    // Same embedded base as above.
    assert_eq!(
        crate::pretend::config_features_string(&config),
        "assume-digests binpkg-docompress binpkg-dostrip binpkg-logs binpkg-multi-instance buildpkg-live compress-index config-protect-if-modified distlocks ebuild-locks fixlafiles ipc-sandbox merge-sync merge-time merge-wait multilib-strict network-sandbox news parallel-fetch pid-sandbox pkgdir-index-trusted preserve-libs protect-owned qa-unresolved-soname-deps sandbox strict unknown-features-warn unmerge-logs unmerge-orphans userfetch userpriv usersandbox usersync xattr"
    );
    assert_eq!(crate::pretend::config_bzip2_command(&config), "lbzip2");
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A client with no `repos.conf` at all resolves on the production path
/// (`place_config_root` + `load_repos_and_config`): the server's global
/// `repos.conf` is seeded into the placed root, and only its content
/// arrives there.
#[test]
fn remote_client_without_repos_conf_resolves_on_the_production_path() {
    let tmp = regen_tmp("norepos-production");
    // A server root with repos in the global slot (the exact file
    // `place_config_root` seeds from) *and* a user slot -- like a
    // genuine server, so `find_repos` on the server root itself keeps
    // working. The user slot carries an extra overlay the global slot
    // lacks, so the placed tree pins the seed's source: only the global
    // content may arrive. The user slot also repeats the global
    // `[gentoo]`: discovery merges the host's installed global file too
    // (Portage `load_repository_config`), and a user key wins over a global one, this keeps the section pinned at the
    // server tree on any host instead of the host's own `gentoo`.
    let server_root = tmp.join("serverroot");
    std::fs::create_dir_all(server_root.join("usr/share/portage/config")).unwrap();
    std::fs::create_dir_all(server_root.join("etc/portage/repos.conf")).unwrap();
    let global_conf = format!(
        "[DEFAULT]\nmain-repo = gentoo\n\n[gentoo]\nlocation = {}\n",
        tmp.join("serverroot/repos/gentoo").display(),
    );
    std::fs::write(
        server_root.join("usr/share/portage/config/repos.conf"),
        &global_conf,
    )
    .unwrap();
    let ovl = tmp.join("serverroot/repos/ovl");
    std::fs::create_dir_all(&ovl).unwrap();
    std::fs::write(
        server_root.join("etc/portage/repos.conf/l171f.conf"),
        format!(
            "[DEFAULT]\nmain-repo = gentoo\n\n[gentoo]\nlocation = {}\n\n[ovl]\nlocation = {}\n",
            tmp.join("serverroot/repos/gentoo").display(),
            ovl.display(),
        ),
    )
    .unwrap();
    std::fs::write(
        server_root.join("usr/share/portage/config/make.globals"),
        "FEATURES=\"sandbox news sign\"\n",
    )
    .unwrap();
    let server_profile = l171e_server_repo(&server_root, "gentoo", L171E_REL, "filecaps -sign");
    let target = format!("/var/db/repos-client-l171f/gentoo/profiles/{L171E_REL}");
    let client_etc = tmp.join("clientroot/etc/portage");
    std::fs::create_dir_all(&client_etc).unwrap();
    std::fs::write(client_etc.join("make.conf"), "FEATURES=\"-news custom\"\n").unwrap();
    std::os::unix::fs::symlink(&target, client_etc.join("make.profile")).unwrap();
    let _env = ServerEnvPin::pin(&server_root);
    let mut ctx = local_ctx(
        tmp.join("root").to_str().unwrap(),
        tmp.join("work").to_str().unwrap(),
    );
    ctx.etc_portage = ConfigPlacement::Client(client_etc.to_string_lossy().into_owned());
    let placed = place_config_root(&ctx, None).expect("client placement resolves");
    assert_eq!(
        std::fs::read_link(placed.dir.join("etc/portage/make.profile")).unwrap(),
        server_profile.join("profiles").join(L171E_REL),
        "the dangling client link must remap onto the server repo's profile"
    );
    let eroot = tmp.join("eroot");
    // The production resolver both mrg paths share -- previously
    // `Err(NoReposConf)` here.
    let (repos, config) = crate::pretend::load_repos_and_config(&placed.dir, &eroot)
        .expect("placed client config resolves without a client repos.conf");
    // Only the seeded global content arrived: the server user
    // slot's overlay must not leak into the placed resolve.
    assert!(
        repos.iter().all(|repo| repo.name == "gentoo"),
        "placed repos must come from the seeded global file: {:?}",
        repos.iter().map(|repo| &repo.name).collect::<Vec<_>>()
    );
    assert!(repos.iter().any(|repo| repo.is_main));
    assert_features_include(
        &config,
        &["custom", "filecaps", "sandbox"],
        "server make.globals + server profile + client make.conf",
    );
    drop(placed);
    drop(_env);
    let _ = std::fs::remove_dir_all(&tmp);
}
