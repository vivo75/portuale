use super::*;

#[test]
fn use_tokens_capture_the_ordered_raw_use_values_that_produced_use_flags() {
    // base: make.defaults sets USE="foo". leaf (its own parent ->
    // base): make.defaults sets USE="-foo bar". make.conf: USE="baz".
    // use_flags ends up {bar, baz} either way -- but the token lists
    // must retain each raw contribution *separately*, in real
    // accumulation order: `use_tokens` = the profile chain
    // (`defaults` layer), `conf_use_tokens` = make.conf (`conf`
    // layer, split out by the "Config depth" slice). Replaying
    // use_tokens then conf_use_tokens via apply_incremental from an
    // empty set reproduces use_flags exactly. This is what lets
    // portage-repo's own effective_use_flags replay them on top of
    // a *different* seed (a package's own IUSE defaults) instead of
    // just union-ing the pre-flattened use_flags on top, which could
    // never let "-foo" cancel an IUSE "+foo" default.
    let root = TempDir::new("portage-profile-test-use-tokens").keep();
    let repo = root.join("repo");
    let repo_profiles = repo.join("profiles");
    let base = repo_profiles.join("base");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&leaf).unwrap();

    fs::write(base.join("make.defaults"), "USE=\"foo\"\n").unwrap();
    fs::write(leaf.join("parent"), "../repo/profiles/base\n").unwrap();
    fs::write(leaf.join("make.defaults"), "USE=\"-foo bar\"\n").unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();
    fs::write(portage_dir.join("make.conf"), "USE=\"baz\"\n").unwrap();

    let config = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &repo,
        overlay_repos: &[],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("config must resolve");
    assert_eq!(
        config.use_tokens,
        vec!["foo".to_string(), "-foo bar".to_string()]
    );
    assert_eq!(config.conf_use_tokens, vec!["baz".to_string()]);
    let mut replayed = HashSet::new();
    for token in config.use_tokens.iter().chain(&config.conf_use_tokens) {
        apply_use_incremental(token, &mut replayed);
    }
    assert_eq!(replayed, config.use_flags);
}

#[test]
fn apply_incremental_is_reusable_for_per_package_use_overrides() {
    let mut set = HashSet::from(["foo".to_string()]);
    apply_incremental("flag1 -foo +flag2", &mut set);
    assert_eq!(
        set,
        HashSet::from(["flag1".to_string(), "flag2".to_string()])
    );
}

#[test]
fn apply_use_incremental_removes_a_prefix_wildcard() {
    // Real `config.py:2925`: `-video_cards_*` drops every accumulated
    // `video_cards_` flag; unrelated flags survive. Tokens reach this
    // applier already `video_cards_`-prefixed (the USE_EXPAND fold,
    // real `config.py:2837-2866`, runs before the USE loop), so the
    // positive token here is the full flag name.
    let mut set = HashSet::from([
        "X".to_string(),
        "video_cards_dummy".to_string(),
        "video_cards_intel".to_string(),
    ]);
    apply_use_incremental("-video_cards_* video_cards_intel", &mut set);
    assert_eq!(
        set,
        HashSet::from(["X".to_string(), "video_cards_intel".to_string()])
    );

    // No accumulated match -> no-op (the literal `video_cards_*` is
    // discarded, not added).
    let mut set = HashSet::from(["X".to_string()]);
    apply_use_incremental("-video_cards_*", &mut set);
    assert_eq!(set, HashSet::from(["X".to_string()]));

    // The generic applier keeps its exact-removal semantics: it must
    // NOT expand `_*` (the wildcard arm is USE-scoped in real).
    let mut set = HashSet::from(["video_cards_dummy".to_string()]);
    apply_incremental("-video_cards_*", &mut set);
    assert_eq!(set, HashSet::from(["video_cards_dummy".to_string()]));

    // Plain tokens are unchanged: `-*` clears, `-flag` removes,
    // `+flag`/`flag` add.
    let mut set = HashSet::from(["a".to_string(), "b".to_string()]);
    apply_use_incremental("-* b +c d", &mut set);
    assert_eq!(
        set,
        HashSet::from(["b".to_string(), "c".to_string(), "d".to_string()])
    );
    apply_use_incremental("-b -c", &mut set);
    assert_eq!(set, HashSet::from(["d".to_string()]));

    // The iter variant behaves identically.
    let mut set = HashSet::from([
        "sane_backends_hp".to_string(),
        "sane_backends_hp3500".to_string(),
    ]);
    apply_use_incremental_iter(&["-sane_backends_*", "sane_backends_hp"], &mut set);
    assert_eq!(set, HashSet::from(["sane_backends_hp".to_string()]));
}

#[test]
fn resolved_incremental_folds_stacked_sources_with_negation_and_sorts() {
    let mut config = Config::default();
    note_incremental(&mut config, "FEATURES", "b a merge-sync");
    note_incremental(&mut config, "FEATURES", "c -merge-sync");
    note_incremental(&mut config, "FEATURES", "-a d");
    assert_eq!(
        config.resolved_incremental("FEATURES"),
        Some(vec!["b".to_string(), "c".to_string(), "d".to_string()])
    );
    assert_eq!(config.resolved_incremental("CONFIG_PROTECT"), None);
}

/// L3 finding: real config files are sourced, so one file's
/// incremental layer is its **final** assignment -- a second
/// `FEATURES=` line in `make.conf` replaces the first (`emerge
/// --info` drops the first line's tokens entirely). The per-file
/// snapshot must collapse to that final value while leaving earlier
/// files' layers stacked.
#[test]
fn a_files_incremental_layer_is_its_final_assignment() {
    let mut config = Config::default();
    note_incremental(&mut config, "FEATURES", "a b");
    let snapshot = IncrementalSnapshot::take(&config);
    note_incremental(&mut config, "FEATURES", "c");
    note_incremental(&mut config, "FEATURES", "-a d");
    snapshot.restore(&mut config);
    // The second file contributed only its final "-a d"; the first
    // file's "a b" still stacks first -> b d.
    assert_eq!(
        config.resolved_incremental("FEATURES"),
        Some(vec!["b".to_string(), "d".to_string()])
    );
    // A file that assigned nothing leaves the previous layers alone.
    let snapshot = IncrementalSnapshot::take(&config);
    snapshot.restore(&mut config);
    assert_eq!(
        config.resolved_incremental("FEATURES"),
        Some(vec!["b".to_string(), "d".to_string()])
    );
}
