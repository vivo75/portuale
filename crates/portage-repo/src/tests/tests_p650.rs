//! #650: a seed committed under a transient guard-demotion must not survive the skip.
//!
//! S0 #649 (`LLM/evidence/2026-11-03-649/REPORT.md`) locks the cause: on the
//! `dev-lang_perl` cell the transient `dev-lang/perl-5.44.0` sits in some
//! pass's `entries`, `select_expansion_arms` (`rev_dep_or.rs:510`,
//! `keep_demoted: false`) runs the bug-531656 downgrade guard (`:546-561`)
//! against those entries, the installed `=perl-5.42*` arm is demoted, the
//! `~perl-core/*` arm survives alone (`survivors.remove(0)`, `:573`), and the
//! seed persists in `grown.virtual_expansion_seeds`
//! (`feedback/mod.rs:318-327`, `backtrack_params.rs:391`) past the skip.
//! Portage demotes the same arm transiently (`dep_check.py:531-544`, filed
//! `:634-635`) but re-resolves post-skip: `=perl-5.42*` lands
//! `preferred_in_graph` (`:636-655`), `~perl-core/*` lands
//! `preferred_non_installed`/unavailable (`:599-616`) — the half portuale
//! has no counterpart for; consumer-expansion ground `dep_check.py:197,220,264`.
//!
//! Expected values are Portage 3.0.83's own playground
//! (`LLM/evidence/2026-11-03-649/probe649_mutations.py` N1/N2 verbatim,
//! re-run 2026-11-03 plus the H1 holder oracle in
//! `LLM/evidence/2026-11-0X-650/probe650_oracle.py`):
//! - N1 (installed perl-5.42.2 + installed 5.42-arm virtuals, tree 5.44.0
//!   with the textually-identical PDEPEND, bare `dev-lang/perl` request):
//!   `SUCCESS True`, `MERGELIST ['dev-lang/perl-5.42.2']` — the virtual kept,
//!   no fall-through merged.
//! - H1 (N1 + a REFERENCED subslot pin-holder, `dev-perl/holder-1` with
//!   `RDEPEND="dev-lang/perl:0/5.42"`, requested alongside perl — the cell's
//!   `dev-lang/perl:0/5.42=` reverse-edge posture; the trailing-`=` rebuild
//!   form is unparseable in tree RDEPEND context so the holder carries the
//!   plain pin, recorded delta): `SUCCESS True`,
//!   `MERGELIST ['dev-lang/perl-5.42.2', 'dev-perl/holder-1']` — reinstalls
//!   only, the transient skipped, no fall-through merged.
//!
//! The `p650*` fixture family mirrors those shapes hermetically
//! (`dev-libs/p650prov-5.42.2`/`-5.44.0` for `dev-lang/perl` with the live
//! `!minimal?` PDEPEND cut to the two interacting conjuncts, per the M4/Carp
//! precedent; `virtual/p650enc-*`/`virtual/p650th-*` with the verbatim real
//! RDEPENDs; `dev-libs/p650enccore-*`/`dev-libs/p650thcore-*` for
//! `perl-core/*`; `dev-perl/p650holder-1` for the background reverse edge).
//! `minimal` defaults OFF by profile-chain absence (no `package.use`,
//! profile, or `make.defaults` enables it), so the PDEPEND block is live;
//! if a profile ever enables `minimal`, both legs go silent for a
//! non-product reason.

use super::*;
use crate::tests::tests_consumer_pin_conflict::{hermetic_root, request_in};

/// Bare-request resolve with binary parity: the CLI derives
/// `selective = update || newuse || ... || noreplace || newrepo`
/// (`crates/portuale/src/pretend/resolve_plan.rs`), so a bare
/// `emerge --pretend <atom>` resolves with `selective = false` while
/// `ResolveRequest::new` defaults it true. Without this the bare legs keep
/// the installed provider outright and the transient never enters entries.
fn resolve_bare_in(root: &Path, atoms: &[&str]) -> GraphResult {
    let mut req = request_in(root, atoms);
    req.selective = false;
    req.resolve()
        .unwrap_or_else(|e| panic!("resolving {atoms:?} failed: {e}"))
}

