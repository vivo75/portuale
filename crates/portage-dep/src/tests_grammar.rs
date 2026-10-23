//! S2 grammar tests: the oracle battery plus focused EAPI/conditional/build-id pins.
//!
//! The battery TSV is generated from Portage 3.0.83-r1 by
//! `LLM/tools/atom_oracle_table.py` (re-probed live, never hand-copied).
//! The 3.0.82.2 table is kept beside it (`testdata/atoms-3.0.82.2.tsv`).
//! Focused tests cite `LLM/evidence/2026-10-11-341-oracle/probe2.py` and the
//! `lib/portage/dep/__init__.py` line the behaviour comes from.

use super::*;

// Portage `eapi=` as a mode name in the TSV battery.
fn mode_options(mode: &str) -> (AtomOptions, &'static str) {
    match mode {
        "None" => (AtomOptions::default(), ""),
        "e5" => (
            AtomOptions {
                eapi: AtomEapi::Known(5),
                ..AtomOptions::default()
            },
            "5",
        ),
        "e7" => (
            AtomOptions {
                eapi: AtomEapi::Known(7),
                ..AtomOptions::default()
            },
            "7",
        ),
        "e8" => (
            AtomOptions {
                eapi: AtomEapi::Known(8),
                ..AtomOptions::default()
            },
            "8",
        ),
        "e9" => (
            AtomOptions {
                eapi: AtomEapi::Known(9),
                ..AtomOptions::default()
            },
            "9",
        ),
        "eX" => (
            AtomOptions {
                eapi: AtomEapi::Unsupported,
                ..AtomOptions::default()
            },
            "10",
        ),
        "e8+bid" => (
            AtomOptions {
                eapi: AtomEapi::Known(8),
                allow_build_id: Some(true),
                ..AtomOptions::default()
            },
            "8",
        ),
        "None-repo" => (
            AtomOptions {
                allow_repo: Some(false),
                allow_build_id: Some(false),
                ..AtomOptions::default()
            },
            "",
        ),
        "wild" => (
            AtomOptions {
                allow_wildcard: true,
                allow_repo: Some(true),
                ..AtomOptions::default()
            },
            "",
        ),
        _ => panic!("unknown TSV mode {mode:?}"),
    }
}

// Same field list as the generator (`LLM/tools/atom_oracle_table.py`).
fn render_fields(atom: &Atom) -> String {
    let version = match (&atom.version, &atom.revision) {
        (Some(v), Some(r)) => format!("{v}-r{r}"),
        (Some(v), None) => v.clone(),
        (None, _) => String::new(),
    };
    let blocker = match atom.blocker {
        Blocker::None => "",
        Blocker::Weak => "!",
        Blocker::Strong => "!!",
    };
    format!(
        "cp={}/{} op={} ver={} slot={} sub={} sop={} repo={} bid={} use={} blocker={} ext={}",
        atom.category,
        atom.package,
        atom.operator.as_str(),
        version,
        atom.slot.as_deref().unwrap_or(""),
        atom.sub_slot.as_deref().unwrap_or(""),
        match &atom.slot_operator {
            None => "",
            Some(SlotOperator::Star) => "*",
            Some(SlotOperator::Equals) => "=",
        },
        atom.repo.as_deref().unwrap_or(""),
        atom.build_id.as_deref().unwrap_or(""),
        atom.use_deps
            .as_ref()
            .map(|deps| deps
                .iter()
                .map(|ud| {
                    let default = match ud.default {
                        None => "",
                        Some(UseDepDefault::Enabled) => "(+)",
                        Some(UseDepDefault::Disabled) => "(-)",
                    };
                    let (prefix, suffix) = match ud.op {
                        UseDepOp::Enabled => ("", ""),
                        UseDepOp::Disabled => ("-", ""),
                        UseDepOp::IfParentEnabled => ("", "?"),
                        UseDepOp::IfParentDisabled => ("!", "?"),
                        UseDepOp::EqualParent => ("", "="),
                        UseDepOp::OppositeParent => ("!", "="),
                    };
                    format!("{prefix}{}{default}{suffix}", ud.flag)
                })
                .collect::<Vec<_>>()
                .join(","))
            .unwrap_or_default(),
        blocker,
        if atom.extended { 1 } else { 0 },
    )
}

