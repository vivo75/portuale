//! #698: an upgrade is withheld only when an installed pin holder has no route
//! to the new version; otherwise the holders join the plan, in every solver.
//!
//! Expected behaviour comes from the charter (property 1: every kept package
//! keeps its dependencies; the system stays as up to date as it can), not from
//! Portage, which gives up and keeps the old version (`LLM/portage-quirks.md`).

use super::tests_consumer_pin_conflict::{hermetic_root, request_in};
use super::*;

/// The shared fixture vdb plus the installed #698 holders, all recording the
/// pins the provider's old sub-slot left in them.
fn p698_root(world: &[&str]) -> TempDir {
    let base = hermetic_root(world);
    let recorded = [
        ("dev-libs/p698rebuild-1", "dev-libs/p569prov:0/5.42="),
        (
            "dev-libs/p698reinst-1",
            "=dev-libs/p569prov-5.42* dev-libs/p569prov:0/5.42=",
        ),
        (
            "dev-libs/p698upg-1",
            "=dev-libs/p569prov-5.42* dev-libs/p569prov:0/5.42=",
        ),
        (
            "dev-libs/p573virt-1",
            "=dev-libs/p569prov-5.42* dev-libs/p569prov:0/5.42=",
        ),
    ];
    for (cpv, rdepend) in recorded {
        let dir = base.join("var/db/pkg").join(cpv);
        fs::create_dir_all(&dir).unwrap();
        let (category, _) = cpv.split_once('/').unwrap();
        fs::write(dir.join("CATEGORY"), format!("{category}\n")).unwrap();
        fs::write(dir.join("EAPI"), "8\n").unwrap();
        fs::write(dir.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        fs::write(dir.join("SLOT"), "0\n").unwrap();
        fs::write(dir.join("repository"), "testrepo\n").unwrap();
    }
    base
}

/// `(cat/pkg-version)` of every merge-bound entry, sorted.
fn merged(result: &GraphResult) -> Vec<String> {
    let mut out: Vec<String> = result
        .entries
        .iter()
        .filter_map(|entry| {
            let version = match &entry.outcome {
                PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
                    version
                }
                PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => to,
                _ => return None,
            };
            // match-str-ok: merge-list entry (Option slot/repo), not a Candidate
            Some(format!("{}/{}-{version}", entry.category, entry.package))
        })
        .collect();
    out.sort();
    out
}

fn resolve_with(kind: SolverKind, root: &Path, world: &[&str]) -> GraphResult {
    let mut req = request_in(root, &["dev-libs/p569prov"]);
    req.solver = kind;
    req.selective = false;
    req.config.complete_seed_atoms = world.iter().map(|w| (*w).to_string()).collect();
    active_resolver_for(kind)
        .resolve(&req)
        .expect("the #698 request resolves")
}

const SOLVERS: [SolverKind; 2] = [SolverKind::Portage, SolverKind::PubGrub];

/// Every holder has a route: the provider moves to 5.44 and the holders come
/// with it (rebuild, same-version reinstall, upgrade to the version that
/// accepts 5.44). Nothing is reported as skipped.
#[test]
fn holders_with_a_route_join_the_upgrade() {
    let world = [
        "dev-libs/p698rebuild",
        "dev-libs/p698reinst",
        "dev-libs/p698upg",
    ];
    let root = p698_root(&world);
    for kind in SOLVERS {
        let result = resolve_with(kind, &root, &world);
        assert_eq!(
            merged(&result),
            [
                "dev-libs/p569prov-5.44",
                "dev-libs/p698rebuild-1",
                "dev-libs/p698reinst-1",
                "dev-libs/p698upg-2",
            ],
            "{kind}: the complete plan"
        );
        assert!(
            result.skipped_updates.is_empty(),
            "{kind}: nothing is withheld when every holder has a route"
        );
    }
}

/// One holder (`p573virt`, whose tree ebuild still pins `=5.42*`) has no route:
/// the provider stays at 5.42 and the update is reported as skipped.
#[test]
fn a_holder_without_a_route_still_withholds_the_upgrade() {
    let world = ["dev-libs/p698rebuild", "dev-libs/p573virt"];
    let root = p698_root(&world);
    for kind in SOLVERS {
        let result = resolve_with(kind, &root, &world);
        assert!(
            !merged(&result).contains(&"dev-libs/p569prov-5.44".to_string()),
            "{kind}: the provider must not move past a holder with no route"
        );
        // The bridge prints no skipped-update block (#431 S1 cut).
        assert!(
            kind == SolverKind::PubGrub || !result.skipped_updates.is_empty(),
            "{kind}: the withheld update is reported"
        );
    }
}

/// The shared fixture vdb plus the installed #715 provider (sub-slot `0/1`) and
/// two holders built against it (`p715prov:0/1=`).
fn p715_root(world: &[&str]) -> TempDir {
    let base = hermetic_root(world);
    for (cpv, slot, pin) in [
        ("dev-libs/p715prov-1", "0/1", ""),
        ("dev-libs/p715h1-1", "0", "dev-libs/p715prov:0/1="),
        ("dev-libs/p715h2-1", "0", "dev-libs/p715prov:0/1="),
    ] {
        let dir = base.join("var/db/pkg").join(cpv);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(dir.join("EAPI"), "8\n").unwrap();
        fs::write(dir.join("SLOT"), format!("{slot}\n")).unwrap();
        fs::write(dir.join("repository"), "testrepo\n").unwrap();
        if !pin.is_empty() {
            fs::write(dir.join("RDEPEND"), format!("{pin}\n")).unwrap();
            fs::write(dir.join("DEPEND"), format!("{pin}\n")).unwrap();
        }
    }
    base
}

/// Position of each `dev-libs/<name>` entry in the merge order.
fn p715_order(kind: SolverKind) -> Vec<String> {
    let world = ["dev-libs/p715h1", "dev-libs/p715h2", "dev-libs/p715prov"];
    let root = p715_root(&world);
    let mut req = request_in(&root, &["dev-libs/p715prov"]);
    req.solver = kind;
    req.selective = false;
    req.config.complete_seed_atoms = world.iter().map(|w| (*w).to_string()).collect();
    let result = active_resolver_for(kind)
        .resolve(&req)
        .expect("the #715 request resolves");
    result
        .entries
        .iter()
        .map(|entry| entry.package.clone())
        .collect()
}

fn assert_provider_before_holders(kind: SolverKind) {
    let order = p715_order(kind);
    let at = |name: &str| {
        order
            .iter()
            .position(|p| p == name)
            .unwrap_or_else(|| panic!("{kind}: {name} is in the plan {order:?}"))
    };
    // PMS dependencies.tex, "Slot dependencies": a `:=` holder breaks unless the
    // provider's new slot/sub-slot is available, so it is rebuilt after it.
    assert!(at("p715lib") < at("p715prov"), "{kind}: {order:?}");
    for holder in ["p715h1", "p715h2"] {
        assert!(
            at("p715prov") < at(holder),
            "{kind}: {holder} is rebuilt before the provider it is rebuilt for: {order:?}"
        );
    }
}

/// #715 (charter property 2): holders rebuilt for a new provider sub-slot come
/// after the provider, even when the provider waits for a dependency of its own.
#[test]
fn p715_pubgrub_rebuild_rows_follow_the_provider() {
    assert_provider_before_holders(SolverKind::PubGrub);
}

/// The default solver already orders the same plan correctly.
#[test]
fn p715_portage_rebuild_rows_follow_the_provider() {
    assert_provider_before_holders(SolverKind::Portage);
}
