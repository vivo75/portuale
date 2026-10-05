#!/usr/bin/env python3
"""Prove a change to Rust files touched only comments.

Usage: strip_comments_eq.py [REV] [FILE...]
Compares every changed .rs file (or the given ones) between REV (default
HEAD) and the working tree after removing `//`, `///`, `//!` and `/* */`
comments and all whitespace outside string/char literals. Prints the files
whose code differs; exit 1 if any. Doc comments are comments here: check
the doctest count separately when `///` blocks with code fences change.
"""

import subprocess
import sys


def strip(src):
    out, i, n = [], 0, len(src)
    while i < n:
        c = src[i]
        if src.startswith("//", i):
            j = src.find("\n", i)
            i = n if j < 0 else j
        elif src.startswith("/*", i):
            depth, i = 1, i + 2
            while depth and i < n:
                if src.startswith("/*", i):
                    depth, i = depth + 1, i + 2
                elif src.startswith("*/", i):
                    depth, i = depth - 1, i + 2
                else:
                    i += 1
        elif c in "rb" and (src.startswith('r"', i) or src.startswith("r#", i)
                            or src.startswith('br"', i) or src.startswith("br#", i)):
            j = i + (2 if src[i] == "b" else 1)
            hashes = 0
            while src[j] == "#":
                hashes, j = hashes + 1, j + 1
            if src[j] != '"':
                out.append(c); i += 1; continue
            end = src.index('"' + "#" * hashes, j + 1) + 1 + hashes
            out.append(src[i:end]); i = end
        elif c == '"':
            j = i + 1
            while src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            out.append(src[i:j + 1]); i = j + 1
        elif c == "'":
            # char literal ('x', '\n', '\u{..}') vs lifetime ('a)
            if src.startswith("\\", i + 1):
                j = src.index("'", i + 2)
                out.append(src[i:j + 1]); i = j + 1
            elif i + 2 < n and src[i + 2] == "'":
                out.append(src[i:i + 3]); i += 3
            else:
                out.append(c); i += 1
        elif c.isspace():
            i += 1
        else:
            out.append(c); i += 1
    return "".join(out)


def main(argv):
    rev = argv[1] if len(argv) > 1 and not argv[1].endswith(".rs") else "HEAD"
    files = [a for a in argv[1:] if a.endswith(".rs")] or subprocess.run(
        ["git", "diff", "--name-only", rev, "--", "*.rs"],
        capture_output=True, text=True, check=True).stdout.split()
    bad = []
    for f in files:
        old = subprocess.run(["git", "show", f"{rev}:{f}"], capture_output=True, text=True)
        before = strip(old.stdout) if old.returncode == 0 else ""
        if strip(open(f).read()) != before:
            bad.append(f)
    print("\n".join(bad))
    print(f"{len(files)} file(s) checked, {len(bad)} with code changes", file=sys.stderr)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
