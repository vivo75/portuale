# Task 9 S0c/S0d: run Portage's emerge_main with hooks that print, per pass,
# _slot_conflict_backtrack_abi calls and results, the full backtrack_data of
# _slot_confict_backtrack (every choice with its conflict atoms), the
# backtracker's node add/reject decisions, and which selector put a
# runtime_pkg_mask-masked package into the graph. Usage:
#   PORTAGE_CONFIGROOT=... ROOT=... python3 -I instrument.py <emerge args>
import sys, traceback
import _emerge.depgraph as D
import _emerge.resolver.backtracking as B
from _emerge.main import emerge_main

def out(*a):
    print("T9", *a, file=sys.stderr, flush=True)

def short(p):
    try:
        return f"{p.cpv}({p.type_name})"
    except Exception:
        return str(p)

def pa(parent_atom):
    p, a = parent_atom
    return f"({short(p)}, '{a}')"

PASS = [0]
orig_select_files = D.depgraph.select_files
def select_files(self, args):
    out(f"=== pass {PASS[0]} runtime_pkg_mask=" + str(sorted(
        f"{short(k)}:{sorted(v)}" for k, v in self._dynamic_config._runtime_pkg_mask.items())))
    PASS[0] += 1
    r = orig_select_files(self, args)
    out(f"--- pass end success={r[0]} need_restart={self.need_restart()} "
        f"infos={sorted(self.get_backtrack_infos())}")
    return r
D.depgraph.select_files = select_files

orig_abi = D.depgraph._slot_conflict_backtrack_abi
def abi(self, pkg, slot_nodes, conflict_atoms):
    r = orig_abi(self, pkg, slot_nodes, conflict_atoms)
    out(f"_slot_conflict_backtrack_abi(pkg={short(pkg)}, slot_nodes={[short(x) for x in slot_nodes]}, "
        f"conflict_atoms={{{', '.join(pa(k)+':'+str([short(x) for x in v]) for k, v in conflict_atoms.items())}}}) -> {r}")
    return r
D.depgraph._slot_conflict_backtrack_abi = abi

orig_probe = D.depgraph._slot_operator_update_probe_slot_conflict
def probe(self, dep):
    r = orig_probe(self, dep)
    out(f"  _slot_operator_update_probe_slot_conflict(parent={short(dep.parent)}, atom={dep.atom}, child={short(dep.child)}) -> {r}")
    return r
D.depgraph._slot_operator_update_probe_slot_conflict = probe

orig_scb = D.depgraph._slot_confict_backtrack
def scb(self, root, slot_atom, all_parents, conflict_pkgs):
    out(f"_slot_confict_backtrack(slot={slot_atom}, conflict_pkgs_in={[short(x) for x in conflict_pkgs]}, all_parents={sorted(pa(x) for x in all_parents)})")
    r = orig_scb(self, root, slot_atom, all_parents, conflict_pkgs)
    bd = self._dynamic_config._backtrack_infos["slot conflict"][-1]
    for i, choice in enumerate(bd):
        out(f"  choice[{i}] (DFS pops last first): " + "; ".join(
            f"mask {short(p)} conflict_atoms={sorted(pa(x) for x in atoms)}" for p, atoms in choice))
    return r
D.depgraph._slot_confict_backtrack = scb

orig_psc = D.depgraph._process_slot_conflict
def psc(self, conflict):
    out(f"_process_slot_conflict(slot={conflict.atom}, pkgs={[short(x) for x in conflict.pkgs]})")
    for pkg in conflict.pkgs:
        out(f"  parent_atoms[{short(pkg)}]={sorted(pa(x) for x in self._dynamic_config._parent_atoms.get(pkg, ()))} masked={pkg in self._dynamic_config._runtime_pkg_mask}")
    return orig_psc(self, conflict)
D.depgraph._process_slot_conflict = psc

orig_add = B.Backtracker._add
def add(self, node, explore=True):
    ok = self._check_runtime_pkg_mask(node.parameter.runtime_pkg_mask)
    dup = node in self._nodes
    depth_ok = node.mask_steps <= self._max_depth
    keys = sorted(f"{short(k)}:{sorted(v)}" for k, v in node.parameter.runtime_pkg_mask.items())
    verdict = ("REJECT _check_runtime_pkg_mask (B:123,B:173)" if not ok else
               "REJECT duplicate (B:126)" if dup else
               "REJECT mask_steps>max_depth (B:126)" if not depth_ok else
               ("ADD explore" if explore else "ADD no-explore"))
    out(f"Backtracker._add mask_steps={node.mask_steps} masks={keys} -> {verdict}")
    return orig_add(self, node, explore)
B.Backtracker._add = add

for name in ("_select_pkg_from_graph", "_select_pkg_from_installed"):
    orig = getattr(D.depgraph, name)
    def wrap(self, root, atom, onlydeps=False, parent=None, _o=orig, _n=name):
        r = _o(self, root, atom, onlydeps=onlydeps, parent=parent)
        pkg = r[0]
        if pkg is not None and pkg in self._dynamic_config._runtime_pkg_mask:
            out(f"{_n}(atom={atom}, parent={short(parent) if parent is not None else None}) -> MASKED {short(pkg)} returned (no D:7910 check here)")
        return r
    setattr(D.depgraph, name, wrap)

sys.argv = ["emerge"] + sys.argv[1:]
sys.exit(emerge_main())
