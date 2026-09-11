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
