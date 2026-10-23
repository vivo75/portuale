use super::*;
use crate::dep_validate::{DepError, DepErrorCategory, invalid_dep_reasons, validate_dep_string};

fn t341_repo() -> Vec<RepoConfig> {
    vec![RepoConfig {
        name: "t341".to_string(),
        location: fixtures_root().join("repo-341"),
        priority: 10,
        is_main: false,
        masters: Vec::new(),
        profile_formats: Vec::new(),
        cache_formats: Vec::new(),
        aliases: Vec::new(),
        sync_type: None,
        sync_uri: None,
        volatile: false,
        module_specific_options: Vec::new(),
    }]
}

fn t341_candidate(package: &str, version: &str) -> Candidate {
    let repos = t341_repo();
    list_candidates(&repos, "app-misc", package)
        .expect("t341 list_candidates")
        .iter()
        .find(|c| c.version == version)
        .expect("t341 version")
        .clone()
}

/// Portage: `emerge -pv app-misc/t341-badrepo`
/// (LLM/evidence/2026-10-11-341-s0/portage/A-ebuild-t341-badrepo.stderr,
/// container `localhost/test-portuale:latest`).
#[test]
fn repo_dep_in_eapi8_is_invalid_with_portage_text() {
    let syntax = |t: &str| {
        Err(DepError {
            category: DepErrorCategory::Syntax,
            text: t.to_string(),
        })
    };
    assert_eq!(
        validate_dep_string("dev-libs/t341-dep::gentoo", "8", &|_| true),
        syntax("Invalid atom (dev-libs/t341-dep::gentoo), token 1")
    );
}

/// Portage: `python3 -c use_reduce('x? ( a/b ) c/d::x', eapi='8', …)`
/// gives token 5 (live probe, 2026-10-11; S0 cell `t341-token`).
#[test]
fn token_position_counts_parens_and_conditionals() {
    assert_eq!(
        validate_dep_string("x? ( a/b ) c/d::x", "8", &|_| true)
            .expect_err("bad trailing atom fails")
            .text,
        "Invalid atom (c/d::x), token 5"
    );
}

/// Portage: lib/_emerge/Package.py:330-356; `emerge -pv
/// app-misc/t341-twobuilt`
/// (LLM/evidence/2026-10-11-341-s0/portage/A-ebuild-t341-twobuilt.stderr).
#[test]
fn unbuilt_ebuild_reports_each_built_slot_operator_atom() {
    let reasons = invalid_dep_reasons(
        &t341_candidate("t341-twobuilt", "1"),
        "app-misc",
        "t341-twobuilt",
        &test_config(),
    );
    assert_eq!(
        reasons,
        vec![
            "invalid: PDEPEND: Improper context for slot-operator \"built\" atom syntax: dev-libs/t341-dep:0/1=",
            "invalid: PDEPEND: Improper context for slot-operator \"built\" atom syntax: dev-libs/t341-dep:0/2=",
        ]
    );
}

