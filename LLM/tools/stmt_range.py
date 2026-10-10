#!/usr/bin/env python3
"""Print the line range of top-level statements of a function body
(#336 Phase 6.3; feeds extract_block.py).

Usage: stmt_range.py FILE LINE [COUNT]
       stmt_range.py FILE LINE --until NEXT

LINE is the first line of a statement at 4-space indent. The range starts
at the comment lines directly above it and ends where COUNT (default 1)
consecutive statements end: a `;` at depth 0, or a block's closing `}`
that is not followed by `else`.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from rust_items import code_mask  # noqa: E402


def stmt_end(src, mask, offs, ln):
    """0-based index of the last line of the statement starting at line ln."""
    i, depth = offs[ln], 0
    while i < len(src):
        c = src[i]
        if mask[i]:
            if c in "([{":
                depth += 1
            elif c in ")]}":
                depth -= 1
                if depth == 0 and c == "}":
                    rest = src[i + 1:]
                    nxt = rest.lstrip()
                    if not nxt.startswith(("else", ".", "?", ")", ";", "=>")) \
                            and src[offs[ln]:i].lstrip().startswith(("if ", "for ", "while ", "match ", "loop ", "{", "if let ")):
                        return src.count("\n", 0, i)
            elif c == ";" and depth == 0:
                return src.count("\n", 0, i)
        i += 1
    raise SystemExit("no end")


def main(path, line, count, until=None):
    src = open(path).read()
    mask = code_mask(src)
    lines = src.split("\n")
    offs, o = [], 0
    for l in lines:
        offs.append(o)
        o += len(l) + 1
    ln = line - 1
    start = ln
    while start > 0 and lines[start - 1].startswith("    //"):
        start -= 1
    if until:
        end = until - 2
        while lines[end - 1].startswith("    //"):
            end -= 1
        while not lines[end].strip() or lines[end].startswith("    //"):
            end -= 1
        print(start + 1, end + 1)
        return
    for _ in range(count):
        end = stmt_end(src, mask, offs, ln)
        ln = end + 1
        while ln < len(lines) and (not lines[ln].strip() or lines[ln].startswith("    //")):
            ln += 1
    print(start + 1, end + 1)


if __name__ == "__main__":
    a = sys.argv[1:]
    if "--until" in a:
        main(a[0], int(a[1]), 0, int(a[a.index("--until") + 1]))
    else:
        main(a[0], int(a[1]), int(a[2]) if len(a) > 2 else 1)
