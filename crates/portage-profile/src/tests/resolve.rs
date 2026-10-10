use super::*;

/// End-to-end check against fixtures/repo/profiles (base,
/// arch/amd64 -> multi-parent -> default) + fixtures/etc/portage/make.conf
/// (which sources fixtures/etc/make.local). Traced by hand:
///   base:            USE="foo"; USE="${USE} bar"      -> {foo, bar}
///   arch/amd64:      ARCH="amd64"; ACCEPT_KEYWORDS="${ARCH}" -> keywords {amd64}
///   default:         USE="-bar baz"                   -> {foo, baz}
///   make.local (sourced first from make.conf):
///                    USE="${USE} localflag"            -> {foo, baz, localflag}
///   make.conf:       USE="confflag"                    -> {foo, baz, localflag, confflag}
/// `env.d` comes from `eroot`, not `config_root` (real `_get_env_d`):
/// with the fixture tree as `config_root` (whose own
/// `etc/profile.env` sets `USE='envdusetestflag'`), an `eroot`
/// without any `profile.env` yields no `envd_use_tokens`, while an
/// `eroot` equal to the fixture root keeps the flag.
#[test]
fn envd_use_tokens_follow_eroot_not_config_root() {
    let root = fixtures_root();
    let overlay_repos = [("overlay".to_string(), root.join("overlay"))];
    let args = |eroot: &Path| {
        resolve_config(
            &root,
            &root.join("repo"),
            &overlay_repos,
            &[],
            "testrepo",
            &HashMap::new(),
            eroot,
        )
        .expect("fixture config must resolve")
    };
    assert_eq!(
        args(&root).envd_use_tokens,
        vec!["envdusetestflag".to_string()]
    );
    let bare = TempDir::new("portage-profile-test-eroot").keep();
    fs::create_dir_all(&bare).unwrap();
    assert_eq!(args(&bare).envd_use_tokens, Vec::<String>::new());
    let _ = fs::remove_dir_all(&bare);
}

/// Backlog #332: real `_emerge/Scheduler.py:1899-1913`
/// (`_allocate_config`) calls `config.reload()` before every package
/// task (`portage/package/ebuild/config.py:2691-2699`), so a package
/// built after a merge that installed an `/etc/env.d/*` file sees the
/// new variables. `Config::reload_env_d` is portuale's `reload()` for
/// the eroot-only `profile.env` it reads (see `resolve_config`'s doc
/// comment for the broot cut): re-reading replaces the env.d scalar
/// layer and incremental layer without touching higher layers.
#[test]
fn reload_env_d_picks_up_a_rewritten_profile_env() {
    let root = TempDir::new("portage-profile-test-envd-reload").keep();
    let repo = root.join("repo");
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(portage_dir.join("env")).unwrap();
    fs::create_dir_all(root.join("etc")).unwrap();
    // `make.conf` sets one key: the higher layer `reload_env_d` must
    // not override. `CONFIG_PROTECT` has an env.d layer and a
    // `make.conf` layer so the test can tell replacement from append.
    fs::write(
        portage_dir.join("make.conf"),
        "ENVDRELOAD_KEEP=\"from-make-conf\"\nENVDRELOAD_LATE=\"from-make-conf\"\n\
         CONFIG_PROTECT=\"/conf-protect\"\n",
    )
    .unwrap();
    fs::write(
        root.join("etc/profile.env"),
        "export ENVDRELOAD_KEEP='from-envd'\n\
         export ENVDRELOAD_GONE='bye'\n\
         export CONFIG_PROTECT='/envd-protect'\n\
         export EAPI='9999'\n",
    )
    .unwrap();
    let resolve = || {
        with_test_env(&[], || {
            resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
                .expect("resolves")
        })
    };
    let mut config = resolve();
    // The first load applies the env blacklist too (real
    // `config.py:724-743` pops `_env_d_blacklist` from `env.d`):
    // `EAPI` (in `ENV_BLACKLIST`) never enters `other_vars`.
    assert_eq!(config.other_vars.get("EAPI"), None);
    assert_eq!(
        config.other_vars.get("ENVDRELOAD_KEEP").map(String::as_str),
        Some("from-make-conf")
    );
    assert_eq!(
        config.other_vars.get("ENVDRELOAD_GONE").map(String::as_str),
        Some("bye")
    );
    assert!(!config.env_d_changed(&root));
    assert_eq!(
        config.resolved_incremental("CONFIG_PROTECT"),
        Some(vec![
            "/conf-protect".to_string(),
            "/envd-protect".to_string()
        ])
    );

    // A merge ran `env-update`: a new key appears, the
    // `make.conf`-owned key changes upstream, a key disappears, and
    // the blacklisted key is still filtered.
    fs::write(
        root.join("etc/profile.env"),
        "export ENVDRELOAD_KEEP='changed-envd'\n\
         export ENVDRELOAD_VAR='1'\n\
         export ENVDRELOAD_LATE='late-envd'\n\
         export CONFIG_PROTECT='/new-protect'\n\
         export EAPI='9999'\n",
    )
    .unwrap();
    assert!(config.env_d_changed(&root));
    config.reload_env_d(&root);
    assert!(!config.env_d_changed(&root));
    assert_eq!(
        config.other_vars.get("ENVDRELOAD_VAR").map(String::as_str),
        Some("1")
    );
    assert_eq!(
        config.other_vars.get("ENVDRELOAD_KEEP").map(String::as_str),
        Some("from-make-conf")
    );
    // A key env.d only starts setting mid-run still loses to the
    // `make.conf` value it lies under (real's layered lookup).
    assert_eq!(
        config.other_vars.get("ENVDRELOAD_LATE").map(String::as_str),
        Some("from-make-conf")
    );
    assert_eq!(config.other_vars.get("ENVDRELOAD_GONE"), None);
    assert_eq!(config.other_vars.get("EAPI"), None);
    // The env.d incremental layer is replaced, not appended: the old
    // token is gone while the `make.conf` layer survives.
    assert_eq!(
        config.resolved_incremental("CONFIG_PROTECT"),
        Some(vec![
            "/conf-protect".to_string(),
            "/new-protect".to_string()
        ])
    );
    let _ = fs::remove_dir_all(&root);
}

