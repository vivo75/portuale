//! ACCEPT_LICENSE, ACCEPT_PROPERTIES, ACCEPT_RESTRICT and their package.* files.

use crate::resolve::*;

/// ACCEPT_LICENSE, ACCEPT_PROPERTIES, ACCEPT_RESTRICT and their package.* files.
pub(crate) fn fold_licenses_and_accepts(
    all_repos: &[(String, PathBuf)],
    config_root: &Path,
    config: &mut Config,
    scalars: &HashMap<String, String>,
    warnings: &mut Vec<ProfileWarning>,
) -> Result<(), Error> {
    // `license_groups` lives in each repo's `profiles/` dir (extend, don't replace),
    // read before ACCEPT_LICENSE/package.license expand `@group`.
    let mut license_groups: HashMap<String, Vec<String>> = HashMap::new();
    for (_name, location) in all_repos {
        for (name, members) in parse_license_groups_lines(&read_config_lines(
            &location.join("profiles").join("license_groups"),
        )?) {
            license_groups.entry(name).or_default().extend(members);
        }
    }
    for (name, members) in parse_license_groups_lines(&read_config_lines(
        &config_root.join("etc/portage/license_groups"),
    )?) {
        license_groups.entry(name).or_default().extend(members);
    }
    config.license_groups = license_groups;

    // ACCEPT_LICENSE: last-level-wins scalar, `* -@EULA` default when unset.
    // `scalars` already holds the profile chain + make.conf value.
    let accept_license_str = scalars
        .get("ACCEPT_LICENSE")
        .cloned()
        .unwrap_or_else(|| "* -@EULA".to_string());
    let accept_license_tokens: Vec<String> = accept_license_str
        .split_whitespace()
        .map(String::from)
        .collect();
    config.accept_license = expand_license_tokens(&accept_license_tokens, &config.license_groups);

    // package.license: user-level only (no profile format replicated).
    // Lines validate with `eapi=None`, wildcards and repo on, but WITHOUT
    // build-ids (Portage never passes `allow_build_id` there); use-dep
    // lines are kept (the LicenseManager takes any atom).
    // Portage: LicenseManager.py:37-42, oracle Q2e
    let package_license_lines = filter_atom_lines(
        &read_config_lines_with_sources(&config_root.join("etc/portage/package.license"))?,
        &user_atom_rules(true, false),
        AtomLineKind::Dict { allow_use: true },
        WARN_RANK_LICENSE,
        warnings,
    );
    config.package_license = parse_package_license_lines(&package_license_lines)
        .into_iter()
        .map(|(atom, tokens)| (atom, expand_license_tokens(&tokens, &config.license_groups)))
        .collect();

    // ACCEPT_PROPERTIES/ACCEPT_RESTRICT: last-level-wins scalars, `*`
    // default, no `@group` expansion (unlike ACCEPT_LICENSE).
    config.accept_properties = scalars
        .get("ACCEPT_PROPERTIES")
        .map(|s| s.split_whitespace().map(String::from).collect())
        .unwrap_or_else(|| vec!["*".to_string()]);
    config.accept_restrict = scalars
        .get("ACCEPT_RESTRICT")
        .map(|s| s.split_whitespace().map(String::from).collect())
        .unwrap_or_else(|| vec!["*".to_string()]);

    // package.properties/package.accept_restrict: user-level only, same shape.
    // Both validate with `eapi=None`, wildcards, repo and build-ids on;
    // use-dep lines are kept (config.py takes any atom).
    // Portage: config.py:857-902, oracle Q2e
    let accept_rules = user_atom_rules(true, true);
    let license_dict = AtomLineKind::Dict { allow_use: true };
    config.package_properties = parse_package_license_lines(&filter_atom_lines(
        &read_config_lines_with_sources(&config_root.join("etc/portage/package.properties"))?,
        &accept_rules,
        license_dict,
        WARN_RANK_LICENSE,
        warnings,
    ));
    config.package_accept_restrict = parse_package_license_lines(&filter_atom_lines(
        &read_config_lines_with_sources(&config_root.join("etc/portage/package.accept_restrict"))?,
        &accept_rules,
        license_dict,
        WARN_RANK_LICENSE,
        warnings,
    ));
    Ok(())
}
