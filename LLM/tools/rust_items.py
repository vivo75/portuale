#!/usr/bin/env python3
"""List the top-level items of a rustfmt-formatted Rust file.

Usage: rust_items.py FILE          -> TSV: start  end  kind  name  (1-based lines)

An item's span starts at its first leading `///`/`//`/`#[..]` line (blank
lines end that run) and ends at its closing `}` or `;`. The scan is
lexer-based (strings, raw strings, char literals and comments never count
as code), so a brace or `;` inside a literal cannot cut an item short.
Used by split_module.py.
"""

import re
import sys

ITEM = re.compile(
    r"(?:pub(?:\([^)]*\))?\s+)?(?:(?:unsafe|async|const|extern\s+\"C\")\s+)*"
    r"(fn\b|struct\b|enum\b|union\b|trait\b|impl\b|mod\b|type\b|const\b|static\b|use\b|macro_rules!|thread_local!)"
)
BLOCK_KINDS = {"fn", "impl", "enum", "union", "trait", "mod", "macro_rules!", "thread_local!"}


def code_mask(src):
    """Per character: True where it is code (not comment, string or char)."""
    mask = [True] * len(src)
    i, n = 0, len(src)

    def blank(a, b):
        for k in range(a, b):
            mask[k] = False

    while i < n:
        if src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            blank(i, j); i = j
        elif src.startswith("/*", i):
            depth, j = 1, i + 2
            while depth and j < n:
                if src.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif src.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            blank(i, j); i = j
        elif (src[i] in "rb" and re.match(r'b?r#*"', src[i:i + 6])
              and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_"))):
            m = re.match(r'(b?r)(#*)"', src[i:])
            close = '"' + m.group(2)
            j = src.index(close, i + len(m.group(0))) + len(close)
            blank(i + 1, j - 1); i = j
        elif src[i] == '"' or (src[i] == "b" and src.startswith('b"', i)
                               and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_"))):
            j = i + (2 if src[i] == "b" else 1)
            while src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            blank(i + 1, j); i = j + 1
        elif src[i] == "'":
            m = re.match(r"'(\\(?:u\{[0-9a-fA-F]+\}|x[0-9a-fA-F]{2}|.)|[^\\'])'", src[i:])
            if m:
                blank(i, i + len(m.group(0))); i += len(m.group(0))
            else:
                i += 1  # lifetime
        else:
            i += 1
    return mask


def items(src):
    mask = code_mask(src)
    lines = src.split("\n")
    offs, o = [], 0
    for l in lines:
        offs.append(o); o += len(l) + 1
    out = []
    ln = 0
    while ln < len(lines):
        line = lines[ln]
        t = line.strip()
        # an item header is a column-0 code line
        if not t or line[0] in " \t}" or t.startswith(("//", "#[", "#!")) or not mask[offs[ln]]:
            ln += 1; continue
        m = ITEM.match(t)
        if not m:
            ln += 1; continue
        kind = m.group(1)
        # leading attributes / doc comments
        start = ln
        while start > 0 and lines[start - 1].strip().startswith(("///", "//", "#[")) \
                and not lines[start - 1].startswith((" ", "\t")):
            start -= 1
        # find the end
        depth, i, end_off = 0, offs[ln], None
        head = re.sub(r"^pub(\([^)]*\))?\s+", "", t)
        braced = kind in BLOCK_KINDS or (
            kind == "struct" and "(" not in head.split("{")[0].split("<")[0]
            and not head.rstrip().endswith(";"))
        while i < len(src):
            c = src[i]
            if mask[i]:
                if c in "([{":
                    depth += 1
                elif c in ")]}":
                    depth -= 1
                    if depth == 0 and c == "}" and braced:
                        end_off = i; break
                elif c == ";" and depth == 0:
                    end_off = i; break
            i += 1
        end_ln = src.count("\n", 0, end_off)
        name = re.sub(r"\s*\{\s*$", "", t)[:100]
        out.append([start + 1, end_ln + 1, kind, name])
        ln = end_ln + 1
    # comments separated from the item by blank lines (section banners)
    # belong to the item that follows them
    prev_end = 0
    for it in out:
        k = it[0]
        while k - 1 > prev_end and (not lines[k - 2].strip() or lines[k - 2].startswith("//")):
            k -= 1
        while k < it[0] and not lines[k - 1].strip():
            k += 1
        it[0] = k
        prev_end = it[1]
    return [tuple(it) for it in out]


if __name__ == "__main__":
    for s, e, k, n in items(open(sys.argv[1]).read()):
        print(f"{s}\t{e}\t{k}\t{n}")
