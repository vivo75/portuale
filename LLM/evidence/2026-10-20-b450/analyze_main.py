# Appended to analyze_core via exec (python3 -I; no sys.path tricks).
import lzma, os
CORE = open('/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-20-b450/analyze_core.py').read()
exec(compile(CORE, 'analyze_core.py', 'exec'))
OUT = {}
TREES = {}
SRCS = {}
for f in FILES:
    if f in TREES: continue
    try:
        SRCS[f], TREES[f] = parse(f)
    except FileNotFoundError:
        pass

def loc(f, fn): return f'{f}:{fn.lineno}'
def where(f, fn): return f'{f}::{fn.name}'

# ---------- A: exact duplicates (code identical incl. literals, name & docstring ignored)
A = {}
for f in ['test_emerge_pretend_contract.py', 'test_portuale.py', 'test_metamorphic.py', 'test_output_invariants.py', 'test_atom_contract.py', 'test_required_use_contract.py', 'test_use_reduce_contract.py', 'test_versions_contract.py']:
    groups = collections.defaultdict(list)
    for fn in tests_in(TREES[f]):
        groups[code_sig(fn)].append(fn)
    for k, fns in groups.items():
        if len(fns) > 1:
            A.setdefault(f, []).append([(where(f, x), loc(f, x), docstring(x)[0] or '') for x in fns])
OUT['A'] = A

# ---------- Argv/expectation per test (pretend + portuale + others)
TESTINFO = []
for f in FILES:
    if f not in TREES: continue
    tree = TREES[f]; menv = module_env(tree)
    for fn in tests_in(tree):
        doc, body = docstring(fn)
        asserting_helpers = set()
        for n in tree.body:
            if isinstance(n, ast.FunctionDef) and not n.name.startswith('test_') and any(isinstance(x, ast.Assert) for x in ast.walk(n)):
                asserting_helpers.add(n.name)
        kinds = []
        consts = set()
        for n in ast.walk(fn):
            if isinstance(n, ast.Assert):
                kinds.append((classify_assert(n.test), n.lineno, ast.unparse(n.test)[:120]))
                for c in ast.walk(n.test):
                    if isinstance(c, ast.Constant) and isinstance(c.value, (str, int)) and not isinstance(c.value, bool):
                        consts.add(c.value)
            elif isinstance(n, ast.With) and any('raises' in ast.unparse(i.context_expr) for i in n.items):
                kinds.append(('raises', n.lineno, 'pytest.raises'))
            elif isinstance(n, ast.Call):
                cn = call_name(n)
                if cn == '_assert_harvested':
                    kinds.append(('corpus', n.lineno, '_assert_harvested(...)'))
                elif cn in asserting_helpers or cn.startswith('_assert') or cn.startswith('assert_'):
                    kinds.append(('helper', n.lineno, cn))
                    for a in n.args:
                        if isinstance(a, ast.Constant) and isinstance(a.value, (str, int)):
                            consts.add(a.value)
        runs = sum(1 for n in ast.walk(fn) if isinstance(n, ast.Call) and is_runner(call_name(n)))
        cmp_outputs = any(isinstance(n, ast.Compare) and len(n.ops) == 1 and isinstance(n.ops[0], ast.Eq)
                          and not any(isinstance(s, ast.Constant) for s in [n.left, n.comparators[0]])
                          and not mentions_rc(n) for n in ast.walk(fn) if isinstance(n, ast.Assert) or isinstance(n, ast.Compare))
        rc_check = any(k[0] in ('rc0', 'rcN') for k in kinds)
        TESTINFO.append(dict(file=f, name=fn.name, line=fn.lineno, end=fn.end_lineno, doc=doc is not None,
                             doclen=len(doc or ''), argvs=argv_of(fn, menv), kinds=kinds, consts=consts,
                             runs=runs, cmp_outputs=cmp_outputs, rc_check=rc_check,
                             decor=[ast.unparse(d) for d in fn.decorator_list]))

# ---------- B: same argv literal and same expectation literal set
B = collections.defaultdict(list)
for t in TESTINFO:
    if not t['argvs'] or t['file'] not in ('test_emerge_pretend_contract.py', 'test_portuale.py'): continue
    key = (t['argvs'][0], tuple(sorted(t['consts'], key=str)))
    B[key].append(t)
B_groups = [v for v in B.values() if len(v) > 1]
OUT['B'] = [[(t['file'], t['name'], t['line'], [k[0] for k in t['kinds']]) for t in g] for g in B_groups]
OUT['B_argv_only_groups'] = sum(1 for v in collections.Counter((t['argvs'][0]) for t in TESTINFO if t['argvs'] and t['file'] in ('test_emerge_pretend_contract.py','test_portuale.py')).items() if v[1] > 1)

