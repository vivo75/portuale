#!/usr/bin/env python3
"""Split a Rust module file into submodules by a spec (#336 Phase 6).

Usage: split_module.py SRC_FILE SPEC.tsv [--dry-run]

SRC_FILE is the crate root (lib.rs/main.rs) or a `foo.rs`/`foo/mod.rs`
module file. SPEC rows are `anchor<TAB>module[<TAB>doc]` (see the spec
files under LLM/plans/). Each top-level item (rust_items.py) goes to the
module of the last anchor at or before it; `=module` rows move one item.

What the move does to an item, and nothing else:
- a private top-level item becomes `pub(crate)`, and so do the private
  fields of a moved struct and the private members of a moved inherent
  `impl`, because code in other modules keeps using them (trait impls,
  enum variants and nested items are untouched; nothing becomes `pub`);
- the source file declares `mod <m>;` and `pub use <m>::*;` for each new
  module, so every existing path (`crate::foo`, the public API) still
  resolves; each new file starts with `use crate::*;`.
Inline `#[cfg(test)] mod tests* { .. }` modules move to `tests/` next to
the source (`tests/mod.rs` for `tests`, `tests/<name>.rs` children for the
others) with their bodies unchanged except `use super::*;` -> `use
crate::*;` in the moved children.
"""

import os
import re
import sys
from collections import OrderedDict

sys.path.insert(0, os.path.dirname(__file__))
from rust_items import code_mask, items  # noqa: E402

VIS = re.compile(r"^pub\b")
TOP = re.compile(r"^((?:(?:async|const|unsafe|extern\s+\"C\")\s+)*)(fn|struct|enum|union|type|const|static|trait)\b")
MEMBER = re.compile(r"^    ((?:(?:async|const|unsafe)\s+)*)(fn|const|type)\b")
FIELD = re.compile(r"^    (r#)?[a-z_][a-z0-9_]*\s*:")


def widen(text, kind):
    lines = text.split("\n")
    mask = code_mask(text)
    offs, o = [], 0
    for l in lines:
        offs.append(o); o += len(l) + 1

    def is_code(i, col=0):
        k = offs[i] + col
        return k < len(mask) and mask[k]

    # header: first column-0 code line that is not an attribute
    h = next(i for i, l in enumerate(lines)
             if l and not l.startswith((" ", "\t", "#", "//")) and is_code(i))
    head = lines[h]
    if not VIS.match(head) and TOP.match(head):
        lines[h] = "pub(crate) " + head
    head_nv = re.sub(r"^pub(\([^)]*\))?\s+", "", lines[h])
    body = range(h + 1, len(lines))
    if kind == "struct" and "{" in head_nv:
        for i in body:
            l = lines[i]
            if FIELD.match(l) and is_code(i, 4):
                lines[i] = "    pub(crate) " + l[4:]
    elif kind == "struct" and "(" in head_nv.split("{")[0]:
        m = re.match(r"^(.*?struct\s+\w+(?:<[^>]*>)?)\((.*)\)(\s*(?:where[^;]*)?;)\s*$", lines[h])
        if m and "pub" not in m.group(2):
            fields, depth, cur = [], 0, ""
            for c in m.group(2):
                depth += c in "<([" ; depth -= c in ">)]"
                if c == "," and depth == 0:
                    fields.append(cur.strip()); cur = ""
                else:
                    cur += c
            if cur.strip():
                fields.append(cur.strip())
            lines[h] = m.group(1) + "(" + ", ".join("pub(crate) " + f for f in fields) + ")" + m.group(3)
        elif not m:
            print(f"warning: tuple struct not widened: {head[:60]}", file=sys.stderr)
    elif kind == "impl" and " for " not in head_nv.split("{")[0]:
        for i in body:
            l = lines[i]
            if MEMBER.match(l) and is_code(i, 4):
                lines[i] = "    pub(crate) " + l[4:]
    elif kind == "thread_local!":
        for i in body:
            l = lines[i]
            if re.match(r"^    static\b", l) and is_code(i, 4):
                lines[i] = "    pub(crate) " + l[4:]
    return "\n".join(lines)


def load_spec(path):
    rows = []
    for line in open(path):
        line = line.rstrip("\n")
        if not line or line.startswith("#"):
            continue
        parts = line.split("\t")
        rows.append((parts[0], parts[1], parts[2] if len(parts) > 2 else ""))
    return rows


