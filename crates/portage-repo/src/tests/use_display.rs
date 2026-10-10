use super::*;

#[test]
fn alnum_sort_key_orders_digit_runs_numerically() {
    let mut v = vec![
        "python3_9",
        "python3_12",
        "python3_10",
        "abi_x86_32",
        "abi_x86_64",
        "n2",
        "n10",
        "n9",
        "plainflag",
    ];
    v.sort_by_key(|x| alnum_sort_key(x));
    assert_eq!(
        v,
        vec![
            "abi_x86_32",
            "abi_x86_64",
            "n2",
            "n9",
            "n10",
            "plainflag",
            "python3_9",
            "python3_10",
            "python3_12",
        ]
    );
    // Structure: alternating Str / Num, leading empty Str for a
    // digit-initial string (real `re.split(r"(\d+)", "1a")` ->
    // `["", "1", "a"]`).
    assert_eq!(
        alnum_sort_key("1a"),
        vec![
            AlnumKey::Str(String::new()),
            AlnumKey::Num(1),
            AlnumKey::Str("a".to_string()),
        ]
    );
    // A short key sorts before a longer one that extends it.
    assert!(alnum_sort_key("a") < alnum_sort_key("a1"));
    // Overflow -> kept as Str, still deterministic.
    assert!(!alnum_sort_key(&format!("x{}", "9".repeat(50))).is_empty());
}

#[test]
fn build_use_expand_display_groups_and_hides() {
    let config = portage_profile::Config {
        use_expand: HashSet::from(["VIDEO_CARDS".to_string(), "CPU_FLAGS_X86".to_string()]),
        use_expand_hidden: HashSet::from(["CPU_FLAGS_X86".to_string()]),
        ..Default::default()
    };
    // VIDEO_CARDS is a USE_EXPAND var: the two video_cards_* flags
    // move into a `VIDEO_CARDS` group (prefix stripped), the plain
    // USE group is empty and so omitted entirely.
    assert_eq!(
        build_use_expand_display(BuildUseExpandDisplayArgs {
            use_flags_display: &[
                ("video_cards_amdgpu".to_string(), false),
                ("video_cards_nvidia".to_string(), true),
            ],
            config: &config,
            installed: None,
            forced: &HashSet::new(),
            all_flags: true,
            reinst_flags: &HashSet::new()
        }),
        vec![("VIDEO_CARDS".to_string(), "nvidia -amdgpu".to_string())]
    );
    // CPU_FLAGS_X86 is USE_EXPAND *and* USE_EXPAND_HIDDEN -> its group
    // is dropped from the display; a plain flag alongside it still
    // shows.
    assert_eq!(
        build_use_expand_display(BuildUseExpandDisplayArgs {
            use_flags_display: &[
                ("cpu_flags_x86_sse2".to_string(), true),
                ("plainflag".to_string(), true),
                ("video_cards_nvidia".to_string(), true),
            ],
            config: &config,
            installed: None,
            forced: &HashSet::new(),
            all_flags: true,
            reinst_flags: &HashSet::new()
        }),
        vec![
            ("USE".to_string(), "plainflag".to_string()),
            ("VIDEO_CARDS".to_string(), "nvidia".to_string()),
        ]
    );
    // No displayable flags -> no groups at all.
    assert!(
        build_use_expand_display(BuildUseExpandDisplayArgs {
            use_flags_display: &[],
            config: &config,
            installed: None,
            forced: &HashSet::new(),
            all_flags: true,
            reinst_flags: &HashSet::new()
        })
        .is_empty()
    );
}

#[test]
fn build_use_expand_display_diffs_against_the_installed_use_iuse() {
    let config = portage_profile::Config {
        use_expand: HashSet::from(["VIDEO_CARDS".to_string()]),
        ..Default::default()
    };
    let installed = InstalledUseState {
        // installed with `alpha` enabled, `beta` in IUSE but off;
        // video_cards had `nvidia` enabled.
        old_iuse: HashSet::from([
            "alpha".to_string(),
            "beta".to_string(),
            "video_cards_nvidia".to_string(),
        ]),
        old_use: HashSet::from(["alpha".to_string(), "video_cards_nvidia".to_string()]),
    };
    // `all_flags` is always on for `emerge -pv`: alpha unchanged ->
    // `alpha` (plain), beta newly enabled -> `beta*`, gamma brand-new
    // IUSE and enabled -> `gamma%*`, delta brand-new IUSE and
    // disabled -> `-delta%`, video_cards_nvidia now off (was on) ->
    // `-nvidia*`. Enabled (alpha, beta, gamma) render before the
    // disabled one (delta).
    assert_eq!(
        build_use_expand_display(BuildUseExpandDisplayArgs {
            use_flags_display: &[
                ("alpha".to_string(), true),
                ("beta".to_string(), true),
                ("delta".to_string(), false),
                ("gamma".to_string(), true),
                ("video_cards_nvidia".to_string(), false),
            ],
            config: &config,
            installed: Some(&installed),
            forced: &HashSet::new(),
            all_flags: true,
            reinst_flags: &HashSet::new()
        }),
        vec![
            ("USE".to_string(), "alpha beta* gamma%* -delta%".to_string()),
            ("VIDEO_CARDS".to_string(), "-nvidia*".to_string()),
        ]
    );
    // Even an Upgrade whose USE is completely unchanged still shows
    // the full `USE=` line under `all_flags` (real
    // `_create_use_string` returns `USE="alpha"`, non-empty).
    assert_eq!(
        build_use_expand_display(BuildUseExpandDisplayArgs {
            use_flags_display: &[("alpha".to_string(), true)],
            config: &config,
            installed: Some(&InstalledUseState {
                old_iuse: HashSet::from(["alpha".to_string()]),
                old_use: HashSet::from(["alpha".to_string()]),
            }),
            forced: &HashSet::new(),
            all_flags: true,
            reinst_flags: &HashSet::new()
        }),
        vec![("USE".to_string(), "alpha".to_string())]
    );
    // A flag the new ebuild dropped from IUSE renders `(-flag%)`,
    // `(-flag%*)` if it was enabled -- real `removed_iuse`.
    assert_eq!(
        build_use_expand_display(BuildUseExpandDisplayArgs {
            use_flags_display: &[("keep".to_string(), true)],
            config: &config,
            installed: Some(&InstalledUseState {
                old_iuse: HashSet::from([
                    "keep".to_string(),
                    "goneoff".to_string(),
                    "goneon".to_string(),
                ]),
                old_use: HashSet::from(["keep".to_string(), "goneon".to_string()]),
            }),
            forced: &HashSet::new(),
            all_flags: true,
            reinst_flags: &HashSet::new()
        }),
        vec![(
            "USE".to_string(),
            "keep (-goneoff%) (-goneon%*)".to_string()
        )]
    );
}

