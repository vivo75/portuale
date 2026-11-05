//! #647: a multi-version installed new-style virtual must expand per version.
//!
//! S0 #645 (`LLM/evidence/2026-11-03-645/REPORT.md` §6) proved the trigger:
//! `virtual/perl-Scalar-List-Utils` shipped 4 tree ebuilds and
//! `virtual/perl-Encode` 2, so the single-tree-version cut fired first in
//! both `virtual_expansion_seeds` (`rev_dep_or.rs:706`) and
//! `expand_new_style_virtual_arm` (`rev_dep_or.rs:823`) before the fall-through
//! arm (`dev-perl/Sub-Name`, `perl-core/Encode-3.210.0`) could become a seed
//! (lifted by Task 2's per-version expansion below).
//!
//! Expected values are Portage 3.0.83's own playground on the verbatim real
//! shapes (`LLM/evidence/2026-11-03-647/probe647_oracle.py`, minuend of the
//! S0 `probe645_mutations.py` M6b/M5/M4/M5b blocks):
//! - leg A (M6b, atom request): mergelist
//!   `[Sub-Name-0.280.0-r1, perl-5.44.0, Try-Tiny-0.320.0]`, no virtual row
//!   (kept nomerge — portuale does not materialize the unqueued kept virtual,
//!   the p03 precedent); no virtual upgrade.
//! - leg B (M5, atom request): mergelist
//!   `[perl-5.44.0, perl-core/Encode-3.210.0, Encode-Locale-1.50.0-r1]`, the
//!   virtual kept at installed `3.210.0-r2`, no virtual upgrade.
//! - not the cell (M4/M5b, `@world --update --deep`, and the bare-atoms
//!   `-uD` shape Portage answers identically — committed `M4-bare-Ud` run in
//!   the oracle probe): the virtual UPGRADES
//!   (`1.700.0` / `3.240.0`) instead of falling through — the scope boundary
//!   Task 2 must preserve, not "fix".
//!
//! The `p647*` fixture family mirrors those shapes hermetically
//! (`dev-libs/p647prov-5.42.2`/`-5.44.0` for `dev-lang/perl`,
//! `virtual/p647slu-*`/`virtual/p647enc-*` with the verbatim real RDEPENDs,
//! `dev-libs/p647try` with the verbatim wrapped `!minimal? ( || ( ... ) )`,
//! `dev-libs/p647enclocale` with plain `>=virtual/p647enc-2.0.0`).

use super::*;
use crate::tests::tests_consumer_pin_conflict::{hermetic_root, request_in, resolve_in};

/// Leg A (M6b mirror): the wrapped `||` of the `!minimal?` consumer names the
/// installed multi-version virtual first, but every tree version's RDEPEND is
/// dead once the co-scheduled upgrade replaces its `=prov-5.42*` arm, so the
/// walk must fall through to `dev-libs/p647sub` while the virtual stays kept.
///
/// Portage playground oracle (`LLM/evidence/2026-11-03-647/probe647_oracle.py`,
/// M6b): `SUCCESS True`, `MERGELIST ['dev-perl/Sub-Name-0.280.0-r1',
/// 'dev-lang/perl-5.44.0', 'dev-perl/Try-Tiny-0.320.0']` — the fall-through
/// leaf plus the perl upgrade plus the requested leaf, the virtual kept
/// (nomerge) at installed `1.680.100_rc`. `minimal` defaults OFF by
/// profile-chain absence (no `package.use`, profile, or `make.defaults`
/// enables it — the M1 oracle runs the conditional active), so the wrapped
/// `||` is live; if a profile ever enables `minimal`, this leg goes silent
/// for a non-product reason.
///
/// RED before Task 2: the `:706`/`:823` single-tree-version cuts skip the
/// 4-version virtual, so no `p647sub` seed exists.
#[test]
fn multi_version_installed_virtual_falls_through_the_wrapped_arm() {
    let root = hermetic_root(&[]);
    let result = resolve_in(&root, &["dev-libs/p647try", "=dev-libs/p647prov-5.44.0"]);
    let names: Vec<String> = result
        .entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    assert!(
        names.iter().any(|n| n == "dev-libs/p647sub"),
        "the wrapped || fell through to dev-libs/p647sub-0.280.0-r1 in Portage, portuale merged: {names:?}"
    );
    let sub = result
        .entries
        .iter()
        .find(|e| e.category == "dev-libs" && e.package == "p647sub")
        .expect("p647sub merged");
    assert!(
        matches!(sub.outcome, PretendOutcome::New { .. }),
        "the fall-through arm merges new, got {:?}",
        sub.outcome
    );
    // Portage keeps the virtual nomerge (M6b mergelist has no virtual row),
    // and portuale does not materialize unqueued kept virtuals as entries:
    // the single-version p03 shape (`or_group_of_a_nested_installed_virtual_`
    // `falls_through_to_the_next_arm`) falls through with no virtual node
    // either. So the virtual is absent here, or kept — never upgraded.
    // Either representation satisfies the oracle; the load-bearing half is
    // the no-upgrade assertion (a winning `1.700.0` conjunction would show
    // up as a virtual `Upgrade` row).
    assert!(
        result
            .entries
            .iter()
            .filter(|e| e.category == "virtual" && e.package == "p647slu")
            .all(|e| matches!(
                &e.outcome,
                PretendOutcome::AlreadyInstalled { version } if version == "1.680.100_rc"
            )),
        "the virtual is absent or kept at installed 1.680.100_rc: {:?}",
        result.entries
    );
    assert!(
        !result.entries.iter().any(|e| matches!(
            e.outcome,
            PretendOutcome::Upgrade { .. } | PretendOutcome::Downgrade { .. }
        ) && e.category == "virtual"),
        "no virtual upgrades alongside the fall-through: {names:?}"
    );
}

