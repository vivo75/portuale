# Run host emerge with depgraph._add_dep wrapped: print which arm of the
# unsatisfied-dep path (depgraph.py:3410-3535) each failing dep took.
import sys, runpy
import _emerge.depgraph as D
orig = D.depgraph._add_dep
def wrapped(self, dep, allow_unsatisfied):
    dc = self._dynamic_config
    n_uns, n_init = len(dc._unsatisfied_deps), len(dc._initially_unsatisfied_deps)
    skip0, info0 = dc._skip_restart, dc._backtrack_infos.get("missing dependency")
    r = orig(self, dep, allow_unsatisfied)
    disp = dc._unsatisfied_deps_for_display and dc._unsatisfied_deps_for_display[-1][0] == (dep.root, dep.atom)
    arm = None
    if r == 0:
        if dc._backtrack_infos.get("missing dependency") is dep and info0 is not dep:
            arm = "D:3517 missing dependency filed"
        elif dc._skip_restart and not skip0:
            arm = "D:3514 bug-693836 skip_restart"
        elif not dc._allow_backtracking:
            arm = "D:3535 no backtracking (allow_backtracking=False)"
        else:
            arm = "D:3494-3499 reselect without_use found a package (USE-only), nothing filed"
    elif len(dc._unsatisfied_deps) > n_uns:
        arm = "D:3419 allow_unsatisfied"
    elif len(dc._initially_unsatisfied_deps) > n_init:
        arm = "D:3451/3485 initially_unsatisfied (installed parent)"
    if arm:
        sys.stderr.write(f"ARM {arm} | parent={dep.parent} atom={dep.atom} bt={dc._allow_backtracking}\n")
    return r
D.depgraph._add_dep = wrapped
sys.argv = ["emerge"] + sys.argv[1:]
runpy.run_path("/usr/bin/emerge", run_name="__main__")