#[test]
fn build_use_expand_display_force_shows_reinstall_trigger_flags_at_plain_p() {
    let config = portage_profile::Config::default();
    // Installed with IUSE `gone keep`, `gone` enabled. The current
    // ebuild dropped `gone` from IUSE -> a `--newuse` reinstall whose
    // trigger set is `{gone}`.
    let installed = InstalledUseState {
        old_iuse: HashSet::from(["gone".to_string(), "keep".to_string()]),
        old_use: HashSet::from(["gone".to_string()]),
    };
    let display = [("keep".to_string(), false)];
    let reinst = HashSet::from(["gone".to_string()]);

    // Plain `-p` (`all_flags = false`): without the trigger set the
    // line is empty (`keep` unchanged-disabled -> omitted, `gone`
    // removed -> omitted). With it, the dropped trigger flag shows.
    assert_eq!(
        build_use_expand_display(BuildUseExpandDisplayArgs {
            use_flags_display: &display,
            config: &config,
            installed: Some(&installed),
            forced: &HashSet::new(),
            all_flags: false,
            reinst_flags: &HashSet::new()
        }),
        vec![]
    );
    assert_eq!(
        build_use_expand_display(BuildUseExpandDisplayArgs {
            use_flags_display: &display,
            config: &config,
            installed: Some(&installed),
            forced: &HashSet::new(),
            all_flags: false,
            reinst_flags: &reinst
        }),
        vec![("USE".to_string(), "(-gone%*)".to_string())]
    );
    // `-pv` (`all_flags = true`) is unaffected -- it already showed
    // every flag regardless of the trigger set.
    assert_eq!(
        build_use_expand_display(BuildUseExpandDisplayArgs {
            use_flags_display: &display,
            config: &config,
            installed: Some(&installed),
            forced: &HashSet::new(),
            all_flags: true,
            reinst_flags: &HashSet::new()
        }),
        build_use_expand_display(BuildUseExpandDisplayArgs {
            use_flags_display: &display,
            config: &config,
            installed: Some(&installed),
            forced: &HashSet::new(),
            all_flags: true,
            reinst_flags: &reinst
        }),
    );
    assert_eq!(
        build_use_expand_display(BuildUseExpandDisplayArgs {
            use_flags_display: &display,
            config: &config,
            installed: Some(&installed),
            forced: &HashSet::new(),
            all_flags: true,
            reinst_flags: &reinst
        }),
        vec![("USE".to_string(), "-keep (-gone%*)".to_string())]
    );
}

#[test]
fn build_use_expand_display_wraps_forced_and_masked_flags() {
    let config = portage_profile::Config {
        use_expand: HashSet::from(["VIDEO_CARDS".to_string()]),
        ..Default::default()
    };
    let forced = HashSet::from([
        "forcedon".to_string(),
        "maskedoff".to_string(),
        "video_cards_nvidia".to_string(),
    ]);
    // New install: forced/masked flags are wrapped in ( ), everything
    // else plain. `forcedon` is force-enabled, `maskedoff`
    // mask-disabled, `video_cards_nvidia` force-enabled. Enabled
    // (forcedon, plain) render before the disabled (maskedoff).
    assert_eq!(
        build_use_expand_display(BuildUseExpandDisplayArgs {
            use_flags_display: &[
                ("forcedon".to_string(), true),
                ("maskedoff".to_string(), false),
                ("plain".to_string(), true),
                ("video_cards_nvidia".to_string(), true),
            ],
            config: &config,
            installed: None,
            forced: &forced,
            all_flags: true,
            reinst_flags: &HashSet::new()
        }),
        vec![
            (
                "USE".to_string(),
                "(forcedon) plain (-maskedoff)".to_string()
            ),
            ("VIDEO_CARDS".to_string(), "(nvidia)".to_string()),
        ]
    );
    // On a diff, a masked flag brand-new to IUSE renders `(-flag)`,
    // NOT `(-flag%)` (real: `if flag not in iuse_forced: += "%"`).
    assert_eq!(
        build_use_expand_display(BuildUseExpandDisplayArgs {
            use_flags_display: &[
                ("maskednew".to_string(), false),
                ("plainnew".to_string(), false)
            ],
            config: &config,
            installed: Some(&InstalledUseState {
                old_iuse: HashSet::new(),
                old_use: HashSet::new(),
            }),
            forced: &HashSet::from(["maskednew".to_string()]),
            all_flags: true,
            reinst_flags: &HashSet::new()
        }),
        vec![("USE".to_string(), "(-maskednew) -plainnew%".to_string())]
    );
}
