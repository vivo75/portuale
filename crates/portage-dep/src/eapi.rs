//! Port of Portage's EAPI attribute table (`lib/portage/eapi.py`:
//! `_get_eapi_attrs`, `Eapi.__init__`) and `portage.eapi_is_supported`
//! (`lib/portage/__init__.py`).

use std::fmt;

/// EAPIs Portage accepts anywhere (`Eapi.ALL_EAPIS`, eapi.py:209-221).
pub const SUPPORTED_EAPIS: &[&str] = &["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];

/// Deprecated spellings `eapi_is_supported` still accepts
/// (`lib/portage/__init__.py`, `_deprecated_eapis`).
const DEPRECATED_EAPIS: &[&str] = &[
    "3_pre1", "3_pre2", "4_pre1", "5_pre1", "5_pre2", "6_pre1", "7_pre1",
];

/// Testing EAPI `eapi_is_supported` accepts (`_testing_eapis`).
const TESTING_EAPIS: &[&str] = &["9-pre1"];

/// Whether Portage accepts `eapi` anywhere an EAPI is named: the digits
/// `0`–`9` plus the testing and deprecated spellings, after stripping
/// whitespace. Never splits on `_`: `3_pre1` is accepted as-is, not as 3.
// Portage: lib/portage/__init__.py, eapi_is_supported
pub fn eapi_is_supported(eapi: &str) -> bool {
    let eapi = eapi.trim();
    SUPPORTED_EAPIS.contains(&eapi)
        || TESTING_EAPIS.contains(&eapi)
        || DEPRECATED_EAPIS.contains(&eapi)
}

/// Portage's `ValueError` from `Eapi.__init__` (eapi.py:224-229) for a
/// *supported* spelling it cannot parse (`3_pre1`, padded `" 8"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EapiValueError {
    pub input: String,
}

impl fmt::Display for EapiValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "'{}' not recognized as a valid EAPI", self.input)
    }
}

impl std::error::Error for EapiValueError {}

/// The `_get_eapi_attrs` fields some portuale code reads or S2 needs; more
/// are added when a caller needs one, citing `eapi.py`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EapiAttrs {
    pub allows_package_provided: bool,
    pub bdepend: bool,
    pub empty_groups_always_true: bool,
    pub idepend: bool,
    pub iuse_defaults: bool,
    pub iuse_effective: bool,
    pub profile_eapi_default: bool,
    pub repo_deps: bool,
    pub required_use: bool,
    pub required_use_at_most_one_of: bool,
    pub selective_src_uri_restriction: bool,
    pub slot_deps: bool,
    pub slot_operator: bool,
    pub src_uri_arrows: bool,
    pub strong_blocks: bool,
    pub use_deps: bool,
    pub use_dep_defaults: bool,
}

/// Portage `_get_eapi_attrs` (eapi.py:236-329): `None` or an unsupported
/// string gives the permissive set; a supported spelling `Eapi.__init__`
/// cannot parse (`3_pre1`, `" 8"`) is `Err`, where Portage raises
/// `ValueError` (eapi.py:224-229). Never splits on `_`.
pub fn eapi_attrs(eapi: Option<&str>) -> Result<EapiAttrs, EapiValueError> {
    match eapi {
        None => Ok(permissive_attrs()),
        Some(s) if !eapi_is_supported(s) => Ok(permissive_attrs()),
        Some(s) => {
            // Portage: eapi.py:224-229 (`Eapi.__init__`: `partition("-")[0]`).
            let key = match s.split_once('-') {
                Some((head, _)) => head,
                None => s,
            };
            if !SUPPORTED_EAPIS.contains(&key) {
                return Err(EapiValueError {
                    input: s.to_string(),
                });
            }
            let n: u8 = key.parse().expect("SUPPORTED_EAPIS are digits");
            Ok(EapiAttrs {
                allows_package_provided: n <= 6,
                bdepend: n >= 7,
                empty_groups_always_true: n <= 6,
                idepend: n >= 8,
                iuse_defaults: n >= 1,
                iuse_effective: n >= 5,
                profile_eapi_default: n >= 9,
                repo_deps: false,
                required_use: n >= 4,
                required_use_at_most_one_of: n >= 5,
                selective_src_uri_restriction: n >= 8,
                slot_deps: n >= 1,
                slot_operator: n >= 5,
                src_uri_arrows: n >= 2,
                strong_blocks: n >= 2,
                use_deps: n >= 2,
                use_dep_defaults: n >= 4,
            })
        }
    }
}

