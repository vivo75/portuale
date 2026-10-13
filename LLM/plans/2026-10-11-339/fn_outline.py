#!/usr/bin/env python3
"""Outline a function body's statements, for planning a split (#339).

Usage: fn_outline.py FILE:LINE[:NAME]... [--min N] [--nest M]

LINE is the function's `fn` line. Prints each top-level statement of the
body (start-end line, code lines, first line) with at least N code lines
(default 8); a statement of at least M code lines (default 100) is opened
and its own block's statements are listed one level deeper, recursively.
Code lines exclude blank and comment-only lines (rust_items.code_mask).
"""
import bisect, os, sys
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'tools'))
sys.path.insert(0, 'LLM/tools')
from rust_items import code_mask

def load(f):
    src = open(f, encoding='utf-8').read()
    return src, code_mask(src), [0] + [i + 1 for i, c in enumerate(src) if c == '\n']

def code_lines(src, m, ls, a, b):
    n = 0
    for L in range(a, b + 1):
        e = ls[L] if L < len(ls) else len(src)
        if any(m[x] and not src[x].isspace() for x in range(ls[L - 1], e)):
            n += 1
    return n

def block_stmts(src, m, open_idx):
    """Top-level statements of the `{` block opening at open_idx: [(start, end)]."""
    out, start, d, k = [], None, 1, open_idx + 1
    while k < len(src):
        c = src[k]
        if m[k] and not c.isspace():
            if start is None and d == 1 and c != '}':
                start = k
            if c in '{([':
                d += 1
            elif c in '})]':
                d -= 1
                if d == 0:
                    break
                if d == 1 and c == '}':
                    rest = src[k + 1:k + 40].lstrip()
                    if not rest.startswith(('else', '.', ';', ')', '?', ',')):
                        out.append((start, k)); start = None
            elif c == ';' and d == 1:
                out.append((start, k)); start = None
        k += 1
    if start is not None:
        out.append((start, k - 1))
    return out

def first_body_brace(src, m, a, b):
    """The `{` that opens the statement's last top-level block (its body)."""
    d, last = 0, None
    for k in range(a, b + 1):
        if not m[k]:
            continue
        c = src[k]
        if c == '{' and d == 0:
            last = k
        if c in '{([':
            d += 1
        elif c in '})]':
            d -= 1
    # prefer the first depth-0 brace (if/loop/for body)
    d = 0
    for k in range(a, b + 1):
        if not m[k]:
            continue
        c = src[k]
        if c == '{' and d == 0:
            return k
        if c in '([':
            d += 1
        elif c in ')]':
            d -= 1
    return last

def show(src, m, ls, open_idx, indent, mn, nest):
    ln = lambda p: bisect.bisect_right(ls, p)
    for a, b in block_stmts(src, m, open_idx):
        la, lb = ln(a), ln(b)
        n = code_lines(src, m, ls, la, lb)
        if n < mn:
            continue
        head = src[ls[la - 1]:ls[la] if la < len(ls) else len(src)].strip()[:88]
        print(f"{'  ' * indent}{la}-{lb} {n:4d}  {head}")
        if n >= nest:
            ob = first_body_brace(src, m, a, b)
            if ob is not None:
                show(src, m, ls, ob, indent + 1, mn, nest)

def main(argv):
    mn, nest, targets = 8, 100, []
    it = iter(argv)
    for x in it:
        if x == '--min': mn = int(next(it))
        elif x == '--nest': nest = int(next(it))
        else: targets.append(x)
    for t in targets:
        parts = t.split(':')
        f, line = parts[0], int(parts[1])
        name = parts[2] if len(parts) > 2 else ''
        src, m, ls = load(f)
        i, d = ls[line - 1], 0
        while True:
            c = src[i]
            if m[i]:
                if c in '([': d += 1
                elif c in ')]': d -= 1
                elif c == '{' and d == 0: break
            i += 1
        print(f"### {name} ({f}:{line})")
        show(src, m, ls, i, 1, mn, nest)
        print()

if __name__ == '__main__':
    main(sys.argv[1:])
