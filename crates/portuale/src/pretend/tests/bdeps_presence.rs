//! #361 S1: `bdeps=auto` is a `--usepkg` key-presence rule, not a value rule.
//!
//! Portage sets `myparams["bdeps"] = "auto"` whenever `--with-bdeps` is
//! absent, `--with-bdeps-auto` is not `n`, and the `--usepkg` key is absent
//! (`create_depgraph_params.py:97-103`); `_emerge/main.py:1109-1114` stores
//! `--usepkg=n` as `False`, so the key is present and an explicit `=n`
//! turns bdeps-auto off exactly like bare `--usepkg`. Implications that
//! insert the key live in `_emerge/actions.py:3709-3723` (`FEATURES`
//! fold, `--getbinpkgonly`/`--getbinpkg`/`--usepkgonly` chain).
//! Expected values below are Portage 3.0.82.2's rows from
//! `LLM/evidence/2026-10-15-359-s0/` (`matrix.md` group C, plus the reused
//! `361-blib-pvuD[-noflags].txt` cells in `../2026-10-15-plan-359/`).

use super::*;

/// Parse `argv` (plus `features` as the resolved `FEATURES` tokens) down to
/// the `with_bdeps` bit `plan_resolve` would carry into the resolver.
fn bdeps_for_argv(argv: &[&str], features: &[&str]) -> bool {
    let args: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
    let opts = parse_emerge_args(&args).expect("S1 matrix argv parses");
    let owned: Vec<String> = features.iter().map(|s| s.to_string()).collect();
    plan_with_bdeps(&opts, &owned)
}

