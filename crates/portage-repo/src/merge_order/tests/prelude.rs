use super::*;

#[test]
fn select_dep_target_elimination_orders_installed_before_merge_bound() {
    let runtime = DepPriority {
        runtime: true,
        ..DepPriority::default()
    };
    // Installed 2.0 first: it is eliminated first, so the merge-bound
    // 1.0 survives (a version compare would keep the installed one).
    let mut installed = new_entry("dev-libs", "x", "2.0", Vec::new());
    installed.outcome = PretendOutcome::AlreadyInstalled {
        version: "2.0".into(),
    };
    let merged = new_entry("dev-libs", "x", "1.0", Vec::new());
    let user = new_entry(
        "dev-libs",
        "user",
        "1.0",
        vec![plain_edge("dev-libs/x", "dev-libs", "x", runtime)],
    );
    let entries = vec![installed, merged, user.clone()];
    let root = Path::new("/nonexistent-root-for-unit-test");
    let pre = digraph_prelude(&entries, root);
    let none = HashSet::new();
    assert_eq!(
        pre.select_dep_target(SelectDepTargetArgs {
            entries: &entries,
            from: 2,
            from_deps: &entries[2].deps,
            ei: 0,
            suppressed: &none,
            merge_bound_only: false
        }),
        Some(1),
        "the installed candidate is eliminated first"
    );
    // Mirrored order: the `(false, true)` arm still sorts the installed
    // one first.
    let mut installed = new_entry("dev-libs", "x", "2.0", Vec::new());
    installed.outcome = PretendOutcome::AlreadyInstalled {
        version: "2.0".into(),
    };
    let merged = new_entry("dev-libs", "x", "1.0", Vec::new());
    let entries = vec![merged, installed, user];
    let pre = digraph_prelude(&entries, root);
    assert_eq!(
        pre.select_dep_target(SelectDepTargetArgs {
            entries: &entries,
            from: 2,
            from_deps: &entries[2].deps,
            ei: 0,
            suppressed: &none,
            merge_bound_only: false
        }),
        Some(0),
        "the merge-bound candidate survives elimination"
    );
}