/// The permissive set for `None`/unsupported (eapi.py:247-287).
fn permissive_attrs() -> EapiAttrs {
    EapiAttrs {
        allows_package_provided: true,
        bdepend: false,
        empty_groups_always_true: false,
        idepend: false,
        iuse_defaults: true,
        iuse_effective: false,
        profile_eapi_default: false,
        repo_deps: true,
        required_use: true,
        required_use_at_most_one_of: true,
        selective_src_uri_restriction: true,
        slot_deps: true,
        slot_operator: true,
        src_uri_arrows: true,
        strong_blocks: true,
        use_deps: true,
        use_dep_defaults: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attrs_match_portage_eapi_table() {
        // Portage: lib/portage/eapi.py:247-329 (_get_eapi_attrs), 3.0.82.2;
        // every row re-checked live 2026-10-12 with
        // `python3 -c 'from portage.eapi import _get_eapi_attrs as g; ...'`.
        let a = |e: Option<&str>| eapi_attrs(e).unwrap();
        let none = a(None);
        assert!(none.repo_deps && none.slot_operator && none.use_dep_defaults);
        assert_eq!(a(Some("10")), none); // unsupported -> permissive
        assert_eq!(a(Some("9-foo")), none);
        assert!(!none.idepend && !none.iuse_effective); // eapi.py:262,264
        assert_eq!(a(Some("9-pre1")), a(Some("9"))); // partition("-")[0]
        for bad in ["3_pre1", "7_pre1", " 8", "8 "] {
            assert!(
                eapi_attrs(Some(bad)).is_err(),
                "Portage raises ValueError for {bad:?}"
            );
        }
        for e in ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"] {
            assert!(!a(Some(e)).repo_deps, "repo_deps is False for EAPI {e}");
        }
        assert!(!a(Some("0")).slot_deps && a(Some("1")).slot_deps);
        assert!(!a(Some("1")).strong_blocks && a(Some("2")).strong_blocks);
        assert!(!a(Some("1")).use_deps && a(Some("2")).use_deps);
        assert!(!a(Some("3")).use_dep_defaults && a(Some("4")).use_dep_defaults);
        assert!(!a(Some("4")).slot_operator && a(Some("5")).slot_operator);
        assert!(a(Some("6")).empty_groups_always_true && !a(Some("7")).empty_groups_always_true);
        assert!(!a(Some("7")).idepend && a(Some("8")).idepend);
        assert!(a(Some("6")).allows_package_provided && !a(Some("7")).allows_package_provided);
        assert!(!a(Some("0")).iuse_defaults && a(Some("1")).iuse_defaults);
        assert!(!a(Some("4")).iuse_effective && a(Some("5")).iuse_effective);
        assert!(!a(Some("6")).bdepend && a(Some("7")).bdepend);
        assert!(!a(Some("3")).required_use && a(Some("4")).required_use);
        assert!(
            !a(Some("4")).required_use_at_most_one_of && a(Some("5")).required_use_at_most_one_of
        );
        assert!(!a(Some("1")).src_uri_arrows && a(Some("2")).src_uri_arrows);
        assert!(
            !a(Some("7")).selective_src_uri_restriction
                && a(Some("8")).selective_src_uri_restriction
        );
        assert!(!a(Some("8")).profile_eapi_default && a(Some("9")).profile_eapi_default);
    }

    #[test]
    fn is_supported_matches_portage() {
        // Portage: lib/portage/__init__.py, eapi_is_supported, 3.0.82.2;
        // re-checked live 2026-10-12 with
        // `python3 -c 'import portage; print([portage.eapi_is_supported(e) for e in ...])'`.
        for e in ["3_pre1", "9-pre1", " 8 ", " 8", "8 "] {
            assert!(eapi_is_supported(e), "{e:?}");
        }
        for e in ["", "10", "9-foo", "8-pre1"] {
            assert!(!eapi_is_supported(e), "{e:?}");
        }
    }
}