/// Backlog #332: the profile half of real `_env_d_blacklist`
/// (`profile_only_variables`, `config.py:734-743`) drops env.d keys the
/// profile's `PROFILE_ONLY_VARIABLES` names, on the first load and on
/// every reload alike (`reload()` reuses the init-time blacklist).
#[test]
fn reload_env_d_never_admits_a_profile_only_variable() {
    let root = TempDir::new("portage-profile-test-envd-profile-only").keep();
    let repo = root.join("repo");
    let profile = root.join("etc/portage/make.profile");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&profile).unwrap();
    fs::write(
        profile.join("make.defaults"),
        "PROFILE_ONLY_VARIABLES=\"ENVDRELOAD_PO\"\n",
    )
    .unwrap();
    fs::write(
        root.join("etc/profile.env"),
        "export ENVDRELOAD_PO='first'\nexport ENVDRELOAD_OK='first'\n",
    )
    .unwrap();
    let mut config = with_test_env(&[], || {
        resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
            .expect("resolves")
    });
    assert_eq!(config.other_vars.get("ENVDRELOAD_PO"), None);
    assert_eq!(
        config.other_vars.get("ENVDRELOAD_OK").map(String::as_str),
        Some("first")
    );
    fs::write(
        root.join("etc/profile.env"),
        "export ENVDRELOAD_PO='second'\nexport ENVDRELOAD_OK='second'\n",
    )
    .unwrap();
    config.reload_env_d(&root);
    assert_eq!(config.other_vars.get("ENVDRELOAD_PO"), None);
    assert_eq!(
        config.other_vars.get("ENVDRELOAD_OK").map(String::as_str),
        Some("second")
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn resolves_fixture_profile_chain_and_make_conf() {
    let root = fixtures_root();
    // The fixture's own default/parent has a real cross-repo entry
    // ("overlay:crossrepo-parent"), so this test needs "overlay" in
    // its own repo list too -- mirroring exactly what production
    // (portuale's pretend.rs, built from real find_repos) already
    // passes for this same fixture tree.
    let overlay_repos = [("overlay".to_string(), root.join("overlay"))];
    let config = resolve_config(
        &root,
        &root.join("repo"),
        &overlay_repos,
        &[],
        "testrepo",
        &HashMap::new(),
        &root,
    )
    .expect("fixture config must resolve");
    assert_eq!(
        config.use_flags,
        HashSet::from([
            "foo".to_string(),
            "baz".to_string(),
            "localflag".to_string(),
            "confflag".to_string(),
            "video_cards_nvidia".to_string(),
            // USE_EXPAND_UNPREFIXED="ARCH" in profiles/arch/amd64/
            // make.defaults, real Gentoo's own mechanism for how
            // "amd64" exists as a plain USE flag at all.
            "amd64".to_string(),
            // ELIBC="glibc" in profiles/base/make.defaults (ELIBC is
            // in USE_EXPAND), same USE_EXPAND folding video_cards_*
            // already exercises -- and ELIBC is also in
            // USE_EXPAND_IMPLICIT, see the iuse_effective assert below.
            "elibc_glibc".to_string(),
            // CPU_FLAGS_X86="sse2" (CPU_FLAGS_X86 is in USE_EXPAND);
            // it is also USE_EXPAND_HIDDEN, but that only affects the
            // `emerge -pv` display, never the resolved flag set.
            "cpu_flags_x86_sse2".to_string(),
            // LUA_SINGLE_TARGET="lua5-1" in profiles/base/make.defaults
            // (dev-libs/singletargetpkg's per-level fold fixture) --
            // folded into the base level's own make_defaults_use.
            "lua_single_target_lua5-1".to_string(),
        ])
    );
    assert_eq!(
        config.use_expand,
        HashSet::from([
            "VIDEO_CARDS".to_string(),
            "ELIBC".to_string(),
            // LINGUAS is in USE_EXPAND for the _* wildcard fixture
            // (dev-libs/wildexpandpkg) -- no LINGUAS= value set, so
            // it folds nothing here.
            "LINGUAS".to_string(),
            // PYTHON_TARGETS + CPU_FLAGS_X86 exist for the
            // `emerge -pv` USE_EXPAND-grouping fixtures
            // (packageuseexpandpkg / hiddenexpandpkg).
            "PYTHON_TARGETS".to_string(),
            "CPU_FLAGS_X86".to_string(),
            // LUA_SINGLE_TARGET for dev-libs/singletargetpkg's
            // per-level USE_EXPAND-fold fixture.
            "LUA_SINGLE_TARGET".to_string(),
        ])
    );
    assert_eq!(
        config.use_expand_hidden,
        HashSet::from(["CPU_FLAGS_X86".to_string()])
    );
    assert_eq!(
        config.use_expand_implicit,
        HashSet::from(["ELIBC".to_string()])
    );
    // Real EAPI 5+ IUSE_EFFECTIVE: ELIBC is in USE_EXPAND ∩
    // USE_EXPAND_IMPLICIT, so every USE_EXPAND_VALUES_ELIBC value gets
    // the lowercase "elibc_" prefix -- valid implicit IUSE for every
    // package even when unlisted in its own IUSE.
    assert_eq!(
        config.iuse_effective,
        HashSet::from(["elibc_glibc".to_string(), "elibc_musl".to_string()])
    );
    assert_eq!(config.accept_keywords, HashSet::from(["amd64".to_string()]));
    // Neither the fixture profile chain nor make.conf sets
    // ACCEPT_LICENSE at all -- real portage's own "* -@EULA"
    // default applies. Real `LicenseManager` reads `license_groups`
    // from each repo's own `profiles/` dir (`LocationsManager.
    // profile_locations`): `repo/profiles/license_groups` defines
    // EULA="SomeEula", `overlay/profiles/license_groups` extends it
    // with "CrossRepoNonfree" -- so "@EULA" expands to both members,
    // main repo first then overlay, rather than staying literal.
    assert_eq!(
        config.accept_license,
        vec![
            "*".to_string(),
            "-SomeEula".to_string(),
            "-CrossRepoNonfree".to_string()
        ]
    );
    assert_eq!(
        config.license_groups.get("EULA"),
        Some(&vec![
            "SomeEula".to_string(),
            "CrossRepoNonfree".to_string()
        ])
    );
}

#[test]
fn license_groups_come_from_each_repo_profiles_dir_not_the_profile_chain() {
    // A `license_groups` file dropped into a profile-chain directory
    // (`repo/profiles/base/`) must be IGNORED -- real portage reads
    // it only from `<repo>/profiles/license_groups`
    // (`LocationsManager.profile_locations`). The fixture's real
    // groups live at `repo/profiles/license_groups` +
    // `overlay/profiles/license_groups`.
    let root = fixtures_root();
    let stray = root.join("repo/profiles/base/license_groups");
    std::fs::write(&stray, "EULA StrayShouldBeIgnored\n").unwrap();
    let config = resolve_config(
        &root,
        &root.join("repo"),
        &[("overlay".to_string(), root.join("overlay"))],
        &[],
        "testrepo",
        &HashMap::new(),
        &root,
    );
    std::fs::remove_file(&stray).ok();
    let config = config.expect("fixture config must resolve");
    // Only the two real repo-level members, in repo-then-overlay
    // order -- the stray chain-level entry is not present.
    assert_eq!(
        config.license_groups.get("EULA"),
        Some(&vec![
            "SomeEula".to_string(),
            "CrossRepoNonfree".to_string()
        ])
    );
}

#[test]
fn missing_profile_and_make_conf_yield_empty_config() {
    let empty_root = TempDir::new("portage-profile-test-empty-root").keep();
    let _ = fs::create_dir_all(&empty_root);
    let config = resolve_config(
        &empty_root,
        &empty_root.join("repo"),
        &[],
        &[],
        "testrepo",
        &HashMap::new(),
        &empty_root,
    )
    .expect("missing profile/make.conf is not an error");
    assert_eq!(config.use_flags, HashSet::new());
    assert_eq!(config.accept_keywords, HashSet::new());
    assert_eq!(
        config.accept_license,
        vec!["*".to_string(), "-@EULA".to_string()]
    );
    assert_eq!(config.accept_properties, vec!["*".to_string()]);
    assert_eq!(config.accept_restrict, vec!["*".to_string()]);
}

#[test]
fn cross_repo_profile_parent_resolves_a_named_repos_own_profile() {
    // The leaf profile lives outside any known repo, but its own
    // "parent" file names "otherrepo:base" -- expand_parent_colon
    // must resolve that to otherrepo's own profiles/base directory
    // regardless, proving repo-name lookup doesn't depend on the
    // referencing node's own repo membership.
    let root = TempDir::new("portage-profile-test-cross-repo-named").keep();
    let main_repo = root.join("repo");
    let other_repo = root.join("otherrepo");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(main_repo.join("profiles")).unwrap();
    fs::create_dir_all(other_repo.join("profiles/base")).unwrap();
    fs::create_dir_all(&leaf).unwrap();

    fs::write(
        other_repo.join("profiles/base/make.defaults"),
        "USE=\"crossrepoflag\"\n",
    )
    .unwrap();
    fs::write(leaf.join("parent"), "otherrepo:base\n").unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

    let overlay_repos = [("otherrepo".to_string(), other_repo.clone())];
    let config = resolve_config(
        &root,
        &main_repo,
        &overlay_repos,
        &[],
        "testrepo",
        &HashMap::new(),
        &root,
    )
    .expect("cross-repo parent must resolve");
    assert!(config.use_flags.contains("crossrepoflag"));
}

#[test]
fn cross_repo_profile_parent_resolves_an_aliased_repo_name() {
    // Same as above, but the `parent` line names the repo by an
    // *alias* ("ovl") rather than its canonical name ("otherrepo").
    // Real `_expand_parent_colon` -> `get_location_for_name` is keyed
    // on aliases too, so `resolve_config` gets `repo_aliases`.
    let root = TempDir::new("portage-profile-test-cross-repo-alias").keep();
    let main_repo = root.join("repo");
    let other_repo = root.join("otherrepo");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(main_repo.join("profiles")).unwrap();
    fs::create_dir_all(other_repo.join("profiles/base")).unwrap();
    fs::create_dir_all(&leaf).unwrap();
    fs::write(
        other_repo.join("profiles/base/make.defaults"),
        "USE=\"aliasedflag\"\n",
    )
    .unwrap();
    fs::write(leaf.join("parent"), "ovl:base\n").unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

    // `otherrepo` is a canonical name, `ovl` its alias.
    let overlay_repos = [("otherrepo".to_string(), other_repo.clone())];
    let repo_aliases = [("ovl".to_string(), other_repo.clone())];
    let config = resolve_config(
        &root,
        &main_repo,
        &overlay_repos,
        &repo_aliases,
        "testrepo",
        &HashMap::new(),
        &root,
    )
    .expect("aliased cross-repo parent must resolve");
    assert!(config.use_flags.contains("aliasedflag"));

    // ...and without the alias registered, it's the same clear error
    // an unknown repo name gets.
    let err = resolve_config(
        &root,
        &main_repo,
        &overlay_repos,
        &[],
        "testrepo",
        &HashMap::new(),
        &root,
    )
    .unwrap_err();
    assert!(err.to_string().contains("no repo named \"ovl\""), "{err}");
}

#[test]
fn cross_repo_profile_parent_unknown_repo_name_is_a_clear_error() {
    let root = TempDir::new("portage-profile-test-cross-repo-unknown").keep();
    let profile_dir = root.join("etc/portage");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&profile_dir).unwrap();
    fs::create_dir_all(&leaf).unwrap();
    fs::write(leaf.join("parent"), "doesnotexist:base\n").unwrap();
    let make_profile = profile_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

    let err = resolve_config(
        &root,
        &root.join("repo"),
        &[],
        &[],
        "testrepo",
        &HashMap::new(),
        &root,
    )
    .expect_err("unknown repo name must be rejected");
    assert!(
        err.to_string().contains("no repo named"),
        "unexpected error: {err}"
    );
}

#[test]
fn same_repo_colon_profile_parent_resolves_within_the_current_repo() {
    // A bare ":base" parent entry (no repo name before the colon)
    // means "this same repo" -- resolved via whichever repo the
    // *referencing* profile node's own directory belongs to, not
    // necessarily the main repo (repo_containing's own longest-
    // prefix-match), so the leaf profile here lives inside the main
    // repo itself.
    let root = TempDir::new("portage-profile-test-same-repo-colon").keep();
    let main_repo = root.join("repo");
    fs::create_dir_all(main_repo.join("profiles/leaf")).unwrap();
    fs::create_dir_all(main_repo.join("profiles/base")).unwrap();
    fs::write(
        main_repo.join("profiles/base/make.defaults"),
        "USE=\"samerepocolon\"\n",
    )
    .unwrap();
    fs::write(main_repo.join("profiles/leaf/parent"), ":base\n").unwrap();
    // Real `_allow_parent_colon` gate: a profile node inside a repo
    // needs that repo's own `layout.conf` to declare `portage-2`.
    fs::create_dir_all(main_repo.join("metadata")).unwrap();
    fs::write(
        main_repo.join("metadata/layout.conf"),
        "profile-formats = portage-2\n",
    )
    .unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(main_repo.join("profiles/leaf"), &make_profile).unwrap();

    let config = resolve_config(
        &root,
        &main_repo,
        &[],
        &[],
        "testrepo",
        &HashMap::new(),
        &root,
    )
    .expect("same-repo colon parent must resolve");
    assert!(config.use_flags.contains("samerepocolon"));
}

#[test]
fn colon_profile_parent_is_not_expanded_when_the_repo_lacks_portage_2() {
    // Real `_allow_parent_colon` gate: the identical setup as
    // `same_repo_colon_profile_parent_resolves` but with NO
    // `metadata/layout.conf` (so `profile-formats` doesn't contain
    // `portage-2`). The `:base` line is then left literal, and
    // resolution fails trying to open `<leaf>/:base` -- proving the
    // gate actually gates.
    let root = TempDir::new("portage-profile-test-colon-no-portage2").keep();
    let main_repo = root.join("repo");
    fs::create_dir_all(main_repo.join("profiles/leaf")).unwrap();
    fs::create_dir_all(main_repo.join("profiles/base")).unwrap();
    fs::write(main_repo.join("profiles/base/make.defaults"), "USE=\"x\"\n").unwrap();
    fs::write(main_repo.join("profiles/leaf/parent"), ":base\n").unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(main_repo.join("profiles/leaf"), &make_profile).unwrap();

    let err = resolve_config(
        &root,
        &main_repo,
        &[],
        &[],
        "testrepo",
        &HashMap::new(),
        &root,
    )
    .expect_err("a colon parent in a non-portage-2 repo must not resolve");
    assert!(err.to_string().contains(":base"), "unexpected error: {err}");
}