# ---------- C: rc-only / substring-only tests whose argv is also pinned by a stronger test
def rc_pair(a_op, a_val, b_op, b_val):
    """Does rc assertion (b) imply (a)? b, a are (op, int) with op in ==, !=."""
    if b_op == '==':
        if a_op == '==': return b_val == a_val
        if a_op == '!=': return b_val != a_val
    return False

def rc_of(node):
    t = node
    if isinstance(t, ast.Compare):
        op = 'and' if False else t.ops[0]
        return ('==' if isinstance(op, ast.Eq) else '!=', t.comparators[0].value)
    return None

def weak_asserts(fn_tree_node):
    out = []
    for n in ast.walk(fn_tree_node):
        if isinstance(n, ast.Assert):
            k = classify_assert(n.test)
            if k in ('rc0', 'rcN'):
                out.append(('rc', rc_of(n.test)))
            elif k == 'substr':
                out.append(('substr', n.test.left.value, n.test.comparators[0].attr))
            else:
                out.append(('other', None))
    return out

# re-parse each TESTINFO test node for assert-level data
NODE = {}
for f in ('test_emerge_pretend_contract.py', 'test_portuale.py'):
    for fn in tests_in(TREES[f]): NODE[(f, fn.name)] = fn
C_pairs = []
by_argv = collections.defaultdict(list)
for t in TESTINFO:
    if t['file'] in ('test_emerge_pretend_contract.py', 'test_portuale.py') and t['argvs']:
        by_argv[t['argvs'][0]].append(t)
CASES_ROWS = []  # filled in E
for argv, ts in by_argv.items():
    if len(ts) < 2: continue
    # weak candidates: all asserts rc (==/!=) or substring
    strongs = [t for t in ts if any(k[0] in ('other', 'strong', 'helper', 'corpus') for k in t['kinds'])]
    weak = [t for t in ts if t['kinds'] and all(k[0] in ('rc0', 'rcN', 'substr') for k in t['kinds'])]
    for w in weak:
        for s in ts:
            if s is w: continue
            wa = weak_asserts(NODE[(w['file'], w['name'])])
            sa = weak_asserts(NODE[(s['file'], s['name'])])
            ok = True
            for wk in wa:
                if wk[0] == 'rc':
                    if not any(sk[0] == 'rc' and rc_pair(wk[1][0], wk[1][1], sk[1][0], sk[1][1]) for sk in sa if sk[1]):
                        ok = False
                elif wk[0] == 'substr':
                    # implied when stronger test pins the same stream to a literal containing the substring
                    cand = False
                    for n in ast.walk(NODE[(s['file'], s['name'])]):
                        if isinstance(n, ast.Compare) and len(n.ops) == 1 and isinstance(n.ops[0], ast.Eq) \
                                and is_rc is not None and isinstance(n.left, ast.Attribute) and n.left.attr == wk[2] \
                                and const_str(n.comparators[0]) and wk[1] in n.comparators[0].value:
                            cand = True
                    if not cand: ok = False
                else:
                    ok = False
            if ok and wa:
                C_pairs.append((w['file'], w['name'], w['line'], s['file'], s['name'], s['line'], argv, [k[2] for k in w['kinds']], [k[2] for k in s['kinds']][:3]))
OUT['C'] = C_pairs

# ---------- D: weak / vacuous tests (all test files)
WEAK = {'rc0', 'rcN', 'substr', 'empty', 'notnone', 'truthy'}
D = collections.defaultdict(list)
for t in TESTINFO:
    loc_s = f"{t['file']}:{t['line']} {t['name']}"
    ks = [k[0] for k in t['kinds']]
    if not ks:
        D['no assertion (no assert, pytest.raises or asserting helper)'].append(loc_s)
        continue
    if all(k in WEAK or k == 'corpus' for k in ks):
        kinds = sorted(set(ks))
        D[f'only weak assertions: {",".join(kinds)}'].append(loc_s)
    if t['runs'] >= 2 and t['cmp_outputs'] and not t['rc_check']:
        D['compares two runs with no returncode check'].append(loc_s)
    if t['runs'] >= 2 and not t['rc_check']:
        D['two+ runs, no returncode assertion at all'].append(loc_s)
OUT['D'] = {k: v for k, v in D.items()}

# ---------- E: CASES table
pe_tree = TREES['test_emerge_pretend_contract.py']
cases_node = [n for n in pe_tree.body if isinstance(n, ast.Assign) and any(isinstance(t, ast.Name) and t.id == 'CASES' for t in n.targets)][0]
rows = []
for i, el in enumerate(cases_node.value.elts):
    if isinstance(el, ast.Tuple) and len(el.elts) == 3:
        d = el.elts[0].value if const_str(el.elts[0]) else ast.unparse(el.elts[0])
        a = resolve_list(el.elts[1], {})
        rc = el.elts[2].value if isinstance(el.elts[2], ast.Constant) else ast.unparse(el.elts[2])
        rows.append(dict(i=i, line=el.lineno, desc=d, args=a, rc=rc))
    else:
        rows.append(dict(i=i, line=el.lineno, desc=None, args=None, rc=None, odd=ast.unparse(el)[:80]))