def main(src_path, spec_path, dry):
    src = open(src_path).read()
    lines = src.split("\n")
    its = items(src)
    spec = load_spec(spec_path)
    docs = {}
    for _, mod, doc in spec:
        if doc and mod.lstrip("=") not in docs:
            docs[mod.lstrip("=")] = doc

    def find(anchor):
        def word_prefix(name):
            if not name.startswith(anchor):
                return False
            rest = name[len(anchor):]
            return not rest or not (anchor[-1].isalnum() or anchor[-1] == "_") \
                or not (rest[0].isalnum() or rest[0] == "_")
        hits = [k for k, it in enumerate(its) if word_prefix(it[3])]
        if len(hits) != 1:
            sys.exit(f"anchor {anchor!r} matches {len(hits)} items")
        return hits[0]

    assign = ["lib"] * len(its)
    ranges = sorted((find(a), m) for a, m, _ in spec if not m.startswith("="))
    for (k, m), nxt in zip(ranges, ranges[1:] + [(len(its), None)]):
        for j in range(k, nxt[0]):
            assign[j] = m
    for a, m, _ in spec:
        if m.startswith("="):
            assign[find(a)] = m[1:]
    # test modules
    tests = []
    for k, (s, e, kind, name) in enumerate(its):
        span = lines[s - 1:e]
        mod_i = next((i for i, l in enumerate(span) if re.match(r"^mod tests\w*\s*\{", l)), None)
        if kind == "mod" and re.match(r"^mod tests\w*$", name) and mod_i is not None \
                and any(l.strip() == "#[cfg(test)]" for l in span[:mod_i]):
            assign[k] = "TEST:" + name.split()[1]
    base = os.path.dirname(src_path)
    is_root = os.path.basename(src_path) in ("lib.rs", "main.rs", "mod.rs")
    moddir = base if is_root else os.path.join(base, os.path.basename(src_path)[:-3])

    def text_of(k):
        s, e = its[k][0], its[k][1]
        return "\n".join(lines[s - 1:e])

    files = OrderedDict()   # module path -> list of item texts
    for k, m in enumerate(assign):
        if m == "lib" or m.startswith("TEST:"):
            continue
        files.setdefault(m, []).append(widen(text_of(k), its[k][2]))
    # module tree
    tree = OrderedDict()
    for m in files:
        parts = m.split("/")
        for i in range(len(parts)):
            tree.setdefault("/".join(parts[:i]), [])
            child = parts[i]
            if child not in tree["/".join(parts[:i])]:
                tree["/".join(parts[:i])].append(child)
    out = {}
    for m, texts in files.items():
        is_dir = m in tree
        path = os.path.join(moddir, m, "mod.rs") if is_dir else os.path.join(moddir, m + ".rs")
        head = f"//! {docs.get(m, '')}\n\nuse crate::*;\n" if docs.get(m) else "use crate::*;\n"
        out[path] = head + "\n" + "\n\n".join(texts) + "\n"
    def has_pub(m):
        """Whether module m (or a submodule) defines any `pub` item."""
        texts = [t for k, ts in files.items() if k == m or k.startswith(m + "/") for t in ts]
        return any(re.search(r"(?m)^pub (?!\()", t) for t in texts)

    def reexport(parent, c):
        full = f"{parent}/{c}" if parent else c
        return f"{'pub' if has_pub(full) else 'pub(crate)'} use {c}::*;\n"

    for parent, children in tree.items():
        if parent == "":
            continue
        path = os.path.join(moddir, parent, "mod.rs")
        decl = "".join(f"mod {c};\n" for c in children) + "\n" + "".join(reexport(parent, c) for c in children)
        if path in out:
            out[path] = out[path].replace("use crate::*;\n", "use crate::*;\n\n" + decl, 1)
        else:
            doc = docs.get(parent, "")
            out[path] = (f"//! {doc}\n\n" if doc else "") + decl
    # tests
    test_names = [m[5:] for m in assign if m.startswith("TEST:")]
    if test_names:
        tdir = os.path.join(moddir, "tests")
        for k, m in enumerate(assign):
            if not m.startswith("TEST:"):
                continue
            name = m[5:]
            body_lines = lines[its[k][0] - 1:its[k][1]]
            open_i = next(i for i, l in enumerate(body_lines) if re.match(rf"^mod {name}\s*\{{", l))
            pre = [l for l in body_lines[:open_i] if l.strip() != "#[cfg(test)]"]
            inner = body_lines[open_i + 1:-1]
            inner = [l[4:] if l.startswith("    ") else l for l in inner]
            text = "\n".join(l[4:] if l.startswith("    ") else l for l in pre if l.strip() != "#[cfg(test)]")
            text = "\n".join(x for x in [text] if x) + ("\n" if text else "") + "\n".join(inner) + "\n"
            if name == "tests":
                out[os.path.join(tdir, "mod.rs")] = text
            else:
                text = text.replace("use super::*;", "use crate::*;", 1)
                out[os.path.join(tdir, f"{name}.rs")] = text
        mp = os.path.join(tdir, "mod.rs")
        children = [n for n in test_names if n != "tests"]
        out[mp] = out.get(mp, "use crate::*;\n") + "\n" + "".join(f"mod {c};\n" for c in children)
    # the source file: lib items in place, module declarations after the last
    # existing `mod` item (or at the first item)
    keep = [k for k, m in enumerate(assign) if m == "lib"]
    last_mod = max([k for k in keep if its[k][2] == "mod"], default=keep[0])
    new_src = []
    roots = tree.get("", [])
    for k in keep:
        s, e = its[k][0], its[k][1]
        new_src.append("\n".join(lines[s - 1:e]))
        if k == last_mod:
            new_src.append("".join(f"mod {c};\n" for c in roots).rstrip("\n"))
            if test_names:
                new_src.append("#[cfg(test)]\nmod tests;")
            new_src.append("".join(reexport("", c) for c in roots).rstrip("\n"))
    out[src_path] = "\n\n".join(new_src) + "\n"
    for p in sorted(out):
        print(f"{sum(1 for _ in out[p].splitlines()):6d}  {p}")
    if not dry:
        for p, t in out.items():
            os.makedirs(os.path.dirname(p), exist_ok=True)
            open(p, "w").write(t)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2], "--dry-run" in sys.argv)
