use super::*;

#[test]
fn use_expand_star_wildcard_strips_the_pseudo_flag_from_the_effective_set() {
    // `effective_use_flags` never returns a `_*`-suffixed token
    // (Portage strips them from PORTAGE_USE). Exercised with the
    // fixture's own `linguas_*` package.use entry against
    // wildexpandpkg's declared IUSE.
    let use_flags = euf_flat(EufFlatArgs {
        iuse: "linguas_en linguas_de",
        use_tokens: &[],
        package_use: &[(
            "dev-libs/wildexpandpkg".to_string(),
            vec!["linguas_*".to_string()],
        )],
        package_use_force: &[],
        package_use_mask: &[],
        use_force: &HashSet::new(),
        use_mask: &HashSet::new(),
        use_stable_force: &HashSet::new(),
        use_stable_mask: &HashSet::new(),
        package_use_stable_force: &[],
        package_use_stable_mask: &[],
        keywords: &["amd64".to_string()],
        accept_keywords: &HashSet::from(["amd64".to_string()]),
        package_accept_keywords: &[],
        candidate_str: "dev-libs/wildexpandpkg-1.0:0/0::testrepo",
        category: "dev-libs",
        package: "wildexpandpkg",
    });
    assert!(use_flags.contains("linguas_en"));
    assert!(use_flags.contains("linguas_de"));
    assert!(!use_flags.iter().any(|f| f.ends_with("_*")));
}

#[test]
fn effective_use_flags_applies_a_plus_iuse_default_when_nothing_else_says_otherwise() {
    let use_flags = euf_flat(EufFlatArgs {
        iuse: "+foo -bar baz",
        use_tokens: &[],
        package_use: &[],
        package_use_force: &[],
        package_use_mask: &[],
        use_force: &HashSet::new(),
        use_mask: &HashSet::new(),
        use_stable_force: &HashSet::new(),
        use_stable_mask: &HashSet::new(),
        package_use_stable_force: &[],
        package_use_stable_mask: &[],
        keywords: &[],
        accept_keywords: &HashSet::new(),
        package_accept_keywords: &[],
        candidate_str: "dev-libs/pkg-1.0:0",
        category: "dev-libs",
        package: "pkg",
    });
    // "+foo" defaults on; "-bar" stays off (same as omission, but a
    // real, explicit marker); "baz" (no marker at all) is genuinely
    // undecided by IUSE itself and stays off too.
    assert_eq!(use_flags, HashSet::from(["foo".to_string()]));
}

#[test]
fn effective_use_flags_lets_use_tokens_add_alongside_a_plus_iuse_default() {
    // `use_tokens` mentioning an entirely different flag doesn't
    // suppress "+foo"'s own default -- the additive half.
    let base = ["other".to_string()].to_vec();
    let use_flags = euf_flat(EufFlatArgs {
        iuse: "+foo",
        use_tokens: &base,
        package_use: &[],
        package_use_force: &[],
        package_use_mask: &[],
        use_force: &HashSet::new(),
        use_mask: &HashSet::new(),
        use_stable_force: &HashSet::new(),
        use_stable_mask: &HashSet::new(),
        package_use_stable_force: &[],
        package_use_stable_mask: &[],
        keywords: &[],
        accept_keywords: &HashSet::new(),
        package_accept_keywords: &[],
        candidate_str: "dev-libs/pkg-1.0:0",
        category: "dev-libs",
        package: "pkg",
    });
    assert_eq!(
        use_flags,
        HashSet::from(["foo".to_string(), "other".to_string()])
    );
}

#[test]
fn effective_use_flags_lets_use_tokens_explicitly_cancel_a_plus_iuse_default() {
    // A genuine `-foo` in profile/make.conf cancels an earlier
    // `+foo` IUSE default: one continuous incremental walk.
    let use_tokens = vec!["foo".to_string(), "-foo bar".to_string()];
    let use_flags = euf_flat(EufFlatArgs {
        iuse: "+foo",
        use_tokens: &use_tokens,
        package_use: &[],
        package_use_force: &[],
        package_use_mask: &[],
        use_force: &HashSet::new(),
        use_mask: &HashSet::new(),
        use_stable_force: &HashSet::new(),
        use_stable_mask: &HashSet::new(),
        package_use_stable_force: &[],
        package_use_stable_mask: &[],
        keywords: &[],
        accept_keywords: &HashSet::new(),
        package_accept_keywords: &[],
        candidate_str: "dev-libs/pkg-1.0:0",
        category: "dev-libs",
        package: "pkg",
    });
    assert_eq!(use_flags, HashSet::from(["bar".to_string()]));
}

