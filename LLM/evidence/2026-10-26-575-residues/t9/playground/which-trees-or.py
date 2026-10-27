"""S0b: adapted t8/playground/which-trees.py (no behaviour change).
Logs every depgraph._select_atoms_highest_available call whose parent is an
INSTALLED package and whose depstring contains a `||` group (optionally only
parents whose cp matches $S0B_PARENT_RE, a regex), with:
  complete_mode, trees (_graph_trees / _filtered_trees / other),
  each top-level `||` arm, its index, whether the vdb (vartree.dbapi) matches it,
  and the selected atoms.
Then applies bt-trace.py's patches (same directory) and runs emerge.
Lines are prefixed "T9OR " on stderr.
"""
import os, re, sys
from _emerge import depgraph as dg
from portage.dep import Atom

PARENT_RE = re.compile(os.environ.get("S0B_PARENT_RE", "."))
orig = dg.depgraph._select_atoms_highest_available

def arms(ds):
    """Yield top-level || groups (as lists) found anywhere in a reduced depstring list."""
    if isinstance(ds, (list, tuple)):
        for i, x in enumerate(ds):
            if x == "||" and i + 1 < len(ds) and isinstance(ds[i + 1], (list, tuple)):
                yield ds[i + 1]
            elif isinstance(x, (list, tuple)):
                yield from arms(x)
        if ds and ds[0] == "||":
            yield ds[1:]

def wrapped(self, root, depstring, myuse=None, parent=None, strict=True, trees=None, priority=None):
    ret = orig(self, root, depstring, myuse=myuse, parent=parent, strict=strict, trees=trees, priority=priority)
    try:
        if parent is not None and getattr(parent, "installed", False) and "||" in str(depstring) \
                and PARENT_RE.search(parent.cp):
            dc = self._dynamic_config
            which = ("_graph_trees" if trees is dc._graph_trees else
                     "_filtered_trees" if trees in (None, dc._filtered_trees) else "other")
            sel = ret.get(parent) if isinstance(ret, dict) else ret
            vardb = self._frozen_config.trees[root]["vartree"].dbapi
            groups = []
            src = depstring
            if isinstance(src, str):
                from portage.dep import use_reduce
                src = use_reduce(src, uselist=self._pkg_use_enabled(parent), token_class=Atom, opconvert=True)
            for g in arms(src):
                desc = []
                for j, a in enumerate(g):
                    if isinstance(a, Atom):
                        desc.append(f"[{j}] {a} vdb={'Y' if vardb.match(a) else 'n'}")
                    else:
                        flat = [str(b) for b in (a if isinstance(a, (list, tuple)) else [a])]
                        desc.append(f"[{j}] ( {' '.join(flat)} ) vdb=" + "".join(
                            'Y' if isinstance(b, Atom) and vardb.match(b) else 'n' for b in (a if isinstance(a, (list, tuple)) else [a])))
                groups.append(" | ".join(desc))
            sys.stderr.write(f"T9OR parent={parent.cpv} installed complete_mode={dc._complete_mode} trees={which} "
                             f"groups={groups} selected={[str(a) for a in sel]}\n")
    except Exception as e:  # never perturb the run
        sys.stderr.write(f"T9OR wrapper-error {e!r}\n")
    return ret

dg.depgraph._select_atoms_highest_available = wrapped
here = os.path.dirname(os.path.abspath(__file__))
code = open(os.path.join(here, "bt-trace.py")).read()
exec(compile(code, os.path.join(here, "bt-trace.py"), "exec"))
