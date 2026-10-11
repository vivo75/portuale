#!/usr/bin/env python3
"""Drop the ControlFlow<()> of a cut-out loop step whose call site loops
either way (#336 Phase 6.3; follows extract_block.py --loop).

Usage: flatten_flow.py HOST_RS FN_RS FNAME

Where the call is the loop body's last statement, or is followed by a
`continue;`, `if FNAME(..).is_break() { continue 'l; }` does the same
as a plain `FNAME(..);`, so the function returns `()` (`return;` for
each `return ControlFlow::Break(())`). Without `?` only.
"""

import re
import sys


def main(host, fn_rs, fname):
    h = open(host).read()
    pat = re.compile(rf"(?m)^(\s*)if {fname}\((.*?)\n\1(\}}?)\)\n\1\.is_break\(\)\n\1\{{\n\1    continue '\w+;\n\1\}}\n", re.S)
    m = pat.search(h)
    assert m, "call site not found (expected the rustfmt'd `if f(..)\\n.is_break()` form)"
    ind = m.group(1)
    rest = h[m.end():]
    nxt = rest.lstrip("\n").split("\n", 1)[0]
    assert nxt.strip() in ("continue;", "}"), f"the call does not end the loop body: {nxt!r}"
    h = h[:m.start()] + f"{ind}{fname}({m.group(2)}\n{ind}{m.group(3)});\n" + h[m.end():]
    t = open(fn_rs).read()
    t, n = re.subn(rf"(pub\(crate\) fn {fname}\([^{{]*?\)) -> ControlFlow<\(\)> \{{", r"\1 {", t, flags=re.S)
    assert n == 1, "signature not found"
    t = t.replace("return ControlFlow::Break(());", "return;")
    t, n = re.subn(r"\n\n?    ControlFlow::Continue\(\(\)\)\n\}\n", "\n}\n", t)
    assert n == 1, "tail not found"
    open(host, "w").write(h)
    open(fn_rs, "w").write(t)


if __name__ == "__main__":
    main(*sys.argv[1:4])
