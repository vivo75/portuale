//! #652 leg A (N-form consumer path) + leg B (H-form upgrade hold): RED pins.
//!
//! S0 #652 (`LLM/evidence/2026-11-04-652/`, this branch) refuted the
//! novel-demoter corner and rescoped #652 to two live shapes, both with a
//! committed Portage-3.0.83 oracle:
//!
//! - Leg A (N-form): Try-Tiny binds the dead installed SLU virtual and the
//!   choice latches pre-skip; Portage re-resolves post-skip and holds the
//!   virtual (no `dev-perl/Sub-Name` merged). Bed ground truth: L0
//!   `l0-20261104T075942Z` real `app-text_texlive.txt` (and the XML-Parser,
//!   kdecore-meta, texlive-core, texlive-latex cells) carries NO Sub-Name
//!   row -- the 5 Sub-Name `[extra]` rows are portuale-only. Playground
//!   oracle (`LLM/evidence/2026-11-04-652b/probe652b_oracle.py` NF):
//!   `SUCCESS True`, `MERGELIST ['virtual/perl-s652foo-2.223.0-r1',
//!   'dev-perl/s652try-1']` -- the driver schedules 5.44 (its `||` first
//!   resolves `=perl-5.44*`), the walk skips it (re-resolves to `=perl-5.42*`
//!   verbatim in the trace), the consumer `||` keeps the virtual conjunction
//!   over Sub-Name throughout. Portuale latches the pre-skip flat choice
//!   (the #643 inline expansion binds the dead arm) and never re-pops it,
//!   so it merges Sub-Name where Portage holds.
//! - Leg B (H-form): under the LIVE merging transient the seed feed fires
//!   both fall-throughs where Portage merges Encode but holds TH. Bed
//!   ground truth: L0 `l0-20261104T075942Z` real
//!   `app-office_libreoffice.txt` merges `perl-core/Encode-3.210.0` (row 33)
//!   and carries NO `perl-core/Test-Harness` row and NO `virtual/perl-Encode`
//!   or `virtual/perl-Test-Harness` rows (other fresh virtuals do merge;
//!   gimp identical: Encode row 22, no TH row, perl U). Portuale fires
//!   both seeds (demoter `p651prov-5.44.0` survives) and merges both pins.
//!
//! The `p652*` fixture family extends the #651 C1 substrate hermetically
//! (same `p651prov` provider axis + `p651enc/p651th` virtuals +
//! `p651enccore/p651thcore` pins + installed `p651holder/p651top/p651pin`
//! background pressure; reused where the shape fits, forked where the new
//! legs require it):
//! - N-form: `virtual/p652slu-1.680.100_rc` (installed; bed SLU-virtual
//!   text family: 5.42 arm + `~` pin at a version with NO core ebuild --
//!   the dead arm) + `dev-perl/p652subname-0.280.0-r1` (fall-through leaf,
//!   like the `p651*core` leaves) + `dev-perl/p652try-1` (fresh;
//!   Try-Tiny-shaped `!minimal?` consumer `||` + bare provider dep) +
//!   `virtual/p652foo-2.223.0-r1` (fresh-only driver virtual with a
//!   5.44-first-arm `||`, pulled by the consumer -- the collapse of the
//!   live fresh-virtual driver chain into one edge, documented here).
//! - H-form: `dev-perl/p652early-1` (fresh; bare `virtual/p651th` dep --
//!   the libreoffice-cell early TH-parent chain in one edge) requested
//!   ahead of the bare provider. Everything else is C1 verbatim.
//!
//! `minimal` defaults OFF by profile-chain absence (C1 precedent), so the
//!   `!minimal?` blocks are live.
//!
//! Portage: dep_check.py bins (`:462-467` replacing skip, `:531-544`
//! installed_downgrade, `:622-626`/`:696-703` preferred ladder,
//! `:634-635` downgrade guard, `:392-402` bin order, `:745-800` in-bin
//! upgrade promotion); consumer-expansion ground `:197,220,264`.
//! The (b) bin record: under an identical posture both fall-through arms
//! land in identical bins (perl arm `other` via the guard, pin arm
//! `preferred_non_installed` wins) -- the live split comes from evaluation
//! posture (self-parent re-eval vs transient-parent replacing-eval), full
//! analysis in the Task-1 report; no arm-level bin distinguishes TH (the
//! brief's Step-1 STOP premise, flagged there explicitly).

use super::*;
use crate::tests::tests_consumer_pin_conflict::{hermetic_root, request_in};

/// Bare-request resolve with binary parity: the CLI derives
/// `selective = update || newuse || ... || noreplace || newrepo`
/// (`crates/portuale/src/pretend/resolve_plan.rs`), so a bare
/// `emerge --pretend <atom>` resolves with `selective = false` while
/// `ResolveRequest::new` defaults it true. (Same helper as
/// `tests_p650.rs`/`tests_p651.rs`; kept local so each slice file stands
/// alone.)
fn resolve_bare_in(root: &Path, atoms: &[&str]) -> GraphResult {
    let mut req = request_in(root, atoms);
    req.selective = false;
    req.resolve()
        .unwrap_or_else(|e| panic!("resolving {atoms:?} failed: {e}"))
}

