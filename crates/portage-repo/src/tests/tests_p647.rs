//! #647: a multi-version installed new-style virtual must expand per version.
//!
//! S0 #645 (`LLM/evidence/2026-11-03-645/REPORT.md` §6) proves the trigger:
//! `virtual/perl-Scalar-List-Utils` ships 4 tree ebuilds and
//! `virtual/perl-Encode` 2, so the single-tree-version cut fires first in
//! both `virtual_expansion_seeds` (`rev_dep_or.rs:706`) and
//! `expand_new_style_virtual_arm` (`rev_dep_or.rs:823`) before the fall-through
//! arm (`dev-perl/Sub-Name`, `perl-core/Encode-3.210.0`) can become a seed.
//!
//! Expected values are Portage 3.0.83's own playground on the verbatim real
//! shapes (`LLM/evidence/2026-11-03-647/probe647_oracle.py`, minuend of the
//! S0 `probe645_mutations.py` M6b/M5/M4/M5b blocks):
//! - leg A (M6b, atom request): mergelist
//!   `[Sub-Name-0.280.0-r1, perl-5.44.0, Try-Tiny-0.320.0]`, the virtual kept
//!   at installed `1.680.100_rc`, no virtual upgrade.
//! - leg B (M5, atom request): mergelist
//!   `[perl-5.44.0, perl-core/Encode-3.210.0, Encode-Locale-1.50.0-r1]`, the
//!   virtual kept at installed `3.210.0-r2`, no virtual upgrade.
//! - not the cell (M4/M5b, `@world --update --deep`, and the bare-atoms
//!   `-uD` shape Portage answers identically): the virtual UPGRADES
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
/// (nomerge) at installed `1.680.100_rc`.
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
    let virt = result
        .entries
        .iter()
        .find(|e| e.category == "virtual" && e.package == "p647slu")
        .expect("the installed virtual stays in the graph");
    assert!(
        matches!(
            &virt.outcome,
            PretendOutcome::AlreadyInstalled { version } if version == "1.680.100_rc"
        ),
        "the virtual is kept at installed 1.680.100_rc, got {:?}",
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
/// bare-atoms `-uD` request identically to `@world --update --deep`
/// (playground: `MERGELIST ['dev-lang/perl-5.44.0',
/// 'virtual/perl-Scalar-List-Utils-1.700.0', 'dev-perl/Try-Tiny-0.320.0']`),
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