/// The bare N1 leg on portuale's side: the transient MERGES here
/// (`U p650prov-5.44.0`) where the cell skips it — the recorded
/// selection-side divergence (S0 §6 "N1 vs N2 perl-row divergence"), NOT
/// #650's scope. Given the merge, the demotion is upgrade-demotion, and
/// upgrade-demoted seeds must still fire (the M6b/M5 precedent Task 2 must
/// preserve): the fall-through arms merge alongside the upgrade.
///
/// Oracle: Portage N1 holds `['dev-lang/perl-5.42.2']`, but that hold rides
/// the skip this leg does not render — so this leg pins NO CHANGE, guarding
/// the fix against over-blunting (a fix green here by dropping the seeds
/// while still merging the transient has confused the two demotions).
/// GREEN before Task 2, must stay green after.
#[test]
fn bare_request_with_transient_merging_seeds_the_fall_through() {
    let root = hermetic_root(&[]);
    let result = resolve_bare_in(&root, &["dev-libs/p650prov"]);
    let names: Vec<String> = result
        .entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    let prov = result
        .entries
        .iter()
        .find(|e| e.category == "dev-libs" && e.package == "p650prov")
        .expect("the provider stays in the graph");
    assert!(
        matches!(
            &prov.outcome,
            PretendOutcome::Upgrade { to, .. } if to == "5.44.0"
        ),
        "the transient merges on the bare leg (selection-side U, out of #650 scope), got {:?}",
        prov.outcome
    );
    for (core, ver) in [("p650enccore", "3.210.0"), ("p650thcore", "3.500.0")] {
        let entry = result
            .entries
            .iter()
            .find(|e| e.category == "dev-libs" && e.package == core)
            .unwrap_or_else(|| panic!("the {core} fall-through arm merges: {names:?}"));
        assert!(
            matches!(&entry.outcome, PretendOutcome::New { version } if version == ver),
            "the {core} fall-through arm merges new, got {:?}",
            entry.outcome
        );
    }
    assert!(
        !result.entries.iter().any(|e| matches!(
            e.outcome,
            PretendOutcome::Upgrade { .. } | PretendOutcome::Downgrade { .. }
        ) && e.category == "virtual"),
        "no virtual upgrades alongside the fall-through: {names:?}"
    );
}

/// The holder leg (H1): with the transient SKIPPED via the referenced
/// subslot pin, the seeds committed under its guard-demotion must not
/// survive — the provider reinstalls, the holder reinstalls, the virtuals
/// stay kept-silent, and neither fall-through arm merges.
///
/// Oracle (`probe650_oracle.py` H1): Portage
/// `MERGELIST ['dev-lang/perl-5.42.2', 'dev-perl/holder-1']`; portuale's own
/// H1 run shows the WARNING-proven skip
/// (`dev-lang/perl-5.44.0 ... scheduled for merge` conflicting with the
/// holder's `dev-lang/perl:0/5.42` edge) with the extras still merged —
/// seeds saw pressure AND survived the skip, so this leg distinguishes
/// "seeds respect the skip" from "seeds never saw pressure".
///
/// RED before Task 2: the extras merge (`N p650enccore-3.210.0` +
/// `N p650thcore-3.500.0`) under the R+skip posture.
#[test]
fn skipped_transient_holder_holds_the_installed_binding() {
    let root = hermetic_root(&[]);
    let result = resolve_bare_in(&root, &["dev-libs/p650prov", "dev-perl/p650holder"]);
    let names: Vec<String> = result
        .entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    let prov = result
        .entries
        .iter()
        .find(|e| e.category == "dev-libs" && e.package == "p650prov")
        .expect("the provider stays in the graph");
    assert!(
        matches!(
            &prov.outcome,
            PretendOutcome::Reinstall { version, .. } if version == "5.42.2"
        ),
        "the provider reinstalls 5.42.2 under the holder's pin (R+skip), got {:?}: {names:?}",
        prov.outcome
    );
    assert!(
        !result.entries.iter().any(|e| e.category == "dev-libs"
            && e.package == "p650prov"
            && matches!(e.outcome, PretendOutcome::Upgrade { .. })),
        "the transient 5.44.0 stays skipped, it must not upgrade: {names:?}"
    );
    let holder = result
        .entries
        .iter()
        .find(|e| e.category == "dev-perl" && e.package == "p650holder")
        .expect("the referenced holder stays in the graph");
    assert!(
        matches!(
            &holder.outcome,
            PretendOutcome::Reinstall { version, .. } if version == "1"
        ),
        "the holder reinstalls (its edge is what holds the skip), got {:?}: {names:?}",
        holder.outcome
    );
    for core in ["p650enccore", "p650thcore"] {
        assert!(
            !names.iter().any(|n| n == &format!("dev-libs/{core}")),
            "no {core} fall-through under the skip (Portage H1 merges neither): {names:?}"
        );
    }
    // Portage keeps both virtuals nomerge (H1 mergelist has no virtual row),
    // and portuale does not materialize unqueued kept virtuals as entries:
    // each virtual is absent or kept — never upgraded (the p647 leg-A
    // precedent). The load-bearing half is the no-upgrade assertion.
    assert!(
        result
            .entries
            .iter()
            .filter(|e| e.category == "virtual"
                && (e.package == "p650enc" || e.package == "p650th"))
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
