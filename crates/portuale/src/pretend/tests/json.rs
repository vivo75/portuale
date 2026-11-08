use super::*;

#[test]
fn abort_outcome_to_json_spells_each_abort_reason() {
    // Slice 4 `--json` provenance: `null` on the complete path, one
    // `{"reason":…}` shape per abort variant. The spellings and
    // payload keys are what `--json` consumers read (the abort
    // contract tests pin the bytes on the fixtures).
    use portage_repo::{AbortReason, ResolveOutcome};
    assert_eq!(abort_outcome_to_json(&ResolveOutcome::Complete), "null");
    assert_eq!(
        abort_outcome_to_json(&ResolveOutcome::Aborted {
            reason: AbortReason::MaskedDep {
                atom: "dev-libs/maskeddep".to_string(),
                parent_cpv: "dev-libs/abort-masked-mid-1.0".to_string(),
            },
            partial: Vec::new(),
        }),
        r#"{"reason":"masked-dep","atom":"dev-libs/maskeddep","parent":"dev-libs/abort-masked-mid-1.0"}"#
    );
    assert_eq!(
        abort_outcome_to_json(&ResolveOutcome::Aborted {
            reason: AbortReason::UnsatisfiedAtom {
                atom: "dev-libs/abort-nonexistent".to_string(),
                parent_cpv: "dev-libs/abort-unsat-mid-1.0".to_string(),
            },
            partial: Vec::new(),
        }),
        r#"{"reason":"unsatisfied-atom","atom":"dev-libs/abort-nonexistent","parent":"dev-libs/abort-unsat-mid-1.0"}"#
    );
    assert_eq!(
        abort_outcome_to_json(&ResolveOutcome::Aborted {
            reason: AbortReason::UnserializableCycle {
                members: vec!["dev-libs/abort-cycle-a-1.0".to_string()],
            },
            partial: Vec::new(),
        }),
        r#"{"reason":"unserializable-cycle","members":["dev-libs/abort-cycle-a-1.0"]}"#
    );
}

fn input(metadata: &[(&str, &str)], use_flags: &[&str]) -> portage_repo::DepInput {
    portage_repo::DepInput {
        metadata: std::sync::Arc::new(
            metadata
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        ),
        use_flags: use_flags.iter().map(|f| f.to_string()).collect(),
    }
}

#[test]
fn dep_input_fields_carry_raw_strings_and_testable_use() {
    // `use` lists only enabled flags a dependency string or `REQUIRED_USE`
    // can test: `IUSE` names plus implicit flags named by a conditional
    // (`elibc_glibc`) or by `REQUIRED_USE` (`kernel_linux`);
    // the profile-wide `amd64` is tested by nothing and stays out.
    let fields = dep_input_fields(&input(
        &[
            (
                "RDEPEND",
                "foo? ( dev-libs/a ) elibc_glibc? ( sys-libs/glibc )",
            ),
            ("DEPEND", " dev-libs/b "),
            ("REQUIRED_USE", "^^ ( foo bar ) kernel_linux"),
            ("IUSE", "+foo -bar"),
        ],
        &["foo", "elibc_glibc", "kernel_linux", "amd64"],
    ));
    assert_eq!(
        fields,
        vec![
            r#""dependencies":{"DEPEND":"dev-libs/b","RDEPEND":"foo? ( dev-libs/a ) elibc_glibc? ( sys-libs/glibc )"}"#,
            r#""required_use":"^^ ( foo bar ) kernel_linux""#,
            r#""iuse":["bar","foo"]"#,
            r#""use":["elibc_glibc","foo","kernel_linux"]"#,
        ]
    );
}

#[test]
fn verbose_json_adds_dep_input_to_merge_bound_entries_only() {
    let mut entry = world_entry("dev-libs", "foo", "0");
    entry.dep_input = Some(std::sync::Arc::new(input(
        &[("RDEPEND", "dev-libs/a")],
        &[],
    )));
    let top = HashSet::new();
    let plain = entry_to_json(&entry, 0, &top, false, None);
    assert!(!plain.contains("\"dependencies\""), "{plain}");
    let verbose = entry_to_json(&entry, 0, &top, true, None);
    assert!(
        verbose.contains(r#""dependencies":{"RDEPEND":"dev-libs/a"}"#),
        "{verbose}"
    );
    entry.outcome = PretendOutcome::AlreadyInstalled {
        version: "1.0".into(),
    };
    let installed = entry_to_json(&entry, 0, &top, true, None);
    assert!(!installed.contains("\"dependencies\""), "{installed}");
}

#[test]
fn kept_packages_drop_replaced_slots_and_uninstalls() {
    let root = TempDir::new("json_kept");
    for (pf, slot) in [
        ("foo-1.0", "0"),
        ("foo-2.0", "2"),
        ("bar-1.0", "0"),
        ("baz-1.0", "0"),
    ] {
        let dir = root.path().join("var/db/pkg/dev-libs").join(pf);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SLOT"), format!("{slot}\n")).unwrap();
    }
    // foo:0 is upgraded, baz-1.0 is uninstalled; foo:2 and bar stay.
    let upgrade = world_entry("dev-libs", "foo", "0");
    let mut uninstall = world_entry("dev-libs", "baz", "0");
    uninstall.outcome = PretendOutcome::Uninstall {
        version: "1.0".into(),
    };
    let kept: Vec<String> = kept_packages(root.path(), &[upgrade, uninstall])
        .iter()
        .map(|p| format!("{}:{}", p.cpv(), p.slot))
        .collect();
    assert_eq!(kept, vec!["dev-libs/bar-1.0:0", "dev-libs/foo-2.0:2"]);
}

#[test]
fn candidate_options_json_names_the_four_binary_options() {
    let options = CandidateOptions {
        usepkg: true,
        usepkgonly: false,
        getbinpkg: true,
        getbinpkgonly: false,
    };
    assert_eq!(
        candidate_options_to_json(options),
        r#"{"usepkg":true,"usepkgonly":false,"getbinpkg":true,"getbinpkgonly":false}"#
    );
}
