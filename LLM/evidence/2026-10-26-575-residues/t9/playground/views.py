"""Task 9 S0a: which view (normal walk vs complete mode) evaluates an
installed consumer's `||`, and why.
Wraps three depgraph methods and logs to stderr, tagged T9:
  ADD    every _add_pkg of an *installed* package whose cp matches
         $T9_CP (regex): depth, myparams deep, requested_depth, and where
         the Package went (dep_stack = normal walk, ignored = deferred to
         _complete_graph, none = previously added).
  OR     every _select_atoms_highest_available call whose parent matches
         $T9_CP and whose depstring holds a `||`: complete_mode, trees,
         parent depth, selected atoms.
  CG     _complete_graph entry/exit (whether complete mode was entered).
pass=<n> counts depgraph instances (one per backtrack try).
Usage (cell env as probe-t8c.sh):  T9_CP='p569virt' python3 views.py <emerge args>
"""
import os, re, sys
from _emerge import depgraph as dg

CP = re.compile(os.environ.get("T9_CP", "."))
PASS = {}

def pid(self):
    # tag the _dynamic_config itself: id() values are reused across backtrack passes
    dc = self._dynamic_config
    if not hasattr(dc, "_t9_pass"):
        dc._t9_pass = len(PASS)
        PASS[dc._t9_pass] = True
    return dc._t9_pass

def has_or(x):
    if isinstance(x, list):
        return any(has_or(y) for y in x) or (bool(x) and x[0] == "||")
    return isinstance(x, str) and "||" in x

orig_sel = dg.depgraph._select_atoms_highest_available
def sel(self, root, depstring, myuse=None, parent=None, strict=True, trees=None, priority=None):
    ret = orig_sel(self, root, depstring, myuse=myuse, parent=parent, strict=strict, trees=trees, priority=priority)
    if parent is not None and getattr(parent, "installed", False) and CP.search(getattr(parent, "cp", "")) and has_or(depstring):
        dc = self._dynamic_config
        which = ("_graph_trees" if trees is dc._graph_trees else
                 "_filtered_trees" if trees in (None, dc._filtered_trees) else "other")
        s = ret.get(parent) if isinstance(ret, dict) else ret
        f = sys._getframe(1)
        while f and f.f_code.co_name in ("_select_atoms_from_graph",):
            f = f.f_back
        chain = []
        g = f
        while g and len(chain) < 12:
            n = g.f_code.co_name
            if n.startswith("_") and n not in ("_add_pkg_dep_string", "_wrapped_add_pkg_dep_string", "_select_atoms_from_graph", "add"):
                chain.append(n)
            g = g.f_back
        caller = (f.f_code.co_name if f else "?") + "<" + "<".join(chain[:4])
        sys.stderr.write(f"\nT9 OR pass={pid(self)} parent={parent.cpv} depth={parent.depth!r} "
                         f"complete_mode={dc._complete_mode} trees={which} caller={caller} priority={priority} "
                         f"depstring={depstring!s} selected={[str(a) for a in s]}\n")
    return ret
dg.depgraph._select_atoms_highest_available = sel

orig_add = dg.depgraph._add_pkg
def add(self, pkg, dep):
    dc = self._dynamic_config
    prev = pkg in dc.digraph
    ret = orig_add(self, pkg, dep)
    if pkg.installed and CP.search(pkg.cp):
        where = ("none(previously_added)" if prev else
                 "dep_stack" if any(x is pkg for x in dc._dep_stack) else
                 "ignored" if any(x is pkg for x in dc._ignored_deps) else "none")
        d = "UNREACHABLE" if pkg.depth is self._UNREACHABLE_DEPTH else pkg.depth
        sys.stderr.write(f"\nT9 ADD pass={pid(self)} pkg={pkg.cpv} depth={d} deep={dc.myparams.get('deep', 0)!r} "
                         f"requested_depth={self._frozen_config.requested_depth!r} complete_mode={dc._complete_mode} "
                         f"parent={getattr(dep, 'parent', None)} -> {where}\n")
    return ret
dg.depgraph._add_pkg = add

orig_cg = dg.depgraph._complete_graph
def cg(self, required_sets=None):
    ret = orig_cg(self, required_sets=required_sets)
    sys.stderr.write(f"\nT9 CG pass={pid(self)} complete_mode={self._dynamic_config._complete_mode} "
                     f"complete_param={'complete' in self._dynamic_config.myparams}\n")
    return ret
dg.depgraph._complete_graph = cg

from _emerge.main import emerge_main
sys.exit(emerge_main(sys.argv[1:]))
