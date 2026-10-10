# crates/portage-repo/src/tests/root_deps.rs
`--root-deps` running-root pins: BDEPEND satisfaction, disjunctive branches, stale slot-ops.

## fn root_deps_drops_a_bdepend_already_satisfied_by_the_running_root
- The same fixture tree is reused purely as a convenient vdb; ordinary dependency resolution never consults a root's own vdb, only the ebuild tree, so this proves the running-root check excludes it rather than some pre-existing mechanism.

## fn root_deps_feeds_running_root_satisfiability_into_disjunctive_branch_selection
- Fixture: `rootdepsorpkg` BDEPENDs `|| ( rootdepsnonexistent rootdepsprovider )`; neither branch has an ebuild, so `all_available` is false for both even without `--root-deps`. `rootdepsprovider` is installed in the fixture vdb (used as the target root regardless of `root_deps_running_root`).
- Portage `dep_zapdeps`'s `other_*` classification (backlog #22 slice 4) files it in `other_installed` (every non-blocker atom's full vdb match succeeds) while `rootdepsnonexistent` falls to plain `other`; the `||` group resolves via the `allow_masked` second pass (Portage's two-pass return), dropping `rootdepsnonexistent` rather than a spurious failure alongside a genuinely-satisfied dep.
- `rootdepsprovider` still reports `NoVisibleCandidate` here -- a pre-existing, slice-4-unrelated quirk (this atom's resolution path checks tree visibility before an install shortcut for a BDEPEND with no root-deps feed-in); it is not queued as a `New` build target, and the trailing `root_deps_satisfied_atoms` filter drops it once `root_deps_running_root` is given.
- With `--root-deps` the disjunctive closure also accepts a running-root-satisfied branch invisible in the tree; no `rootdepsnonexistent` entry is ever queued (never selected) -- the bug this fix closes.

## fn stale_built_slot_operator_dep_of_an_installed_package_settles_installed
- Backlog #233: walking an installed package's recorded deps (`Deep::Unlimited`, no `--update`); the stale built atom is `dev-libs/reinstslottarget:0/1=` (installed 1.0 at `SLOT="0/1"`, only the `0/2` ebuild in the tree). `resolve_pretend` still reports `NoVisibleCandidate`; Portage `_wrapped_select_pkg_highest_available_imp` (depgraph.py:7799-7840) iterates the installed db for every resolve.
- The live `:=` edge settles the same instance via the ordinary avoid-update path and dedups (`other_outcomes`), so the walk holds exactly two entries and never reports. Before the fix the `:0/1=` edge produced a `NoVisibleCandidate` entry the `@world` walk rendered fatal (rc 1) before the #210 scan engaged. Entry order is walk order: the dep settles before its parent entry is pushed.
