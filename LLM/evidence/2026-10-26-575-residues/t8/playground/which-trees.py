"""Task 8: which trees does dep_zapdeps see for an installed consumer's `||`?
Wraps depgraph._select_atoms_highest_available and logs, for every call
whose parent is dev-libs/p569virt, whether the depgraph is in complete mode
and whether the trees passed are _graph_trees (post-merge PackageTracker
view, set by _select_atoms_from_graph) or _filtered_trees (composite db).
Run with the probe-t8c.sh cell env (PORTAGE_CONFIGROOT/ROOT/...):
  python3 which-trees.py <emerge args...>
"""
import sys
from _emerge import depgraph as dg

orig = dg.depgraph._select_atoms_highest_available

def wrapped(self, root, depstring, myuse=None, parent=None, strict=True, trees=None, priority=None):
    ret = orig(self, root, depstring, myuse=myuse, parent=parent, strict=strict, trees=trees, priority=priority)
    if parent is not None and getattr(parent, "cp", "") == "dev-libs/p569virt":
        dc = self._dynamic_config
        which = ("_graph_trees" if trees is dc._graph_trees else
                 "_filtered_trees" if trees in (None, dc._filtered_trees) else "other")
        sel = ret.get(parent) if isinstance(ret, dict) else ret
        sys.stderr.write(f"T8PROBE parent={parent.cpv} complete_mode={dc._complete_mode} trees={which} "
                         f"depstring={depstring!s} selected={[str(a) for a in sel]}\n")
    return ret

dg.depgraph._select_atoms_highest_available = wrapped
from _emerge.main import emerge_main
sys.exit(emerge_main(sys.argv[1:]))