/// Every row of testdata/atoms-3.0.83.tsv, generated from Portage 3.0.83-r1
/// by LLM/tools/atom_oracle_table.py (oracle probe battery, 2026-10-23).
/// The `S3OK` rows need the wildcard branch (S3); rows whose verdict starts
/// with `S3` but is not `S3OK` stay skipped.
#[test]
fn grammar_matches_portage_on_the_oracle_battery() {
    let mut rows = 0;
    for row in include_str!("testdata/atoms-3.0.83.tsv").lines().skip(1) {
        if row.is_empty() {
            continue;
        }
        rows += 1;
        let [atom, mode, verdict, detail]: [&str; 4] =
            row.splitn(4, '\t').collect::<Vec<_>>().try_into().unwrap();
        if verdict.starts_with("S3") && verdict != "S3OK" {
            continue;
        }
        let (opts, eapi_str) = mode_options(mode);
        let got = parse_atom_with(atom, &opts, eapi_str);
        match verdict {
            "OK" | "S3OK" => {
                assert_eq!(render_fields(&got.expect(atom)), detail, "{atom} [{mode}]")
            }
            "ERR" => assert_eq!(got.expect_err(atom).message, detail, "{atom} [{mode}]"),
            _ => unreachable!("unknown verdict {verdict:?}"),
        }
    }
    // 83 oracle strings x 9 modes; the generator asserts both counts too.
    assert_eq!(rows, 83 * 9);
}

// Portage: probe2.py rows 39-44 + dep/__init__.py:2101-2153 (EAPI gates,
// 3.0.82.2). Battery modes never reach EAPI 0/1/3/4, so these pin the gates
// the battery cannot.
#[test]
fn eapi_gates_reject_with_portage_text() {
    let eapi = |n: u8| AtomOptions {
        eapi: AtomEapi::Known(n),
        ..AtomOptions::default()
    };
    let err = |s: &str, opts: &AtomOptions, eapi_str: &str| {
        parse_atom_with(s, opts, eapi_str).expect_err(s)
    };
    assert_eq!(
        err("dev-libs/foo:0", &eapi(0), "0"),
        AtomError {
            kind: AtomErrorKind::EapiIncompatible,
            message: "Slot deps are not allowed in EAPI 0: 'dev-libs/foo:0'".into(),
        }
    );
    assert_eq!(
        err("dev-libs/foo[a]", &eapi(1), "1"),
        AtomError {
            kind: AtomErrorKind::EapiIncompatible,
            message: "Use deps are not allowed in EAPI 1: 'dev-libs/foo[a]'".into(),
        }
    );
    assert_eq!(
        err("dev-libs/foo[a(+)]", &eapi(3), "3"),
        AtomError {
            kind: AtomErrorKind::EapiIncompatible,
            message: "Use dep defaults are not allowed in EAPI 3: 'dev-libs/foo[a(+)]'".into(),
        }
    );
    assert_eq!(
        err("!!dev-libs/foo", &eapi(1), "1"),
        AtomError {
            kind: AtomErrorKind::EapiIncompatible,
            message: "Strong blocks are not allowed in EAPI 1: '!!dev-libs/foo'".into(),
        }
    );
    // Without slot_operator (EAPI < 5) the slot regex is plain: `:0/1` and
    // `:0=` are bare InvalidAtoms (probe2.py rows 41, 44).
    assert_eq!(
        err("dev-libs/foo:0/1", &eapi(4), "4").message,
        "dev-libs/foo:0/1"
    );
    assert_eq!(
        err("dev-libs/foo:0=", &eapi(4), "4").message,
        "dev-libs/foo:0="
    );
}

