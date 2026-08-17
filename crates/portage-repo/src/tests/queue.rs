use super::*;

#[test]
fn exclude_leaves_an_already_installed_package_alone_even_with_update() {
    // --exclude matching it overrides --update entirely.
    // Portage: _want_update_pkg / _replace_installed_atom (excluded-checked-first).
    assert_eq!(
        resolve_update_excluded(
            "dev-libs",
            "upgradepkg",
            &["dev-libs/upgradepkg".to_string()]
        ),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn exclude_matches_via_a_wildcard_atom_too() {
    // Portage's WildcardPackageSet accepts wildcard atoms too.
    assert_eq!(
        resolve_update_excluded("dev-libs", "upgradepkg", &["dev-libs/*".to_string()]),
        PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string()
        }
    );
}

#[test]
fn exclude_does_not_affect_a_non_matching_package() {
    // An unrelated --exclude atom changes nothing.
    assert_eq!(
        resolve_update_excluded(
            "dev-libs",
            "upgradepkg",
            &["dev-libs/does-not-exist".to_string()]
        ),
        PretendOutcome::Upgrade {
            from: "1.0".to_string(),
            to: "2.0".to_string()
        }
    );
}
