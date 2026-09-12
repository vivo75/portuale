#!/usr/bin/env python3
"""Inject the `RT_SEL` merge-order trace into real portage's
`_serialize_tasks` -- the real half of the harness described in
`TEST/scripts/mo-trace/README.md`.

The injected block prints one line per selection iteration, just before
real removes the selected nodes:

    RT_SEL iter=<N> retlist=<R> alive=<A> asap=[<cpv ...>] prefer_asap=<0|1>
           drop_satisfied=<0|1> ig=<name> pick=<cat/pkg-ver ...>

with exactly the field order portuale's `PORTUALE_MO_SEL` line uses
(`rust/portage-repo/src/merge_order.rs::mo_sel_trace_line`), so
`align-traces.py` can walk the two streams in step. `retlist` counts
merge packages only (real appends only those), `alive` counts every
non-uninstall package still in `mygraph`, `pick` the merge nodes chosen
this iteration.

Usage (inside the container, or against any tree):

    python3 real-trace.py            # patch the imported _emerge.depgraph
    python3 real-trace.py --unpatch  # remove the injected block
    python3 real-trace.py PATH       # patch a specific depgraph.py

Patching is idempotent; the file is left byte-identical by --unpatch.
The container is throwaway, so patching it is safe; do NOT run this
against a system portage you care about.
"""

import importlib.util
import os
import re
import sys

BEGIN = "            # MO_SEL_TRACE_BEGIN (injected by TEST/scripts/mo-trace/real-trace.py)\n"
END = "            # MO_SEL_TRACE_END\n"

# 3.0.81.3 has an extra `self._spinner_update()` inside the loop head;
# 3.0.82.2 does not. Match both.
COUNTER_RE = re.compile(
    r"(?m)^(        while mygraph:\n)"
    r"((?:            self\._spinner_update\(\)\n)?)"
    r"(            selected_nodes = None\n)"
)

SELECT_ANCHOR = """            # At this point, we've succeeded in selecting one or more nodes, so
            # reset state variables for leaf node selection.
            prefer_asap = True
            drop_satisfied = False
"""

ORDER_BEGIN = "        # MO_ORDER_TRACE_BEGIN (injected by TEST/scripts/mo-trace/real-trace.py)\n"
ORDER_END = "        # MO_ORDER_TRACE_END\n"
ORDER_ANCHOR = "        self._merge_order_bias(mygraph)\n"
ORDER_BLOCK = (
    ORDER_BEGIN
    + """        try:
            import sys as _mo_sys3

            _mo_order = [
                ("m:" if _mo_n.operation == "merge" else "n:") + _mo_n.cpv
                for _mo_n in mygraph.order
            ]
            _mo_sys3.stderr.write(
                "MO_ORDER count=%d %s\\n" % (len(_mo_order), " ".join(_mo_order))
            )
        except Exception:
            pass
"""
    + ORDER_END
)

TRACE_BLOCK = (
    BEGIN
    + """            try:
                import sys as _mo_sys

                _mo_alive = sum(
                    1
                    for _mo_n in mygraph
                    if isinstance(_mo_n, Package) and _mo_n.operation != "uninstall"
                )
                _mo_retlist = sum(
                    1
                    for _mo_n in retlist
                    if isinstance(_mo_n, Package) and _mo_n.operation == "merge"
                )
                _mo_pick = [
                    ("m:" if _mo_n.operation == "merge" else "n:") + _mo_n.cpv
                    for _mo_n in selected_nodes
                    if isinstance(_mo_n, Package) and _mo_n.operation != "uninstall"
                ]
                _mo_asap = [
                    ("m:" if _mo_n.operation == "merge" else "n:") + _mo_n.cpv
                    for _mo_n in asap_nodes
                    if mygraph.contains(_mo_n)
                ]
                _mo_ig = getattr(ignore_priority, "__name__", "none") or "none"
                _mo_sys.stderr.write(
                    "RT_SEL iter=%d retlist=%d alive=%d asap=[%s] "
                    "prefer_asap=%d drop_satisfied=%d ig=%s pick=%s\\n"
                    % (
                        _mo_iter,
                        _mo_retlist,
                        _mo_alive,
                        " ".join(_mo_asap),
                        int(prefer_asap),
                        int(drop_satisfied),
                        _mo_ig,
                        " ".join(_mo_pick),
                    )
                )
            except Exception:
                pass
"""
    + END
    + "\n"
)


def default_path():
    try:
        import _emerge.depgraph as dg

        return dg.__file__
    except Exception:
        return None


def patch(text):
    if BEGIN in text:
        return text, False
    if SELECT_ANCHOR not in text:
        raise SystemExit(
            "real-trace: anchors not found -- depgraph.py does not match the "
            "expected _serialize_tasks shape (portage version drift?)"
        )
    text, n = COUNTER_RE.subn(
        lambda m: (
            "        _mo_iter = 0\n"
            + "        try:\n"
            + "            import sys as _mo_sys2\n"
            + "\n"
            + "            _mo_nodes = [\n"
            + "                ('m:' if _mo_n.operation == 'merge' else 'n:') + _mo_n.cpv\n"
            + "                for _mo_n in mygraph\n"
            + "                if isinstance(_mo_n, Package) and _mo_n.operation != 'uninstall'\n"
            + "            ]\n"
            + "            _mo_sys2.stderr.write(\n"
            + '                "MO_NODES count=%d %s\\n" % (len(_mo_nodes), " ".join(_mo_nodes))\n'
            + "            )\n"
            + "        except Exception:\n"
            + "            pass\n"
            + m.group(1)
            + "            _mo_iter += 1\n"
            + m.group(2)
            + m.group(3)
        ),
        text,
        count=1,
    )
    if n != 1:
        raise SystemExit("real-trace: could not find the `while mygraph:` loop head")
    text = text.replace(SELECT_ANCHOR, TRACE_BLOCK + SELECT_ANCHOR, 1)
    if ORDER_ANCHOR not in text:
        raise SystemExit("real-trace: could not find _merge_order_bias")
    text = text.replace(ORDER_ANCHOR, ORDER_BLOCK + ORDER_ANCHOR, 1)
    return text, True


def unpatch(text):
    if BEGIN not in text:
        return text, False
    text = re.sub(re.escape(BEGIN) + r".*?" + re.escape(END) + "\n", "", text, flags=re.S)
    text = re.sub(
        re.escape(ORDER_BEGIN) + r".*?" + re.escape(ORDER_END),
        "",
        text,
        flags=re.S,
    )
    text = re.sub(
        r"        _mo_iter = 0\n.*?        while mygraph:\n",
        "        while mygraph:\n",
        text,
        count=1,
        flags=re.S,
    )
    text = text.replace("            _mo_iter += 1\n", "", 1)
    return text, True


def main(argv):
    args = [a for a in argv if a != "--unpatch"]
    do_unpatch = "--unpatch" in argv
    path = args[0] if args else default_path()
    if not path:
        raise SystemExit("real-trace: no depgraph.py given and no importable _emerge.depgraph")
    with open(path) as f:
        text = f.read()
    new, changed = unpatch(text) if do_unpatch else patch(text)
    if not changed and not do_unpatch and BEGIN in text:
        print(f"real-trace: already patched: {path}")
        return 0
    with open(path, "w") as f:
        f.write(new)
    print(("real-trace: unpatched " if do_unpatch else "real-trace: patched ") + path)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