// Portage: probe2.py rows 26-31 (conditional use-dep flags vs is_valid_flag,
// dep/__init__.py:2125-2144, 3.0.82.2). The check runs only when eapi is
// given; with eapi None any flag passes.
#[test]
fn conditional_flags_checked_against_iuse_like_portage() {
    let e8 = AtomOptions {
        eapi: AtomEapi::Known(8),
        ..AtomOptions::default()
    };
    let none = AtomOptions::default();
    let no_b = |f: &str| f == "a";
    for s in [
        "dev-libs/foo[b?]",
        "dev-libs/foo[!b=]",
        "dev-libs/foo[a?,b=]",
    ] {
        let atom = parse_atom_with(s, &e8, "8").expect(s);
        let got = check_conditional_flags(&atom, AtomEapi::Known(8), &no_b).expect_err(s);
        assert_eq!(got.kind, AtomErrorKind::IuseMissing);
    }
    let atom = parse_atom_with("dev-libs/foo[b?]", &e8, "8").unwrap();
    assert_eq!(
        check_conditional_flags(&atom, AtomEapi::Known(8), &no_b).expect_err("msg"),
        AtomError {
            kind: AtomErrorKind::IuseMissing,
            message: "USE flag 'b' referenced in conditional 'b?' in atom \
                      'dev-libs/foo[b?]' is not in IUSE"
                .into(),
        }
    );
    // eapi None skips the check even for an unknown flag.
    let atom_none = parse_atom_with("dev-libs/foo[b?]", &none, "").unwrap();
    assert!(check_conditional_flags(&atom_none, AtomEapi::None, &no_b).is_ok());
    // A valid flag passes.
    assert!(check_conditional_flags(&atom, AtomEapi::Known(8), &|_| true).is_ok());
}

// Portage: dep/__init__.py:2994-3008 + oracle probe_match.py (`=dev-libs/foo-1.0-2`
// matches only the build_id=2 candidate, never an ebuild). Every portuale
// candidate models a _pkg_str/Package, so an `=` atom with a build-id matches
// no candidate without one (S9 adds the positive binary match). Without this,
// a user package.mask line `=dev-libs/foo-1.0-2` would mask every foo-1.0.
#[test]
fn eq_build_id_atom_matches_no_plain_candidate() {
    let candidates = [
        "dev-libs/foo-1.0",
        "dev-libs/foo-1.0-r1",
        "dev-libs/foo-1.0:0::gentoo",
    ];
    assert_eq!(
        match_from_list("=dev-libs/foo-1.0-2", &candidates),
        Some(vec![])
    );
    // Other operators ignore the build-id, like Portage.
    assert_eq!(
        match_from_list(">=dev-libs/foo-1.0-2", &candidates),
        Some(candidates.to_vec())
    );
}

