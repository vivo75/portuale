"""Run the upstream SlotCollisionTestCase shapes through Portage's own
ResolverPlayground and dump what the slot_conflict_handler computed.
Usage: python3 -I sc_upstream.py [case-index ...]"""
import os, sys, ast
P = "/home/vivo/repo/PORTUALE/portuale/3rdparty/portage"
sys.path.insert(0, P + "/lib")
from portage.tests.resolver.ResolverPlayground import ResolverPlayground
import _emerge.depgraph as D

# Pull ebuilds/installed and the cases verbatim out of the upstream test file.
src = open(P + "/lib/portage/tests/resolver/test_slot_collisions.py").read()
tree = ast.parse(src)
fn = [n for n in ast.walk(tree) if isinstance(n, ast.FunctionDef) and n.name == "testSlotCollision"][0]
env = {}
for st in fn.body:
    if isinstance(st, ast.Assign) and st.targets[0].id in ("ebuilds", "installed"):
        env[st.targets[0].id] = ast.literal_eval(st.value)
    if isinstance(st, ast.Assign) and st.targets[0].id == "test_cases":
        cases = []
        for call in st.value.elts:
            kw = {k.arg: ast.literal_eval(k.value) for k in call.keywords}
            cases.append((ast.literal_eval(call.args[0]), kw))

bt = {}
orig = D._backtrack_depgraph
def wrap(*a, **k):
    r = orig(*a, **k)
    bt["backtracked"], bt["max"] = r[3], r[4]
    return r
D._backtrack_depgraph = wrap
ncc_calls = []
orig_ncc = D.depgraph.need_config_change
def ncc(self):
    pre = self._dynamic_config._slot_conflict_handler is None
    r = orig_ncc(self)
    ncc_calls.append((pre, self._dynamic_config._slot_conflict_handler is not None, r, self._dynamic_config._allow_backtracking))
    return r
D.depgraph.need_config_change = ncc

sel = [int(x) for x in sys.argv[1:]] or range(len(cases))
pg = ResolverPlayground(ebuilds=env["ebuilds"], installed=env["installed"])
try:
    for i in sel:
        atoms, kw = cases[i]
        perms = [atoms]
        if kw.get("all_permutations"):
            import itertools; perms = list(itertools.permutations(atoms))
        for req in perms:
            bt.clear(); ncc_calls.clear()
            opts = dict(kw.get("options", {}))
            res = pg.run(list(req), options=opts)
            d = res.depgraph
            h = d._dynamic_config._slot_conflict_handler
            print(f"### case {i} request={list(req)} options={opts}")
            print("  expected slot_collision_solutions:", kw.get("slot_collision_solutions"))
            print("  success:", res.success, "backtracked:", bt.get("backtracked"), "need_config_change calls (handler_was_none, handler_now, ret, allow_bt):", ncc_calls)
            if h is None:
                print("  handler: None (not built)"); continue
            print("  all_conflicts:", [(str(a), [str(p) for p in pk]) for r, a, pk in h.all_conflicts])
            print("  is_a_version_conflict:", h.is_a_version_conflict, "conflict_is_unspecific:", h.conflict_is_unspecific)
            print("  solutions:", [{str(p): v for p, v in s.items()} for s in h.solutions])
            print("  changes:", [{p.cpv: v for p, v in c.items()} for c in h.changes])
            print("  explanation repr:", repr(h.get_explanation()))
finally:
    pg.cleanup()
