//! #572 (from #567's S0): an installed new-style virtual's `||` must be
//! re-resolved in the consumer's graph, so a co-scheduled upgrade that replaces
//! the arm it bound to makes the walk fall through to the next arm.
//!
//! Expected values are Portage 3.0.83's own resolver on the same shape
//! (`portage/tests/resolver/ResolverPlayground.py`; captures and the mechanism
//! write-up in `LLM/evidence/2026-10-28-572/`): `p572enc` is merged as a child of
//! the kept `[nomerge] virtual/p572virt-2.0`, because the scheduled
//! `p572prov-5.44` replaces the installed `5.42` in slot 0 and
//! `dep_zapdeps`'s downgrade guard (bug #531656) drops the `=p572prov-5.42*` arm.

use super::*;
use crate::tests::tests_consumer_pin_conflict::{hermetic_root, request_in, resolve_in};

/// The `#567` p02 shape: no `--update`, no `--deep`. The perl-shape upgrade is
/// an argument; the virtual is installed and kept; its `||` must fall through.
///
/// Ignored: RED on `main` — the pin #572 turns green
/// (`LLM/evidence/2026-10-28-572/REPORT.md`).
#[test]
#[ignore = "#572 RED: portuale never inlines an installed virtual's RDEPEND"]
fn or_group_of_an_installed_virtual_falls_through_when_an_upgrade_replaces_its_arm() {
    let root = hermetic_root(&["dev-libs/p572leaf"]);
    let result = resolve_in(&root, &["=dev-libs/p572prov-5.44", "dev-libs/p572leaf"]);
    let names: Vec<String> = result
        .entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    assert!(
        names.iter().any(|n| n == "dev-libs/p572enc"),
        "the || fell through to dev-libs/p572enc-3.210 in Portage, portuale merged: {names:?}"
    );
}

/// The same graph under `-uD @world`: already green before #572, and it must stay
/// green — the deep walk reaches the virtual by a different route.
#[test]
fn deep_update_still_lists_the_fall_through_arm() {
    let root = hermetic_root(&["dev-libs/p572leaf", "dev-libs/p572prov"]);
    let mut req = request_in(&root, &["dev-libs/p572leaf", "dev-libs/p572prov"]);
    req.update = true;
    req.deep = Deep::Unlimited;
    req.selective = true;
    let result = req.resolve().unwrap();
    let names: Vec<String> = result
        .entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    assert!(names.contains(&"dev-libs/p572enc".to_string()), "{names:?}");
}
