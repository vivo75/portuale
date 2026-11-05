//! #651: a seed committed BEFORE the slot conflict opens must not survive it.
//!
//! S0 #649 (`LLM/evidence/2026-11-03-649/REPORT.md`) locks the cause and #650
//! proved the open-conflict half hermetically (the withhold gate declines a
//! seed whose demoter sits in the committing pass's open slot conflict).
//! #651's residue is the commit-before-conflict ordering the bed re-grade
//! proved (`LLM/backlog-tasks.md` #651): on the `dev-lang_perl` cell both
//! seeds `commit` in the first pass with demoter `perl-5.44.0` and zero
//! `decline` lines — the committing pass has no open conflict yet, the
//! transient's conflict surfacing only as try-2 `SlotConflict` feedback with
//! a try-3 settle mask — and the feed never re-visits a settled seed.
//!
//! Expected values are Portage 3.0.83's own playground
//! (`LLM/evidence/2026-11-03-649/probe649_mutations.py` N1/N2 verbatim,
//! re-run 2026-11-04, plus the C1 chained-holder run in
//! `LLM/evidence/2026-11-04-651/probe651_oracle.py`):
//! - N1 (installed perl-5.42.2 + installed 5.42-arm virtuals, tree 5.44.0
//!   with the textually-identical PDEPEND, bare `dev-lang/perl` request):
//!   `SUCCESS True`, `MERGELIST ['dev-lang/perl-5.42.2']` — the virtual kept,
//!   no fall-through merged.
//! - C1 (N1 + a tree+installed `dev-perl/holder-1` carrying the cell's
//!   `dev-lang/perl:0/5.42` reverse edge, pulled transitively through a
//!   requested `dev-perl/top-1` instead of requested directly — the N2 leaf
//!   is inert, the direct request resolves the conflict too early): Portage
//!   `SUCCESS True`, `MERGELIST ['dev-perl/top-1', 'dev-lang/perl-5.42.2']`
//!   — reinstalls only, the transient skipped, no fall-through merged.
//!
//! The `p651*` fixture family mirrors that C1 shape hermetically
//! (`dev-libs/p651prov-5.42.2`/`-5.44.0` for `dev-lang/perl` with the live
//! `!minimal?` PDEPEND cut to the two interacting conjuncts, per the M4/Carp
//! precedent; `virtual/p651enc-*`/`virtual/p651th-*` with the verbatim real
//! RDEPENDs; `dev-libs/p651enccore-*`/`dev-libs/p651thcore-*` for
//! `perl-core/*`; `dev-perl/p651holder-1` for the background reverse edge,
//! pulled through requested `dev-perl/p651top-1`; `dev-perl/p651pin-1`
//! installed-only for the cell's background reverse-edge pressure, N2
//! mirror — inert, never merged). `minimal` defaults OFF by profile-chain
//! absence, so the PDEPEND block is live; the commit-before-conflict ordering
//! is verified by the seed-decision debug log (`SEED commit` with demoter
//! `p651prov-5.44.0` before any `SlotConflict` mask involving it, zero
//! `decline` lines — recorded verbatim in `probe651_oracle.py`).
//!
//! Portage: dep_check.py (`:531-544`/`:634-635` transient demotion,
//! `:636-655`/`:599-616` post-skip re-resolution); consumer-expansion
//! ground `:197,220,264`.

use super::*;
use crate::tests::tests_consumer_pin_conflict::{hermetic_root, request_in};

/// Bare-request resolve with binary parity: the CLI derives
/// `selective = update || newuse || ... || noreplace || newrepo`
/// (`crates/portuale/src/pretend/resolve_plan.rs`), so a bare
/// `emerge --pretend <atom>` resolves with `selective = false` while
/// `ResolveRequest::new` defaults it true. Without this the bare legs keep
/// the installed provider outright and the transient never enters entries.
/// (Same helper as `tests_p650.rs`; kept local so each slice file stands
/// alone.)
fn resolve_bare_in(root: &Path, atoms: &[&str]) -> GraphResult {
    let mut req = request_in(root, atoms);
    req.selective = false;
    req.resolve()
        .unwrap_or_else(|e| panic!("resolving {atoms:?} failed: {e}"))
}

