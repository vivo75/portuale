//! #656 RED pins: leg (b) parent-posture hold + leg (a) enforcement-open.
//!
//! New file (not an extension of `tests_p651.rs` / `tests_p652b.rs`): both
//! legs need new fixture atoms and new worlds that fit neither the C1
//! substrate legs (bare / chained-holder) nor the N-form / H-form legs.
//! The `p656*` atoms extend the #651 C1 substrate hermetically (same
//! `p651prov` provider axis + `p651enc`/`p651th` virtuals +
//! `p651enccore`/`p651thcore` pins + installed `p651holder`/`p651top`/
//! `p651pin` background pressure; reused where the shape fits, forked where
//! the new legs require it):
//! - `dev-perl/p656zip-1` (fresh; Archive-Zip twin pulling
//!   `>=virtual/p651enc-2.0.0`, the M5 Encode-Locale twin): the
//!   non-replacing Encode consumer whose posture Portage bins opposite the
//!   replacing transient (committed NF bins,
//!   `LLM/evidence/2026-11-04-652/probe652_oracle.txt:15-24`).
//! - `dev-perl/p656hold{1,2,3}-1` (installed + tree-present, unrequested;
//!   `=dev-libs/p651prov-5.42*`): the bed cell's 48 installed `=perl-5.42*`
//!   requirers in miniature. Treeless `p651pin` must NOT trip the gate (N2
//!   oracle: reinstall, no skip); tree-present ones must (bed `dev-lang_perl`
//!   oracle: R + skip WARNING). The pair pins the gate's holder boundary.
//!
//! Expected values are Portage 3.0.83's own playground
//! (`LLM/evidence/2026-11-04-656/probe656_oracle.py`, re-run 2026-11-05):
//! - P1 (leg (b)): N1 + fresh Encode consumer + PINNED transient:
//!   `SUCCESS False`,
//!   `MERGELIST ['perl-core/Encode-3.210.0', 'dev-lang/perl-5.44.0',
//!   'dev-perl/s656zip-1']` -- the transient merges (replacing), the fresh
//!   leg merges Encode, TH is held (no TH row). The merge ROWS are the
//!   oracle the RED pins; the SUCCESS flag diverges (Portage aborts the
//!   resolve while portuale's H-form precedent succeeds holding TH under a
//!   live transient) and is recorded, not pinned -- see the Task-1 report.
//!   Bed ground truth: L0 `l0-20261104T163402Z` real
//!   `app-office_libreoffice.txt` / `media-gfx_gimp.txt` merge
//!   `perl-core/Encode-3.210.0` and carry NO `perl-core/Test-Harness` row.
//! - P2 (leg (a) render attempt): N1 + three installed `=5.42*` holders
//!   under the bare request: `SUCCESS True`,
//!   `MERGELIST ['dev-lang/perl-5.42.2']` -- reinstall, NO skip. A hermetic
//!   holder set does not render the bed skip, so leg (a) stays bed-oracled
//!   (bed `dev-lang_perl.txt`: R 5.42.2 + skip WARNING, Total 1 vs portuale
//!   U 5.44.0 + N Encode + N TH, Total 3; `SEED tolerate` x52,
//!   `cell-perl.err`, zero conflict lines where Portage reports 48+
//!   installed holders).
//!
//! Portage: `dep_check.py:480/:531` (replacing bypass), `:490/:541` (the
//! downgrade flags), `:531-544` (filed guard), `:636-655` (post-skip bins),
//! `:197-220/:264` (expansion); enforcement ground `depgraph.py`
//! `_slot_operator_check_reverse_dependencies` (unconditional open).

use super::*;
use crate::tests::tests_consumer_pin_conflict::{hermetic_root, request_in, resolve_in};

/// Bare-request resolve with binary parity: the CLI derives
/// `selective = update || newuse || ... || noreplace || newrepo`
/// (`crates/portuale/src/pretend/resolve_plan.rs`), so a bare
/// `emerge --pretend <atom>` resolves with `selective = false` while
/// `ResolveRequest::new` defaults it true. (Same helper as
/// `tests_p650.rs`/`tests_p651.rs`/`tests_p652b.rs`; kept local so each
/// slice file stands alone.)
fn resolve_bare_in(root: &Path, atoms: &[&str]) -> GraphResult {
    let mut req = request_in(root, atoms);
    req.selective = false;
    req.resolve()
        .unwrap_or_else(|e| panic!("resolving {atoms:?} failed: {e}"))
}