#[test]
fn effective_use_flags_lets_package_use_override_a_plus_iuse_default() {
    let package_use = vec![("dev-libs/pkg".to_string(), vec!["-foo".to_string()])];
    let use_flags = euf_flat(EufFlatArgs {
        iuse: "+foo",
        use_tokens: &[],
        package_use: &package_use,
        package_use_force: &[],
        package_use_mask: &[],
        use_force: &HashSet::new(),
        use_mask: &HashSet::new(),
        use_stable_force: &HashSet::new(),
        use_stable_mask: &HashSet::new(),
        package_use_stable_force: &[],
        package_use_stable_mask: &[],
        keywords: &[],
        accept_keywords: &HashSet::new(),
        package_accept_keywords: &[],
        candidate_str: "dev-libs/pkg-1.0:0",
        category: "dev-libs",
        package: "pkg",
    });
    assert!(use_flags.is_empty());
}

#[test]
fn effective_use_flags_layers_a_matching_package_use_entry_on_top_of_base() {
    let base = ["foo".to_string()].to_vec();
    let package_use = vec![(
        "dev-libs/bar".to_string(),
        vec!["baz".to_string(), "-foo".to_string()],
    )];
    let use_flags = euf_flat(EufFlatArgs {
        iuse: "",
        use_tokens: &base,
        package_use: &package_use,
        package_use_force: &[],
        package_use_mask: &[],
        use_force: &HashSet::new(),
        use_mask: &HashSet::new(),
        use_stable_force: &HashSet::new(),
        use_stable_mask: &HashSet::new(),
        package_use_stable_force: &[],
        package_use_stable_mask: &[],
        keywords: &[],
        accept_keywords: &HashSet::new(),
        package_accept_keywords: &[],
        candidate_str: "dev-libs/bar-1.0:0",
        category: "dev-libs",
        package: "bar",
    });
    assert_eq!(use_flags, HashSet::from(["baz".to_string()]));
}

#[test]
fn effective_use_flags_does_not_affect_a_non_matching_package() {
    let base = ["foo".to_string()].to_vec();
    let package_use = vec![("dev-libs/bar".to_string(), vec!["baz".to_string()])];
    let use_flags = euf_flat(EufFlatArgs {
        iuse: "",
        use_tokens: &base,
        package_use: &package_use,
        package_use_force: &[],
        package_use_mask: &[],
        use_force: &HashSet::new(),
        use_mask: &HashSet::new(),
        use_stable_force: &HashSet::new(),
        use_stable_mask: &HashSet::new(),
        package_use_stable_force: &[],
        package_use_stable_mask: &[],
        keywords: &[],
        accept_keywords: &HashSet::new(),
        package_accept_keywords: &[],
        candidate_str: "dev-libs/unrelated-1.0:0",
        category: "dev-libs",
        package: "unrelated",
    });
    assert_eq!(use_flags, HashSet::from(["foo".to_string()]));
}

#[test]
fn effective_use_flags_matches_a_wildcard_package_use_entry() {
    let base: Vec<String> = Vec::new();
    let package_use = vec![("*/bar".to_string(), vec!["baz".to_string()])];
    let use_flags = euf_flat(EufFlatArgs {
        iuse: "",
        use_tokens: &base,
        package_use: &package_use,
        package_use_force: &[],
        package_use_mask: &[],
        use_force: &HashSet::new(),
        use_mask: &HashSet::new(),
        use_stable_force: &HashSet::new(),
        use_stable_mask: &HashSet::new(),
        package_use_stable_force: &[],
        package_use_stable_mask: &[],
        keywords: &[],
        accept_keywords: &HashSet::new(),
        package_accept_keywords: &[],
        candidate_str: "dev-libs/bar-1.0:0",
        category: "dev-libs",
        package: "bar",
    });
    assert_eq!(use_flags, HashSet::from(["baz".to_string()]));
}