// Portage `match_from_list` on `_pkg_str` candidates with build-ids
// (lib/portage/dep/__init__.py:2994-3008), re-run live 2026-10-12
// (`python3 LLM/evidence/2026-10-11-341-oracle/probe_match.py`,
// Portage 3.0.82.2):
//   =dev-libs/foo-1.0-2  -> ['dev-libs/foo-1.0#2']
//   =dev-libs/foo-1.0    -> ['dev-libs/foo-1.0#1', 'dev-libs/foo-1.0#2', 'dev-libs/foo-1.0#None']
//   >=dev-libs/foo-1.0-2 -> ['dev-libs/foo-1.0#1', 'dev-libs/foo-1.0#2', 'dev-libs/foo-1.0#None',
//                            'dev-libs/foo-1.0-r1#3', 'dev-libs/foo-10#4']
//   ~dev-libs/foo-1.0-2  -> ['…1.0#1', '…1.0#2', '…1.0#None', '…1.0-r1#3']
// (The `#<id>` suffix is the candidate's `build_id`; `#None` an ebuild-like
// one. The probe's `::gentoo`/`::x` repos and `:0` slots pass through
// `matches_repo`/`matches_slot` untouched, so plain strings pin the same rows.)
#[test]
fn eq_build_id_atom_matches_only_equal_build_id_candidate() {
    let strs = [
        "dev-libs/foo-1.0",
        "dev-libs/foo-1.0",
        "dev-libs/foo-1.0",
        "dev-libs/foo-1.0-r1",
        "dev-libs/foo-10",
        "dev-libs/foobar-1.12",
    ];
    let bids: [Option<&str>; 6] = [Some("1"), Some("2"), None, Some("3"), Some("4"), None];
    let paired: Vec<(&str, Option<&str>)> = strs.iter().copied().zip(bids).collect();
    // `=` consults the build-id: only the equal instance matches.
    assert_eq!(
        match_from_list_with_build_id("=dev-libs/foo-1.0-2", &paired),
        Some(vec![strs[1]])
    );
    // A `=` atom without one matches every version-equal candidate,
    // build-id or not.
    assert_eq!(
        match_from_list_with_build_id("=dev-libs/foo-1.0", &paired),
        Some(vec![strs[0], strs[1], strs[2]])
    );
    // `>=` and `~` ignore the build-id entirely.
    assert_eq!(
        match_from_list_with_build_id(">=dev-libs/foo-1.0-2", &paired),
        Some(vec![strs[0], strs[1], strs[2], strs[3], strs[4]])
    );
    assert_eq!(
        match_from_list_with_build_id("~dev-libs/foo-1.0-2", &paired),
        Some(vec![strs[0], strs[1], strs[2], strs[3]])
    );
}

// #510: the three atoms the 3.0.83 C-parser fixes (450282d84, 8bf578fd7,
// bdce305cd; the same fixes are in the 3.0.82.2 tag as 6a64fb2f4, 4f76d9ee5,
// a2206fc32) were written against. Oracle: LLM/evidence/2026-10-23-510/
// probe.py on the host Portage 3.0.83-r1, `python3 -I probe.py`:
//   games-arcade/2048      Atom(eapi 8 and None): cp=games-arcade/2048, OK;
//                          use_reduce(token_class=Atom): OK, the same atom.
//   dev-util/timidity--    Atom: cp=dev-util/timidity--, OK (trailing hyphens
//                          are name characters); use_reduce: OK.
//   cat/pkg[a(+),-a]       Atom: ERR `Invalid use dep: '-a'`; use_reduce with
//                          token_class=Atom: InvalidDependString (`invalid use
//                          dep token: '-a'`, 3.0.83 C path). Without
//                          token_class use_reduce returns the plain string
//                          and does not validate it (quirk Q8).
// portage-dep has no reducer: the reducer's Atom hook is this constructor, so
// the reducer rows are asserted through it.
#[test]
fn reducer_probe_atoms_match_portage_3_0_83() {
    let e8 = AtomOptions {
        eapi: AtomEapi::Known(8),
        ..AtomOptions::default()
    };
    let none = AtomOptions::default();
    for (s, opts, eapi_str) in [
        ("games-arcade/2048", &e8, "8"),
        ("games-arcade/2048", &none, ""),
        ("dev-util/timidity--", &e8, "8"),
        ("dev-util/timidity--", &none, ""),
    ] {
        let atom = parse_atom_with(s, opts, eapi_str).expect(s);
        let cp = s.to_string();
        assert_eq!(
            render_fields(&atom),
            format!("cp={cp} op= ver= slot= sub= sop= repo= bid= use= blocker= ext=0"),
            "{s} [eapi {eapi_str:?}]"
        );
    }
    for (opts, eapi_str) in [(&e8, "8"), (&none, "")] {
        assert_eq!(
            parse_atom_with("cat/pkg[a(+),-a]", opts, eapi_str).expect_err("cat/pkg[a(+),-a]"),
            AtomError {
                kind: AtomErrorKind::Invalid,
                message: "Invalid use dep: '-a'".into(),
            },
            "[eapi {eapi_str:?}]"
        );
    }
}
