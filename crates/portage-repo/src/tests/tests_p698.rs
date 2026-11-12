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

/// #713, charter property 5: a holder that joins the plan only because the provider
/// moves names that provider as its parent, so the plan says why it is there.
#[test]
fn p713_co_upgraded_holders_name_the_provider_as_parent() {
    let world = [
        "dev-libs/p698rebuild",
        "dev-libs/p698reinst",
        "dev-libs/p698upg",
    ];
    let root = p698_root(&world);
    let result = resolve_with(SolverKind::Portage, &root, &world);
    let provider = ("dev-libs".to_string(), "p569prov".to_string());
    for name in ["p698rebuild", "p698reinst", "p698upg"] {
        let entry = result
            .entries
            .iter()
            .find(|e| e.package == name)
            .unwrap_or_else(|| panic!("{name} is in the plan"));
        assert_eq!(entry.required_by, std::slice::from_ref(&provider), "{name}");
    }
}

/// #713, charter property 5: a consumer rebuilt only because the provider's sub-slot changed
/// names that provider as its parent (the walk queues such rebuilds as parentless roots).
#[test]
fn p713_slot_operator_rebuilds_name_the_provider_as_parent() {
    let world = ["dev-libs/p715h1", "dev-libs/p715h2", "dev-libs/p715prov"];
    let root = p715_root(&world);
    let mut req = request_in(&root, &["dev-libs/p715prov"]);
    req.selective = false;
    req.config.complete_seed_atoms = world.iter().map(|w| (*w).to_string()).collect();
    let result = active_resolver_for(SolverKind::Portage)
        .resolve(&req)
        .expect("the #715 request resolves");
    let provider = ("dev-libs".to_string(), "p715prov".to_string());
    for name in ["p715h1", "p715h2"] {
        let entry = result
            .entries
            .iter()
            .find(|e| e.package == name)
            .unwrap_or_else(|| panic!("{name} is in the plan"));
        assert_eq!(entry.required_by, std::slice::from_ref(&provider), "{name}");
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

/// The #698 holders, the not-installed `p727top` (its `||` group lists the new
/// provider first) and a request for it, with the given `--update` setting.
fn p727_resolve(kind: SolverKind, root: &Path, atoms: &[&str], update: bool) -> GraphResult {
    let world = [
        "dev-libs/p698rebuild",
        "dev-libs/p698reinst",
        "dev-libs/p698upg",
    ];
    let mut req = request_in(root, atoms);
    req.solver = kind;
    req.selective = false;
    req.update = update;
    if update {
        req.deep = Deep::Unlimited;
    }
    req.config.complete_seed_atoms = world.iter().map(|w| (*w).to_string()).collect();
    active_resolver_for(kind)
        .resolve(&req)
        .expect("the #727 request resolves")
}

/// #727, owner decision 2026-11-12 (smallest plan): a request that names neither the
/// provider nor `--update` leaves the provider where it is, in every solver; the
/// co-upgrade applies when the provider is requested or the request updates deeply.
#[test]
fn p727_a_plain_request_leaves_the_provider_where_it_is() {
    let world = [
        "dev-libs/p698rebuild",
        "dev-libs/p698reinst",
        "dev-libs/p698upg",
    ];
    let root = p698_root(&world);
    for kind in SOLVERS {
        let plain = p727_resolve(kind, &root, &["dev-libs/p727top"], false);
        assert_eq!(
            merged(&plain),
            ["dev-libs/p727top-1"],
            "{kind}: plain request"
        );
        let updating = p727_resolve(kind, &root, &["dev-libs/p727top"], true);
        assert_eq!(
            merged(&updating),
            [
                "dev-libs/p569prov-5.44",
                "dev-libs/p698rebuild-1",
                "dev-libs/p698reinst-1",
                "dev-libs/p698upg-2",
                "dev-libs/p727top-1",
            ],
            "{kind}: --update --deep"
        );
        let named = p727_resolve(
            kind,
            &root,
            &["dev-libs/p727top", "dev-libs/p569prov"],
            false,
        );
        assert!(
            merged(&named).contains(&"dev-libs/p569prov-5.44".to_string()),
            "{kind}: the provider is requested"
        );
    }
}

/// #728, charter property 3: a package rebuilt for the provider's new sub-slot moves to its
/// best visible version, in every solver. `p728virt` pins `p728core` with `~`, so both move
/// together and the plan stays consistent.
#[test]
fn p728_a_rebuilt_package_moves_to_its_best_visible_version() {
    let world = ["dev-libs/p728virt"];
    let root = hermetic_root(&world);
    for (cpv, rdepend) in [
        ("dev-libs/p728core-1", "dev-libs/p569prov:0/5.42="),
        (
            "dev-libs/p728virt-1",
            "~dev-libs/p728core-1 dev-libs/p569prov:0/5.42=",
        ),
    ] {
        let dir = root.join("var/db/pkg").join(cpv);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(dir.join("EAPI"), "8\n").unwrap();
        fs::write(dir.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        fs::write(dir.join("SLOT"), "0\n").unwrap();
        fs::write(dir.join("repository"), "testrepo\n").unwrap();
    }
    for kind in SOLVERS {
        let result = resolve_with(kind, &root, &world);
        assert_eq!(
            merged(&result),
            [
                "dev-libs/p569prov-5.44",
                "dev-libs/p728core-2",
                "dev-libs/p728virt-2",
            ],
            "{kind}"
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

/// The note #703 records: the provider slot, both versions and each route-less
/// holder with the recorded atom that pins the old version. `p698rebuild` has a
/// route, so it is not listed.
fn expected_no_route() -> Vec<HeldBackNoRoute> {
    vec![HeldBackNoRoute {
        category: "dev-libs".to_string(),
        package: "p569prov".to_string(),
        slot: "0".to_string(),
        installed_version: "5.42".to_string(),
        skipped_version: "5.44".to_string(),
        holders: vec![(
            "dev-libs/p573virt".to_string(),
            "1".to_string(),
            "=dev-libs/p569prov-5.42*".to_string(),
        )],
    }]
}

/// #703, charter property 5 (messages help): the withheld upgrade names the one
/// holder that cannot follow it (default solver).
#[test]
fn p703_route_less_holder_is_recorded() {
    let world = ["dev-libs/p698rebuild", "dev-libs/p573virt"];
    let root = p698_root(&world);
    let result = resolve_with(SolverKind::Portage, &root, &world);
    // The skipped-update block already names the holder: no second note.
    assert!(result.held_back_no_route.is_empty());
    assert!(!result.skipped_updates.is_empty());
}

/// Same record from the pubgrub bridge, which prints no skipped-update block.
#[test]
fn p703_route_less_holder_is_recorded_pubgrub() {
    let world = ["dev-libs/p698rebuild", "dev-libs/p573virt"];
    let root = p698_root(&world);
    let result = resolve_with(SolverKind::PubGrub, &root, &world);
    assert_eq!(result.held_back_no_route, expected_no_route());
}
