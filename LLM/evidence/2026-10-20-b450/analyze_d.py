import json
exec(open('/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-20-b450/analyze_main.py').read().split('# ---------- A:')[0])
src = open('/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-20-b450/analyze_main.py').read()
exec(src.split('# ---------- Argv/expectation per test (pretend + portuale + others)')[1].split('# ---------- B:')[0])
WEAK = {'rc0', 'rcN', 'substr', 'empty', 'notnone', 'truthy'}
def runner_vars(fn):
    names = set()
    for n in ast.walk(fn):
        if isinstance(n, ast.Assign) and any(isinstance(x, ast.Call) and is_runner(call_name(x)) for x in ast.walk(n.value)):
            for t in n.targets:
                for x in ast.walk(t):
                    if isinstance(x, ast.Name): names.add(x.id)
    return names
def mentions(node, names):
    return any(isinstance(x, ast.Name) and x.id in names for x in ast.walk(node))
def toplevel_literal(node):
    return isinstance(node, (ast.List, ast.Tuple, ast.Constant, ast.JoinedStr, ast.Dict, ast.Set))
RES = {'weak_only': [], 'no_assert': [], 'cmp_two_runs_no_rc': [], 'two_runs_no_rc_any': []}
NODE_OF = {}
for f in FILES:
    if f not in TREES: continue
    for fn in tests_in(TREES[f]):
        NODE_OF[(f, fn.name, fn.lineno)] = fn
for t in TESTINFO:
    fn = NODE_OF[(t['file'], t['name'], t['line'])]
    rv = runner_vars(fn)
    asserts = [n.test for n in ast.walk(fn) if isinstance(n, ast.Assert)]
    kinds = [k[0] for k in t['kinds']]
    loc = f"{t['file']}:{t['line']} {t['name']}"
    if not kinds:
        RES['no_assert'].append(loc)
    elif all(k in WEAK or k == 'corpus' for k in kinds):
        RES['weak_only'].append((loc, sorted(set(kinds))))
    rc_any = any(mentions_rc(a) for a in asserts)
    cmp2 = False
    for a in asserts:
        for c in ast.walk(a):
            if isinstance(c, ast.Compare) and len(c.ops) == 1 and isinstance(c.ops[0], ast.Eq):
                l, r = c.left, c.comparators[0]
                if mentions(l, rv) and mentions(r, rv) and not toplevel_literal(l) and not toplevel_literal(r):
                    cmp2 = True
    if len(rv) and cmp2 and not rc_any:
        RES['cmp_two_runs_no_rc'].append(loc)
    if t['runs'] >= 2 and not rc_any:
        RES['two_runs_no_rc_any'].append(loc)
json.dump(RES, open('/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-20-b450/raw_D.json', 'w'), indent=1)
from collections import Counter
print({k: len(v) for k, v in RES.items()})
print(Counter(x[0].split(':')[0] for x in RES['weak_only']))
print(Counter(x.split(':')[0] for x in RES['cmp_two_runs_no_rc']))
print(Counter(x.split(':')[0] for x in RES['two_runs_no_rc_any']))
print(Counter(x.split(':')[0] for x in RES['no_assert']))
for x in RES['cmp_two_runs_no_rc'][:40]: print(' ', x)
