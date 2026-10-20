import json
exec(open('/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-20-b450/analyze_main.py').read().split('# ---------- A:')[0])
src = open('/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-20-b450/analyze_main.py').read()
exec(src.split('# ---------- Argv/expectation per test (pretend + portuale + others)')[1].split('# ---------- B:')[0])
PE_, PO_ = 'test_emerge_pretend_contract.py', 'test_portuale.py'
pe_tree = TREES[PE_]
cases_node = [n for n in pe_tree.body if isinstance(n, ast.Assign) and any(isinstance(t, ast.Name) and t.id == 'CASES' for t in n.targets)][0]
CROWS = []
for i, el in enumerate(cases_node.value.elts):
    a = resolve_list(el.elts[1], {}); rc = el.elts[2].value
    CROWS.append(dict(i=i, line=el.lineno, desc=el.elts[0].value, argv=a, rc=rc))

# expectation literal = non-trivial constant pins in asserts (exclude 0, '', short tokens)
def pins(t):
    out = set()
    for n in ast.walk(t):
        if isinstance(n, ast.Assert):
            for c in ast.walk(n.test):
                if isinstance(c, ast.Constant) and isinstance(c.value, str) and len(c.value) > 3:
                    out.add(c.value)
                if isinstance(c, (ast.List, ast.Tuple)) and c.elts and all(isinstance(e, ast.Constant) for e in c.elts):
                    out.add(ast.unparse(c))
    return frozenset(out)
NODEMAP = {}
for _f in (PE_, PO_):
    for _n in tests_in(TREES[_f]): NODEMAP[(_f, _n.name, _n.lineno)] = _n

B = collections.defaultdict(list)
for t in TESTINFO:
    if t['file'] not in (PE_, PO_) or not t['argvs']: continue
    fn = [n for n in TREES[t['file']].body if False]
    node = NODEMAP[(t['file'], t['name'], t['line'])]
    p = pins(node)
    if p:
        B[(t['argvs'][0], p)].append(t)
B_out = [g for g in B.values() if len(g) > 1]
json.dump(dict(B=[[(x['file'], x['name'], x['line']) for x in g] for g in B_out], B_argv_groups_ge2=sum(1 for k,v in collections.Counter(t['argvs'][0] for t in TESTINFO if t['file'] in (PE_, PO_) and t['argvs']).items() if v>1)), open('/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-20-b450/raw_B.json','w'), indent=1)
print('B groups (argv + pinned literal equal):', len(B_out), 'argv groups>=2:', json.load(open('/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-20-b450/raw_B.json'))['B_argv_groups_ge2'])

# C: weak claims (rc-only / substring-only) vs stronger same-argv tests and CASES rows
def wclaims(node):
    out = []
    for n in ast.walk(node):
        if isinstance(n, ast.Assert):
            k = classify_assert(n.test)
            if k in ('rc0', 'rcN'):
                c = n.test
                out.append(('rc', ('==' if isinstance(c.ops[0], ast.Eq) else '!=', c.comparators[0].value)))
            elif k == 'substr':
                out.append(('substr', n.test.left.value, n.test.comparators[0].attr))
            else:
                out.append(('other', ast.unparse(n.test)[:80]))
    return out
def rc_implies(s_claim, w):
    (sop, sv), (wop, wv) = s_claim, w
    if sop == '==':
        return (wop == '==' and sv == wv) or (wop == '!=' and sv != wv)
    return False
C = []
argv_groups = collections.defaultdict(list)
for t in TESTINFO:
    if t['file'] in (PE_, PO_) and t['argvs']:
        node = NODEMAP[(t['file'], t['name'], t['line'])]
        argv_groups[t['argvs'][0]].append(dict(src=f"{t['file']}::{t['name']}", line=t['line'], claims=wclaims(node), rc_only_weak=all(c[0] in ('rc','substr') for c in wclaims(node)) and bool(wclaims(node))))
for r in CROWS:
    if r['argv'] is not None:
        argv_groups[r['argv']].append(dict(src=f"CASES[{r['i']}] {r['desc'][:50]!r}", line=r['line'], claims=[('rc', ('==', r['rc']))], rc_only_weak=True, synthetic=True))
for argv, ts in argv_groups.items():
    if len(ts) < 2: continue
    for w in ts:
        if not w['rc_only_weak']: continue
        for s in ts:
            if s is w or s.get('synthetic'): continue
            s_rc = [c[1] for c in s['claims'] if c[0] == 'rc']
            w_rc = [c[1] for c in w['claims'] if c[0] == 'rc']
            s_sub = [c for c in s['claims'] if c[0] == 'substr']
            if w_rc and all(any(rc_implies(sc, wc) for sc in s_rc) for wc in w_rc):
                C.append(dict(kind='rc-implied', weak=w['src'], wline=w['line'], weak_claims=[list(c) for c in w['claims']], strong=s['src'], sline=s['line'], strong_rc=[list(x) for x in s_rc], argv=list(argv)))
            elif w_rc and not s_rc:
                pass
            elif w_rc and s_rc:
                pass
    # rc-only W vs pinned-stdout S that does not check rc
    for w in ts:
        if not w['rc_only_weak'] or w.get('synthetic'): continue
        for s in ts:
            if s is w or s.get('synthetic'): continue
            if not any(c[0] == 'rc' for c in s['claims']) and any(c[0] == 'other' for c in s['claims']):
                C.append(dict(kind='weak-rc-not-checked-by-stronger', weak=w['src'], wline=w['line'], strong=s['src'], sline=s['line'], argv=list(argv)))
json.dump(C, open('/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-20-b450/raw_C.json','w'), indent=1)
print('C', collections.Counter(c['kind'] for c in C))
