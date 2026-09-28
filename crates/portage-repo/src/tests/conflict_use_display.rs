use super::*;

/// The skipped-update block renders Portage's full `pkg_use_display`: effective
/// USE masked to valid IUSE, one group per visible USE_EXPAND var.
#[test]
fn skipped_use_display_masks_to_the_valid_domain_and_groups_expands() {
    let config = portage_profile::Config {
        use_expand: HashSet::from(["ELIBC".to_string(), "CPU_FLAGS_X86".to_string()]),
        use_expand_hidden: HashSet::from(["CPU_FLAGS_X86".to_string()]),
        iuse_effective: HashSet::from(["elibc_glibc".to_string(), "elibc_musl".to_string()]),
        archlist: HashSet::from(["amd64".to_string(), "x86".to_string()]),
        ..Default::default()
    };
    // EAPI 8: only declared IUSE and IUSE_EFFECTIVE survive.
    let explicit: HashSet<String> = HashSet::from(["alpha".to_string(), "beta".to_string()]);
    let use_flags: HashSet<String> = HashSet::from([
        "alpha".to_string(),
        "foo".to_string(),
        "amd64".to_string(),
        "globalforceflag".to_string(),
        "elibc_glibc".to_string(),
    ]);
    let kept = mask_use_to_valid_domain(
        &explicit,
        &use_flags,
        true,
        &config,
        &HashSet::from(["globalforceflag".to_string()]),
    );
    assert_eq!(
        kept,
        HashSet::from(["alpha".to_string(), "elibc_glibc".to_string()]),
        "IUSE_EFFECTIVE keeps the implicit expand flag only",
    );
    assert_eq!(
        assemble_pkg_use_display(
            &kept,
            &explicit,
            &HashSet::from(["globalforceflag".to_string()]),
            &config,
        ),
        vec![
            ("USE".to_string(), "alpha -beta".to_string()),
            ("ELIBC".to_string(), "glibc".to_string()),
        ],
        "real `emerge --info dev-libs/pkginfopkg` shape: USE=\"alpha -beta\" ELIBC=\"glibc\"",
    );
    // Pre-EAPI-5: ARCH kept-then-discarded, hidden groups hidden, per-package
    // force/mask keeps an undeclared flag (wrapped).
    let mut pre_config = portage_profile::Config {
        use_expand: HashSet::from(["CPU_FLAGS_X86".to_string()]),
        use_expand_hidden: HashSet::from(["CPU_FLAGS_X86".to_string()]),
        archlist: HashSet::from(["amd64".to_string()]),
        ..Default::default()
    };
    pre_config
        .other_vars
        .insert("ARCH".to_string(), "amd64".to_string());
    let kept_pre = mask_use_to_valid_domain(
        &HashSet::new(),
        &use_flags,
        false,
        &pre_config,
        &HashSet::from(["globalforceflag".to_string()]),
    );
    assert_eq!(
        kept_pre,
        HashSet::from(["amd64".to_string(), "globalforceflag".to_string()]),
        "ARCH + useforce survive the pre-EAPI-5 mask; foo/alpha/elibc drop",
    );
    assert_eq!(
        assemble_pkg_use_display(
            &kept_pre,
            &HashSet::new(),
            &HashSet::from(["globalforceflag".to_string()]),
            &pre_config,
        ),
        vec![("USE".to_string(), "(globalforceflag)".to_string())],
        "ARCH discarded, hidden group skipped, forced flag wrapped",
    );
}

/// Within-group order is enabled-first then byte-sorted; groups are USE-first
/// then byte-sorted.
#[test]
fn skipped_use_display_orders_enabled_first_then_byte_sorted() {
    let config = portage_profile::Config {
        use_expand: HashSet::from(["ELIBC".to_string(), "ABI_X86".to_string()]),
        ..Default::default()
    };
    let enabled: HashSet<String> = HashSet::from([
        "zebra".to_string(),
        "apple".to_string(),
        "abi_x86_64".to_string(),
        "elibc_glibc".to_string(),
    ]);
    let explicit: HashSet<String> = HashSet::from([
        "zebra".to_string(),
        "apple".to_string(),
        "mango".to_string(),
        "abi_x86_64".to_string(),
        "abi_x86_32".to_string(),
        "elibc_glibc".to_string(),
        "elibc_musl".to_string(),
    ]);
    assert_eq!(
        assemble_pkg_use_display(&enabled, &explicit, &HashSet::new(), &config),
        vec![
            ("USE".to_string(), "apple zebra -mango".to_string()),
            ("ABI_X86".to_string(), "64 -32".to_string()),
            ("ELIBC".to_string(), "glibc -musl".to_string()),
        ],
    );
}

/// End to end over the fixture tree: a flagless blk0 package renders `USE=""`
/// with the implicit group; a missing version yields no display.
#[test]
fn skipped_update_use_display_for_renders_the_fixture_blk0_shape() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf resolves");
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    assert_eq!(
        skipped_update_use_display_for(&repos, &config, "dev-libs", "blk0x", "3"),
        vec![
            ("USE".to_string(), String::new()),
            ("ELIBC".to_string(), "glibc".to_string()),
        ],
    );
    assert_eq!(
        skipped_update_use_display_for(&repos, &config, "dev-libs", "blk0b", "1"),
        vec![
            ("USE".to_string(), String::new()),
            ("ELIBC".to_string(), "glibc".to_string()),
        ],
    );
    // EAPI 8, no IUSE: only the implicit group survives.
    assert_eq!(
        skipped_update_use_display_for(&repos, &config, "dev-libs", "mgxc", "2"),
        vec![
            ("USE".to_string(), String::new()),
            ("ELIBC".to_string(), "glibc".to_string()),
        ],
    );
    assert!(skipped_update_use_display_for(&repos, &config, "dev-libs", "blk0x", "9").is_empty());
}

/// The slot-conflict display carries the same USE_EXPAND groups (Portage's
/// `slot_collision.py` shares `pkg_use_display` with this block).
#[test]
fn slot_conflict_use_display_renders_use_expand_groups() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf resolves");
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    assert_eq!(
        pkg_use_display_for(&repos, &config, "dev-libs", "aub0d", "0"),
        vec![
            ("USE".to_string(), "foo".to_string()),
            ("ELIBC".to_string(), "glibc".to_string()),
        ],
    );
    assert_eq!(
        pkg_use_display_for(&repos, &config, "dev-libs", "aub0d", "1"),
        vec![
            ("USE".to_string(), "-bar".to_string()),
            ("ELIBC".to_string(), "glibc".to_string()),
        ],
    );
    // EAPI 8, no IUSE: the bare display gains the implicit group.
    assert_eq!(
        pkg_use_display_for(&repos, &config, "dev-libs", "aub0b", "0"),
        vec![
            ("USE".to_string(), String::new()),
            ("ELIBC".to_string(), "glibc".to_string()),
        ],
    );
    assert!(pkg_use_display_for(&repos, &config, "dev-libs", "aub0d", "9").is_empty());
}