#[test]
fn same_repo_colon_profile_parent_outside_any_known_repo_is_a_clear_error() {
    let root = TempDir::new("portage-profile-test-same-repo-colon-outside").keep();
    let profile_dir = root.join("etc/portage");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&profile_dir).unwrap();
    fs::create_dir_all(&leaf).unwrap();
    fs::write(leaf.join("parent"), ":base\n").unwrap();
    let make_profile = profile_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

    let err = resolve_config(
        &root,
        &root.join("repo"),
        &[],
        &[],
        "testrepo",
        &HashMap::new(),
        &root,
    )
    .expect_err("same-repo colon outside any known repo must be rejected");
    assert!(
        err.to_string().contains("not inside any known repo"),
        "unexpected error: {err}"
    );
}

#[test]
fn diamond_profile_inheritance_does_not_double_apply_a_shared_ancestor() {
    // level "top" has two parents ("left", "right") that both inherit
    // from the same "shared" ancestor; "shared" must only contribute
    // its USE flag once, not twice (which -- since it's a set -- isn't
    // observable via *membership*, but the visited-set/cycle-safety
    // mechanism is exactly what also protects a genuine cycle, so this
    // proves that mechanism doesn't accidentally block a legitimate
    // multi-path DAG from resolving at all).
    let root = TempDir::new("portage-profile-test-diamond").keep();
    let profile_dir = root.join("etc/portage");
    fs::create_dir_all(&profile_dir).unwrap();
    for name in ["shared", "left", "right", "top"] {
        fs::create_dir_all(root.join(name)).unwrap();
    }
    fs::write(root.join("shared/make.defaults"), "USE=\"sharedflag\"\n").unwrap();
    fs::write(root.join("left/parent"), "../shared\n").unwrap();
    fs::write(root.join("right/parent"), "../shared\n").unwrap();
    fs::write(root.join("top/parent"), "../left\n../right\n").unwrap();
    fs::write(root.join("top/make.defaults"), "USE=\"${USE} topflag\"\n").unwrap();
    let make_profile = profile_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("top"), &make_profile).unwrap();

    let config = resolve_config(
        &root,
        &root.join("repo"),
        &[],
        &[],
        "testrepo",
        &HashMap::new(),
        &root,
    )
    .expect("diamond inheritance must resolve");
    assert_eq!(
        config.use_flags,
        HashSet::from(["sharedflag".to_string(), "topflag".to_string()])
    );
}

#[test]
fn package_mask_unmask_accept_keywords_load_correctly() {
    let root = TempDir::new("portage-profile-test-package-star").keep();
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();

    // package.mask: two atoms added, then one removed via "-atom" --
    // only the surviving one should remain.
    fs::write(
        portage_dir.join("package.mask"),
        "dev-libs/foo\ndev-libs/bar\n-dev-libs/bar\n",
    )
    .unwrap();
    // package.unmask: unrelated to the removal above -- a completely
    // separate list, checked per-candidate by the caller.
    fs::write(portage_dir.join("package.unmask"), "dev-libs/baz\n").unwrap();
    // package.accept_keywords: a normal entry, a "**" entry, and a
    // bare-atom (no keywords) line -- this fixture sets no global
    // ACCEPT_KEYWORDS at all, so the bare atom's own real
    // accept_keywords_defaults substitution has nothing to derive a
    // "~arch" set from and it stays present with an empty token
    // list (see the dedicated accept_keywords_defaults tests below
    // for the substitution actually producing tokens).
    fs::write(
        portage_dir.join("package.accept_keywords"),
        "dev-qt/* ~amd64\nsci-misc/live-thing **\ndev-libs/bare-no-op\n",
    )
    .unwrap();

    let config = resolve_config(
        &root,
        &root.join("repo"),
        &[],
        &[],
        "testrepo",
        &HashMap::new(),
        &root,
    )
    .expect("config with package.* files must resolve");
    assert_eq!(config.package_mask, vec!["dev-libs/foo".to_string()]);
    assert_eq!(config.package_unmask, vec!["dev-libs/baz".to_string()]);
    assert_eq!(
        config.package_accept_keywords,
        vec![
            ("dev-qt/*".to_string(), vec!["~amd64".to_string()]),
            ("sci-misc/live-thing".to_string(), vec!["**".to_string()]),
            ("dev-libs/bare-no-op".to_string(), Vec::new()),
        ]
    );
}

#[test]
fn package_mask_atom_removal_applies_across_repo_profile_and_user_sources() {
    // repo-level: masks a and b -- gets "::testrepo"-scoped (the
    // main repo's own repo-level entries now get the same scoping
    // an overlay's own already did).
    // profile-level (the one chain level): "-a" removes the
    // repo-level entry (its own unscoped "-dev-libs/a" still cancels
    // the now-scoped "dev-libs/a::testrepo" -- see stack_mask_lines's
    // own "ignore_repo" doc comment), adds c.
    // user-level: "-c" removes the profile-level entry, adds d.
    // Final: b::testrepo (repo, survives) + d (user) -- a and c were
    // each removed by a LATER source than the one that added them,
    // which only works if -atom removal spans all three sources, not
    // just within each file on its own.
    let root = TempDir::new("portage-profile-test-cross-source-mask").keep();
    let repo = root.join("repo");
    let repo_profiles = repo.join("profiles");
    let portage_dir = root.join("etc/portage");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&repo_profiles).unwrap();
    fs::create_dir_all(&portage_dir).unwrap();
    fs::create_dir_all(&leaf).unwrap();

    fs::write(
        repo_profiles.join("package.mask"),
        "dev-libs/a\ndev-libs/b\n",
    )
    .unwrap();
    fs::write(leaf.join("package.mask"), "-dev-libs/a\ndev-libs/c\n").unwrap();
    fs::write(
        portage_dir.join("package.mask"),
        "-dev-libs/c\ndev-libs/d\n",
    )
    .unwrap();

    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    assert_eq!(
        config.package_mask,
        vec!["dev-libs/b::testrepo".to_string(), "dev-libs/d".to_string()]
    );
}

#[test]
fn overlay_package_mask_is_scoped_to_its_own_repo_only() {
    // Main repo has no mask at all; the overlay masks dev-libs/a
    // with a bare atom (no "::repo" part) -- resolve_config must
    // auto-scope it to "::overlay" via scope_repo_mask_lines, so it
    // never also masks a same-named package in the main repo.
    let root = TempDir::new("portage-profile-test-overlay-mask").keep();
    let repo = root.join("repo");
    let overlay = root.join("overlay");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(overlay.join("profiles")).unwrap();

    fs::write(overlay.join("profiles/package.mask"), "dev-libs/a\n").unwrap();

    let overlay_repos = [("overlay".to_string(), overlay.to_path_buf())];
    let config = resolve_config(
        &root,
        &repo,
        &overlay_repos,
        &[],
        "testrepo",
        &HashMap::new(),
        &root,
    )
    .expect("config must resolve");
    assert_eq!(config.package_mask, vec!["dev-libs/a::overlay".to_string()]);
}

#[test]
fn overlay_package_unmask_gets_the_same_scoping_as_overlay_package_mask() {
    // Both files live in the same overlay, so both entries get the
    // identical "::overlay" auto-scoping -- the actual mask/unmask
    // cancellation happens downstream in portage-repo's is_visible,
    // which matches both against the same "::overlay"-suffixed
    // candidate string; this test only checks that resolve_config
    // itself scopes both sources consistently.
    let root = TempDir::new("portage-profile-test-overlay-mask-unmask").keep();
    let repo = root.join("repo");
    let overlay = root.join("overlay");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(overlay.join("profiles")).unwrap();

    fs::write(overlay.join("profiles/package.mask"), "dev-libs/a\n").unwrap();
    fs::write(overlay.join("profiles/package.unmask"), "dev-libs/a\n").unwrap();

    let overlay_repos = [("overlay".to_string(), overlay.to_path_buf())];
    let config = resolve_config(
        &root,
        &repo,
        &overlay_repos,
        &[],
        "testrepo",
        &HashMap::new(),
        &root,
    )
    .expect("config must resolve");
    assert_eq!(config.package_mask, vec!["dev-libs/a::overlay".to_string()]);
    assert_eq!(
        config.package_unmask,
        vec!["dev-libs/a::overlay".to_string()]
    );
}

#[test]
fn overlay_package_mask_inherits_the_main_repos_own_entries_via_implicit_masters() {
    // Real masters: an overlay with no explicit "masters =" always
    // implicitly masters the main repo -- so the main repo's own
    // package.mask entry for "dev-libs/a" must also end up
    // "::overlay"-scoped (on top of its own, separate "::testrepo"
    // scoping from being the main repo's own repo-level entry, see
    // this function's own doc comment on that follow-up), even
    // though the overlay's own package.mask never mentions
    // "dev-libs/a" at all (only "b").
    let root = TempDir::new("portage-profile-test-overlay-masters-mask").keep();
    let repo = root.join("repo");
    let overlay = root.join("overlay");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(overlay.join("profiles")).unwrap();

    fs::write(repo.join("profiles/package.mask"), "dev-libs/a\n").unwrap();
    fs::write(overlay.join("profiles/package.mask"), "dev-libs/b\n").unwrap();

    let overlay_repos = [("overlay".to_string(), overlay.to_path_buf())];
    let config = resolve_config(
        &root,
        &repo,
        &overlay_repos,
        &[],
        "testrepo",
        &HashMap::new(),
        &root,
    )
    .expect("config must resolve");
    assert_eq!(
        config.package_mask,
        vec![
            "dev-libs/a::testrepo".to_string(),
            "dev-libs/a::overlay".to_string(),
            "dev-libs/b::overlay".to_string(),
        ]
    );
}

