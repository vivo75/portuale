use super::*;

fn parsed(args: &[&str]) -> Result<EmergeOpts<'static>, std::process::ExitCode> {
    let owned: &'static [String] = Box::leak(
        args.iter()
            .map(|a| a.to_string())
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );
    parse_emerge_args(owned)
}

/// Backlog #503: `--binpkg-respect-user-patches` is `true_y_or_n` with an
/// optional value, like `--binpkg-respect-use`. Oracle: Portage 3.0.83-r1,
/// `LLM/evidence/2026-10-23-503/` (`01`..`07`).
#[test]
fn binpkg_respect_user_patches_takes_an_optional_y_n_value() {
    let p = |a: &[&str]| parsed(a).expect("parses").binpkg_respect_user_patches;
    assert_eq!(p(&["a/b"]), None);
    assert_eq!(p(&["--binpkg-respect-user-patches", "a/b"]), Some(true));
    assert_eq!(
        p(&["--binpkg-respect-user-patches=True", "a/b"]),
        Some(true)
    );
    assert_eq!(p(&["--binpkg-respect-user-patches=y", "a/b"]), Some(true));
    assert_eq!(p(&["--binpkg-respect-user-patches=n", "a/b"]), Some(false));
    // A following `n` is the value, the atom stays an atom.
    let opts = parsed(&["--binpkg-respect-user-patches", "n", "a/b"]).unwrap();
    assert_eq!(opts.binpkg_respect_user_patches, Some(false));
    assert_eq!(opts.atom_args, vec!["a/b"]);
    assert!(parsed(&["--binpkg-respect-user-patches=true"]).is_err());
    assert!(parsed(&["--binpkg-respect-user-patches=maybe"]).is_err());
}

/// Backlog #503: the bare `--circular-deps-report` means `text`; only
/// `text`/`json` following the bare flag are consumed. Oracle: Portage
/// 3.0.83-r1 (`08`..`13`).
#[test]
fn circular_deps_report_defaults_to_text_and_accepts_json() {
    let p = |a: &[&str]| parsed(a).expect("parses").circular_deps_report;
    assert_eq!(p(&["a/b"]), None);
    assert_eq!(
        p(&["--circular-deps-report", "a/b"]),
        Some(CircularDepsReport::Text)
    );
    assert_eq!(
        p(&["--circular-deps-report=True"]),
        Some(CircularDepsReport::Text)
    );
    assert_eq!(
        p(&["--circular-deps-report=text"]),
        Some(CircularDepsReport::Text)
    );
    assert_eq!(
        p(&["--circular-deps-report=json"]),
        Some(CircularDepsReport::Json)
    );
    let opts = parsed(&["--circular-deps-report", "json", "a/b"]).unwrap();
    assert_eq!(opts.circular_deps_report, Some(CircularDepsReport::Json));
    assert_eq!(opts.atom_args, vec!["a/b"]);
    assert!(parsed(&["--circular-deps-report=xml"]).is_err());
    assert!(parsed(&["--circular-deps-report="]).is_err());
}

/// Backlog #503: `--merge-wait-scope` requires one of four values.
/// Oracle: Portage 3.0.83-r1 (`14`..`20`).
#[test]
fn merge_wait_scope_requires_one_of_four_values() {
    let p = |a: &[&str]| parsed(a).expect("parses").merge_wait_scope;
    assert_eq!(p(&["a/b"]), None);
    assert_eq!(p(&["--merge-wait-scope=deep"]), Some(MergeWaitScope::Deep));
    assert_eq!(
        p(&["--merge-wait-scope=system"]),
        Some(MergeWaitScope::System)
    );
    assert_eq!(
        p(&["--merge-wait-scope=toolchain"]),
        Some(MergeWaitScope::Toolchain)
    );
    assert_eq!(
        p(&["--merge-wait-scope", "none", "a/b"]),
        Some(MergeWaitScope::None)
    );
    assert!(parsed(&["--merge-wait-scope=all"]).is_err());
    assert!(parsed(&["--merge-wait-scope"]).is_err());
    // argparse takes the next token as the value, valid or not.
    assert!(parsed(&["--merge-wait-scope", "a/b"]).is_err());
}

/// Backlog #503: `--cycle-break` is a non-goal and stays rejected.
#[test]
fn cycle_break_stays_unrecognized() {
    assert!(parsed(&["--cycle-break"]).is_err());
    assert!(parsed(&["--cycle-break=y"]).is_err());
}
