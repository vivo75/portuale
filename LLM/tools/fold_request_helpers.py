#!/usr/bin/env python3
"""#336 Phase 5: fold test helpers that differ only in ResolveRequest fields.

Usage: fold_request_helpers.py FILE BASE VARIANT...

BASE is a test helper `fn BASE(atom_str: &str) -> T` whose body builds
`ResolveRequest::new(..)` and resolves it. Each VARIANT has the same body
except for a `ResolveRequest { <fields>, ..ResolveRequest::new(..) }`
wrapper. The script checks that (refusing otherwise), then adds
`fn BASE_with(atom_str, tweak: impl FnOnce(&mut ResolveRequest)) -> T`,
and rewrites BASE and every VARIANT as one-line calls of it.
"""

import re
import sys


def fn_span(src, name):
    m = re.search(rf"\n    fn {name}\(atom_str: &str\) -> ([^{{]+) \{{\n", src)
    if not m:
        sys.exit(f"no single-argument helper {name}")
    start = m.start() + 1
    end = src.index("\n    }\n", m.end()) + 7
    return start, end, m.group(1).strip(), src[m.end():end - 7]


def strip_fields(body):
    """Return (fields, body without the struct-update wrapper)."""
    m = re.search(r"ResolveRequest \{\n((?:\s+[a-z_]+(?:: [^\n]+)?,\n)+)\s+\.\.(ResolveRequest::new\((?:.|\n)*?\n\s+\))\n\s+\}", body)
    if not m:
        return None, body
    fields = [l.strip().rstrip(",") for l in m.group(1).splitlines()]
    ctor = m.group(2)
    # re-indent the constructor one level up, as in the base body
    ctor = "\n".join(l[4:] if l.startswith("            ") else l for l in ctor.split("\n"))
    return fields, body[:m.start()] + ctor + body[m.end():]


def main(path, base, variants):
    src = open(path).read()
    b_start, b_end, ret, b_body = fn_span(src, base)
    tweak_body = b_body.replace("        ResolveRequest::new(", "        let mut request = ResolveRequest::new(", 1)
    tweak_body = tweak_body.replace("\n        )\n        .resolve()", "\n        );\n        tweak(&mut request);\n        request\n        .resolve()", 1)
    if "tweak(&mut request)" not in tweak_body:
        sys.exit(f"{base}: unexpected body shape")
    with_fn = (f"    fn {base}_with(atom_str: &str, tweak: impl FnOnce(&mut ResolveRequest)) -> {ret} {{\n"
               f"{tweak_body}\n    }}\n\n")
    edits = []
    for v in variants:
        s, e, r, body = fn_span(src, v)
        fields, rest = strip_fields(body)
        if fields is None or r != ret or rest != b_body:
            sys.exit(f"{v}: not BASE plus fields (fields={fields})")
        sets = "; ".join(f"r.{f} = {f}" if ":" not in f else f"r.{f.split(':', 1)[0]} = {f.split(':', 1)[1].strip()}" for f in fields)
        header = src[s:src.index("{\n", s) + 2]
        edits.append((s, e, f"{header}        {base}_with(atom_str, |r| {sets})\n    }}\n"))
    header = src[b_start:src.index("{\n", b_start) + 2]
    edits.append((b_start, b_end, with_fn + f"{header}        {base}_with(atom_str, |_| {{}})\n    }}\n"))
    for s, e, text in sorted(edits, reverse=True):
        src = src[:s] + text + src[e:]
    open(path, "w").write(src)
    print(f"{base}: folded {len(variants)} variants")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2], sys.argv[3:])