/// Leg A0 (#657): the same consumer with no perl upgrade in the graph keeps
/// the installed virtual. Portage expands one conjunction for the selected
/// (installed) virtual, resolves its nested `||` first and ranks the result
/// `preferred_installed` over `dev-libs/p647sub`; it never builds a
/// conjunction per tree version.
///
/// Portage playground oracle (scratch probe `probe_tt.py`, case A0, Portage
/// 3.0.83, M4 dicts of `probe647_oracle.py`): `MERGELIST
/// ['dev-perl/Try-Tiny-0.320.0']`, no Sub-Name, no perl, no virtual row.
///
/// RED before #657: the per-version conjunctions each carried a dead
/// `~perl-core` member, ranked `OtherInstalledSome`, and `p647sub` won.
#[test]
fn bare_consumer_keeps_the_installed_multi_version_virtual() {
    let root = hermetic_root(&[]);
    let result = resolve_in(&root, &["dev-libs/p647try"]);
    let names: Vec<String> = result
        .entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    assert!(
        !names.iter().any(|n| n == "dev-libs/p647sub"),
        "Portage keeps the installed virtual arm, portuale merged the fall-through: {names:?}"
    );
    assert!(
        result
            .entries
            .iter()
            .filter(|e| e.package == "p647prov" || e.package == "p647slu")
            .all(|e| matches!(e.outcome, PretendOutcome::AlreadyInstalled { .. })),
        "perl and the virtual stay installed, nothing is scheduled: {:?}",
        result.entries
    );
}

/// Leg B (M5 mirror): the plain top-level `>=virtual/...` consumer never
/// reaches the consumer expander by design (Portage expands it inline,
/// `dep_check.py:287-288`) — it is seed-feed territory only. With the
/// co-scheduled upgrade killing the installed virtual's `=prov-5.42*` arm, the
/// seed walk must surface `dev-libs/p647enccore-3.210.0` while the virtual
/// stays kept.
///
/// Portage playground oracle (`LLM/evidence/2026-11-03-647/probe647_oracle.py`,
/// M5): `SUCCESS True`, `MERGELIST ['dev-lang/perl-5.44.0',
/// 'perl-core/Encode-3.210.0', 'dev-perl/Encode-Locale-1.50.0-r1']` — the
/// `~` arm plus the perl upgrade plus the requested leaf, the virtual kept
/// (nomerge) at installed `3.210.0-r2`.
///
/// RED before Task 2: the `:706` cut skips the 2-version virtual, so no
/// `p647enccore` seed exists.
#[test]
fn multi_version_installed_virtual_feeds_the_plain_consumer_arm() {
    let root = hermetic_root(&[]);
    let result = resolve_in(
        &root,
        &["dev-libs/p647enclocale", "=dev-libs/p647prov-5.44.0"],
    );
    let names: Vec<String> = result
        .entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    assert!(
        names.iter().any(|n| n == "dev-libs/p647enccore"),
        "the seed feed surfaced dev-libs/p647enccore-3.210.0 in Portage, portuale merged: {names:?}"
    );
    let virt = result
        .entries
        .iter()
        .find(|e| e.category == "virtual" && e.package == "p647enc")
        .expect("the installed virtual stays in the graph");
    assert!(
        matches!(
            &virt.outcome,
            PretendOutcome::AlreadyInstalled { version } if version == "3.210.0-r2"
        ),
        "the virtual is kept at installed 3.210.0-r2, got {:?}",
        virt.outcome
    );
    assert!(
        !result.entries.iter().any(|e| matches!(
            e.outcome,
            PretendOutcome::Upgrade { .. } | PretendOutcome::Downgrade { .. }
        ) && e.category == "virtual"),
        "no virtual upgrades alongside the fall-through: {names:?}"
    );
}

