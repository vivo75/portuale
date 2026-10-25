//! #563 heal: the perl-5.44 stabilization shape, hermetically.
//!
//! Newly-visible `p563prov-5.44` (`0/5.44`), hard-required by merge-bound
//! `p563pull-2` (`=dev-libs/p563prov-5.44*`), conflicts with the installed
//! 5.42-era reverse dep (`p563con-1` pins `dev-libs/p563prov:0/5.42=` and
//! `=dev-libs/p563prov-5.42*`). Mirrors the L0 `dev-perl_XML-Parser` probe:
//! merge-bound virtuals pull `=dev-lang/perl-5.44*` while installed
//! virtuals/dev-perls pin `dev-lang/perl:0/5.42=`.
//!
//! Portage heals the bare-puller shape (old puller merged, installed provider
//! kept, skip-block + backtracking notice, rc 0); a pinned-newver request
//! aborts with the slot conflict (rc 1); a pinned-oldver request resolves
//! clean. Oracle: Portage 3.0.83 host runs saved under
//! `LLM/evidence/2026-10-25-563/captures/`; L0 log
//! `../pmtest/differential-test-bed/logs/l0-20261025T084209Z`
//! (`dev-perl_XML-Parser` probe; `dev-lang_perl` is the bare-provider twin).

use super::*;

#[test]
fn p563_pull_heals_with_skip_block_and_backtrack_notice() {
    // Portage oracle (`captures/portage-pull-bare.*`): rc 0, backtrack 2/20,
    // merges `p563pull-1` (N); stderr carries the WARNING skip-block for
    // `dev-libs/p563prov:0` (5.44 skipped over the installed pinner) plus the
    // `triggered by backtracking` notice for `dev-libs/p563pull:0`.
    // Portage: depgraph.py, _slot_confict_backtrack + _get_missed_updates.
    let result = graph_result_real("dev-libs/p563pull");
    assert!(
        matches!(result.outcome, ResolveOutcome::Complete),
        "the pull shape heals clean, got {:?}",
        result.outcome
    );
    assert!(
        result.slot_conflicts.is_empty(),
        "no residual slot conflict after the heal, got {:?}",
        result.slot_conflicts
    );
    let entry = |cat: &str, pkg: &str| {
        result
            .entries
            .iter()
            .find(|e| e.category == cat && e.package == pkg)
            .unwrap_or_else(|| panic!("no entry for {cat}/{pkg}"))
    };
    assert!(
        matches!(
            &entry("dev-libs", "p563pull").outcome,
            PretendOutcome::New { version } if version == "1"
        ),
        "the old puller merges: {:?}",
        entry("dev-libs", "p563pull").outcome
    );
    assert!(
        matches!(
            &entry("dev-libs", "p563prov").outcome,
            PretendOutcome::AlreadyInstalled { version } if version == "5.42"
        ),
        "the installed provider is kept: {:?}",
        entry("dev-libs", "p563prov").outcome
    );
    assert!(
        matches!(
            &entry("dev-libs", "p563con").outcome,
            PretendOutcome::AlreadyInstalled { .. }
        ),
        "the installed pinner is kept: {:?}",
        entry("dev-libs", "p563con").outcome
    );
    assert_eq!(
        result.backtrack_restarts, 2,
        "two retries like the oracle's backtrack 2/20"
    );
    assert_eq!(
        result.skipped_updates.len(),
        1,
        "the WARNING skip-block names the skipped provider update: {:?}",
        result.skipped_updates
    );
    let row = &result.skipped_updates[0];
    assert_eq!(
        (
            row.category.as_str(),
            row.package.as_str(),
            row.slot.as_str(),
            row.skipped_version.as_str(),
        ),
        ("dev-libs", "p563prov", "0", "5.44"),
        "5.44 skipped over the installed pinner"
    );
    assert_eq!(
        row.atom, "=dev-libs/p563prov-5.42*",
        "the conflicting pin: {:?}",
        row.atom
    );
    assert!(
        row.consumer_cpv.starts_with("dev-libs/p563con-1:") && row.consumer_installed,
        "the pinner is the installed consumer: {:?}",
        row.consumer_cpv
    );
    assert_eq!(
        result.skipped_missing_deps,
        vec![SkippedMissingDep {
            category: "dev-libs".to_string(),
            package: "p563pull".to_string(),
            slot: "0".to_string(),
        }],
        "the `triggered by backtracking` notice names the puller slot"
    );
    assert!(
        result.skipped_missing_dep_full.is_empty(),
        "abbreviated tail, not the full form: {:?}",
        result.skipped_missing_dep_full
    );
    assert!(
        result.missing_args.is_empty(),
        "explicit args never drop into missing_args: {:?}",
        result.missing_args
    );
}