/// Every other S0 Group-A cell: expected `invalid:` lines pasted from the
/// matching `LLM/evidence/2026-10-11-341-s0/portage/A-ebuild-<cell>.stderr`
/// capture (Portage 3.0.82.2, container `localhost/test-portuale:latest`).
#[test]
fn s0_group_a_cells_mask_with_portage_text() {
    let config = test_config();
    let reasons = |package: &str| {
        invalid_dep_reasons(&t341_candidate(package, "1"), "app-misc", package, &config)
    };
    // A-ebuild-t341-badbid.stderr
    assert_eq!(
        reasons("t341-badbid"),
        vec!["invalid: RDEPEND: Invalid atom (=dev-libs/t341-dep-1-2), token 1"]
    );
    // A-ebuild-t341-badcond.stderr (categorized: no `Invalid atom` wrapper)
    assert_eq!(
        reasons("t341-badcond"),
        vec![
            "invalid: RDEPEND: USE flag 'x' referenced in conditional 'x?' \
             in atom 'dev-libs/t341-dep[x?]' is not in IUSE"
        ]
    );
    // A-ebuild-t341-badslot.stderr
    assert_eq!(
        reasons("t341-badslot"),
        vec!["invalid: RDEPEND: Invalid atom (dev-libs/t341-dep:0*), token 1"]
    );
    // A-ebuild-t341-built.stderr
    assert_eq!(
        reasons("t341-built"),
        vec![
            "invalid: RDEPEND: Improper context for slot-operator \"built\" \
             atom syntax: dev-libs/t341-dep:0/1="
        ]
    );
    // A-ebuild-t341-token.stderr
    assert_eq!(
        reasons("t341-token"),
        vec!["invalid: RDEPEND: Invalid atom (dev-libs/t341-dep::gentoo), token 5"]
    );
    // A-ebuild-t341-twokeys.stderr (same category: key order)
    assert_eq!(
        reasons("t341-twokeys"),
        vec![
            "invalid: DEPEND: Invalid atom (dev-libs/t341-dep::a), token 1",
            "invalid: RDEPEND: Invalid atom (dev-libs/t341-dep:0*), token 1",
        ]
    );
    // A-ebuild-t341-mixcat.stderr (categories in first-insertion order:
    // BDEPEND IUSE.missing, RDEPEND IUSE.missing, DEPEND syntax)
    assert_eq!(
        reasons("t341-mixcat"),
        vec![
            "invalid: BDEPEND: USE flag 'y' referenced in conditional 'y?' is not in IUSE",
            "invalid: RDEPEND: USE flag 'z' referenced in conditional 'z?' is not in IUSE",
            "invalid: DEPEND: Invalid atom (dev-libs/t341-dep::a), token 1",
        ]
    );
    // A-ebuild-t341-emptygrp.stderr
    assert_eq!(
        reasons("t341-emptygrp"),
        vec!["invalid: RDEPEND: expected: dependency string, got: ')', token 2"]
    );
    // A-ebuild-t341-glued.stderr
    assert_eq!(
        reasons("t341-glued"),
        vec![
            "invalid: RDEPEND: missing whitespace around '(' \
             at 'dev-libs/t341-dep(', token 1"
        ]
    );
    // A-ebuild-t341-xor.stderr (`^^` is a plain token in *DEPEND)
    assert_eq!(
        reasons("t341-xor"),
        vec!["invalid: RDEPEND: Invalid atom (^^), token 1"]
    );
    // A-ebuild-t341-qq.stderr (`??` is a conditional on flag `?`)
    assert_eq!(
        reasons("t341-qq"),
        vec!["invalid: RDEPEND: USE flag '?' referenced in conditional '??' is not in IUSE"]
    );
    // A-ebuild-t341-condfirst.stderr (atom inside the bad conditional first)
    assert_eq!(
        reasons("t341-condfirst"),
        vec!["invalid: RDEPEND: Invalid atom (dev-libs/t341-dep::a), token 3"]
    );
}

/// S0 controls that stay visible: `t341-good` (`:=` + `!!` block) and
/// `t341-eapi7` (`[x(+)]` valid from EAPI 4; the `x` default flag needs no
/// IUSE membership — live probe, 2026-10-11).
#[test]
fn s0_good_and_eapi7_cells_stay_visible() {
    let config = test_config();
    assert!(reasons_of("t341-good", &config).is_empty());
    assert!(reasons_of("t341-eapi7", &config).is_empty());
}

fn reasons_of(package: &str, config: &portage_profile::Config) -> Vec<String> {
    invalid_dep_reasons(&t341_candidate(package, "1"), "app-misc", package, config)
}

/// A built (binary) candidate skips the `Improper context` loop
/// (Package.py:347-356 runs it only `if not self.built`): the same `:0/1=`
/// atom that masks the ebuild is fine on the binary.
#[test]
fn built_binary_skips_the_improper_context_check() {
    let binary = Candidate {
        source: CandidateSource::Binary,
        binary_deps: HashMap::from([("RDEPEND".to_string(), "dev-libs/t341-dep:0/1=".to_string())]),
        user_patches: String::new(),
        binary_use: Some(HashSet::new()),
        eapi: "8".to_string(),
        iuse: String::new(),
        repo_name: "t341".to_string(),
        version: "1".to_string(),
        ..candidate("1", &["amd64"])
    };
    assert!(invalid_dep_reasons(&binary, "app-misc", "t341-built", &test_config()).is_empty());
}

/// The visibility memo keys on the instance's own dep inputs: a masked
/// binary must not poison its ebuild twin's lookup (nor vice versa). The
/// ebuild reads `repo-341` metadata (valid `RDEPEND`); the binary twin
/// carries a rewritten, invalid one (S0 `F-binary-rewritten` shape).
#[test]
fn visibility_memo_distinguishes_an_ebuild_from_its_binary_twin() {
    let config = test_config();
    let ebuild = t341_candidate("t341-good", "1");
    let mut binary = ebuild.clone();
    binary.source = CandidateSource::Binary;
    binary.binary_use = Some(HashSet::new());
    binary.binary_deps = HashMap::from([(
        "RDEPEND".to_string(),
        "dev-libs/t341-dep::gentoo".to_string(),
    )]);
    // Either order: the binary is masked, the ebuild stays visible.
    assert!(!is_visible(&binary, "app-misc", "t341-good", &config));
    assert!(is_visible(&ebuild, "app-misc", "t341-good", &config));
    assert!(is_visible(&ebuild, "app-misc", "t341-good", &config));
    assert!(!is_visible(&binary, "app-misc", "t341-good", &config));
}
