"""S0b: for parents matching $S0B_VPARENT_RE, dump the per-virtual selected atoms that
_select_atoms_highest_available returns (D:6107-6178), with id() of each Atom, to see
whether one Atom object is shared by several virtual nodes. Then run emerge."""
import os, re, sys
from _emerge import depgraph as dg
RX = re.compile(os.environ.get("S0B_VPARENT_RE", "dev-perl/List-MoreUtils-XS"))
orig = dg.depgraph._select_atoms_highest_available
def wrapped(self, root, depstring, myuse=None, parent=None, strict=True, trees=None, priority=None):
    ret = orig(self, root, depstring, myuse=myuse, parent=parent, strict=strict, trees=trees, priority=priority)
    if parent is not None and RX.search(getattr(parent, "cp", "")) and isinstance(ret, dict) and len(ret) > 1:
        dc = self._dynamic_config
        sys.stderr.write(f"VIDS parent={parent.cpv} complete_mode={dc._complete_mode}\n")
        for k, atoms in ret.items():
            name = k.cpv if k is parent else f"virt {k.child.cpv} (via {k.atom})"
            sys.stderr.write(f"VIDS   {name}: " + ", ".join(f"{a}@{id(a):x}" for a in atoms) + "\n")
    return ret
dg.depgraph._select_atoms_highest_available = wrapped
from _emerge.main import emerge_main
sys.exit(emerge_main(sys.argv[1:]))
