//! ACCEPT_LICENSE, ACCEPT_PROPERTIES, ACCEPT_RESTRICT and their package.* files.

use crate::resolve::*;

/// ACCEPT_LICENSE, ACCEPT_PROPERTIES, ACCEPT_RESTRICT and their package.* files.
pub(crate) fn fold_licenses_and_accepts(
    all_repos: &[(String, PathBuf)],
    config_root: &Path,
    config: &mut Config,
    scalars: &HashMap<String, String>,
) -> Result<(), Error> {
    // license_groups: real `LicenseManager._read_license_groups`
    // (`LicenseManager.py:47`) over `LocationsManager.profile_locations`
    // (`LocationsManager.py:432`) -- which is the `profiles/` directory
    // of the *main repo and each overlay*, NOT the individual
    // profile-chain levels (real gentoo puts `license_groups` at
    // `<repo>/profiles/license_groups`, never in a `profiles/<foo>/`
    // profile dir -- verified live against a real tree). Then the
    // user-level `/etc/portage/license_groups`. "extend, don't
    // stack/replace" semantics -- see `parse_license_groups_lines`. Read
    // before ACCEPT_LICENSE/package.license below, both of which need
    // the full, final group map to expand `@group` tokens against.
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

    // ACCEPT_LICENSE: last-level-wins scalar (see `accept_license`'s own
    // doc comment for why, and the real "* -@EULA" default portuale
    // replicates when it's never set anywhere at all) -- `scalars`
    // already holds whatever the profile chain + make.conf left it as,
    // via the same catch-all `scalars.insert` every other scalar
    // variable already goes through in `process_lines`/
    // `process_make_conf_file`.
    let accept_license_str = scalars
        .get("ACCEPT_LICENSE")
        .cloned()
        .unwrap_or_else(|| "* -@EULA".to_string());
    let accept_license_tokens: Vec<String> = accept_license_str
        .split_whitespace()
        .map(String::from)
        .collect();
    config.accept_license = expand_license_tokens(&accept_license_tokens, &config.license_groups);

    // package.license: user-level only -- see `package_license`'s own
    // doc comment for the deliberately-not-replicated `profile-license`
    // profile-format and `*/*`-global-extraction real quirks.
    let package_license_lines =
        read_config_lines(&config_root.join("etc/portage/package.license"))?;
    config.package_license = parse_package_license_lines(&package_license_lines)
        .into_iter()
        .map(|(atom, tokens)| (atom, expand_license_tokens(&tokens, &config.license_groups)))
        .collect();

    // ACCEPT_PROPERTIES/ACCEPT_RESTRICT: last-level-wins scalars, real
    // "*" default (see `accept_properties`'s own doc comment) -- no
    // `@group` expansion for either, unlike ACCEPT_LICENSE/
    // package.license just above.
    config.accept_properties = scalars
        .get("ACCEPT_PROPERTIES")
        .map(|s| s.split_whitespace().map(String::from).collect())
        .unwrap_or_else(|| vec!["*".to_string()]);
    config.accept_restrict = scalars
        .get("ACCEPT_RESTRICT")
        .map(|s| s.split_whitespace().map(String::from).collect())
        .unwrap_or_else(|| vec!["*".to_string()]);

    // package.properties/package.accept_restrict: user-level only, same
    // "atom + raw tokens" shape package.license already reads (reused
    // directly -- see parse_package_license_lines's own doc comment).
    config.package_properties = parse_package_license_lines(&read_config_lines(
        &config_root.join("etc/portage/package.properties"),
    )?);
    config.package_accept_restrict = parse_package_license_lines(&read_config_lines(
        &config_root.join("etc/portage/package.accept_restrict"),
    )?);
    Ok(())
}