/// Not the cell (M4 mirror): the SAME tree under an update+deep request
/// upgrades the virtual instead of falling through. Portage answers the
/// bare-atoms `-uD` request identically to `@world --update --deep` —
/// committed run `M4-bare-Ud` in
/// `LLM/evidence/2026-11-03-647/probe647_oracle.py` (M4 ebuilds verbatim):
/// `SUCCESS True`, `MERGELIST ['dev-lang/perl-5.44.0',
/// 'virtual/perl-Scalar-List-Utils-1.700.0', 'dev-perl/Try-Tiny-0.320.0']` —
/// so this pins the in-repo `-uD` shape (`tests_p572.rs`
/// `deep_update_still_lists_the_fall_through_arm` precedent): the virtual
/// upgrades to `1.700.0` and the fall-through arm stays absent. Task 2 must
/// keep this behavior, not "fix" it.
#[test]
fn update_deep_upgrades_the_multi_version_virtual_instead_of_falling_through() {
    let root = hermetic_root(&[]);
    let mut req = request_in(&root, &["dev-libs/p647try", "dev-libs/p647prov"]);
    req.update = true;
    req.deep = Deep::Unlimited;
    req.selective = true;
    let result = req
        .resolve()
        .unwrap_or_else(|e| panic!("resolving -uD failed: {e}"));
    let names: Vec<String> = result
        .entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    let virt = result
        .entries
        .iter()
        .find(|e| e.category == "virtual" && e.package == "p647slu")
        .expect("the virtual stays in the graph under -uD");
    assert!(
        matches!(
            &virt.outcome,
            PretendOutcome::Upgrade { to, .. } if to == "1.700.0"
        ),
        "under -uD the virtual upgrades to 1.700.0 (Portage M4), got {:?}",
        virt.outcome
    );
    assert!(
        !names.iter().any(|n| n == "dev-libs/p647sub"),
        "under -uD there is no fall-through to dev-libs/p647sub: {names:?}"
    );
}

/// Not the cell (M5b mirror): the plain-consumer tree under an update+deep
/// request upgrades the virtual instead of feeding the fall-through arm.
/// Portage answers the bare-atoms `-uD` request identically to
/// `@world --update --deep` — committed run `M5b-bare-Ud` in
/// `LLM/evidence/2026-11-03-647/probe647_oracle.py` (M5b ebuilds verbatim):
/// `SUCCESS True`, `MERGELIST ['dev-lang/perl-5.44.0',
/// 'virtual/perl-Encode-3.240.0']` — the virtual UPGRADES to `3.240.0` and
/// the `~` arm stays absent. This pins the seed feed's update behavior: an
/// upgrading virtual is no kept instance, so its installed RDEPEND pulls
/// nothing (the `AlreadyInstalled` gate skips it). Task 2 must keep this
/// behavior, not "fix" it.
///
/// (The Rust request names the leaf explicitly because the test tree leaves
/// it uninstalled, where M5b has it installed: the leaf merges `New` here
/// and is absent there. The pinned boundary — virtual upgrades, no
/// `p647enccore` — is identical.)
#[test]
fn update_deep_upgrades_the_plain_consumer_virtual_without_feeding_the_arm() {
    let root = hermetic_root(&[]);
    let mut req = request_in(&root, &["dev-libs/p647enclocale", "dev-libs/p647prov"]);
    req.update = true;
    req.deep = Deep::Unlimited;
    req.selective = true;
    let result = req
        .resolve()
        .unwrap_or_else(|e| panic!("resolving -uD failed: {e}"));
    let names: Vec<String> = result
        .entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    let virt = result
        .entries
        .iter()
        .find(|e| e.category == "virtual" && e.package == "p647enc")
        .expect("the virtual stays in the graph under -uD");
    assert!(
        matches!(
            &virt.outcome,
            PretendOutcome::Upgrade { to, .. } if to == "3.240.0"
        ),
        "under -uD the virtual upgrades to 3.240.0 (Portage M5b), got {:?}",
        virt.outcome
    );
    assert!(
        !names.iter().any(|n| n == "dev-libs/p647enccore"),
        "under -uD there is no seed feed of dev-libs/p647enccore-3.210.0: {names:?}"
    );
}
