exec(open('/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-20-b450/analyze_core.py').read())
import collections
for f in FILES:
    try: src,t = parse(f)
    except FileNotFoundError: continue
    c = collections.defaultdict(list)
    for n in ast.walk(t):
        if isinstance(n, ast.FunctionDef):
            c[n.name].append(n.lineno)
    d = {k:v for k,v in c.items() if len(v)>1}
    if d: print(f, d)
    # test-name dup ids
    tn = {k:v for k,v in d.items() if k.startswith('test_')}
    if tn: print('  TEST DUP NAMES', f, tn)