#[test]
fn overlay_package_unmask_does_not_inherit_via_masters() {
    // Real MaskManager.py's own package.unmask loop never consults
    // masters at all -- only package.mask does. The main repo's own
    // package.unmask entry must stay "::testrepo"-scoped only (its
    // own repo-level scoping, see this function's own doc comment on
    // that follow-up), never *also* "::overlay"-scoped just because
    // the overlay implicitly masters the main repo.
    let root = TempDir::new("portage-profile-test-overlay-masters-unmask").keep();
    let repo = root.join("repo");
    let overlay = root.join("overlay");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(overlay.join("profiles")).unwrap();

    fs::write(repo.join("profiles/package.unmask"), "dev-libs/a\n").unwrap();

    let overlay_repos = [("overlay".to_string(), overlay.to_path_buf())];
    let config = resolve_config(
        &root,
        &repo,
        &overlay_repos,
        &[],
        "testrepo",
        &HashMap::new(),
        &root,
    )
    .expect("config must resolve");
    assert_eq!(
        config.package_unmask,
        vec!["dev-libs/a::testrepo".to_string()]
    );
}

#[test]
fn explicit_repo_masters_overrides_the_implicit_main_repo_default() {
    // Real explicit "masters =" (portage_repo::find_repos, resolved
    // into `repo_masters` by the caller) fully replaces the implicit
    // "masters the main repo alone" default this crate always fell
    // back to before -- an explicit empty masters list means NO
    // package.mask inheritance at all, even from the main repo.
    let root = TempDir::new("portage-profile-test-explicit-masters-empty").keep();
    let repo = root.join("repo");
    let overlay = root.join("overlay");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(overlay.join("profiles")).unwrap();

    fs::write(repo.join("profiles/package.mask"), "dev-libs/a\n").unwrap();

    let overlay_repos = [("overlay".to_string(), overlay.to_path_buf())];
    let repo_masters = HashMap::from([("overlay".to_string(), Vec::<PathBuf>::new())]);
    let config = resolve_config(
        &root,
        &repo,
        &overlay_repos,
        &[],
        "testrepo",
        &repo_masters,
        &root,
    )
    .expect("config must resolve");
    // The main repo's own "dev-libs/a" entry still applies to
    // itself (real portage's own repo-level package.mask always
    // masks its own repo, independent of who masters whom) -- but
    // is NOT inherited by the overlay, since its own masters is
    // explicitly empty.
    assert_eq!(
        config.package_mask,
        vec!["dev-libs/a::testrepo".to_string()]
    );
}

#[test]
fn explicit_repo_masters_resolves_a_non_main_master_chain() {
    // A repo can explicitly declare a master other than the main
    // repo -- its own package.mask must be stacked in too, exactly
    // like the implicit main-repo case already is.
    let root = TempDir::new("portage-profile-test-explicit-masters-chain").keep();
    let repo = root.join("repo");
    let overlay = root.join("overlay");
    let downstream = root.join("downstream");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(overlay.join("profiles")).unwrap();
    fs::create_dir_all(downstream.join("profiles")).unwrap();

    // The main repo's own entry must NOT apply (downstream doesn't
    // declare it as a master); the overlay's own entry must.
    fs::write(repo.join("profiles/package.mask"), "dev-libs/a\n").unwrap();
    fs::write(overlay.join("profiles/package.mask"), "dev-libs/b\n").unwrap();

    let overlay_repos = [
        ("overlay".to_string(), overlay.to_path_buf()),
        ("downstream".to_string(), downstream.clone()),
    ];
    let repo_masters = HashMap::from([("downstream".to_string(), vec![overlay.to_path_buf()])]);
    let config = resolve_config(
        &root,
        &repo,
        &overlay_repos,
        &[],
        "testrepo",
        &repo_masters,
        &root,
    )
    .expect("config must resolve");
    assert!(
        config
            .package_mask
            .contains(&"dev-libs/b::downstream".to_string())
    );
    assert!(
        !config
            .package_mask
            .contains(&"dev-libs/a::downstream".to_string())
    );
}

#[test]
fn overlay_package_use_is_scoped_to_its_own_repo_only() {
    // Main repo has no package.use at all; the overlay sets a flag
    // for dev-libs/a with a bare atom (no "::repo" part) --
    // resolve_config must auto-scope it to "::overlay" via
    // scope_repo_package_use_lines, the same way package.mask's own
    // overlay entries are scoped, so it never also applies to a
    // same-named package in the main repo.
    let root = TempDir::new("portage-profile-test-overlay-package-use").keep();
    let repo = root.join("repo");
    let overlay = root.join("overlay");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(overlay.join("profiles")).unwrap();

    fs::write(overlay.join("profiles/package.use"), "dev-libs/a flag\n").unwrap();

    let overlay_repos = [("overlay".to_string(), overlay.to_path_buf())];
    let config = resolve_config(
        &root,
        &repo,
        &overlay_repos,
        &[],
        "testrepo",
        &HashMap::new(),
        &root,
    )
    .expect("config must resolve");
    assert_eq!(
        config.package_use_repo,
        vec![("dev-libs/a::overlay".to_string(), vec!["flag".to_string()])]
    );
}

#[test]
fn system_packages_stack_across_profile_levels_with_atom_removal_and_star_filter() {
    // base: adds "*dev-libs/a" (a real @system atom) and a bare
    // "dev-libs/hint" line (no "*" -- a real "known but not system"
    // hint, must never contribute an atom on its own).
    // leaf (its own parent -> base): "-*dev-libs/a" removes base's
    // own system atom (proving -atom removal spans levels, not just
    // within one file), and adds "*dev-libs/b".
    // Final @system list: just "dev-libs/b" -- "a" was added then
    // removed, "hint" was never eligible in the first place.
    let root = TempDir::new("portage-profile-test-system-packages").keep();
    let repo = root.join("repo");
    let repo_profiles = repo.join("profiles");
    let base = repo_profiles.join("base");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&leaf).unwrap();

    fs::write(base.join("packages"), "*dev-libs/a\ndev-libs/hint\n").unwrap();
    fs::write(leaf.join("parent"), "../repo/profiles/base\n").unwrap();
    fs::write(leaf.join("packages"), "-*dev-libs/a\n*dev-libs/b\n").unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    assert_eq!(config.system_packages, vec!["dev-libs/b".to_string()]);
}

#[test]
fn profile_packages_are_the_unstarred_lines_of_profile_set_repo_levels() {
    // Both levels live in a repo whose layout.conf declares
    // `profile-set`, so their own `packages` files are @profile
    // sources: unstarred lines stack (with cross-level -atom
    // removal), starred lines still go to @system only.
    // base: "*dev-libs/sysbase" + "dev-libs/baseonly"
    // leaf (parent -> ../base): "-dev-libs/baseonly" +
    //        "dev-libs/leafonly" + "*dev-libs/sysleaf"
    // @profile: ["dev-libs/leafonly"] (baseonly removed)
    // @system:  ["dev-libs/sysbase", "dev-libs/sysleaf"]
    let root = TempDir::new("portage-profile-test-profile-packages").keep();
    let repo = root.join("repo");
    let base = repo.join("profiles/base");
    let leaf = repo.join("profiles/leaf");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&leaf).unwrap();
    fs::create_dir_all(repo.join("metadata")).unwrap();
    fs::write(
        repo.join("metadata/layout.conf"),
        "profile-formats = portage-2 profile-set\n",
    )
    .unwrap();
    fs::write(
        base.join("packages"),
        "*dev-libs/sysbase\ndev-libs/baseonly\n",
    )
    .unwrap();
    fs::write(leaf.join("parent"), "../base\n").unwrap();
    fs::write(
        leaf.join("packages"),
        "-dev-libs/baseonly\ndev-libs/leafonly\n*dev-libs/sysleaf\n",
    )
    .unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    assert_eq!(
        config.profile_packages,
        vec!["dev-libs/leafonly".to_string()]
    );
    assert_eq!(
        config.system_packages,
        vec![
            "dev-libs/sysbase".to_string(),
            "dev-libs/sysleaf".to_string()
        ]
    );
}

#[test]
fn profile_packages_require_the_profile_set_format() {
    // The committed fixture repo's shape: `profile-formats =
    // portage-2` without `profile-set`, so nothing is an @profile
    // source and the whole `packages` file stays @system-only (real
    // `ProfilePackageSet.load`'s own gate). @system is unaffected.
    let root = TempDir::new("portage-profile-test-no-profile-set").keep();
    let repo = root.join("repo");
    let base = repo.join("profiles/base");
    let leaf = repo.join("profiles/leaf");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&leaf).unwrap();
    fs::create_dir_all(repo.join("metadata")).unwrap();
    fs::write(
        repo.join("metadata/layout.conf"),
        "profile-formats = portage-2\n",
    )
    .unwrap();
    fs::write(
        base.join("packages"),
        "*dev-libs/sysbase\ndev-libs/hintonly\n",
    )
    .unwrap();
    fs::write(leaf.join("parent"), "../base\n").unwrap();
    fs::write(leaf.join("packages"), "*dev-libs/sysleaf\n").unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    assert!(config.profile_packages.is_empty());
    assert_eq!(
        config.system_packages,
        vec![
            "dev-libs/sysbase".to_string(),
            "dev-libs/sysleaf".to_string()
        ]
    );
}

#[test]
fn package_provided_stacks_the_profile_chain_plus_the_user_file_with_atom_removal() {
    // base: provides dev-libs/a-1.0 and dev-libs/b-1.0.
    // leaf (parent -> base): removes b, adds dev-libs/c-1.0.
    // user (/etc/portage/profile/package.provided): removes a.
    // Final: just dev-libs/c-1.0.
    let root = TempDir::new("portage-profile-test-package-provided").keep();
    let repo = root.join("repo");
    let base = repo.join("profiles/base");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&leaf).unwrap();
    fs::write(
        base.join("package.provided"),
        "dev-libs/a-1.0\ndev-libs/b-1.0\n",
    )
    .unwrap();
    fs::write(leaf.join("parent"), "../repo/profiles/base\n").unwrap();
    fs::write(
        leaf.join("package.provided"),
        "-dev-libs/b-1.0\ndev-libs/c-1.0\n",
    )
    .unwrap();
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(portage_dir.join("profile")).unwrap();
    fs::write(
        portage_dir.join("profile/package.provided"),
        "-dev-libs/a-1.0\n",
    )
    .unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    assert_eq!(config.package_provided, vec!["dev-libs/c-1.0".to_string()]);
}