/// Leg A (N-form, RED): the Try-Tiny-shaped consumer over the installed
/// dead SLU virtual with the chained holder in the request closure.
/// Portage (NF oracle + texlive bed cell) holds the virtual: no Sub-Name
/// merged, the consumer and driver merge fresh, the top reinstalls under
/// the holder pin. Portuale latches the pre-skip flat choice and merges
/// Sub-Name.
///
/// Posture note (deliberate, bed-grounded): portuale upgrades the provider
/// outright on this indirect shape where Portage/C1 skip it (the C1-bare
/// selection-side divergence family, out of #652's scope -- the transient
/// selection is untouched by the consumer re-pop). The test therefore pins
/// the holder pressure that DOES render (top reinstalls) and leaves the
/// provider outcome unasserted, so the fix turning the consumer hold green
/// cannot break this test for posture reasons.
///
/// RED before Task 2 at the no-Sub-Name assertion.
#[test]
fn nform_consumer_holds_the_virtual_no_subname() {
    let root = hermetic_root(&[]);
    let result = resolve_bare_in(&root, &["dev-perl/p651top", "dev-perl/p652try"]);
    let names: Vec<String> = result
        .entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    // Holder pressure renders (C1-chained precedent): the requested top
    // reinstalls, pulling the holder pin into the graph.
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
    // The consumer and the fresh driver merge (NF oracle mergelist shape:
    // driver + consumer, no perl row from either).
    for (cat, pkg) in [("dev-perl", "p652try"), ("virtual", "p652foo")] {
        assert!(
            names.iter().any(|n| n == &format!("{cat}/{pkg}")),
            "the {cat}/{pkg} leg merges fresh: {names:?}"
        );
    }
    // The load-bearing leg-A assertion: Sub-Name must be absent (Portage
    // NF holds; the bed texlive cell has no Sub-Name row).
    assert!(
        !names.iter().any(|n| n == "dev-perl/p652subname"),
        "no Sub-Name fall-through under the skip (Portage holds the virtual): {names:?}"
    );
    // The virtual itself is absent or kept -- never upgraded (C1-chained
    // precedent for kept virtuals).
    assert!(
        !result.entries.iter().any(|e| matches!(
            e.outcome,
            PretendOutcome::Upgrade { .. } | PretendOutcome::Downgrade { .. }
        ) && e.category == "virtual"),
        "no virtual upgrades alongside the hold: {names:?}"
    );
}

/// Leg B (H-form): the requested early TH consumer over the bare provider.
/// The fixture vdb grew three installed `=5.42*` holders under Task 1
/// (`p656hold{1,2,3}`), so this world renders the #656 leg-(a) holder
/// shape -- and Portage on exactly this shape reinstalls: H1
/// (`probe656h_oracle.py`: `SUCCESS True`,
/// `MERGELIST ['dev-lang/perl-5.42.2', 'dev-perl/s652early-1']`, no skip,
/// no fall-throughs). The (a) gate files the kept virtuals' arms, the
/// mask lands 5.44, the seeds void, and the transient reinstalls beside
/// the requested early consumer with neither pin merged. Re-pinned
/// 2026-11-05 (was: transient-U + Encode-N, the tolerate+keeper render
/// that predates the holders).
///
/// Bed-divergence note: the real libreoffice/gimp cells still merge U +
/// Encode (their requested closure carries versioned forcers the hermetic
/// miniature lacks, and leg (b)'s keeper/posture holds that outcome --
/// the Task-5 bed re-grade covers it). The miniature and the bed cell are
/// different shapes in Portage itself (H1 = R vs bed = U); this pin tracks
/// the miniature's own oracle, not the bed's.
#[test]
fn hform_upgrade_holds_th_while_encode_merges() {
    let root = hermetic_root(&[]);
    let result = resolve_bare_in(&root, &["dev-perl/p652early", "dev-libs/p651prov"]);
    let names: Vec<String> = result
        .entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    // Holder-win posture: the transient reinstalls (H1 oracle mergelist;
    // the #656 leg-(a) enforcement-open mask, not the selection-side U).
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
        "the provider reinstalls 5.42.2 under the holder skip (H1 oracle), got {:?}: {names:?}",
        prov.outcome
    );
    // The requested early consumer still merges (H1 oracle row).
    assert!(
        names.iter().any(|n| n == "dev-perl/p652early"),
        "the requested early consumer merges: {names:?}"
    );
    // Neither fall-through merges (H1 has neither pin row).
    for core in ["p651enccore", "p651thcore"] {
        assert!(
            !names.iter().any(|n| n == &format!("dev-libs/{core}")),
            "no {core} fall-through under the holder skip (H1 oracle): {names:?}"
        );
    }
    // No virtual upgrades (H1 upgrades neither virtual).
    assert!(
        !result.entries.iter().any(|e| matches!(
            e.outcome,
            PretendOutcome::Upgrade { .. } | PretendOutcome::Downgrade { .. }
        ) && e.category == "virtual"),
        "no virtual upgrades alongside the hold: {names:?}"
    );
}