/// The bare N1-mirror leg: the fixture vdb grew three installed `=5.42*`
/// holders under Task 1 (`p656hold{1,2,3}`), so this world is N1 + holders
/// and Portage's verdict on exactly that shape is reinstall, not upgrade:
/// P2 (`probe656_oracle.py`: `SUCCESS True`,
/// `MERGELIST ['dev-lang/perl-5.42.2']`, no skip) and H0 (same with
/// tree-present holders: tree-presence is invisible to Portage).
/// The #656 leg-(a) gate files the kept virtuals' `||` arms on the holder
/// shape, the mask lands 5.44, the seeds void, and the provider reinstalls
/// with neither fall-through merged -- the R posture the old U pin
/// predates. Re-pinned 2026-11-05 (was: transient-U + both fall-throughs,
/// written when the fixture vdb carried no `=5.42*` holder).
#[test]
fn bare_request_with_transient_merging_seeds_the_fall_through() {
    let root = hermetic_root(&[]);
    let result = resolve_bare_in(&root, &["dev-libs/p651prov"]);
    let names: Vec<String> = result
        .entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    let prov = result
        .entries
        .iter()
        .find(|e| e.category == "dev-libs" && e.package == "p651prov")
        .expect("the provider stays in the graph");
    assert!(
        matches!(
            &prov.outcome,
            PretendOutcome::Reinstall { version, .. } if version == "5.42.2"
        ),
        "the provider reinstalls 5.42.2 under the holder skip (P2/H0 oracle), got {:?}: {names:?}",
        prov.outcome
    );
    for core in ["p651enccore", "p651thcore"] {
        assert!(
            !names.iter().any(|n| n == &format!("dev-libs/{core}")),
            "no {core} fall-through under the holder skip (P2/H0 oracle): {names:?}"
        );
    }
    assert!(
        !result.entries.iter().any(|e| matches!(
            e.outcome,
            PretendOutcome::Upgrade { .. } | PretendOutcome::Downgrade { .. }
        ) && e.category == "virtual"),
        "no virtual upgrades alongside the hold: {names:?}"
    );
}

/// The chained-holder leg (C1): the conflict opens AFTER the seed commit —
/// the seed-decision log shows both `SEED commit` lines (demoter
/// `p651prov-5.44.0`) before any `SlotConflict` mask involving the demoter,
/// with zero `decline` lines — so the provider reinstalls, the top
/// reinstalls, the transient stays skipped (WARNING-proven), and the seeds
/// committed under its guard-demotion must not survive: neither fall-through
/// arm merges.
///
/// Oracle (`probe651_oracle.py` C1): Portage
/// `MERGELIST ['dev-perl/top-1', 'dev-lang/perl-5.42.2']`; portuale's own C1
/// run shows the WARNING-proven skip with the extras still merged — seeds
/// saw pressure AND survived the skip, so this leg distinguishes "seeds
/// respect the skip" from "seeds never saw pressure".
///
/// RED before Task 2: the extras merge (`N p651enccore-3.210.0` +
/// `N p651thcore-3.500.0`) under the R+skip posture.
#[test]
fn chained_holder_skips_the_transient_and_holds_the_installed_binding() {
    let root = hermetic_root(&[]);
    let result = resolve_bare_in(&root, &["dev-libs/p651prov", "dev-perl/p651top"]);
    let names: Vec<String> = result
        .entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    let prov = result
        .entries
        .iter()
        .find(|e| e.category == "dev-libs" && e.package == "p651prov")
        .expect("the provider stays in the graph");
    assert!(
        matches!(
            &prov.outcome,
            PretendOutcome::Reinstall { version, .. } if version == "5.42.2"
        ),
        "the provider reinstalls 5.42.2 under the chained holder's pin (R+skip), got {:?}: {names:?}",
        prov.outcome
    );
    assert!(
        !result.entries.iter().any(|e| e.category == "dev-libs"
            && e.package == "p651prov"
            && matches!(e.outcome, PretendOutcome::Upgrade { .. })),
        "the transient 5.44.0 stays skipped, it must not upgrade: {names:?}"
    );
    let top = result
        .entries
        .iter()
        .find(|e| e.category == "dev-perl" && e.package == "p651top")
        .expect("the requested top stays in the graph");
    assert!(
        matches!(
            &top.outcome,
            PretendOutcome::Reinstall { version, .. } if version == "1"
        ),
        "the requested top reinstalls (its edge pulls the holder), got {:?}: {names:?}",
        top.outcome
    );
    for core in ["p651enccore", "p651thcore"] {
        assert!(
            !names.iter().any(|n| n == &format!("dev-libs/{core}")),
            "no {core} fall-through under the skip (Portage C1 merges neither): {names:?}"
        );
    }
    // Portage keeps both virtuals nomerge (C1 mergelist has no virtual row),
    // and portuale does not materialize unqueued kept virtuals as entries:
    // each virtual is absent or kept — never upgraded (the p647 leg-A
    // precedent). The load-bearing half is the no-upgrade assertion.
    assert!(
        result
            .entries
            .iter()
            .filter(|e| e.category == "virtual"
                && (e.package == "p651enc" || e.package == "p651th"))
            .all(|e| matches!(
                &e.outcome,
                PretendOutcome::AlreadyInstalled { .. }
            )),
        "virtuals absent or kept, never upgraded: {:?}",
        result.entries
    );
    assert!(
        !result.entries.iter().any(|e| matches!(
            e.outcome,
            PretendOutcome::Upgrade { .. } | PretendOutcome::Downgrade { .. }
        ) && e.category == "virtual"),
        "no virtual upgrades alongside the hold: {names:?}"
    );
}