#[test]
fn usepkg_n_marks_the_key_present_with_a_false_value() {
    // Portage `_emerge/main.py:1111-1112`: `--usepkg=n` stores `False`.
    let args: Vec<String> = ["--usepkg=n", "dev-libs/pg34-blib"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let opts = parse_emerge_args(&args).expect("parses");
    assert!(!opts.usepkg);
    assert!(opts.usepkg_given);
}

#[test]
fn every_usepkg_spelling_marks_the_key_present() {
    // (argv, expected value): presence is what the bdeps rule reads.
    for (argv, value) in [
        (vec!["--usepkg"], true),
        (vec!["--usepkg=y"], true),
        (vec!["--usepkg", "y"], true),
        (vec!["--usepkg=n"], false),
        (vec!["--usepkg", "n"], false),
        (vec!["-k"], true),
        (vec!["-pvuDk", "dev-libs/pg34-blib"], true),
    ] {
        let args: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        let opts = parse_emerge_args(&args).expect("parses");
        assert!(opts.usepkg_given, "{argv:?} marks --usepkg given");
        assert_eq!(opts.usepkg, value, "{argv:?} value");
    }
}

#[test]
fn getbinpkg_n_marks_given_so_it_vetoes_the_features_fold() {
    // Portage `_emerge/actions.py:3709-3711`: the fold runs unless the
    // command line said `--getbinpkg=n` (`is not False`).
    let args: Vec<String> = ["--getbinpkg=n", "dev-libs/pg34-blib"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let opts = parse_emerge_args(&args).expect("parses");
    assert!(!opts.getbinpkg);
    assert!(opts.getbinpkg_given);
}

#[test]
fn untouched_defaults_leave_both_keys_absent() {
    let opts = EmergeOpts::default();
    assert!(!opts.usepkg_given);
    assert!(!opts.getbinpkg_given);
}

#[test]
fn emerge_default_opts_words_parse_through_the_same_function() {
    // `EMERGE_DEFAULT_OPTS` prepends to argv (`args_with_emerge_defaults`,
    // Portage `_emerge/main.py`), so an env `--usepkg=n` marks the key
    // present too. S0 `s0-c-env-usepkgn`: Portage `Total: 0`.
    let config = portage_profile::Config {
        other_vars: [("EMERGE_DEFAULT_OPTS".to_string(), "--usepkg=n".to_string())]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let argv = vec!["-pvuD".to_string(), "dev-libs/pg34-blib".to_string()];
    let effective = args_with_emerge_defaults(&config, &argv);
    let opts = parse_emerge_args(&effective).expect("parses");
    assert!(!opts.usepkg);
    assert!(opts.usepkg_given);
    assert!(
        !plan_with_bdeps(&opts, &[]),
        "env --usepkg=n turns bdeps-auto off like argv --usepkg=n"
    );
}

#[test]
fn no_flags_keep_bdeps_auto_on() {
    // S0 `361-blib-pvuD-noflags.txt`: Portage `U pg34-btool-2`.
    assert!(bdeps_for_argv(&["-pvuD", "dev-libs/pg34-blib"], &[]));
}

#[test]
fn explicit_usepkg_n_alongside_getbinpkg_n_turns_bdeps_auto_off() {
    // S0 `#34`-era `361-blib-pvuD.txt`
    // (`-pvuD --getbinpkg=n --usepkg=n`): Portage `Total: 0`.
    assert!(!bdeps_for_argv(
        &["-pvuD", "--getbinpkg=n", "--usepkg=n", "dev-libs/pg34-blib"],
        &[]
    ));
}

#[test]
fn bare_usepkg_and_k_turn_bdeps_auto_off() {
    // S0 `s0-c-usepkg.txt` / `s0-c-k.txt`: Portage `Total: 0` both.
    assert!(!bdeps_for_argv(
        &["-pvuD", "--usepkg", "dev-libs/pg34-blib"],
        &[]
    ));
    assert!(!bdeps_for_argv(&["-pvuD", "-k", "dev-libs/pg34-blib"], &[]));
}

#[test]
fn getbinpkg_n_alone_leaves_bdeps_auto_on() {
    // No `--usepkg` key is inserted (`actions.py` implications need
    // `is True`); the S0 `s0-c-features-getbinpkg-off.txt` control pins
    // the same shape under `FEATURES=getbinpkg`.
    assert!(bdeps_for_argv(
        &["-pvuD", "--getbinpkg=n", "dev-libs/pg34-blib"],
        &[]
    ));
}

#[test]
fn getbinpkgonly_n_leaves_bdeps_auto_on() {
    // S0 `s0-c-getbinpkgonly-n.txt`: Portage `U pg34-btool-2`.
    assert!(bdeps_for_argv(
        &["-pvuD", "--getbinpkgonly=n", "dev-libs/pg34-blib"],
        &[]
    ));
}

#[test]
fn features_getbinpkg_implies_the_key_unless_cmdline_said_n() {
    // S0 `s0-c-features-getbinpkg.txt`: Portage `Total: 0` — the fold
    // (`actions.py:3709-3711`) inserts `--getbinpkg`, which implies
    // `--usepkg`. S0 `s0-c-features-getbinpkg-off.txt`: an explicit
    // `--getbinpkg=n` vetoes it, Portage `U pg34-btool-2`.
    assert!(!bdeps_for_argv(
        &["-pvuD", "dev-libs/pg34-blib"],
        &["getbinpkg"]
    ));
    assert!(bdeps_for_argv(
        &["-pvuD", "--getbinpkg=n", "dev-libs/pg34-blib"],
        &["getbinpkg"]
    ));
}

#[test]
fn usepkgonly_n_leaves_bdeps_auto_on() {
    // S0 `s0-c-usepkgonly-n.txt`: the bdeps rule
    // (`create_depgraph_params.py:97-103`) tests only the `--usepkg`
    // key, and `--usepkgonly=n` (stored `False`) implies nothing
    // (`actions.py` implications need `is True`), so bdeps-auto stays
    // on. Portage still prints `Total: 0` there, but through the
    // ebuild-db drop (`depgraph.py:809`, `"--usepkgonly" not in myopts`),
    // which this rule does not model — not a bdeps divergence.
    assert!(bdeps_for_argv(
        &["-pvuD", "--usepkgonly=n", "dev-libs/pg34-blib"],
        &[]
    ));
}

#[test]
fn explicit_with_bdeps_wins_over_the_presence_rule() {
    // `--with-bdeps=n` stays off even with `--usepkg=n` given, and
    // `--with-bdeps=y` stays on: Portage checks the explicit flag first.
    let off = EmergeOpts {
        with_bdeps_given: true,
        with_bdeps: false,
        usepkg_given: true,
        ..Default::default()
    };
    assert!(!plan_with_bdeps(&off, &[]));
    let on = EmergeOpts {
        with_bdeps_given: true,
        with_bdeps: true,
        usepkg_given: true,
        ..Default::default()
    };
    assert!(plan_with_bdeps(&on, &[]));
}

#[test]
fn with_bdeps_auto_n_disables_the_auto_bit() {
    let opts = EmergeOpts {
        with_bdeps_auto: false,
        ..Default::default()
    };
    assert!(!plan_with_bdeps(&opts, &[]));
}