OUT['E_rows'] = len(rows)
OUT['E_odd'] = [r for r in rows if r.get('odd')]
def canon(args):
    m = {'-p': '--pretend', '-v': '--verbose', '-u': '--update', '-D': '--deep', '-N': '--newuse', '-e': '--emptytree', '-q': '--quiet', '-n': '--noreplace', '-t': '--tree', '-1': '--oneshot'}
    return tuple(sorted(m.get(x, x) for x in args))
byargs = collections.defaultdict(list); bydesc = collections.defaultdict(list); bycanon = collections.defaultdict(list); byrow = collections.defaultdict(list)
for r in rows:
    if r['args'] is None: continue
    byargs[r['args']].append(r); bydesc[r['desc']].append(r); bycanon[canon(r['args'])].append(r); byrow[(r['desc'], r['args'], r['rc'])].append(r)
OUT['E_same_args'] = [[(r['i'], r['line'], r['desc'], r['rc']) for r in g] for g in byargs.values() if len(g) > 1]
OUT['E_flag_order_or_pretend_spelling'] = [[(r['i'], r['line'], r['args'], r['rc']) for r in g] for g in bycanon.values() if len({r['args'] for r in g}) > 1]
OUT['E_same_desc'] = [[(r['i'], r['line'], r['args'], r['rc']) for r in g] for g in bydesc.values() if len(g) > 1]
OUT['E_exact_rows'] = [[(r['i'], r['line']) for r in g] for g in byrow.values() if len(g) > 1]
# consumers
meta = TREES['test_metamorphic.py']
sub_desc = []
for n in meta.body:
    if isinstance(n, ast.Assign) and isinstance(n.targets[0], ast.Name) and n.targets[0].id == 'SUBSET_DESCRIPTIONS':
        sub_desc = [e.value for e in n.value.elts if const_str(e)]
OUT['metamorphic_SUBSET_DESCRIPTIONS'] = [(d, d in bydesc) for d in sub_desc]
OUT['metamorphic_SUBSET_DESCRIPTIONS_present_in_CASES'] = sum(1 for d in sub_desc if d in bydesc)

# ---------- F: corpus keys vs pytest ids and test-function existence
def load_corpus(name):
    d = json.loads(lzma.open(f'{PS}/corpus/{name}.json.xz', 'rt', encoding='utf-8').read())
    return list(d['cases'])
ck = load_corpus('contract')
def fn_of(key):
    nid = key.rsplit('#', 1)[0]
    nid = nid.split('/', 1)[1] if '/' in nid else nid
    return nid
existing = {f: {fn.name for fn in tests_in(TREES[f])} for f in TREES}
F = collections.Counter()
F_missing = collections.Counter()
F_ids_ok = 0; F_ids_stale = []
pe_case_ids = {f"{r['desc']}-args{r['i']}-{r['rc']}": r for r in rows if r['desc'] is not None}
for k in ck:
    nid = fn_of(k)
    fname = nid.split('::')[0]
    base = nid.split('::')[1].split('[')[0] if '::' in nid else nid
    F[(fname, base)] += 1
    if fname in existing and base not in existing[fname]:
        F_missing[(fname, base)] += 1
    if base == 'test_pretend_case_exit_code':
        pid = nid.split('[', 1)[1][:-1]
        if pid in pe_case_ids: F_ids_ok += 1
        else: F_ids_stale.append(nid)
OUT['F_per_test'] = sorted(((k[0], k[1], v) for k, v in F.items()), key=lambda x: -x[2])[:0]
OUT['F_summary'] = dict(total=len(ck), nodeid_fn_pairs=len(F), pid_match_current_rows=F_ids_ok, pid_stale=len(F_ids_stale),
                        orphan_functions={f'{k[0]}::{k[1]}': v for k, v in F_missing.items()})
OUT['F_stale_examples'] = F_ids_stale[:5]
ek = load_corpus('expanded')
OUT['F_expanded'] = dict(total=len(ek), key_is_nodeid=False)

# ---------- counts
OUT['counts'] = {f: len(tests_in(TREES[f])) for f in TREES}
OUT['asserting_no'] = sum(1 for t in TESTINFO if not t['kinds'])

json.dump(OUT, open('/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-20-b450/raw.json', 'w'), indent=1, default=str, ensure_ascii=False)
print('written')