#[test]
fn effective_use_flags_applies_package_use_force_and_mask() {
    let base: Vec<String> = Vec::new();
    let package_use_force = vec![("dev-libs/bar".to_string(), vec!["forceflag".to_string()])];
    let package_use_mask = vec![("dev-libs/bar".to_string(), vec!["maskflag".to_string()])];
    let use_flags = euf_flat(EufFlatArgs {
        iuse: "",
        use_tokens: &base,
        package_use: &[],
        package_use_force: &package_use_force,
        package_use_mask: &package_use_mask,
        use_force: &HashSet::new(),
        use_mask: &HashSet::new(),
        use_stable_force: &HashSet::new(),
        use_stable_mask: &HashSet::new(),
        package_use_stable_force: &[],
        package_use_stable_mask: &[],
        keywords: &[],
        accept_keywords: &HashSet::new(),
        package_accept_keywords: &[],
        candidate_str: "dev-libs/bar-1.0:0",
        category: "dev-libs",
        package: "bar",
    });
    assert_eq!(use_flags, HashSet::from(["forceflag".to_string()]));
}

#[test]
fn effective_use_flags_package_use_mask_wins_over_force_on_conflict() {
    // Same flag forced and masked at the same tier: mask wins
    // (force-then-mask order).
    let base: Vec<String> = Vec::new();
    let package_use_force = vec![("dev-libs/bar".to_string(), vec!["bothflag".to_string()])];
    let package_use_mask = vec![("dev-libs/bar".to_string(), vec!["bothflag".to_string()])];
    let use_flags = euf_flat(EufFlatArgs {
        iuse: "",
        use_tokens: &base,
        package_use: &[],
        package_use_force: &package_use_force,
        package_use_mask: &package_use_mask,
        use_force: &HashSet::new(),
        use_mask: &HashSet::new(),
        use_stable_force: &HashSet::new(),
        use_stable_mask: &HashSet::new(),
        package_use_stable_force: &[],
        package_use_stable_mask: &[],
        keywords: &[],
        accept_keywords: &HashSet::new(),
        package_accept_keywords: &[],
        candidate_str: "dev-libs/bar-1.0:0",
        category: "dev-libs",
        package: "bar",
    });
    assert!(!use_flags.contains("bothflag"));
}

#[test]
fn effective_use_flags_global_use_force_wins_over_a_package_use_disable() {
    // Global use.force applies last in the walk, so a package.use
    // `-flag` can never turn off a globally forced flag.
    let use_force = HashSet::from(["forceflag".to_string()]);
    let package_use = vec![("dev-libs/bar".to_string(), vec!["-forceflag".to_string()])];
    let use_flags = euf_flat(EufFlatArgs {
        iuse: "",
        use_tokens: &[],
        package_use: &package_use,
        package_use_force: &[],
        package_use_mask: &[],
        use_force: &use_force,
        use_mask: &HashSet::new(),
        use_stable_force: &HashSet::new(),
        use_stable_mask: &HashSet::new(),
        package_use_stable_force: &[],
        package_use_stable_mask: &[],
        keywords: &[],
        accept_keywords: &HashSet::new(),
        package_accept_keywords: &[],
        candidate_str: "dev-libs/bar-1.0:0",
        category: "dev-libs",
        package: "bar",
    });
    assert!(use_flags.contains("forceflag"));
}

#[test]
fn effective_use_flags_global_use_mask_wins_over_a_package_use_enable() {
    // Mirror: global use.mask beats a package.use `+flag`.
    let use_mask = HashSet::from(["maskflag".to_string()]);
    let package_use = vec![("dev-libs/bar".to_string(), vec!["maskflag".to_string()])];
    let use_flags = euf_flat(EufFlatArgs {
        iuse: "",
        use_tokens: &[],
        package_use: &package_use,
        package_use_force: &[],
        package_use_mask: &[],
        use_force: &HashSet::new(),
        use_mask: &use_mask,
        use_stable_force: &HashSet::new(),
        use_stable_mask: &HashSet::new(),
        package_use_stable_force: &[],
        package_use_stable_mask: &[],
        keywords: &[],
        accept_keywords: &HashSet::new(),
        package_accept_keywords: &[],
        candidate_str: "dev-libs/bar-1.0:0",
        category: "dev-libs",
        package: "bar",
    });
    assert!(!use_flags.contains("maskflag"));
}

