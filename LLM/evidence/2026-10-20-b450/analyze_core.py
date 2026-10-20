"""B450 analysis core: AST-based survey of the pmtest contract suite (read-only).

Run with: python3 -I analyze_core.py   (reads pmtest files; writes JSON to stdout only)
"""
import ast, collections, json, re, sys

PS = '/home/vivo/repo/PORTUALE/pmtest/pytests-contract-suite'
FILES = ['test_emerge_pretend_contract.py', 'test_portuale.py', 'test_metamorphic.py',
         'test_output_invariants.py', 'test_atom_contract.py', 'test_required_use_contract.py',
         'test_use_reduce_contract.py', 'test_versions_contract.py', 'test_cached_harness.py',
         'test_registry.py', 'test_primitive_regressions.py', 'test_fixture_caches.py',
         'test_translator_floor.py', 'test_musl_smoke.py', 'test_benchmark_gate.py',
         'test_harvested_corpus.py', 'test_required_use_contract.py']


def parse(name):
    src = open(f'{PS}/{name}', encoding='utf-8').read()
    return src, ast.parse(src, filename=name)


def docstring(fn):
    b = fn.body
    if b and isinstance(b[0], ast.Expr) and isinstance(b[0].value, ast.Constant) \
            and isinstance(b[0].value.value, str):
        return b[0].value.value, b[1:]
    return None, b


def code_sig(fn):
    """Everything except the function name and docstring, literals included."""
    _, body = docstring(fn)
    parts = [ast.dump(fn.args)] + [ast.dump(d) for d in fn.decorator_list] + [ast.dump(s) for s in body]
    return '\n'.join(parts)


def tests_in(tree):
    return sorted([n for n in ast.walk(tree) if isinstance(n, ast.FunctionDef) and n.name.startswith('test_')],
                  key=lambda n: n.lineno)


def const_str(n):
    return isinstance(n, ast.Constant) and isinstance(n.value, str)


def is_rc(node):
    return isinstance(node, ast.Attribute) and node.attr == 'returncode'


def mentions_rc(node):
    return any(is_rc(x) for x in ast.walk(node))


def resolve_list(node, env):
    """Tuple of str for a list/tuple/concat of constants and known names; None if not literal."""
    if isinstance(node, (ast.List, ast.Tuple)):
        out = []
        for e in node.elts:
            if const_str(e):
                out.append(e.value)
            else:
                return None
        return tuple(out)
    if isinstance(node, ast.Name):
        return env.get(node.id)
    if isinstance(node, ast.BinOp) and isinstance(node.op, ast.Add):
        a, b = resolve_list(node.left, env), resolve_list(node.right, env)
        if a is not None and b is not None:
            return a + b
    return None


def module_env(tree):
    env = {}
    for n in tree.body:
        if isinstance(n, ast.Assign) and len(n.targets) == 1 and isinstance(n.targets[0], ast.Name):
            v = resolve_list(n.value, env)
            if v is not None:
                env[n.targets[0].id] = v
    return env


def call_name(c):
    f = c.func
    if isinstance(f, ast.Name):
        return f.id
    if isinstance(f, ast.Attribute):
        return f.attr
    return ''


def is_runner(name):
    return 'run' in name.lower() and name not in ('running',)


def argv_of(fn, modenv):
    """Literal argv (tuple of str) passed to runner calls in fn, in order."""
    env = dict(modenv)
    events = []
    for n in ast.walk(fn):
        if isinstance(n, ast.Assign) and len(n.targets) == 1 and isinstance(n.targets[0], ast.Name):
            events.append((n.lineno, 0, n))
        if isinstance(n, ast.Call) and is_runner(call_name(n)):
            events.append((n.lineno, 1, n))
    events.sort(key=lambda e: (e[0], e[1]))
    argvs = []
    for _, kind, n in events:
        if kind == 0:
            v = resolve_list(n.value, env)
            if v is not None:
                env[n.targets[0].id] = v
            else:
                env.pop(n.targets[0].id, None)
        else:
            for a in n.args:
                v = resolve_list(a, env)
                if v is not None and any(s[:1] in ('-', '@') or '/' in s for s in v):
                    argvs.append(v)
                    break
    return argvs


def asserts_of(fn):
    """Direct asserts, pytest.raises and asserting-helper calls, with their kinds."""
    out = []
    for n in ast.walk(fn):
        if isinstance(n, ast.Assert):
            out.append(('assert', n))
        elif isinstance(n, ast.With):
            for it in n.items:
                if isinstance(it.context_expr, ast.Call) and 'raises' in ast.unparse(it.context_expr.func):
                    out.append(('raises', n))
    return out


def classify_assert(t):
    """Weak classes: rc, substr, empty, notnone, truthy. Strong: anything else."""
    if isinstance(t, ast.Compare) and len(t.ops) == 1:
        op, left, right = t.ops[0], t.left, t.comparators[0]
        if is_rc(left) and isinstance(op, (ast.Eq, ast.NotEq)) and isinstance(right, ast.Constant):
            return 'rc0' if right.value == 0 else 'rcN'
        if isinstance(op, ast.In) and const_str(left) and isinstance(right, ast.Attribute) and right.attr in ('stdout', 'stderr'):
            return 'substr'
        if isinstance(op, ast.Eq) and isinstance(right, (ast.Constant, ast.List)) and \
                (right.value == '' if isinstance(right, ast.Constant) else not right.elts):
            return 'empty'
        if isinstance(op, ast.IsNot) and isinstance(right, ast.Constant) and right.value is None:
            return 'notnone'
    if isinstance(t, (ast.Name, ast.Attribute, ast.Call, ast.Subscript, ast.UnaryOp)):
        return 'truthy'
    return 'strong'