#[test]
fn use_mask_and_use_force_stack_across_levels_but_stay_out_of_use_flags() {
    // base: make.defaults enables "normalflag" and "maskflag"
    // normally; use.force forces on "forceflag" and "bothflag".
    // leaf (its own parent -> base): use.mask masks "maskflag" (an
    // otherwise-normal USE flag) and "bothflag" (proving mask wins
    // when a flag is both forced AND masked, matching real
    // regenerate()'s update-then-difference_update order).
    // use_force/use_mask stack correctly across levels -- but,
    // unlike an earlier version of portuale, are deliberately NOT
    // folded into use_flags at all: real regenerate() applies them
    // as the literal last step of its own incremental USE walk,
    // strictly after package.use, so `use_flags` here stays exactly
    // what make.defaults/make.conf alone produced (see
    // `use_force`'s own doc comment for the full grounding);
    // `portage-repo`'s own `effective_use_flags` applies
    // `use_force`/`use_mask` at that later, correct position
    // instead.
    let root = TempDir::new("portage-profile-test-use-mask-force").keep();
    let repo = root.join("repo");
    let repo_profiles = repo.join("profiles");
    let base = repo_profiles.join("base");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&leaf).unwrap();

    fs::write(base.join("make.defaults"), "USE=\"normalflag maskflag\"\n").unwrap();
    fs::write(base.join("use.force"), "forceflag\nbothflag\n").unwrap();
    fs::write(leaf.join("parent"), "../repo/profiles/base\n").unwrap();
    fs::write(leaf.join("use.mask"), "maskflag\nbothflag\n").unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    assert_eq!(
        config.use_flags,
        HashSet::from(["normalflag".to_string(), "maskflag".to_string()])
    );
    assert_eq!(
        config.use_force,
        HashSet::from(["forceflag".to_string(), "bothflag".to_string()])
    );
    assert_eq!(
        config.use_mask,
        HashSet::from(["maskflag".to_string(), "bothflag".to_string()])
    );
}

#[test]
fn archlist_stacks_across_profile_levels_with_removal_semantics() {
    // base: arch.list declares "amd64" and "x86". leaf (its own
    // parent -> base): arch.list removes "x86" (a "-x86" line) and
    // adds "arm64" -- proving the same chain-order, "-entry"-removal
    // stacking use.mask/use.force already get (real config.py's own
    // stack_lists(archlist, incremental=1)).
    let root = TempDir::new("portage-profile-test-archlist").keep();
    let repo = root.join("repo");
    let repo_profiles = repo.join("profiles");
    let base = repo_profiles.join("base");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&leaf).unwrap();

    fs::write(base.join("arch.list"), "amd64\nx86\n").unwrap();
    fs::write(leaf.join("parent"), "../repo/profiles/base\n").unwrap();
    fs::write(leaf.join("arch.list"), "-x86\narm64\n").unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    assert_eq!(
        config.archlist,
        HashSet::from(["amd64".to_string(), "arm64".to_string()])
    );
}

#[test]
fn use_expand_variable_names_stack_incrementally_across_profile_levels() {
    // base declares USE_EXPAND="VIDEO_CARDS" and VIDEO_CARDS="nvidia";
    // leaf (its own parent -> base) declares USE_EXPAND="PYTHON_TARGETS"
    // (incremental add, not a replace -- both variable names must end
    // up recognized) and PYTHON_TARGETS="python3_11". Each variable's
    // own value is set at only one level, proving expansion works
    // regardless of which level actually declared USE_EXPAND for it.
    let root = TempDir::new("portage-profile-test-use-expand-names-stack").keep();
    let repo = root.join("repo");
    let repo_profiles = repo.join("profiles");
    let base = repo_profiles.join("base");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&leaf).unwrap();

    fs::write(
        base.join("make.defaults"),
        "USE_EXPAND=\"VIDEO_CARDS\"\nVIDEO_CARDS=\"nvidia\"\n",
    )
    .unwrap();
    fs::write(leaf.join("parent"), "../repo/profiles/base\n").unwrap();
    fs::write(
        leaf.join("make.defaults"),
        "USE_EXPAND=\"PYTHON_TARGETS\"\nPYTHON_TARGETS=\"python3_11\"\n",
    )
    .unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    assert_eq!(
        config.use_expand,
        HashSet::from(["VIDEO_CARDS".to_string(), "PYTHON_TARGETS".to_string()])
    );
    assert!(config.use_flags.contains("video_cards_nvidia"));
    assert!(config.use_flags.contains("python_targets_python3_11"));
}

#[test]
fn use_expand_variable_value_expands_with_negation_and_plus_stripped() {
    // A single USE_EXPAND variable's own value exercises all three
    // ordinary incremental token forms in one pass (real config.py's
    // own early-expand loop keeps a "-" token's own "-" outside the
    // new prefix, and a "+" token has it stripped, same as any
    // ordinary USE token): "nvidia" adds, "+intel" adds (with the
    // "+" stripped, not baked into the flag name), "-nvidia" then
    // removes the flag "nvidia" itself already added earlier in the
    // very same value list -- final: only video_cards_intel remains.
    let root = TempDir::new("portage-profile-test-use-expand-negation").keep();
    let repo = root.join("repo");
    let repo_profiles = repo.join("profiles");
    let base = repo_profiles.join("base");
    fs::create_dir_all(&base).unwrap();

    fs::write(
        base.join("make.defaults"),
        "USE_EXPAND=\"VIDEO_CARDS\"\nVIDEO_CARDS=\"nvidia +intel -nvidia\"\n",
    )
    .unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&base, &make_profile).unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    assert!(!config.use_flags.contains("video_cards_nvidia"));
    assert!(config.use_flags.contains("video_cards_intel"));
    assert!(!config.use_flags.contains("+video_cards_intel"));
}

#[test]
fn profile_make_defaults_use_expand_is_folded_per_level_before_package_use() {
    // Real config.py 2849-2889: a USE_EXPAND value set in a profile
    // level's make.defaults is translated to its equivalent USE flag
    // *in that level's* make_defaults_use, prepended before the raw
    // `USE=`, so the same level's package.use can still cancel it --
    // and it is NOT also folded into the high-priority conf tier
    // (real skips configdict["defaults"] in its own re-fold, 2964).
    // The neovim `^^ ( lua_single_target_* )` case in miniature.
    let root = TempDir::new("portage-profile-test-use-expand-per-level-fold").keep();
    let repo = root.join("repo");
    let base = repo.join("profiles/base");
    fs::create_dir_all(&base).unwrap();
    fs::write(
        base.join("make.defaults"),
        "USE_EXPAND=\"LUA_SINGLE_TARGET\"\nLUA_SINGLE_TARGET=\"lua5-1\"\nUSE=\"rawflag\"\n",
    )
    .unwrap();
    fs::write(
        base.join("package.use"),
        "dev-libs/pkg lua_single_target_luajit -lua_single_target_lua5-1\n",
    )
    .unwrap();
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&base, &make_profile).unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    let layer = &config.profile_use_layers[0];
    // folded flag lands in this level's make_defaults_use, prepended.
    assert_eq!(
        layer.make_defaults_use.first().map(String::as_str),
        Some("lua_single_target_lua5-1")
    );
    assert!(layer.make_defaults_use.iter().any(|t| t == "rawflag"));
    // ...and NOT in the conf tier, where it would out-rank package.use.
    assert!(
        !config
            .conf_use_tokens
            .iter()
            .any(|t| t.contains("lua_single_target")),
        "conf_use_tokens = {:?}",
        config.conf_use_tokens
    );
    // the same level's package.use override is still present.
    assert!(
        layer
            .package_use
            .iter()
            .any(|(atom, toks)| atom == "dev-libs/pkg"
                && toks.iter().any(|t| t == "lua_single_target_luajit"))
    );
}

#[test]
fn use_expand_value_set_at_two_profile_levels_stacks_incrementally() {
    // Real Gentoo: profiles/default/linux/make.defaults sets
    // VIDEO_CARDS="dummy fbdev"; profiles/arch/amd64/make.defaults sets
    // VIDEO_CARDS="amdgpu fbdev intel ...". VIDEO_CARDS is not in
    // INCREMENTALS, but real config.py folds *each* level's own value
    // into that level's USE (2853-2866), and USE is incremental -- so
    // the later level does NOT wipe `dummy`; the union stands. A
    // genuine `-video_cards_*` in a later level still cancels.
    let root = TempDir::new("portage-profile-test-use-expand-two-level-stack").keep();
    let repo = root.join("repo");
    let base = repo.join("profiles/base");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&leaf).unwrap();
    fs::write(
        base.join("make.defaults"),
        "USE_EXPAND=\"VIDEO_CARDS\"\nVIDEO_CARDS=\"dummy fbdev radeon\"\n",
    )
    .unwrap();
    fs::write(leaf.join("parent"), "../repo/profiles/base\n").unwrap();
    fs::write(
        leaf.join("make.defaults"),
        "VIDEO_CARDS=\"amdgpu fbdev vesa -radeon\"\n",
    )
    .unwrap();
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    // base-only value survives the leaf's own VIDEO_CARDS assignment.
    assert!(config.use_flags.contains("video_cards_dummy"));
    // both levels' additive values are present.
    assert!(config.use_flags.contains("video_cards_amdgpu"));
    assert!(config.use_flags.contains("video_cards_fbdev"));
    assert!(config.use_flags.contains("video_cards_vesa"));
    // a real `-` in the later level still cancels the earlier value.
    assert!(!config.use_flags.contains("video_cards_radeon"));
}

#[test]
fn use_expand_prefix_wildcard_cancels_parent_level_values() {
    // #67 S2 regression pin: a leaf level's `VIDEO_CARDS="-* intel"`
    // must cancel the parent levels' `video_cards_*` flags (real
    // config.py:2925's `-<prefix>_*` removal) while keeping the
    // leaf's own `intel`. Pre-fix the expanded token was
    // `-video_cards_*`, which apply_incremental treated as an exact
    // removal of a flag literally named `video_cards_*` -- a no-op --
    // so the parent's cards survived.
    let root = TempDir::new("portage-profile-test-use-expand-prefix-wildcard").keep();
    let repo = root.join("repo");
    let base = repo.join("profiles/base");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&leaf).unwrap();
    fs::write(
        base.join("make.defaults"),
        "USE_EXPAND=\"VIDEO_CARDS\"\nVIDEO_CARDS=\"dummy fbdev intel\"\n",
    )
    .unwrap();
    fs::write(leaf.join("parent"), "../repo/profiles/base\n").unwrap();
    fs::write(leaf.join("make.defaults"), "VIDEO_CARDS=\"-* intel\"\n").unwrap();
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    assert!(config.use_flags.contains("video_cards_intel"));
    assert!(
        !config.use_flags.contains("video_cards_dummy"),
        "the leaf `-*` must cancel the parent's dummy: {:?}",
        config.use_flags
    );
    assert!(!config.use_flags.contains("video_cards_fbdev"));
}