#[test]
fn p563_pinned_newver_aborts_with_slot_conflict() {
    // Portage oracle (`captures/portage-pull-newver.*`): rc 1, the merge list
    // still prints but the slot-conflict block aborts — the explicit new
    // puller cannot be masked away, so nothing heals. Guards over-masking
    // (no silent downgrade to `p563pull-1`).
    // Portage: depgraph.py, _show_slot_collision.
    let result = graph_result_real("=dev-libs/p563pull-2");
    assert_eq!(
        result.slot_conflicts.len(),
        1,
        "the pinned newver keeps the provider slot conflict, got {:?}",
        result.slot_conflicts
    );
    let c = &result.slot_conflicts[0];
    assert_eq!(
        (
            c.category.as_str(),
            c.package.as_str(),
            c.slot.as_str(),
            c.resolved_version.as_str(),
        ),
        ("dev-libs", "p563prov", "0", "5.44"),
        "the conflict is the provider's slot, merge version first"
    );
    assert_eq!(
        c.instances
            .iter()
            .map(|i| (i.version.as_str(), i.installed))
            .collect::<Vec<_>>(),
        vec![("5.44", false), ("5.42", true)],
        "merge-bound new subslot beside the installed old one"
    );
    assert!(
        c.instances[0]
            .parents
            .iter()
            .any(|p| p.parent_cpv.starts_with("dev-libs/p563pull-2:")
                && p.atom == "=dev-libs/p563prov-5.44*"
                && !p.installed),
        "the new instance is pulled by the merge-bound puller: {:?}",
        c.instances[0].parents
    );
    assert!(
        c.instances[1]
            .parents
            .iter()
            .any(|p| p.parent_cpv.starts_with("dev-libs/p563con-1:") && p.installed),
        "the installed instance carries the installed pinner: {:?}",
        c.instances[1].parents
    );
    assert!(
        !result.entries.iter().any(|e| e.category == "dev-libs"
            && e.package == "p563pull"
            && matches!(&e.outcome, PretendOutcome::New { version } if version == "1")),
        "no silent downgrade to the old puller: {:?}",
        result
            .entries
            .iter()
            .map(|e| format!("{}/{} {:?}", e.category, e.package, e.outcome))
            .collect::<Vec<_>>()
    );
}

#[test]
fn p563_pinned_oldver_still_resolves_clean() {
    // Portage oracle (`captures/portage-pull-oldver.*`): rc 0, a clean merge
    // of `p563pull-1` on both sides. Guards over-masking: the heal's masks
    // must not touch a path that never conflicted.
    let result = graph_result_real("=dev-libs/p563pull-1");
    assert!(
        matches!(result.outcome, ResolveOutcome::Complete),
        "the pinned oldver resolves clean, got {:?}",
        result.outcome
    );
    assert!(
        result.slot_conflicts.is_empty()
            && result.skipped_updates.is_empty()
            && result.skipped_missing_deps.is_empty()
            && result.skipped_missing_dep_full.is_empty()
            && result.missing_args.is_empty(),
        "no conflict and no skip rows on the clean path: {:?} {:?} {:?}",
        result.slot_conflicts,
        result.skipped_updates,
        result.skipped_missing_deps
    );
    assert!(
        result.entries.iter().any(|e| e.category == "dev-libs"
            && e.package == "p563pull"
            && matches!(&e.outcome, PretendOutcome::New { version } if version == "1")),
        "the old puller merges: {:?}",
        result
            .entries
            .iter()
            .map(|e| format!("{}/{} {:?}", e.category, e.package, e.outcome))
            .collect::<Vec<_>>()
    );
    assert!(
        result.entries.iter().any(|e| e.category == "dev-libs"
            && e.package == "p563prov"
            && matches!(&e.outcome, PretendOutcome::AlreadyInstalled { version } if version == "5.42")),
        "the provider stays installed: {:?}",
        result.entries.iter().map(|e| format!(
            "{}/{} {:?}",
            e.category, e.package, e.outcome
        )).collect::<Vec<_>>()
    );
}
