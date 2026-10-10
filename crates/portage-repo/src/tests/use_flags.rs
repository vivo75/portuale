use super::*;

#[test]
fn use_expand_star_wildcard_strips_the_pseudo_flag_from_the_effective_set() {
    // `effective_use_flags` never returns a `_*`-suffixed token (real
    // config.py strips them from PORTAGE_USE). Exercised directly
    // with the fixture's own `linguas_*` package.use entry against
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
    // The gap portuale's own IUSE-defaults slice originally left
    // open, now closed: real regenerate() runs ONE continuous
    // incremental walk (pkginternal -> defaults -> conf -> pkg), so
    // a genuine "-foo" in profile/make.conf really does cancel an
    // earlier "+foo" IUSE default -- not just fail to add on top of
    // it. `use_tokens` here is two separate raw USE= values
    // ("foo" then "-foo bar"), exactly what `resolve_config` would
    // produce from two profile levels -- replayed via
    // apply_incremental, not unioned as a pre-flattened set, so the
    // "-foo" genuinely reaches back and cancels IUSE's own "+foo".
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
fn effective_use_flags_applies_a_use_prefix_wildcard() {
    // #67 S2: conf-tier USE_EXPAND folding produces
    // `-video_cards_*`; it shares real's USE loop with the IUSE
    // defaults, so it cancels an ebuild's `+video_cards_dummy`
    // default, and the same level's plain `video_cards_intel` token
    // still lands.
    let mut config = test_config();
    config.conf_use_tokens = vec!["-video_cards_* video_cards_intel".to_string()];
    let flags = effective_use_flags(EffectiveUseFlagsArgs {
        config: &config,
        iuse: "+video_cards_dummy video_cards_intel",
        keywords: &["amd64".to_string()],
        candidate_str: "dev-libs/pkg-1.0::testrepo",
        category: "dev-libs",
        package: "pkg",
    });
    assert_eq!(*flags, HashSet::from(["video_cards_intel".to_string()]));
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
fn effective_use_flags_defaults_walk_is_per_profile_level() {
    // Two chain levels: the parent's package.use enables `foo` for
    // this package; the child's make.defaults disables it. Real
    // portage walks make.defaults-then-package.use one profile at a
    // time, so the child's `-foo` (applied after the parent's
    // package.use) wins.
    let layers = vec![
        portage_profile::ProfileUseLayer {
            make_defaults_use: vec![],
            package_use: pu("foo"),
        },
        portage_profile::ProfileUseLayer {
            make_defaults_use: vec!["-foo".to_string()],
            package_use: vec![],
        },
    ];
    let flags = {
        let cfg = portage_profile::Config {
            profile_use_layers: layers.to_vec(),
            accept_keywords: HashSet::from(["amd64".to_string()]),
            ..Default::default()
        };
        effective_use_flags(EffectiveUseFlagsArgs {
            config: &cfg,
            iuse: "foo",
            keywords: &["amd64".to_string()],
            candidate_str: "dev-libs/pkg-1.0:0/0::testrepo",
            category: "dev-libs",
            package: "pkg",
        })
    };
    assert!(
        !flags.contains("foo"),
        "child make.defaults -foo cancels parent package.use foo"
    );
    // Reversed order (child's -foo first, parent's package.use foo
    // last) would leave it on -- proving order matters.
    let reversed: Vec<_> = layers.into_iter().rev().collect();
    let flags = {
        let cfg = portage_profile::Config {
            profile_use_layers: reversed.to_vec(),
            accept_keywords: HashSet::from(["amd64".to_string()]),
            ..Default::default()
        };
        effective_use_flags(EffectiveUseFlagsArgs {
            config: &cfg,
            iuse: "foo",
            keywords: &["amd64".to_string()],
            candidate_str: "dev-libs/pkg-1.0:0/0::testrepo",
            category: "dev-libs",
            package: "pkg",
        })
    };
    assert!(flags.contains("foo"));
}

#[test]
fn effective_use_flags_repo_make_defaults_use_is_the_weakest_repo_layer() {
    let euf = |iuse: &str, repo_md: &[(String, Vec<String>)], repo_pu: &[(String, Vec<String>)]| {
        let cfg = portage_profile::Config {
            repo_make_defaults_use: repo_md.to_vec(),
            package_use_repo: repo_pu.to_vec(),
            accept_keywords: HashSet::from(["amd64".to_string()]),
            ..Default::default()
        };
        effective_use_flags(EffectiveUseFlagsArgs {
            config: &cfg,
            iuse,
            keywords: &["amd64".to_string()],
            candidate_str: "dev-libs/pkg-1.0:0/0::testrepo",
            category: "dev-libs",
            package: "pkg",
        })
    };
    let md = |t: &str| vec![(String::new(), vec![t.to_string()])];
    // repo make.defaults USE decides an unmarked IUSE flag...
    assert!(euf("foo", &md("foo"), &[]).contains("foo"));
    // ...but loses to the ebuild's own IUSE default (applied after)...
    assert!(!euf("-foo", &md("foo"), &[]).contains("foo"));
    // ...and to a repo package.use entry (also applied after, same tier).
    assert!(!euf("foo", &md("foo"), &pu("-foo")).contains("foo"));
    // A named (overlay) entry only applies to a candidate from that
    // repo -- `candidate_str` here is `::testrepo`, so `::other` is inert.
    assert!(
        !euf(
            "foo",
            &[("other".to_string(), vec!["foo".to_string()])],
            &[]
        )
        .contains("foo")
    );
    assert!(
        euf(
            "foo",
            &[("testrepo".to_string(), vec!["foo".to_string()])],
            &[]
        )
        .contains("foo")
    );
}

#[test]
fn effective_use_flags_features_use_enables_test_between_repo_and_pkginternal() {
    let euf = |iuse: &str, features_use: &[String]| {
        let cfg = portage_profile::Config {
            features_use: features_use.to_vec(),
            accept_keywords: HashSet::from(["amd64".to_string()]),
            ..Default::default()
        };
        effective_use_flags(EffectiveUseFlagsArgs {
            config: &cfg,
            iuse,
            keywords: &["amd64".to_string()],
            candidate_str: "dev-libs/pkg-1.0:0/0::testrepo",
            category: "dev-libs",
            package: "pkg",
        })
    };
    let test_on = vec!["test".to_string()];
    // `FEATURES=test` enables `test` for a package that declares it...
    assert!(euf("test", &test_on).contains("test"));
    // ...and, being a weaker tier than `pkginternal`, an explicit
    // IUSE `-test` default (applied right after) still wins.
    assert!(!euf("-test", &test_on).contains("test"));
    // No `FEATURES=test` -> nothing added here.
    assert!(!euf("test", &[]).contains("test"));
    // A use.mask on `test` drops it even with FEATURES=test.
    let masked = {
        let cfg = portage_profile::Config {
            features_use: test_on.to_vec(),
            use_mask: HashSet::from(["test".to_string()]),
            use_mask_force_levels: vec![portage_profile::UseMaskForceLevel {
                use_mask: vec!["test".to_string()],
                ..Default::default()
            }],
            accept_keywords: HashSet::from(["amd64".to_string()]),
            ..Default::default()
        };
        effective_use_flags(EffectiveUseFlagsArgs {
            config: &cfg,
            iuse: "test",
            keywords: &["amd64".to_string()],
            candidate_str: "dev-libs/pkg-1.0:0/0::testrepo",
            category: "dev-libs",
            package: "pkg",
        })
    };
    assert!(!masked.contains("test"));
}

#[test]
fn effective_use_flags_repo_package_use_is_weaker_than_an_iuse_default() {
    // real `configdict["repo"]` is applied BEFORE `pkginternal`, so
    // an ebuild's own explicit IUSE default overrides a repo-level
    // `package.use` -- the divergence the old flat model had (it
    // applied every `package.use` source last, strongest).
    let a = euf_layers(EufLayersArgs {
        iuse: "-foo",
        use_tokens: &[],
        conf_use_tokens: &[],
        package_use_repo: &pu("foo"),
        package_use: &[],
        package_use_user: &[],
    });
    assert!(
        !a.contains("foo"),
        "IUSE `-foo` cancels repo package.use `foo`"
    );
    let b = euf_layers(EufLayersArgs {
        iuse: "+foo",
        use_tokens: &[],
        conf_use_tokens: &[],
        package_use_repo: &pu("-foo"),
        package_use: &[],
        package_use_user: &[],
    });
    assert!(
        b.contains("foo"),
        "IUSE `+foo` overrides repo package.use `-foo`"
    );
    // ...but a repo `package.use` still takes effect on a flag the
    // ebuild's IUSE leaves undecided (no `+`/`-` marker).
    let c = euf_layers(EufLayersArgs {
        iuse: "foo",
        use_tokens: &[],
        conf_use_tokens: &[],
        package_use_repo: &pu("foo"),
        package_use: &[],
        package_use_user: &[],
    });
    assert!(
        c.contains("foo"),
        "repo package.use decides an unmarked IUSE flag"
    );
}

#[test]
fn effective_use_flags_profile_package_use_loses_to_make_conf() {
    // profile `package.use` is real `configdict["defaults"]` --
    // applied BEFORE `conf`. So `make.conf USE="-foo"` wins over a
    // profile `package.use` line enabling `foo` (the old flat model
    // applied profile `package.use` after make.conf, so `foo` won).
    let flags = euf_layers(EufLayersArgs {
        iuse: "foo",
        use_tokens: &[],
        conf_use_tokens: &["-foo".to_string()],
        package_use_repo: &[],
        package_use: &pu("foo"),
        package_use_user: &[],
    });
    assert!(
        !flags.contains("foo"),
        "make.conf -foo beats profile package.use"
    );
}

#[test]
fn effective_use_flags_package_env_use_is_pkg_layer_and_loses_to_user_package_use() {
    // package.env's own USE= sits in the `pkg` layer, applied just
    // before user `/etc/portage/package.use` (real _grab_pkg_env
    // fills pkg_configdict, then self.puse is appended after). So it
    // beats make.conf...
    let with_env = {
        let cfg = portage_profile::Config {
            conf_use_tokens: vec!["-foo".to_string()],
            package_env_use: pu("foo").to_vec(),
            accept_keywords: HashSet::from(["amd64".to_string()]),
            ..Default::default()
        };
        effective_use_flags(EffectiveUseFlagsArgs {
            config: &cfg,
            iuse: "foo",
            keywords: &["amd64".to_string()],
            candidate_str: "dev-libs/pkg-1.0:0/0::testrepo",
            category: "dev-libs",
            package: "pkg",
        })
    };
    assert!(
        with_env.contains("foo"),
        "package.env foo beats make.conf -foo"
    );

    // ...but a user package.use `-foo` still wins over package.env `foo`.
    let user_wins = {
        let cfg = portage_profile::Config {
            package_env_use: pu("foo").to_vec(),
            package_use_user: pu("-foo").to_vec(),
            accept_keywords: HashSet::from(["amd64".to_string()]),
            ..Default::default()
        };
        effective_use_flags(EffectiveUseFlagsArgs {
            config: &cfg,
            iuse: "foo",
            keywords: &["amd64".to_string()],
            candidate_str: "dev-libs/pkg-1.0:0/0::testrepo",
            category: "dev-libs",
            package: "pkg",
        })
    };
    assert!(
        !user_wins.contains("foo"),
        "user package.use -foo beats package.env foo"
    );
}

#[test]
fn effective_use_flags_env_use_is_the_highest_tier_and_beats_user_package_use() {
    // Real `configdict["env"]` is above `pkg`: `USE="-foo"` in the
    // process env overrides a user `/etc/portage/package.use foo`.
    let with_env_use = |env_tokens: &[String], user_pu: &[(String, Vec<String>)]| {
        let cfg = portage_profile::Config {
            package_use_user: user_pu.to_vec(),
            env_use_tokens: env_tokens.to_vec(),
            accept_keywords: HashSet::from(["amd64".to_string()]),
            ..Default::default()
        };
        effective_use_flags(EffectiveUseFlagsArgs {
            config: &cfg,
            iuse: "foo",
            keywords: &["amd64".to_string()],
            candidate_str: "dev-libs/pkg-1.0:0/0::testrepo",
            category: "dev-libs",
            package: "pkg",
        })
    };
    // user package.use enables `foo`, env `USE="-foo"` cancels it.
    assert!(!with_env_use(&["-foo".to_string()], &pu("foo")).contains("foo"));
    // ...and the other way: env `USE="foo"` beats user package.use `-foo`.
    assert!(with_env_use(&["foo".to_string()], &pu("-foo")).contains("foo"));
    // no env tokens -> user package.use stands.
    assert!(with_env_use(&[], &pu("foo")).contains("foo"));
}

#[test]
fn effective_use_flags_envd_is_the_lowest_tier() {
    // Real `configdict["env.d"]` (from `/etc/profile.env`) is the
    // bottom of `USE_ORDER` -- it enables a flag nothing else touches,
    // but any higher tier (here a repo `package.use -foo`) overrides.
    let euf = |envd: &[String], repo_pu: &[(String, Vec<String>)]| {
        let cfg = portage_profile::Config {
            package_use_repo: repo_pu.to_vec(),
            accept_keywords: HashSet::from(["amd64".to_string()]),
            envd_use_tokens: envd.to_vec(),
            ..Default::default()
        };
        effective_use_flags(EffectiveUseFlagsArgs {
            config: &cfg,
            iuse: "foo bar",
            keywords: &["amd64".to_string()],
            candidate_str: "dev-libs/pkg-1.0:0/0::testrepo",
            category: "dev-libs",
            package: "pkg",
        })
    };
    // env.d `USE="foo bar"` with nothing else -> both on.
    let both = euf(&["foo bar".to_string()], &[]);
    assert!(both.contains("foo") && both.contains("bar"));
    // a repo `package.use -foo` (a higher tier) cancels env.d's `foo`.
    assert!(!euf(&["foo bar".to_string()], &pu("-foo")).contains("foo"));
    // ...leaving `bar`, which nothing higher touched.
    assert!(euf(&["foo bar".to_string()], &pu("-foo")).contains("bar"));
}

#[test]
fn effective_use_flags_user_package_use_wins_over_make_conf() {
    // user `/etc/portage/package.use` is real `configdict["pkg"]` --
    // applied AFTER `conf`, the strongest layer before force/mask.
    let flags = euf_layers(EufLayersArgs {
        iuse: "foo",
        use_tokens: &[],
        conf_use_tokens: &["foo".to_string()],
        package_use_repo: &[],
        package_use: &[],
        package_use_user: &pu("-foo"),
    });
    assert!(
        !flags.contains("foo"),
        "user package.use -foo beats make.conf foo"
    );
}

#[test]
fn effective_use_flags_layer_precedence_is_repo_then_profile_then_user() {
    // All three sources touch `foo`; the walk order repo -> profile
    // -> user means the user entry is the last word.
    let flags = euf_layers(EufLayersArgs {
        iuse: "foo",
        use_tokens: &[],
        conf_use_tokens: &[],
        package_use_repo: &pu("-foo"),
        package_use: &pu("-foo"),
        package_use_user: &pu("foo"),
    });
    assert!(flags.contains("foo"));
    // ...and with the user entry disabling, it stays off regardless
    // of repo/profile enabling it.
    let flags = euf_layers(EufLayersArgs {
        iuse: "foo",
        use_tokens: &[],
        conf_use_tokens: &[],
        package_use_repo: &pu("foo"),
        package_use: &pu("foo"),
        package_use_user: &pu("-foo"),
    });
    assert!(!flags.contains("foo"));
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

/// Backlog #104 S3: the `resolved_use_mask_or_force` memo is
/// transparent -- a second identical call shares the allocation.
#[test]
fn resolved_use_mask_or_force_memo_shares_the_allocation() {
    let config = portage_profile::Config::default();
    let a = resolved_use_mask_or_force(ResolvedUseMaskOrForceArgs {
        which: MaskOrForce::Mask,
        config: &config,
        candidate_str: "dev-libs/pkg-1.0:0/0::testrepo",
        category: "dev-libs",
        package: "pkg",
        stable: true,
    });
    let b = resolved_use_mask_or_force(ResolvedUseMaskOrForceArgs {
        which: MaskOrForce::Mask,
        config: &config,
        candidate_str: "dev-libs/pkg-1.0:0/0::testrepo",
        category: "dev-libs",
        package: "pkg",
        stable: true,
    });
    assert!(Rc::ptr_eq(&a, &b));
    // A different `which` is a different entry.
    let c = resolved_use_mask_or_force(ResolvedUseMaskOrForceArgs {
        which: MaskOrForce::Force,
        config: &config,
        candidate_str: "dev-libs/pkg-1.0:0/0::testrepo",
        category: "dev-libs",
        package: "pkg",
        stable: true,
    });
    assert!(!Rc::ptr_eq(&a, &c));
    // Backlog #122: `category`/`package` are key components, not
    // merely implied by `candidate_str` -- the same string with a
    // different package is a different entry with its own value.
    let config = portage_profile::Config {
        use_mask_force_levels: vec![portage_profile::UseMaskForceLevel {
            package_use_mask: vec![("dev-libs/pkg".to_string(), vec!["special".to_string()])],
            ..Default::default()
        }],
        ..Default::default()
    };
    let s = "dev-libs/pkg-1.0:0/0::testrepo";
    let for_pkg = resolved_use_mask_or_force(ResolvedUseMaskOrForceArgs {
        which: MaskOrForce::Mask,
        config: &config,
        candidate_str: s,
        category: "dev-libs",
        package: "pkg",
        stable: false,
    });
    assert!(for_pkg.contains("special"));
    let for_other = resolved_use_mask_or_force(ResolvedUseMaskOrForceArgs {
        which: MaskOrForce::Mask,
        config: &config,
        candidate_str: s,
        category: "dev-libs",
        package: "other",
        stable: false,
    });
    assert!(!for_other.contains("special"));
    assert!(!Rc::ptr_eq(&for_pkg, &for_other));
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
    // Same flag both forced and masked by two entries at the SAME
    // specificity tier -- mask must win, matching real portage's own
    // force-then-mask application order (see effective_use_flags's
    // own doc comment).
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
    // Real regenerate() applies self.useforce (global use.force
    // combined with per-package getUseForce(pkg)) as the literal
    // last step of its own incremental USE walk, strictly after the
    // "pkg" (package.use) tier -- so a package.use "-flag" entry can
    // NEVER turn off a globally use.force'd flag, unlike an earlier
    // version of portuale which folded use_force into `base` too
    // early, letting package.use incorrectly win.
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
    // Mirror of the force case above: global use.mask (folded into
    // real self.usemask, applied via difference_update as the
    // literal last operation of all) beats a package.use "+flag"
    // entry trying to turn it on.
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
    // "amd64" (no "~") is stable in this synthetic config: converting
    // it to "~amd64" would fall outside accept_keywords={"amd64"},
    // so is_stable's own "would masking every keyword make this
    // invisible" check is true. use_stable_force/package_use_stable_force
    // and use_stable_mask/package_use_stable_mask should all apply.
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
    // "~amd64" (already unstable) is NOT stable: replacing it with
    // its own already-"~"-prefixed form changes nothing, so it stays
    // visible either way -- is_stable's own check is false. None of
    // the stable-only sources should apply at all.
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
fn specificity_ordered_flags_processes_entries_in_source_order() {
    // Entries arrive already in profile-chain / source order. A
    // `-flag` in a *later* source cancels a `flag` from an *earlier*
    // one, regardless of the two atoms' relative specificity -- this
    // is the real `stack_lists(incremental=True)` over `[src0, src1]`
    // and the `targets/*` un-mask pattern (a broad child `cat/pkg
    // -flag` overriding a narrow parent `>=cat/pkg-N flag`).
    let parent_masks_child_unmasks = vec![
        (">=dev-libs/bar-1".to_string(), vec!["flag".to_string()]),
        ("dev-libs/bar".to_string(), vec!["-flag".to_string()]),
    ];
    assert!(
        !specificity_ordered_flags(
            &parent_masks_child_unmasks,
            "dev-libs/bar-1.0:0",
            "dev-libs",
            "bar",
            HashSet::new(),
        )
        .contains("flag"),
        "a later `-flag` cancels an earlier mask"
    );

    // ...and the reverse order keeps the flag (last source wins).
    let child_masks = vec![
        ("dev-libs/bar".to_string(), vec!["-flag".to_string()]),
        (">=dev-libs/bar-1".to_string(), vec!["flag".to_string()]),
    ];
    assert!(
        specificity_ordered_flags(
            &child_masks,
            "dev-libs/bar-1.0:0",
            "dev-libs",
            "bar",
            HashSet::new(),
        )
        .contains("flag")
    );
}

#[test]
fn resolved_use_mask_interleaves_per_level_across_sources() {
    use portage_profile::UseMaskForceLevel;
    // Real `getUseMask`'s per-level interleave: level 0's
    // `package.use.mask` masks `xmc` for the candidate, a *later*
    // level's *global* `use.mask` un-masks it with `-xmc`. The flat
    // union (global stack {} ∪ per-package {xmc}) would keep it
    // masked; the interleave must not.
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

#[test]
fn effective_use_flags_memo_does_not_leak_between_differing_configs() {
    // The `use_context_fingerprint` cache key must distinguish two
    // configs that differ only in a *middle* `package_use` entry
    // (same length, same first + last). Same candidate / iuse /
    // keywords, so only the config content can produce a different
    // answer.
    let base = |mid_flag: &str| portage_profile::Config {
        package_use: vec![
            ("dev-libs/other".to_string(), vec!["x".to_string()]),
            ("dev-libs/pkg".to_string(), vec![mid_flag.to_string()]),
            ("dev-libs/last".to_string(), vec!["z".to_string()]),
        ],
        accept_keywords: HashSet::from(["amd64".to_string()]),
        ..Default::default()
    };
    let call = |cfg: &portage_profile::Config| {
        effective_use_flags(EffectiveUseFlagsArgs {
            config: cfg,
            iuse: "foo bar",
            keywords: &["amd64".to_string()],
            candidate_str: "dev-libs/pkg-1.0:0/0::testrepo",
            category: "dev-libs",
            package: "pkg",
        })
    };
    let with_foo = call(&base("foo"));
    let with_bar = call(&base("bar"));
    assert!(with_foo.contains("foo") && !with_foo.contains("bar"));
    assert!(with_bar.contains("bar") && !with_bar.contains("foo"));
    // And the memo is transparent: a second call matches the uncached body.
    assert_eq!(
        call(&base("foo")).as_ref(),
        &effective_use_flags_uncached(EffectiveUseFlagsUncachedArgs {
            config: &base("foo"),
            iuse: "foo bar",
            keywords: &["amd64".to_string()],
            candidate_str: "dev-libs/pkg-1.0:0/0::testrepo",
            category: "dev-libs",
            package: "pkg"
        })
    );
}

/// #105: the composed fingerprint uses a frozen `use_context_base`
/// when one is present (the production path, set by `resolve_config`)
/// and still separates `autounmask_use` -- the one field the
/// backtracking loop mutates on a clone. A hand-built config without
/// a base falls back to the full content hash.
#[test]
fn use_context_fingerprint_uses_the_frozen_base_and_separates_autounmask_use() {
    let base_config = portage_profile::Config::default();
    let frozen = portage_profile::Config {
        use_context_base: Some(portage_profile::use_context_base_fingerprint(&base_config)),
        ..base_config
    };
    let plain = use_context_fingerprint(&frozen);
    // A clone carrying the frozen base is identical...
    assert_eq!(plain, use_context_fingerprint(&frozen.clone()));
    // ...until `autounmask_use` changes, which the key must see.
    let mut with_autounmask = frozen.clone();
    with_autounmask.autounmask_use = vec![("dev-libs/foo".to_string(), vec!["flag".to_string()])];
    assert_ne!(plain, use_context_fingerprint(&with_autounmask));

    // No base (hand-built config): full content hash, content-sensitive.
    let mut unfrozen = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        ..Default::default()
    };
    let before = use_context_fingerprint(&unfrozen);
    unfrozen.accept_keywords.insert("~amd64".to_string());
    assert_ne!(before, use_context_fingerprint(&unfrozen));
}

/// Backlog #121 (audit O6): mutating a USE-context field after
/// `resolve_config` froze its digest trips the frozen-base
/// `debug_assert_eq!` instead of silently poisoning the memo.
#[test]
#[should_panic(expected = "mutated after resolve_config")]
#[cfg(debug_assertions)]
fn use_context_fingerprint_debug_asserts_on_post_freeze_mutation() {
    let base_config = portage_profile::Config::default();
    let mut frozen = portage_profile::Config {
        use_context_base: Some(portage_profile::use_context_base_fingerprint(&base_config)),
        ..base_config
    };
    frozen.accept_keywords.insert("~amd64".to_string());
    let _ = use_context_fingerprint(&frozen);
}
