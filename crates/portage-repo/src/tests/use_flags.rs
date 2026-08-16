use super::*;

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
