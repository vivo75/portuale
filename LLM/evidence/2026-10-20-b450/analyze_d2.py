import json
exec(open('/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-20-b450/analyze_d.py').read().split("RES = {")[0])
out = []
for f in FILES:
    if f not in TREES: continue
    for fn in tests_in(TREES[f]):
        rv = runner_vars(fn)
        asserts = [n.test for n in ast.walk(fn) if isinstance(n, ast.Assert)]
        if not rv or not asserts: continue
        cmp_pairs = []
        for a in asserts:
            for c in ast.walk(a):
                if isinstance(c, ast.Compare) and len(c.ops) == 1 and isinstance(c.ops[0], ast.Eq):
                    l, r = c.left, c.comparators[0]
                    if mentions(l, rv) and mentions(r, rv) and not toplevel_literal(l) and not toplevel_literal(r):
                        cmp_pairs.append(ast.unparse(c)[:90])
        if not cmp_pairs: continue
        rc_any = any(mentions_rc(a) for a in asserts)
        # is every run variable pinned by some assert that also carries a literal?
        pinned = set()
        for a in asserts:
            has_lit = any(isinstance(x, ast.Constant) and isinstance(x.value, str) and len(x.value) > 2 for x in ast.walk(a)) or any(isinstance(x, ast.List) for x in ast.walk(a))
            if has_lit:
                for x in ast.walk(a):
                    if isinstance(x, ast.Name) and x.id in rv: pinned.add(x.id)
        out.append(dict(loc=f"{f}:{fn.lineno} {fn.name}", rc_check=rc_any, unpinned_runs=sorted(rv - pinned), cmp=cmp_pairs[:2]))
json.dump(out, open('/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-20-b450/raw_D2.json', 'w'), indent=1)
nr = [o for o in out if not o['rc_check']]
print('two-run comparison tests', len(out), 'without rc check', len(nr))
print('of those, with a run var never pinned by a literal assert:', sum(1 for o in nr if o['unpinned_runs']))
for o in nr:
    if o['unpinned_runs']: print(' ', o['loc'], o['unpinned_runs'], o['cmp'][0][:70])
