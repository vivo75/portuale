"""S0b: compact backtracking trace of Portage's own resolver (no behaviour change).
Wraps (read-only, call-through):
  depgraph._slot_confict_backtrack  -> full backtrack_data (every choice, in order)
  depgraph._slot_conflict_backtrack_abi -> its return value per conflict pkg
  Backtracker.get / _add / feedback -> per-try runtime_pkg_mask, node accepted/rejected
  depgraph.need_config_change      -> its value, and whether the slot conflict handler had changes
Usage (cell env as probe-s0b-cgp.sh): python3 bt-trace.py <emerge args...>
Output lines are prefixed "S0B " on stderr.
"""
import sys
from _emerge import depgraph as dg
from _emerge.resolver import backtracking as bt

def P(pkg):
    try:
        return f"{pkg.cpv}({pkg.type_name}{',' + pkg.operation if pkg.operation != 'merge' else ''})"
    except Exception:
        return str(pkg)
def PA(pa):
    p, a = pa
    return f"{P(p) if hasattr(p, 'cpv') else str(p)}->'{a}'"
def W(s):
    sys.stderr.write("S0B " + s + "\n")

o_scb = dg.depgraph._slot_confict_backtrack
def scb(self, root, slot_atom, all_parents, conflict_pkgs):
    r = o_scb(self, root, slot_atom, all_parents, conflict_pkgs)
    data = self._dynamic_config._backtrack_infos["slot conflict"][-1]
    W(f"_slot_confict_backtrack slot={slot_atom} all_parents=[{', '.join(sorted(PA(x) for x in all_parents))}]")
    for i, choice in enumerate(data):
        W(f"  choice[{i}]{' (explored first)' if i == len(data)-1 else ''}: " + "; ".join(
            f"mask {P(p)} conflict_atoms=[{', '.join(sorted(PA(x) for x in ca))}]" for p, ca in choice))
    return r
dg.depgraph._slot_confict_backtrack = scb

o_abi = dg.depgraph._slot_conflict_backtrack_abi
def abi(self, pkg, slot_nodes, conflict_atoms):
    r = o_abi(self, pkg, slot_nodes, conflict_atoms)
    W(f"_slot_conflict_backtrack_abi pkg={P(pkg)} -> {r}")
    return r
dg.depgraph._slot_conflict_backtrack_abi = abi

o_ncc = dg.depgraph.need_config_change
def ncc(self):
    r = o_ncc(self)
    h = self._dynamic_config._slot_conflict_handler
    W(f"need_config_change -> {r} (slot_conflict_handler changes={getattr(h, 'changes', None) if h else None})")
    return r
dg.depgraph.need_config_change = ncc

def M(mask):
    return "{" + "; ".join(f"{P(p)}: " + ", ".join(
        f"{k}[{', '.join(sorted(PA(x) if len(x) == 2 else f'{P(x[0])}->{x[2]}' for x in v))}]" if isinstance(v, set) else k
        for k, v in info.items()) for p, info in mask.items()) + "}"

o_get = bt.Backtracker.get
def get(self):
    r = o_get(self)
    if r is not None:
        W(f"Backtracker.get -> runtime_pkg_mask={M(r.runtime_pkg_mask)} (unexplored left {len(self._unexplored_nodes)})")
    return r
bt.Backtracker.get = get

o_add = bt.Backtracker._add
def add(self, node, explore=True):
    n = len(self._nodes)
    o_add(self, node, explore=explore)
    W(f"Backtracker._add mask_steps={node.mask_steps} {'ACCEPTED' if len(self._nodes) > n else 'REJECTED'} mask={M(node.parameter.runtime_pkg_mask)}")
bt.Backtracker._add = add

o_fb = bt.Backtracker.feedback
def fb(self, infos):
    W(f"Backtracker.feedback keys={sorted(infos)}" + (f" missing dep parent={P(infos['missing dependency'].parent)} atom={infos['missing dependency'].atom}" if 'missing dependency' in infos else ""))
    return o_fb(self, infos)
bt.Backtracker.feedback = fb

o_best = bt.Backtracker.get_best_run
def best(self):
    r = o_best(self)
    W(f"get_best_run -> runtime_pkg_mask={M(r.runtime_pkg_mask)}")
    return r
bt.Backtracker.get_best_run = best

from _emerge.main import emerge_main
sys.exit(emerge_main(sys.argv[1:]))
