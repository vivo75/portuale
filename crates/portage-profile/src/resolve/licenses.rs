//! ACCEPT_LICENSE, ACCEPT_PROPERTIES, ACCEPT_RESTRICT and their package.* files.

use crate::resolve::*;

/// ACCEPT_LICENSE, ACCEPT_PROPERTIES, ACCEPT_RESTRICT and their package.* files.
pub(crate) fn fold_licenses_and_accepts(
    all_repos: &[(String, PathBuf)],
    config_root: &Path,
    config: &mut Config,
    scalars: &HashMap<String, String>,
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
    let package_license_lines =
        read_config_lines(&config_root.join("etc/portage/package.license"))?;
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
    config.package_properties = parse_package_license_lines(&read_config_lines(
        &config_root.join("etc/portage/package.properties"),
    )?);
    config.package_accept_restrict = parse_package_license_lines(&read_config_lines(
        &config_root.join("etc/portage/package.accept_restrict"),
    )?);
    Ok(())
}