#[test]
fn use_expand_unprefixed_variable_contributes_a_bare_flag_no_prefix() {
    // Real Gentoo's own USE_EXPAND_UNPREFIXED="ARCH" mechanism:
    // ARCH="amd64" must contribute the bare flag "amd64" directly,
    // NOT "arch_amd64" -- the defining difference from an ordinary
    // USE_EXPAND variable, which use_expand_variable_names_stack_
    // incrementally_across_profile_levels above already covers.
    let root = TempDir::new("portage-profile-test-use-expand-unprefixed").keep();
    let repo = root.join("repo");
    let repo_profiles = repo.join("profiles");
    let base = repo_profiles.join("base");
    fs::create_dir_all(&base).unwrap();

    fs::write(
        base.join("make.defaults"),
        "USE_EXPAND_UNPREFIXED=\"ARCH\"\nARCH=\"amd64\"\n",
    )
    .unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&base, &make_profile).unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    assert_eq!(
        config.use_expand_unprefixed,
        HashSet::from(["ARCH".to_string()])
    );
    assert!(config.use_flags.contains("amd64"));
    assert!(!config.use_flags.contains("arch_amd64"));
}

#[test]
fn use_expand_unprefixed_variable_value_expands_with_negation_and_plus_stripped() {
    // Same three ordinary incremental token forms
    // use_expand_variable_value_expands_with_negation_and_plus_stripped
    // above already covers for a prefixed USE_EXPAND variable, but
    // for an unprefixed one: "foo" adds, "+bar" adds ("+" stripped),
    // "-foo" then removes "foo" itself, added earlier in the same
    // value list -- final: only "bar" remains, no prefix on either.
    let root = TempDir::new("portage-profile-test-use-expand-unprefixed-negation").keep();
    let repo = root.join("repo");
    let repo_profiles = repo.join("profiles");
    let base = repo_profiles.join("base");
    fs::create_dir_all(&base).unwrap();

    fs::write(
        base.join("make.defaults"),
        "USE_EXPAND_UNPREFIXED=\"ARCH\"\nARCH=\"foo +bar -foo\"\n",
    )
    .unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&base, &make_profile).unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    assert!(!config.use_flags.contains("foo"));
    assert!(config.use_flags.contains("bar"));
    assert!(!config.use_flags.contains("+bar"));
}

#[test]
fn package_accept_keywords_stacks_profile_chain_then_user_no_repo_source() {
    // Profile-level entry for "a", user-level entry for "b" -- both
    // must appear, in that order (profile-chain first, matching real
    // KeywordsManager.getPKeywords), proving there's no repo-level
    // source at all (only portuale's existing repo/profiles/package.mask
    // convention would exist if there were one, and this test
    // deliberately never creates that file).
    let root = TempDir::new("portage-profile-test-accept-keywords-stack").keep();
    let repo = root.join("repo");
    let portage_dir = root.join("etc/portage");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&portage_dir).unwrap();
    fs::create_dir_all(&leaf).unwrap();

    fs::write(leaf.join("package.accept_keywords"), "dev-libs/a ~amd64\n").unwrap();
    fs::write(
        portage_dir.join("package.accept_keywords"),
        "dev-libs/b ~amd64\ndev-libs/bare-no-op\n",
    )
    .unwrap();

    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    assert_eq!(
        config.package_accept_keywords,
        vec![
            ("dev-libs/a".to_string(), vec!["~amd64".to_string()]),
            ("dev-libs/b".to_string(), vec!["~amd64".to_string()]),
            // No global ACCEPT_KEYWORDS is set anywhere in this
            // fixture, so the bare atom's own accept_keywords_defaults
            // substitution has nothing to derive from and it stays
            // present with an empty token list.
            ("dev-libs/bare-no-op".to_string(), Vec::new()),
        ]
    );
}

#[test]
fn accept_keywords_defaults_substitutes_tilde_arch_for_a_bare_atom() {
    // Real accept_keywords_defaults: a bare package.accept_keywords
    // atom means "~" + every plain token in the *current* global
    // ACCEPT_KEYWORDS -- here just "amd64", so "dev-libs/foo" gets
    // an implicit "~amd64", the same as if the user had written
    // "dev-libs/foo ~amd64" themselves.
    let root = TempDir::new("portage-profile-test-accept-keywords-defaults").keep();
    let repo = root.join("repo");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&leaf).unwrap();
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();

    fs::write(leaf.join("make.defaults"), "ACCEPT_KEYWORDS=\"amd64\"\n").unwrap();
    fs::write(
        portage_dir.join("package.accept_keywords"),
        "dev-libs/foo\n",
    )
    .unwrap();

    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    assert_eq!(
        config.package_accept_keywords,
        vec![("dev-libs/foo".to_string(), vec!["~amd64".to_string()])]
    );
}

#[test]
fn accept_keywords_defaults_excludes_already_prefixed_tokens() {
    // Real accept_keywords_defaults only derives from *plain* global
    // ACCEPT_KEYWORDS tokens (keyword[:1] not in "~-") -- an
    // already-"~"-prefixed or "-"-prefixed global token is excluded
    // entirely, not doubly-prefixed or left bare.
    let root = TempDir::new("portage-profile-test-accept-keywords-defaults-filter").keep();
    let repo = root.join("repo");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&leaf).unwrap();
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();

    fs::write(
        leaf.join("make.defaults"),
        "ACCEPT_KEYWORDS=\"amd64 ~x86 -arm\"\n",
    )
    .unwrap();
    fs::write(
        portage_dir.join("package.accept_keywords"),
        "dev-libs/foo\n",
    )
    .unwrap();

    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    assert_eq!(
        config.package_accept_keywords,
        vec![("dev-libs/foo".to_string(), vec!["~amd64".to_string()])]
    );
}

#[test]
fn package_use_splits_repo_profile_and_user_into_their_own_fields() {
    // Repo-level entry for "a", profile-level entry for "b",
    // user-level entry for "c" -- each lands in its own `Config`
    // field at its own real USE_ORDER position (the "Config depth"
    // slice), proving no source is silently dropped and no `-atom`
    // removal happens anywhere (package.use is purely additive,
    // unlike package.mask).
    let root = TempDir::new("portage-profile-test-package-use-stack").keep();
    let repo = root.join("repo");
    let repo_profiles = repo.join("profiles");
    let portage_dir = root.join("etc/portage");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&repo_profiles).unwrap();
    fs::create_dir_all(&portage_dir).unwrap();
    fs::create_dir_all(&leaf).unwrap();

    fs::write(repo_profiles.join("package.use"), "dev-libs/a flaga\n").unwrap();
    fs::write(leaf.join("package.use"), "dev-libs/b flagb\n").unwrap();
    fs::write(portage_dir.join("package.use"), "dev-libs/c flagc\n").unwrap();

    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    assert_eq!(
        config.package_use_repo,
        vec![("dev-libs/a".to_string(), vec!["flaga".to_string()])]
    );
    assert_eq!(
        config.package_use,
        vec![("dev-libs/b".to_string(), vec!["flagb".to_string()])]
    );
    assert_eq!(
        config.package_use_user,
        vec![("dev-libs/c".to_string(), vec!["flagc".to_string()])]
    );
}

#[test]
fn env_layer_overrides_config_vars() {
    // A minimal profile: ARCH/ACCEPT_KEYWORDS + a USE_EXPAND var, so
    // the env layer's incremental (ACCEPT_KEYWORDS, USE), last-wins
    // (a USE_EXPAND value + a plain scalar) and USE_EXPAND-name paths
    // are all exercised.
    let root = TempDir::new("portage-profile-test-env-layer").keep();
    let repo = root.join("repo");
    let prof = repo.join("profiles/default");
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&prof).unwrap();
    fs::create_dir_all(&portage_dir).unwrap();
    fs::write(
        prof.join("make.defaults"),
        "ARCH=\"amd64\"\nACCEPT_KEYWORDS=\"${ARCH}\"\n\
         USE_EXPAND_UNPREFIXED=\"ARCH\"\nUSE=\"baseflag\"\n\
         USE_EXPAND=\"VIDEO_CARDS\"\nVIDEO_CARDS=\"nvidia\"\n",
    )
    .unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&prof, &make_profile).unwrap();

    // No env: baseline.
    with_test_env(&[], || {
        let c = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
            .expect("resolves");
        assert!(c.accept_keywords.contains("amd64"));
        assert!(!c.accept_keywords.contains("~amd64"));
        assert!(c.use_flags.contains("video_cards_nvidia"));
        assert!(!c.other_vars.contains_key("CFLAGS"));
    });

    with_test_env(
        &[
            ("ACCEPT_KEYWORDS", "~amd64"),
            ("USE", "-baseflag envflag"),
            ("VIDEO_CARDS", "amdgpu"),
            ("CFLAGS", "-O3"),
        ],
        || {
            let c = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
                .expect("resolves");
            // ACCEPT_KEYWORDS: incremental -- profile amd64 kept, env ~amd64 added.
            assert!(c.accept_keywords.contains("amd64"));
            assert!(c.accept_keywords.contains("~amd64"));
            // USE: env stacks on the conf layer.
            assert!(c.use_flags.contains("envflag"));
            assert!(!c.use_flags.contains("baseflag"));
            // USE_EXPAND value: env replaces the profile value.
            assert!(c.use_flags.contains("video_cards_amdgpu"));
            assert!(!c.use_flags.contains("video_cards_nvidia"));
            // Plain scalar -> other_vars (emerge --info).
            assert_eq!(c.other_vars.get("CFLAGS").map(String::as_str), Some("-O3"));
        },
    );
}

