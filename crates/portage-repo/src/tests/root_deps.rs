use super::*;

#[test]
fn without_root_deps_a_bdepend_on_an_installed_only_package_is_kept() {
    // Without `--root-deps` a BDEPEND on an installed-only package is kept
    // from the installed db (Portage consults it for every dep kind of a
    // package being merged): `AlreadyInstalled`, not a miss. Oracle:
    // Portage 3.0.82.2 S0 `s0-e-gbapp-pvuD` (#360 S4: `N gextra + N gbapp`
    // for a `BDEPEND` on installed-only `pg34-gone`).
    assert_eq!(
        graph_root_deps("dev-libs/rootdepspkg", None),
        vec![
            (
                "dev-libs/rootdepsprovider".to_string(),
                PretendOutcome::AlreadyInstalled {
                    version: "1.0".to_string()
                }
            ),
            (
                "dev-libs/rootdepspkg".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            )
        ]
    );
}

#[test]
fn root_deps_drops_a_bdepend_already_satisfied_by_the_running_root() {
    // With `--root-deps` pointed at a root where the provider is
    // installed, no NoVisibleCandidate entry is produced (see
    // `running_root_satisfies_atom`: resolution never reads a vdb).
    let root = fixtures_root();
    assert_eq!(
        graph_root_deps("dev-libs/rootdepspkg", Some(&root)),
        vec![(
            "dev-libs/rootdepspkg".to_string(),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        ),]
    );
}

#[test]
fn root_deps_feeds_running_root_satisfiability_into_disjunctive_branch_selection() {
    // Neither `||` branch has an ebuild, but the installed branch
    // outranks (`other_installed` over `other`) so only it is queued — and
    // the queued installed-only branch is kept from the installed db, like
    // any other installed-only dep (Portage `dep_zapdeps` all-installed
    // pick plus the vardb match; oracle for the keep: Portage 3.0.82.2 S0
    // `s0-e-gbapp-pvuD`, #360 S4).
    assert_eq!(
        graph_root_deps("dev-libs/rootdepsorpkg", None),
        vec![
            (
                "dev-libs/rootdepsprovider".to_string(),
                PretendOutcome::AlreadyInstalled {
                    version: "1.0".to_string()
                }
            ),
            (
                "dev-libs/rootdepsorpkg".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            )
        ],
        "other_installed outranks other even without --root-deps, so only the \
         genuinely-installed branch is ever queued"
    );

    // With `--root-deps` the running-root-satisfied branch is selected
    // and then dropped as already-satisfied; the other is never queued.

    let root = fixtures_root();
    assert_eq!(
        graph_root_deps("dev-libs/rootdepsorpkg", Some(&root)),
        vec![(
            "dev-libs/rootdepsorpkg".to_string(),
            PretendOutcome::New {
                version: "1.0".to_string()
            }
        ),],
        "the running-root-satisfied branch is selected and then dropped as already-satisfied"
    );
}

#[test]
fn stale_built_slot_operator_dep_of_an_installed_package_settles_installed() {
    // A stale built slot-operator dep of an installed package settles
    // `AlreadyInstalled` via the queue loop's runtime-keyed rewrite.
    // Portage: depgraph.py, _wrapped_select_pkg_highest_available_imp
    assert_eq!(
        graph_root_deps("dev-libs/reinstslotbound", None),
        vec![
            (
                "dev-libs/reinstslottarget".to_string(),
                PretendOutcome::AlreadyInstalled {
                    version: "1.0".to_string()
                }
            ),
            (
                "dev-libs/reinstslotbound".to_string(),
                PretendOutcome::AlreadyInstalled {
                    version: "1.0".to_string()
                }
            ),
        ]
    );
}