/// Leg (b) (RED): the pinned transient with the fresh Encode consumer and
/// the TH virtual present. Portage (P1 oracle + libreoffice/gimp bed cells)
/// merges the transient and the Encode fall-through while the installed TH
/// arm holds: the replacing parent's `||` judgment keeps the installed arm
/// (`dep_check.py:480/:531` bypass). Portuale fires the seed under the
/// pinned demoter with no posture and merges TH.
///
/// RED before Task 2 at the no-TH-pin assertion.
#[test]
fn pinned_transient_with_fresh_encode_consumer_holds_th() {
    let root = hermetic_root(&[]);
    let result = resolve_in(&root, &["dev-perl/p656zip", "=dev-libs/p651prov-5.44.0"]);
    let names: Vec<String> = result
        .entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    // Upgrade posture: the pinned transient merges (P1 mergelist;
    // M6b/M5/Q19 tolerate precedent -- the (a) gate never touches pinned
    // demoters).
    let prov = result
        .entries
        .iter()
        .find(|e| e.category == "dev-libs" && e.package == "p651prov")
        .expect("the provider stays in the graph");
    assert!(
        matches!(
            &prov.outcome,
            PretendOutcome::Upgrade { to, .. } if to == "5.44.0"
        ),
        "the pinned transient merges (upgrade posture), got {:?}: {names:?}",
        prov.outcome
    );
    // The fresh consumer merges (P1 mergelist).
    assert!(
        names.iter().any(|n| n == "dev-perl/p656zip"),
        "the fresh Encode consumer merges: {names:?}"
    );
    // Encode merges (P1 mergelist; bed libreoffice row 33 / gimp row 22).
    let enc = result
        .entries
        .iter()
        .find(|e| e.category == "dev-libs" && e.package == "p651enccore")
        .unwrap_or_else(|| panic!("the Encode fall-through merges: {names:?}"));
    assert!(
        matches!(
            &enc.outcome,
            PretendOutcome::New { version } if version == "3.210.0"
        ),
        "the Encode fall-through merges new, got {:?}: {names:?}",
        enc.outcome
    );
    // The load-bearing leg-(b) assertion: TH must be held (P1 has no TH
    // row; bed cells have no TH row). Portuale commits the seed today.
    assert!(
        !names.iter().any(|n| n == "dev-libs/p651thcore"),
        "no TH fall-through under the replacing transient (Portage holds TH): {names:?}"
    );
    // No virtual upgrades (bed upgrades neither the Encode nor the TH virtual).
    assert!(
        !result.entries.iter().any(|e| matches!(
            e.outcome,
            PretendOutcome::Upgrade { .. } | PretendOutcome::Downgrade { .. }
        ) && e.category == "virtual"),
        "no virtual upgrades alongside the hold: {names:?}"
    );
}

/// Leg (a) (RED): the bare provider request with tree-present unrequested
/// holders. Portage (bed `dev-lang_perl.txt`) opens the 48-holder conflict
/// unconditionally and backtracks (mask 5.44 -> R + skip WARNING, Total 1).
/// Portuale tolerates every kept virtual's `||` arm for the bare demoter
/// (`SEED tolerate` x52, zero conflict lines) and merges U + both
/// fall-throughs (Total 3).
///
/// The treeless `p651pin` (N2 oracle: reinstall, no skip) must NOT trip the
/// gate -- the three `p656hold` atoms pin the boundary: installed AND
/// tree-present requirers file the conflict.
///
/// RED before Task 3 at the no-TH-pin assertion (first of the three
/// single-gate flips: TH, Encode, provider posture -- all three move via
/// the one enforcement-open link; execution stops at the first failure
/// pre-fix, all three pass post-fix).
#[test]
fn bare_request_with_tree_present_holders_skips_the_transient() {
    let root = hermetic_root(&[]);
    let result = resolve_bare_in(&root, &["dev-libs/p651prov"]);
    let names: Vec<String> = result
        .entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    // The load-bearing leg-(a) assertion: no TH fall-through under the skip
    // (bed has no TH row). Portuale commits the seed today.
    assert!(
        !names.iter().any(|n| n == "dev-libs/p651thcore"),
        "no TH fall-through under the holder skip (Portage holds it): {names:?}"
    );
    // Same gate, same flip: no Encode fall-through either (bed has no
    // Encode row on the perl cell).
    assert!(
        !names.iter().any(|n| n == "dev-libs/p651enccore"),
        "no Encode fall-through under the holder skip (Portage holds it): {names:?}"
    );
    // Same gate, same flip: the provider reinstalls 5.42.2 (bed R row) --
    // the enforcement-open mask, not the selection-side U.
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
        "the provider reinstalls 5.42.2 under the holder skip, got {:?}: {names:?}",
        prov.outcome
    );
    // No virtual upgrades alongside the hold.
    assert!(
        !result.entries.iter().any(|e| matches!(
            e.outcome,
            PretendOutcome::Upgrade { .. } | PretendOutcome::Downgrade { .. }
        ) && e.category == "virtual"),
        "no virtual upgrades alongside the hold: {names:?}"
    );
}

/// Installs `cat/pkg-version` (no deps) into the scratch root's vdb.
fn install_bare(root: &Path, category: &str, pkg_version: &str) {
    let dir = root.join("var/db/pkg").join(category).join(pkg_version);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("CATEGORY"), format!("{category}\n")).unwrap();
    fs::write(dir.join("EAPI"), "8\n").unwrap();
    fs::write(dir.join("SLOT"), "0\n").unwrap();
    fs::write(dir.join("repository"), "testrepo\n").unwrap();
}

/// Leg (a), seedless: the H-form world of
/// `hform_upgrade_holds_th_while_encode_merges` (requested early TH
/// consumer + bare provider + the three installed `=5.42*` holders) with the
/// Encode core already installed. The Encode virtual then files no seed, and
/// the TH seed is held by the requested keeper, so the only thing that can
/// make the walk see the holder-open pin is the `RevDep` restart. The pin is
/// applied to `grown` before the seed feed (`feedback/mod.rs`); the later
/// `step_reverse_dep_pins` re-apply is a latched no-op, so `added` stays
/// false and nothing restarts. The provider must still reinstall 5.42.2
/// (H1 oracle), not upgrade to 5.44.0.
///
/// Review finding 1 (2026-11-05, commits since 7dae729b): a RED here means
/// the early apply swallowed the `RevDep` restart.
#[test]
fn seedless_holder_open_pin_still_restarts_the_walk() {
    let root = hermetic_root(&[]);
    install_bare(&root, "dev-libs", "p651enccore-3.210.0");
    let result = resolve_bare_in(&root, &["dev-perl/p652early", "dev-libs/p651prov"]);
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
        "the holder-open pin must bite without a seed (reinstall 5.42.2), got {:?}",
        prov.outcome
    );
}
