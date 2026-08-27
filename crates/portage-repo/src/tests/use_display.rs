use super::*;

#[test]
fn build_use_expand_display_groups_and_hides() {
    let config = portage_profile::Config {
        use_expand: HashSet::from(["VIDEO_CARDS".to_string(), "CPU_FLAGS_X86".to_string()]),
        use_expand_hidden: HashSet::from(["CPU_FLAGS_X86".to_string()]),
        ..Default::default()
    };
    // VIDEO_CARDS is a USE_EXPAND var: the two video_cards_* flags
    // move into a `VIDEO_CARDS` group; the empty USE group is omitted.
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
    // CPU_FLAGS_X86 is USE_EXPAND *and* USE_EXPAND_HIDDEN, so its
    // group is dropped; a plain flag alongside it still shows.
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
        // Installed with `alpha` on and `beta` off; video_cards had
        // `nvidia` on.
        old_iuse: HashSet::from([
            "alpha".to_string(),
            "beta".to_string(),
            "video_cards_nvidia".to_string(),
        ]),
        old_use: HashSet::from(["alpha".to_string(), "video_cards_nvidia".to_string()]),
    };
    // `all_flags` is always on for `emerge -pv`: every flag shows
    // (`*` = changed vs installed, `%` = new to IUSE).
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
    // Even an Upgrade whose USE is unchanged still shows the full
    // `USE=` line (Portage returns `USE="alpha"`, non-empty).
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
    // `(-flag%*)` if it was enabled.
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
