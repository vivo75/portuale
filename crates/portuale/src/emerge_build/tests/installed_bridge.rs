//! Order edges through installed packages (#798). Expected behaviour comes from
//! the build-order property in `docs/charter.md` (a package must not be built
//! against a library the same plan is about to replace); Portage's complete
//! graph keeps installed packages' edges, which is where the idea comes from.
use super::*;
use crate::emerge_build::installed_bridge;
use portage_repo::{DepEdge, DepPriority};
use std::collections::HashSet;

fn edge(category: &str, package: &str) -> DepEdge {
    DepEdge {
        atom: format!("{category}/{package}"),
        evaluated: format!("{category}/{package}"),
        category: category.into(),
        package: package.into(),
        priority: DepPriority {
            runtime: true,
            ..DepPriority::default()
        },
        disjunctive: false,
        alt: None,
        key: 0,
        walk_chosen: false,
    }
}

fn installed(root: &std::path::Path, cpv: &str, rdepend: &str) {
    let dir = root.join("var/db/pkg").join(cpv);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
    fs::write(dir.join("SLOT"), "0\n").unwrap();
    fs::write(dir.join("USE"), "\n").unwrap();
}

fn new(package: &str) -> GraphEntry {
    source_entry(
        package,
        PretendOutcome::New {
            version: "1.0".into(),
        },
    )
}

/// `app` names the installed `mid`, whose installed RDEPEND names the plan's `lib`
/// (two hops through `mid` and `mid2`): `app` must wait for `lib`.
#[test]
fn a_plan_entry_waits_for_a_plan_entry_its_installed_dependency_needs() {
    let root = tempdir();
    installed(&root, "dev-libs/mid-1.0", "dev-libs/mid2");
    installed(
        &root,
        "dev-libs/mid2-1.0",
        ">=dev-libs/lib-1.0:0 !dev-libs/other",
    );
    let mut app = new("app");
    app.deps = vec![edge("dev-libs", "mid")];
    let entries = vec![app, new("lib")];
    assert_eq!(
        installed_bridge::installed_bridge_edges(&entries, &root),
        vec![(0, 1)]
    );
    let _ = fs::remove_dir_all(&root);
}

/// A dependency the plan itself contains is already an edge, and an entry with no
/// installed dependency in its way gets none.
#[test]
fn no_bridge_edge_where_the_plan_already_names_the_dependency() {
    let root = tempdir();
    installed(&root, "dev-libs/mid-1.0", "dev-libs/lib");
    let mut app = new("app");
    app.deps = vec![edge("dev-libs", "lib")];
    let entries = vec![app, new("lib")];
    assert!(installed_bridge::installed_bridge_edges(&entries, &root).is_empty());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn reaches_follows_edges_transitively() {
    let deps: Vec<HashSet<usize>> = vec![HashSet::from([1]), HashSet::from([2]), HashSet::new()];
    assert!(installed_bridge::reaches(&deps, 0, 2));
    assert!(!installed_bridge::reaches(&deps, 2, 0));
}
