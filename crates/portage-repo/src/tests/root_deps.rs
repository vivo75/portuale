use super::*;

#[test]
fn without_root_deps_a_bdepend_only_satisfiable_on_the_running_root_is_no_visible_candidate() {
    // rootdepspkg's own BDEPEND (dev-libs/rootdepsprovider) has no
    // ebuild anywhere in the fixture repo tree -- without
    // `--root-deps`, portuale has no way to know it's already
    // satisfied elsewhere, so it's reported (not fatal, since it's a
    // dependency, not a top-level atom) exactly like any other
    // unresolvable dependency.
    assert_eq!(
        graph_root_deps("dev-libs/rootdepspkg", None),
        vec![
            (
                "dev-libs/rootdepsprovider".to_string(),
                PretendOutcome::NoVisibleCandidate
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
    // Same atom, but now with `--root-deps` pointed at a running
    // root where rootdepsprovider genuinely is installed (here, the
    // same fixture tree, reused purely as a convenient real vdb --
    // see `running_root_satisfies_atom`'s own doc comment: ordinary
    // dependency resolution never consults a root's own vdb at all,
    // only the ebuild repo tree, so this is a valid, real proof the
    // new running-root check -- not some other pre-existing
    // mechanism -- is what's excluding it). No separate
    // NoVisibleCandidate entry for rootdepsprovider is produced.
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
    // rootdepsorpkg's own BDEPEND is `|| ( rootdepsnonexistent
    // rootdepsprovider )` -- neither branch has an ebuild anywhere in
    // the fixture repo tree, so `all_available` is false for both
    // even without `--root-deps`. `rootdepsprovider` IS installed in
    // the fixture vdb (used as the target root here regardless of
    // `root_deps_running_root`), so real `dep_zapdeps`'s `other_*`
    // classification (backlog #22 slice 4) files it in `other_installed`
    // (every non-blocker atom's full vdb match succeeds) while
    // `rootdepsnonexistent` -- matched nowhere at all -- falls to
    // plain `other`; `other_installed` outranks `other`, so the `||`
    // group resolves to `rootdepsprovider` alone via the
    // `allow_masked` second pass (real's own two-pass return),
    // dropping `rootdepsnonexistent` entirely rather than reporting
    // it as a spurious failure alongside a genuinely-satisfied dep.
    // `rootdepsprovider` still reports `NoVisibleCandidate` here --
    // a pre-existing, slice-4-unrelated quirk (this atom's own
    // resolution path checks tree visibility before an install
    // shortcut for a BDEPEND with no root-deps feed-in); it is NOT
    // itself queued as a build target as a `PretendOutcome::New` and
    // is dropped by the trailing `root_deps_satisfied_atoms` filter
    // once `root_deps_running_root` IS given, per the second
    // assertion below.
    assert_eq!(
        graph_root_deps("dev-libs/rootdepsorpkg", None),
        vec![
            (
                "dev-libs/rootdepsprovider".to_string(),
                PretendOutcome::NoVisibleCandidate
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

    // With `--root-deps` pointed at a running root where
    // `rootdepsprovider` genuinely is installed, the disjunctive
    // closure now also accepts a branch that's running-root-
    // satisfied even though it's invisible in the tree -- so the
    // `||` group resolves to that one branch specifically (not
    // both), which the trailing `root_deps_satisfied_atoms` filter
    // then correctly drops from the queue entirely: no
    // `rootdepsprovider` entry (already satisfied), and critically
    // no `rootdepsnonexistent` entry either (never selected, so
    // never queued at all) -- the real bug this fix closes.

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
    // Backlog #233: walking an installed package's recorded deps
    // (`Deep::Unlimited`, no `--update`), its stale built
    // slot-operator atom `dev-libs/reinstslottarget:0/1=` (installed
    // 1.0 at `SLOT="0/1"`, only the `0/2` ebuild in the tree) settles
    // `AlreadyInstalled` -- the queue loop's runtime-keyed rewrite of
    // the `NoVisibleCandidate` `resolve_pretend` still reports (real
    // `_wrapped_select_pkg_highest_available_imp`,
    // `depgraph.py:7799-7840`, iterates the installed db for every
    // resolve). The live `:=` edge settles the same instance via the
    // ordinary avoid-update path and dedups (`other_outcomes`), so
    // the walk holds exactly two entries and never reports. Before
    // the fix the `:0/1=` edge produced a `NoVisibleCandidate`
    // entry, which the `@world` walk then rendered fatal (rc 1)
    // before the #210 scan engaged. (Entry order is walk order: the
    // dep settles before its parent entry is pushed.)
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