#[test]
fn env_layer_stacks_onto_the_config_protect_and_env_unset_folds() {
    // Backlog #219 fix round 1: `CONFIG_PROTECT`,
    // `CONFIG_PROTECT_MASK` and `ENV_UNSET` are in real
    // `const.INCREMENTALS` (`const.py:125-138`), so real
    // `config.regenerate()` stacks the calling env as the final
    // source (`mydbs = configlist[:-1] + [backupenv]`,
    // `config.py:2735-2736`) -- like `FEATURES`, not last-wins.
    // `emerge --info`'s `settings.get(k)` then prints the stacked,
    // sorted union.
    let root = TempDir::new("portage-profile-test-info-incrementals").keep();
    let repo = root.join("repo");
    let prof = repo.join("profiles/default");
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&prof).unwrap();
    fs::create_dir_all(&portage_dir).unwrap();
    fs::write(
        prof.join("make.defaults"),
        "ARCH=\"amd64\"\nACCEPT_KEYWORDS=\"${ARCH}\"\n",
    )
    .unwrap();
    fs::write(
        portage_dir.join("make.conf"),
        "CONFIG_PROTECT=\"/conf-protect\"\n\
         CONFIG_PROTECT_MASK=\"/conf-mask\"\n\
         ENV_UNSET=\"CONF_UNSET\"\n",
    )
    .unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&prof, &make_profile).unwrap();
    let resolve = || {
        resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
            .expect("resolves")
    };

    // No env: the `make.conf` layer alone.
    with_test_env(&[], || {
        let c = resolve();
        assert_eq!(
            c.resolved_incremental("CONFIG_PROTECT"),
            Some(vec!["/conf-protect".to_string()])
        );
        assert_eq!(
            c.resolved_incremental("ENV_UNSET"),
            Some(vec!["CONF_UNSET".to_string()])
        );
    });

    // Calling env stacks onto the file fold (union, sorted --
    // including `-tok` removal, as in real's fold).
    with_test_env(
        &[
            ("CONFIG_PROTECT", "/env-protect"),
            ("CONFIG_PROTECT_MASK", "-/conf-mask /env-mask"),
            ("ENV_UNSET", "ENV_UNSET_X"),
        ],
        || {
            let c = resolve();
            assert_eq!(
                c.resolved_incremental("CONFIG_PROTECT"),
                Some(vec![
                    "/conf-protect".to_string(),
                    "/env-protect".to_string()
                ])
            );
            assert_eq!(
                c.resolved_incremental("CONFIG_PROTECT_MASK"),
                Some(vec!["/env-mask".to_string()])
            );
            assert_eq!(
                c.resolved_incremental("ENV_UNSET"),
                Some(vec!["CONF_UNSET".to_string(), "ENV_UNSET_X".to_string()])
            );
        },
    );
}

#[test]
fn user_package_use_star_atom_folds_onto_the_global_use() {
    // Real UseManager.extract_global_USE_changes: a "*/*" entry in the
    // USER package.use is popped out of the per-package dict and
    // stacked onto the global USE like make.conf USE= would --
    // USE_EXPAND-prefix shorthand included.
    let root = TempDir::new("portage-profile-test-global-puse").keep();
    let repo = root.join("repo");
    let prof = repo.join("profiles/default");
    let portage_dir = root.join("etc/portage/package.use");
    fs::create_dir_all(&prof).unwrap();
    fs::create_dir_all(&portage_dir).unwrap();
    fs::write(
        prof.join("make.defaults"),
        "ARCH=\"amd64\"\nUSE_EXPAND_UNPREFIXED=\"ARCH\"\n\
         USE_EXPAND=\"VIDEO_CARDS\"\nVIDEO_CARDS=\"nvidia\"\n\
         USE=\"profileflag\"\n",
    )
    .unwrap();
    let make_profile = root.join("etc/portage/make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&prof, &make_profile).unwrap();
    fs::write(
        portage_dir.join("00-global"),
        "*/* globalflag -profileflag\n*/* VIDEO_CARDS: -nvidia nouveau\ndev-libs/x perpkgflag\n",
    )
    .unwrap();

    with_test_env(&[], || {
        let c = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
            .expect("resolves");
        assert!(c.use_flags.contains("globalflag"));
        assert!(!c.use_flags.contains("profileflag"));
        assert!(c.use_flags.contains("video_cards_nouveau"));
        assert!(!c.use_flags.contains("video_cards_nvidia"));
        // The "*/*" entry is popped; only the real per-package one stays.
        assert!(!c.package_use_user.iter().any(|(a, _)| a == "*/*"));
        assert!(c.package_use_user.iter().any(|(a, _)| a == "dev-libs/x"));
    });
}

#[test]
fn profile_use_layers_are_built_one_per_chain_level() {
    let root = TempDir::new("portage-profile-test-profile-use-layers").keep();
    let repo = root.join("repo");
    let base = repo.join("profiles/base");
    let leaf = repo.join("profiles/leaf");
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&leaf).unwrap();
    fs::create_dir_all(&portage_dir).unwrap();
    fs::write(base.join("make.defaults"), "USE=\"a b\"\n").unwrap();
    fs::write(base.join("package.use"), "dev-libs/x flag\n").unwrap();
    fs::write(leaf.join("parent"), "../base\n").unwrap();
    fs::write(leaf.join("make.defaults"), "USE=\"-b c\"\n").unwrap();
    // leaf has no package.use.
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    assert_eq!(
        config.profile_use_layers,
        vec![
            ProfileUseLayer {
                make_defaults_use: vec!["a b".to_string()],
                package_use: vec![("dev-libs/x".to_string(), vec!["flag".to_string()])],
            },
            ProfileUseLayer {
                make_defaults_use: vec!["-b c".to_string()],
                package_use: vec![],
            },
        ]
    );
    // The flat `use_tokens` still carries every level's USE= in order.
    assert_eq!(
        config.use_tokens,
        vec!["a b".to_string(), "-b c".to_string()]
    );
}

#[test]
fn repo_make_defaults_use_is_read_per_repo_with_var_expansion() {
    let root = TempDir::new("portage-profile-test-repo-make-defaults").keep();
    let repo = root.join("repo");
    let overlay = root.join("overlay");
    let prof = repo.join("profiles/default");
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&prof).unwrap();
    fs::create_dir_all(overlay.join("profiles")).unwrap();
    fs::create_dir_all(&portage_dir).unwrap();
    fs::write(
        prof.join("make.defaults"),
        "ARCH=\"amd64\"\nACCEPT_KEYWORDS=\"${ARCH}\"\nUSE_EXPAND_UNPREFIXED=\"ARCH\"\n",
    )
    .unwrap();
    // The main repo's top-level profiles/make.defaults.
    fs::write(
        repo.join("profiles/make.defaults"),
        "USE=\"repoflag\"\nUSE=\"more arch_${ARCH}\"\n",
    )
    .unwrap();
    // An overlay's own -- keyed by its repo name.
    fs::write(
        overlay.join("profiles/make.defaults"),
        "USE=\"overlayflag\"\n",
    )
    .unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&prof, &make_profile).unwrap();

    let config = resolve_config(
        &root,
        &repo,
        &[("myoverlay".to_string(), overlay.to_path_buf())],
        &[],
        "testrepo",
        &HashMap::new(),
        &root,
    )
    .expect("config must resolve");
    assert_eq!(
        config.repo_make_defaults_use,
        vec![
            (
                String::new(),
                vec!["repoflag".to_string(), "more arch_amd64".to_string()]
            ),
            ("myoverlay".to_string(), vec!["overlayflag".to_string()]),
        ]
    );
}

