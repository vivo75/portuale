# Task 9 S0d COUNTERFACTUAL (not Portage's behaviour): make Portage's
# _complete_graph treat a runtime_pkg_mask-masked installed package as absent
# at the D:8803-8835 re-add (the dep goes to _initially_unsatisfied_deps, as
# D:8807-8809 does when vardb has no match). Shows what the run would be if
# the masked installed instance were hidden, as portuale's
# MaskChoice.installed does.
import sys, traceback
import _emerge.depgraph as D
from _emerge.main import emerge_main
o_add = D.depgraph._add_pkg
def add_pkg(self, pkg, dep):
    if (pkg.installed and self._dynamic_config._complete_mode
            and pkg in self._dynamic_config._runtime_pkg_mask
            and any(f.name == "_complete_graph" and f.lineno == 8834
                    for f in traceback.extract_stack()[:-1])):
        print(f"T9 CF: hide masked {pkg.cpv}(installed) for {dep.atom}", file=sys.stderr)
        self._dynamic_config._initially_unsatisfied_deps.append(dep)
        return 1
    return o_add(self, pkg, dep)
D.depgraph._add_pkg = add_pkg
sys.argv = ["emerge"] + sys.argv[1:]
sys.exit(emerge_main())
