//! Port of Portage's EAPI attribute table (`lib/portage/eapi.py`:
//! `_eapi_attrs`, `_get_eapi_attrs`, `Eapi.__init__`) and
//! `portage.eapi_is_supported` (`lib/portage/__init__.py`).
//!
//! The struct carries all 38 `_eapi_attrs` fields; later slices wire the
//! phase-side ones to their call sites (plan #348 S2/S5/S6).

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

/// All 38 `_get_eapi_attrs` fields (eapi.py:163-205), in Portage's order.
/// Uppercase Portage names read lowercase here (`exports_PORTDIR` is
/// `exports_portdir`): an uppercase field would trip `non_snake_case`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EapiAttrs {
    pub allows_package_provided: bool,
    pub bdepend: bool,
    pub broot: bool,
    pub dosed_dohard: bool,
    pub empty_groups_always_true: bool,
    pub exports_aa: bool,
    pub exports_ebuild_phase_func: bool,
    pub exports_eclassdir: bool,
    pub exports_kv: bool,
    pub exports_merge_type: bool,
    pub exports_pms_vars: bool,
    pub exports_portdir: bool,
    pub exports_replace_vars: bool,
    pub idepend: bool,
    pub iuse_defaults: bool,
    pub iuse_effective: bool,
    pub posixish_locale: bool,
    pub path_variables_end_with_trailing_slash: bool,
    pub pkg_pretend: bool,
    pub prefix: bool,
    pub profile_file_dirs: bool,
    pub profile_eapi_default: bool,
    pub rdepend_depend: bool,
    pub repo_deps: bool,
    pub required_use: bool,
    pub required_use_at_most_one_of: bool,
    pub selective_src_uri_restriction: bool,
    pub slot_operator: bool,
    pub slot_deps: bool,
    pub src_prepare_src_configure: bool,
    pub src_uri_arrows: bool,
    pub stablemask: bool,
    pub use_stable: bool,
    pub strong_blocks: bool,
    pub symlink_rewrite: bool,
    pub sysroot: bool,
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
                broot: n >= 7,
                dosed_dohard: n <= 3,
                empty_groups_always_true: n <= 6,
                exports_aa: n <= 3,
                exports_ebuild_phase_func: n >= 5,
                exports_eclassdir: n <= 6,
                exports_kv: n <= 3,
                exports_merge_type: n >= 4,
                exports_pms_vars: n <= 8,
                exports_portdir: n <= 6,
                exports_replace_vars: n >= 4,
                idepend: n >= 8,
                iuse_defaults: n >= 1,
                iuse_effective: n >= 5,
                posixish_locale: n >= 6,
                path_variables_end_with_trailing_slash: n <= 6,
                pkg_pretend: n >= 4,
                prefix: n >= 3,
                profile_file_dirs: n >= 7,
                profile_eapi_default: n >= 9,
                rdepend_depend: n <= 3,
                repo_deps: false,
                required_use: n >= 4,
                required_use_at_most_one_of: n >= 5,
                selective_src_uri_restriction: n >= 8,
                slot_operator: n >= 5,
                slot_deps: n >= 1,
                src_prepare_src_configure: n >= 2,
                src_uri_arrows: n >= 2,
                stablemask: n >= 5,
                use_stable: n >= 9,
                strong_blocks: n >= 2,
                symlink_rewrite: n <= 8,
                sysroot: n >= 7,
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
        broot: true,
        dosed_dohard: false,
        empty_groups_always_true: false,
        exports_aa: false,
        exports_ebuild_phase_func: true,
        exports_eclassdir: false,
        exports_kv: false,
        exports_merge_type: true,
        exports_pms_vars: true,
        exports_portdir: true,
        exports_replace_vars: true,
        idepend: false,
        iuse_defaults: true,
        iuse_effective: false,
        posixish_locale: false,
        path_variables_end_with_trailing_slash: false,
        pkg_pretend: true,
        prefix: true,
        profile_file_dirs: false,
        profile_eapi_default: false,
        rdepend_depend: false,
        repo_deps: true,
        required_use: true,
        required_use_at_most_one_of: true,
        selective_src_uri_restriction: true,
        slot_operator: true,
        slot_deps: true,
        src_prepare_src_configure: true,
        src_uri_arrows: true,
        stablemask: true,
        use_stable: true,
        strong_blocks: true,
        symlink_rewrite: false,
        sysroot: true,
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
    fn attrs_match_generated_oracle_tsv() {
        // Every row of testdata/eapi-attrs-3.0.82.2.tsv, generated from
        // Portage 3.0.82.2 by LLM/tools/eapi_attrs_table.py. The expected
        // struct is built field-by-field from the TSV rows, so a field
        // added without a row fails: the literal misses the new field at
        // compile time and `cell` panics on the missing row at run time.
        // Portage names (some uppercase) map to the lowercase struct
        // fields; see the struct docs.
        use std::collections::HashMap;
        let mut ok_cells: HashMap<(String, String), bool> = HashMap::new();
        let mut err_rows = 0;
        let mut rows = 0;
        for row in include_str!("testdata/eapi-attrs-3.0.82.2.tsv")
            .lines()
            .skip(1)
        {
            if row.is_empty() {
                continue;
            }
            rows += 1;
            let [eapi, field, verdict, value]: [&str; 4] =
                row.splitn(4, '\t').collect::<Vec<_>>().try_into().unwrap();
            match verdict {
                "OK" => {
                    let v = match value {
                        "True" => true,
                        "False" => false,
                        _ => unreachable!("unknown value {value:?}"),
                    };
                    assert!(
                        ok_cells
                            .insert((eapi.to_string(), field.to_string()), v)
                            .is_none(),
                        "duplicate row {row:?}"
                    );
                }
                "ERR" => {
                    assert_eq!(field, "-", "{row:?}");
                    let err = eapi_attrs(Some(eapi)).expect_err(eapi);
                    assert_eq!(err.to_string(), value, "{eapi:?}");
                    err_rows += 1;
                }
                _ => unreachable!("unknown verdict {verdict:?}"),
            }
        }
        // 13 eapis x 38 fields plus the 2 ERR spellings; the generator
        // asserts both counts too.
        assert_eq!(err_rows, 2);
        assert_eq!(rows, 13 * 38 + 2);
        let mut cell = |eapi: &str, field: &str| {
            ok_cells
                .remove(&(eapi.to_string(), field.to_string()))
                .unwrap_or_else(|| panic!("no TSV row for {eapi:?} {field:?}"))
        };
        for eapi in [
            "None", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "9-pre1", "99",
        ] {
            let arg = if eapi == "None" { None } else { Some(eapi) };
            let want = EapiAttrs {
                allows_package_provided: cell(eapi, "allows_package_provided"),
                bdepend: cell(eapi, "bdepend"),
                broot: cell(eapi, "broot"),
                dosed_dohard: cell(eapi, "dosed_dohard"),
                empty_groups_always_true: cell(eapi, "empty_groups_always_true"),
                exports_aa: cell(eapi, "exports_AA"),
                exports_ebuild_phase_func: cell(eapi, "exports_EBUILD_PHASE_FUNC"),
                exports_eclassdir: cell(eapi, "exports_ECLASSDIR"),
                exports_kv: cell(eapi, "exports_KV"),
                exports_merge_type: cell(eapi, "exports_merge_type"),
                exports_pms_vars: cell(eapi, "exports_pms_vars"),
                exports_portdir: cell(eapi, "exports_PORTDIR"),
                exports_replace_vars: cell(eapi, "exports_replace_vars"),
                idepend: cell(eapi, "idepend"),
                iuse_defaults: cell(eapi, "iuse_defaults"),
                iuse_effective: cell(eapi, "iuse_effective"),
                posixish_locale: cell(eapi, "posixish_locale"),
                path_variables_end_with_trailing_slash: cell(
                    eapi,
                    "path_variables_end_with_trailing_slash",
                ),
                pkg_pretend: cell(eapi, "pkg_pretend"),
                prefix: cell(eapi, "prefix"),
                profile_file_dirs: cell(eapi, "profile_file_dirs"),
                profile_eapi_default: cell(eapi, "profile_eapi_default"),
                rdepend_depend: cell(eapi, "rdepend_depend"),
                repo_deps: cell(eapi, "repo_deps"),
                required_use: cell(eapi, "required_use"),
                required_use_at_most_one_of: cell(eapi, "required_use_at_most_one_of"),
                selective_src_uri_restriction: cell(eapi, "selective_src_uri_restriction"),
                slot_operator: cell(eapi, "slot_operator"),
                slot_deps: cell(eapi, "slot_deps"),
                src_prepare_src_configure: cell(eapi, "src_prepare_src_configure"),
                src_uri_arrows: cell(eapi, "src_uri_arrows"),
                stablemask: cell(eapi, "stablemask"),
                use_stable: cell(eapi, "use_stable"),
                strong_blocks: cell(eapi, "strong_blocks"),
                symlink_rewrite: cell(eapi, "symlink_rewrite"),
                sysroot: cell(eapi, "sysroot"),
                use_deps: cell(eapi, "use_deps"),
                use_dep_defaults: cell(eapi, "use_dep_defaults"),
            };
            assert_eq!(eapi_attrs(arg).unwrap(), want, "EAPI {eapi:?}");
        }
        assert!(ok_cells.is_empty(), "extra TSV rows: {ok_cells:?}");
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