#[test]
fn envd_use_tokens_are_read_from_profile_env() {
    let root = TempDir::new("portage-profile-test-envd").keep();
    let repo = root.join("repo");
    let etc = root.join("etc");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(&etc).unwrap();
    fs::write(repo.join("profiles/repo_name"), "testrepo\n").unwrap();

    // real `env-update` output shape: `export KEY='value'` lines, a
    // plain assignment, and unrelated keys that must be ignored.
    fs::write(
        etc.join("profile.env"),
        "# generated\n\
         export PATH='/usr/bin'\n\
         export USE='envdflag -other'\n\
         LDPATH=/usr/lib\n",
    )
    .unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    assert_eq!(config.envd_use_tokens, vec!["envdflag -other".to_string()]);

    // No profile.env -> empty.
    fs::remove_file(etc.join("profile.env")).unwrap();
    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    assert!(config.envd_use_tokens.is_empty());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn package_env_resolves_env_file_use_including_source_and_missing_files() {
    let root = TempDir::new("portage-profile-test-package-env").keep();
    let repo = root.join("repo");
    let portage_dir = root.join("etc/portage");
    let env_dir = portage_dir.join("env");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(&env_dir).unwrap();

    fs::write(
        portage_dir.join("package.env"),
        "dev-libs/a debug shared\n\
         dev-libs/b gone\n\
         dev-libs/c\n",
    )
    .unwrap();
    // `debug` sources `common` (relative) which sets one flag; `debug`
    // itself sets another and a non-USE var (ignored by this slice).
    fs::write(
        env_dir.join("debug"),
        "source common\nUSE=\"dbgflag\"\nCFLAGS=\"-O0 -g\"\n",
    )
    .unwrap();
    fs::write(env_dir.join("common"), "USE=\"commonflag\"\n").unwrap();
    fs::write(env_dir.join("shared"), "USE=\"sharedflag -commonflag\"\n").unwrap();
    // `gone` is referenced but doesn't exist -> contributes nothing.

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");

    assert_eq!(
        config.package_env,
        vec![
            (
                "dev-libs/a".to_string(),
                vec!["debug".to_string(), "shared".to_string()]
            ),
            ("dev-libs/b".to_string(), vec!["gone".to_string()]),
        ]
    );
    // dev-libs/c (no file names) is dropped entirely.
    assert_eq!(
        config.package_env_use,
        vec![(
            "dev-libs/a".to_string(),
            // `debug` -> source common ("commonflag"), then "dbgflag";
            // then `shared` -> "sharedflag", "-commonflag". Order and
            // negation are preserved for `apply_incremental`.
            vec![
                "commonflag".to_string(),
                "dbgflag".to_string(),
                "sharedflag".to_string(),
                "-commonflag".to_string(),
            ]
        )]
    );
    // The non-`USE` scalar half: `debug`'s CFLAGS only (`shared` sets
    // no scalar, `common` none, `gone`/`c` contribute nothing).
    assert_eq!(
        config.package_env_vars,
        vec![(
            "dev-libs/a".to_string(),
            vec![("CFLAGS".to_string(), "-O0 -g".to_string())]
        )]
    );
}

#[test]
fn package_env_expands_dollar_vars_against_the_global_map_with_chaining() {
    // Real `_grab_pkg_env`'s per-package expand map: `${SHARED}` from
    // `make.conf` (already in the global map), `INNER` chained from
    // the same file's own earlier line, and `ACROSS` chained from an
    // earlier file of the same entry. The scalar half expands the
    // same way.
    let root = TempDir::new("portage-profile-test-package-env-expand").keep();
    let repo = root.join("repo");
    let portage_dir = root.join("etc/portage");
    let env_dir = portage_dir.join("env");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(&env_dir).unwrap();
    fs::write(portage_dir.join("make.conf"), "SHARED=\"sharedflag\"\n").unwrap();
    fs::write(portage_dir.join("package.env"), "dev-libs/a first second\n").unwrap();
    fs::write(
        env_dir.join("first"),
        "INNER=\"innerflag\"\nUSE=\"${SHARED} ${INNER}\"\n",
    )
    .unwrap();
    fs::write(
        env_dir.join("second"),
        "USE=\"across-${INNER}\"\nCFLAGS=\"-O2 ${SHARED}\"\n",
    )
    .unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    assert_eq!(
        config.package_env_use,
        vec![(
            "dev-libs/a".to_string(),
            vec![
                "sharedflag".to_string(),
                "innerflag".to_string(),
                "across-innerflag".to_string(),
            ]
        )]
    );
    assert_eq!(
        config.package_env_vars,
        vec![(
            "dev-libs/a".to_string(),
            vec![
                ("INNER".to_string(), "innerflag".to_string()),
                ("CFLAGS".to_string(), "-O2 sharedflag".to_string()),
            ]
        )]
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn package_use_expand_shorthand_applies_prefix_to_following_tokens_on_the_same_line() {
    // User-level "dev-libs/a VIDEO_CARDS: nvidia -intel plainflag" --
    // "nvidia"/"intel" get the video_cards_ prefix (negation kept
    // outside it), "plainflag" (before the "VIDEO_CARDS:" marker)
    // does not.
    let root = TempDir::new("portage-profile-test-package-use-expand-shorthand").keep();
    let repo = root.join("repo");
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(&portage_dir).unwrap();

    fs::write(
        portage_dir.join("package.use"),
        "dev-libs/a plainflag VIDEO_CARDS: nvidia -intel\n",
    )
    .unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    assert_eq!(
        config.package_use_user,
        vec![(
            "dev-libs/a".to_string(),
            vec![
                "plainflag".to_string(),
                "video_cards_nvidia".to_string(),
                "-video_cards_intel".to_string(),
            ]
        )]
    );
}

#[test]
fn package_use_expand_shorthand_resets_at_the_start_of_each_line() {
    // Two lines for the SAME atom: the first sets a shorthand prefix
    // that must not leak into the second line's own tokens.
    let root = TempDir::new("portage-profile-test-package-use-expand-reset").keep();
    let repo = root.join("repo");
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(&portage_dir).unwrap();

    fs::write(
        portage_dir.join("package.use"),
        "dev-libs/a VIDEO_CARDS: nvidia\ndev-libs/a plainflag\n",
    )
    .unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    assert_eq!(
        config.package_use_user,
        vec![
            (
                "dev-libs/a".to_string(),
                vec!["video_cards_nvidia".to_string()]
            ),
            ("dev-libs/a".to_string(), vec!["plainflag".to_string()]),
        ]
    );
}

#[test]
fn package_use_expand_shorthand_is_user_level_only() {
    // The same "VIDEO_CARDS:" syntax in a repo-level or profile-level
    // package.use file is genuine real behavior's own literal,
    // unexpanded token -- real portage's shorthand support is
    // user-only (see parse_package_use_lines's own doc comment).
    let root = TempDir::new("portage-profile-test-package-use-expand-user-only").keep();
    let repo = root.join("repo");
    let repo_profiles = repo.join("profiles");
    fs::create_dir_all(&repo_profiles).unwrap();
    fs::create_dir_all(root.join("etc/portage")).unwrap();

    fs::write(
        repo_profiles.join("package.use"),
        "dev-libs/a VIDEO_CARDS: nvidia\n",
    )
    .unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    assert_eq!(
        config.package_use_repo,
        vec![(
            "dev-libs/a".to_string(),
            vec!["VIDEO_CARDS:".to_string(), "nvidia".to_string()]
        )]
    );
}

#[test]
fn package_use_mask_and_force_stack_repo_then_profile_only_no_user_source() {
    // Repo-level entry for "a", profile-level entry for "b" -- both
    // must appear, in that order; a user-level package.use.mask/
    // .force file is deliberately written too, and must be
    // completely ignored -- real portage has no such source at all
    // (confirmed by reading UseManager.__init__'s own file/variable
    // table), unlike package.use itself.
    let root = TempDir::new("portage-profile-test-package-use-mask-force").keep();
    let repo = root.join("repo");
    let repo_profiles = repo.join("profiles");
    let portage_dir = root.join("etc/portage");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&repo_profiles).unwrap();
    fs::create_dir_all(&portage_dir).unwrap();
    fs::create_dir_all(&leaf).unwrap();

    fs::write(repo_profiles.join("package.use.mask"), "dev-libs/a maska\n").unwrap();
    fs::write(leaf.join("package.use.mask"), "dev-libs/b maskb\n").unwrap();
    fs::write(portage_dir.join("package.use.mask"), "dev-libs/c maskc\n").unwrap();
    fs::write(
        repo_profiles.join("package.use.force"),
        "dev-libs/a forcea\n",
    )
    .unwrap();
    fs::write(leaf.join("package.use.force"), "dev-libs/b forceb\n").unwrap();
    fs::write(portage_dir.join("package.use.force"), "dev-libs/c forcec\n").unwrap();

    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    assert_eq!(
        config.package_use_mask,
        vec![
            ("dev-libs/a".to_string(), vec!["maska".to_string()]),
            ("dev-libs/b".to_string(), vec!["maskb".to_string()]),
        ]
    );
    assert_eq!(
        config.package_use_force,
        vec![
            ("dev-libs/a".to_string(), vec!["forcea".to_string()]),
            ("dev-libs/b".to_string(), vec!["forceb".to_string()]),
        ]
    );
}

#[test]
fn use_stable_mask_and_force_stack_profile_chain_only_and_stay_out_of_use_flags() {
    // use.stable.mask/.force: profile-chain only (no repo-level
    // source, matching use_mask/use_force's own existing profile-
    // chain-only sourcing) -- and, unlike use_mask/use_force,
    // deliberately never folded into use_flags itself (see
    // use_stable_force's own doc comment: stability is per-candidate,
    // so portage-repo applies these conditionally instead).
    let root = TempDir::new("portage-profile-test-use-stable-mask-force").keep();
    let repo = root.join("repo");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(&leaf).unwrap();

    fs::write(leaf.join("use.stable.mask"), "stablemaskflag\n").unwrap();
    fs::write(leaf.join("use.stable.force"), "stableforceflag\n").unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    assert_eq!(
        config.use_stable_mask,
        HashSet::from(["stablemaskflag".to_string()])
    );
    assert_eq!(
        config.use_stable_force,
        HashSet::from(["stableforceflag".to_string()])
    );
    assert!(!config.use_flags.contains("stablemaskflag"));
    assert!(!config.use_flags.contains("stableforceflag"));
}

#[test]
fn package_use_stable_mask_and_force_stack_repo_then_profile_only_no_user_source() {
    // Mirrors package_use_mask_and_force_stack_repo_then_profile_only_no_user_source
    // exactly, for the stable variant: repo-level entry for "a",
    // profile-level entry for "b", a deliberately-written user-level
    // file completely ignored (no such source exists in real
    // portage).
    let root = TempDir::new("portage-profile-test-package-use-stable-mask-force").keep();
    let repo = root.join("repo");
    let repo_profiles = repo.join("profiles");
    let portage_dir = root.join("etc/portage");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&repo_profiles).unwrap();
    fs::create_dir_all(&portage_dir).unwrap();
    fs::create_dir_all(&leaf).unwrap();

    fs::write(
        repo_profiles.join("package.use.stable.mask"),
        "dev-libs/a maska\n",
    )
    .unwrap();
    fs::write(leaf.join("package.use.stable.mask"), "dev-libs/b maskb\n").unwrap();
    fs::write(
        portage_dir.join("package.use.stable.mask"),
        "dev-libs/c maskc\n",
    )
    .unwrap();
    fs::write(
        repo_profiles.join("package.use.stable.force"),
        "dev-libs/a forcea\n",
    )
    .unwrap();
    fs::write(leaf.join("package.use.stable.force"), "dev-libs/b forceb\n").unwrap();
    fs::write(
        portage_dir.join("package.use.stable.force"),
        "dev-libs/c forcec\n",
    )
    .unwrap();

    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

    let config = resolve_config(&root, &repo, &[], &[], "testrepo", &HashMap::new(), &root)
        .expect("config must resolve");
    assert_eq!(
        config.package_use_stable_mask,
        vec![
            ("dev-libs/a".to_string(), vec!["maska".to_string()]),
            ("dev-libs/b".to_string(), vec!["maskb".to_string()]),
        ]
    );
    assert_eq!(
        config.package_use_stable_force,
        vec![
            ("dev-libs/a".to_string(), vec!["forcea".to_string()]),
            ("dev-libs/b".to_string(), vec!["forceb".to_string()]),
        ]
    );
}

#[test]
fn package_use_loads_tokens_and_skips_bare_atom_lines() {
    let root = TempDir::new("portage-profile-test-package-use").keep();
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    fs::write(
        portage_dir.join("package.use"),
        "dev-libs/foo flag1 -flag2\n*/bar +flag3\ndev-libs/bare-no-op\n",
    )
    .unwrap();

    let config = resolve_config(
        &root,
        &root.join("repo"),
        &[],
        &[],
        "testrepo",
        &HashMap::new(),
        &root,
    )
    .expect("config with package.use must resolve");
    assert_eq!(
        config.package_use_user,
        vec![
            (
                "dev-libs/foo".to_string(),
                vec!["flag1".to_string(), "-flag2".to_string()]
            ),
            ("*/bar".to_string(), vec!["+flag3".to_string()]),
        ]
    );
}
