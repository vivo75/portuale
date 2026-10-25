use super::*;

/// The enforced pin's rejection renders as one skipped-update row naming the
/// installed enforcing consumer (vdb USE, `__unknown__` repo fallback).
#[test]
fn enforced_pin_miss_renders_the_installed_enforcing_consumer() {
    let result = graph_result_abk0_worldb(2);
    let versions: Vec<(&str, &str)> = result
        .entries
        .iter()
        .filter_map(|e| merge_bound_version(&e.outcome).map(|v| (e.package.as_str(), v.as_str())))
        .collect();
    assert_eq!(
        versions,
        vec![("abk0c", "1"), ("abk0a", "2"), ("abk0d", "1")],
        "settles C-1 + A-2 + D-1, saw {versions:?}"
    );
    assert_eq!(
        result.backtrack_restarts, 2,
        "two retries mask A-3 away, restarts={}",
        result.backtrack_restarts
    );
    assert_eq!(
        result.skipped_updates.len(),
        1,
        "exactly the rejected A-3 rides out: {:?}",
        result.skipped_updates
    );
    let row = &result.skipped_updates[0];
    assert_eq!(
        (
            row.category.as_str(),
            row.package.as_str(),
            row.slot.as_str()
        ),
        ("dev-libs", "abk0a", "0")
    );
    assert_eq!(row.skipped_version, "3");
    assert_eq!(row.atom, "<dev-libs/abk0a-3");
    assert_eq!(row.consumer_cpv, "dev-libs/abk0b-1:0/0::__unknown__");
    assert!(
        row.consumer_installed,
        "the enforcing consumer renders installed (vdb-based), not merge-scheduled"
    );
    // Empty-valued USE group: the row carries the vdb set, not the tree IUSE (the
    // non-empty shape is pinned by the next test).
    let fixture_root = fixtures_root();
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &fixture_root,
        main_repo_location: &fixture_root.join("repo"),
        overlay_repos: &[("overlay".to_string(), fixture_root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &fixture_root,
    })
    .expect("fixture config resolves");
    let expected =
        skipped_update_installed_use_display_for(&fixture_root, &config, "dev-libs", "abk0b", "1");
    assert_eq!(
        row.consumer_use, expected,
        "vdb USE of the fixture abk0b-1 entry: {:?}",
        row.consumer_use
    );
}

/// Same row with staged vdb files: recorded vdb USE and repo, no fallback.
#[test]
fn enforced_pin_miss_carries_the_vdb_use_and_repo() {
    let fixture_root = fixtures_root();
    let dir = slotundo_temp_dir("205-vdb-consumer");
    for pkg in ["abk0a-1", "abk0b-1", "abk0c-1"] {
        let src = fixture_root.join("var/db/pkg/dev-libs").join(pkg);
        let dst = dir.join("var/db/pkg/dev-libs").join(pkg);
        std::fs::create_dir_all(&dst).expect("stage vdb dir");
        for entry in std::fs::read_dir(&src).expect("fixture vdb dir") {
            let entry = entry.expect("vdb entry");
            std::fs::copy(entry.path(), dst.join(entry.file_name())).expect("stage vdb file");
        }
    }
    let consumer = dir.join("var/db/pkg/dev-libs/abk0b-1");
    std::fs::write(consumer.join("IUSE"), "x y\n").expect("stage IUSE");
    std::fs::write(consumer.join("USE"), "x\n").expect("stage USE");
    std::fs::write(consumer.join("repository"), "testrepo\n").expect("stage repository");
    let result = graph_result_abk0_worldb_on(&dir, 2);
    let versions: Vec<(&str, &str)> = result
        .entries
        .iter()
        .filter_map(|e| merge_bound_version(&e.outcome).map(|v| (e.package.as_str(), v.as_str())))
        .collect();
    assert_eq!(
        versions,
        vec![("abk0c", "1"), ("abk0a", "2"), ("abk0d", "1")],
        "the staged USE/repository files must not move selection, saw {versions:?}"
    );
    assert_eq!(result.skipped_updates.len(), 1);
    let row = &result.skipped_updates[0];
    assert_eq!(row.skipped_version, "3");
    assert_eq!(row.atom, "<dev-libs/abk0a-3");
    assert_eq!(row.consumer_cpv, "dev-libs/abk0b-1:0/0::testrepo");
    assert!(row.consumer_installed);
    // The row carries the staged vdb display, not the tree IUSE (empty there).
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &fixture_root,
        main_repo_location: &fixture_root.join("repo"),
        overlay_repos: &[("overlay".to_string(), fixture_root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &fixture_root,
    })
    .expect("fixture config resolves");
    let expected = installed_use_display_for(&dir, &config, "dev-libs", "abk0b", "1");
    assert!(!expected.is_empty(), "the staged USE file must surface");
    assert_eq!(row.consumer_use, expected);
    let _ = fs::remove_dir_all(&dir);
}

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
        skipped_update_use_display_for(crate::test_gate(&repos), &config, "dev-libs", "blk0x", "3"),
        vec![
            ("USE".to_string(), String::new()),
            ("ELIBC".to_string(), "glibc".to_string()),
        ],
    );
    assert_eq!(
        skipped_update_use_display_for(crate::test_gate(&repos), &config, "dev-libs", "blk0b", "1"),
        vec![
            ("USE".to_string(), String::new()),
            ("ELIBC".to_string(), "glibc".to_string()),
        ],
    );
    // EAPI 8, no IUSE: only the implicit group survives.
    assert_eq!(
        skipped_update_use_display_for(crate::test_gate(&repos), &config, "dev-libs", "mgxc", "2"),
        vec![
            ("USE".to_string(), String::new()),
            ("ELIBC".to_string(), "glibc".to_string()),
        ],
    );
    assert!(
        skipped_update_use_display_for(crate::test_gate(&repos), &config, "dev-libs", "blk0x", "9")
            .is_empty()
    );
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
        pkg_use_display_for(crate::test_gate(&repos), &config, "dev-libs", "aub0d", "0"),
        vec![
            ("USE".to_string(), "foo".to_string()),
            ("ELIBC".to_string(), "glibc".to_string()),
        ],
    );
    assert_eq!(
        pkg_use_display_for(crate::test_gate(&repos), &config, "dev-libs", "aub0d", "1"),
        vec![
            ("USE".to_string(), "-bar".to_string()),
            ("ELIBC".to_string(), "glibc".to_string()),
        ],
    );
    // EAPI 8, no IUSE: the bare display gains the implicit group.
    assert_eq!(
        pkg_use_display_for(crate::test_gate(&repos), &config, "dev-libs", "aub0b", "0"),
        vec![
            ("USE".to_string(), String::new()),
            ("ELIBC".to_string(), "glibc".to_string()),
        ],
    );
    assert!(
        pkg_use_display_for(crate::test_gate(&repos), &config, "dev-libs", "aub0d", "9").is_empty()
    );
}