#[test]
fn effective_use_flags_applies_stable_force_and_mask_only_when_stable() {
    // `amd64` is stable in this synthetic config, so the stable-only
    // sources all apply.
    let base: Vec<String> = Vec::new();
    let use_stable_force = HashSet::from(["globalstableforce".to_string()]);
    let use_stable_mask = HashSet::from(["globalstablemask".to_string()]);
    let package_use_stable_force = vec![(
        "dev-libs/bar".to_string(),
        vec!["pkgstableforce".to_string()],
    )];
    let package_use_stable_mask = vec![(
        "dev-libs/bar".to_string(),
        vec!["pkgstablemask".to_string()],
    )];
    let accept_keywords = HashSet::from(["amd64".to_string()]);
    let use_flags = euf_flat(EufFlatArgs {
        iuse: "",
        use_tokens: &base,
        package_use: &[],
        package_use_force: &[],
        package_use_mask: &[],
        use_force: &HashSet::new(),
        use_mask: &HashSet::new(),
        use_stable_force: &use_stable_force,
        use_stable_mask: &use_stable_mask,
        package_use_stable_force: &package_use_stable_force,
        package_use_stable_mask: &package_use_stable_mask,
        keywords: &["amd64".to_string()],
        accept_keywords: &accept_keywords,
        package_accept_keywords: &[],
        candidate_str: "dev-libs/bar-1.0:0",
        category: "dev-libs",
        package: "bar",
    });
    assert_eq!(
        use_flags,
        HashSet::from([
            "globalstableforce".to_string(),
            "pkgstableforce".to_string()
        ])
    );
}

#[test]
fn effective_use_flags_skips_stable_force_and_mask_when_not_stable() {
    // `~amd64` is not stable, so no stable-only source applies.
    let base: Vec<String> = Vec::new();
    let use_stable_force = HashSet::from(["globalstableforce".to_string()]);
    let accept_keywords = HashSet::from(["~amd64".to_string()]);
    let use_flags = euf_flat(EufFlatArgs {
        iuse: "",
        use_tokens: &base,
        package_use: &[],
        package_use_force: &[],
        package_use_mask: &[],
        use_force: &HashSet::new(),
        use_mask: &HashSet::new(),
        use_stable_force: &use_stable_force,
        use_stable_mask: &HashSet::new(),
        package_use_stable_force: &[],
        package_use_stable_mask: &[],
        keywords: &["~amd64".to_string()],
        accept_keywords: &accept_keywords,
        package_accept_keywords: &[],
        candidate_str: "dev-libs/bar-1.0:0",
        category: "dev-libs",
        package: "bar",
    });
    assert!(use_flags.is_empty());
}

#[test]
fn resolved_use_mask_interleaves_per_level_across_sources() {
    use portage_profile::UseMaskForceLevel;
    // Per-level interleave: a later level's global `-xmc` cancels an
    // earlier level's `package.use.mask xmc`.
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        use_mask_force_levels: vec![
            UseMaskForceLevel {
                package_use_mask: vec![("dev-libs/bar".to_string(), vec!["xmc".to_string()])],
                ..Default::default()
            },
            UseMaskForceLevel {
                use_mask: vec!["-xmc".to_string()],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    assert!(
        !resolved_use_mask_or_force(ResolvedUseMaskOrForceArgs {
            which: MaskOrForce::Mask,
            config: &config,
            candidate_str: "dev-libs/bar-1.0:0/0::testrepo",
            category: "dev-libs",
            package: "bar",
            stable: true
        })
        .contains("xmc"),
        "a later level's global -xmc must cancel an earlier level's package.use.mask xmc"
    );
    assert!(
        !forced_or_masked_flags(ForcedOrMaskedFlagsArgs {
            iuse: "xmc",
            keywords: &["amd64".to_string()],
            candidate_str: "dev-libs/bar-1.0:0/0::testrepo",
            category: "dev-libs",
            package: "bar",
            config: &config
        })
        .contains("xmc")
    );
    // Reverse level order keeps it masked (last level wins).
    let config_rev = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        use_mask_force_levels: vec![
            UseMaskForceLevel {
                use_mask: vec!["-xmc".to_string()],
                ..Default::default()
            },
            UseMaskForceLevel {
                package_use_mask: vec![("dev-libs/bar".to_string(), vec!["xmc".to_string()])],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    assert!(
        resolved_use_mask_or_force(ResolvedUseMaskOrForceArgs {
            which: MaskOrForce::Mask,
            config: &config_rev,
            candidate_str: "dev-libs/bar-1.0:0/0::testrepo",
            category: "dev-libs",
            package: "bar",
            stable: true
        })
        .contains("xmc")
    );
}

#[test]
fn atom_specificity_ranks_exact_version_above_bare_above_wildcard() {
    assert!(atom_specificity("=dev-libs/bar-1.0") > atom_specificity("dev-libs/bar:0"));
    assert!(atom_specificity("dev-libs/bar:0") > atom_specificity("dev-libs/bar"));
    assert!(atom_specificity("dev-libs/bar") > atom_specificity("*/*"));
}
