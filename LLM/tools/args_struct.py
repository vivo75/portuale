#!/usr/bin/env python3
"""Give functions on the Phase 5 worklist a named-field params struct
(#336 Phase 5.2).

Usage: args_struct.py FILE [FNAME ...] [--crate PKG]

For each plain `fn` in FILE carrying `#[allow(clippy::too_many_arguments)]`
or `#[allow(clippy::fn_params_excessive_bools)]` (or only the FNAMEs
given): the allow goes, a `<Fname>Args` struct with one field per
parameter goes above it, and the function takes `args: <Fname>Args` and
destructures it, so its body does not change. The compiler then names
every call (E0061); each becomes `fname(FnameArgs { p: arg, .. })`
(shorthand when the argument is the parameter's own name), with the
struct imported where rustc suggests it. Methods, generic functions and
`impl Trait` parameters are skipped (listed). On any error left, every
file touched is restored.
"""

import json
import os
import re
import subprocess
import sys

sys.path.insert(0, os.path.dirname(__file__))
from extract_block import code_only, pascal  # noqa: E402

ALLOW = re.compile(r"(?m)^[ \t]*#\[allow\(clippy::(?:too_many_arguments|fn_params_excessive_bools)"
                   r"[^\]]*\]\n")


def split_top(text):
    """Split at commas outside brackets (`->` is not a bracket; a closure's
    `|a, b|` parameter list is skipped). Commas inside comments and
    strings do not split; the pieces keep their comments."""
    code = code_only(text)
    parts, depth, start, i = [], 0, 0, 0
    while i < len(code):
        c = code[i]
        if depth == 0 and c == "|" and re.fullmatch(r"\s*(move\s+)?", code[start:i]) \
                and not code.startswith("||", i):
            i = code.index("|", i + 1) + 1
            continue
        if code.startswith("->", i):
            i += 2
            continue
        # `<`/`>` are brackets only glued to a type (`Vec<u8>`), not
        # spaced comparisons (`a < b`)
        glued = i > 0 and not code[i - 1].isspace()
        if c in "([{" or (c == "<" and glued):
            depth += 1
        elif c in ")]}" or (c == ">" and glued and depth > 0):
            depth -= 1
        if c == "," and depth == 0:
            parts.append(text[start:i])
            start = i + 1
        i += 1
    parts.append(text[start:])
    return [p.strip() for p in parts if code_only(p).strip()]


def field(name, arg):
    """`name: arg`, its leading comment lines kept above it."""
    lead = re.match(r"(?:\s*//[^\n]*\n)*", arg).group(0)
    val = arg[len(lead):].strip()
    lead = "\n".join(l.strip() for l in lead.strip().split("\n")) + "\n" if lead.strip() else ""
    return lead + (name if val == name else f"{name}: {val}")


def lifetimed(ty):
    """`&T` -> `&'a T` and `'_` -> `'a`, except inside `Fn(..)`/`fn(..)`
    argument lists (higher-ranked there)."""
    out, i, stack = "", 0, []
    while i < len(ty):
        m = re.match(r"(?:dyn\s+)?(?:Fn|FnMut|FnOnce|fn)\s*\(", ty[i:])
        if m:
            out += m.group(0)
            i += len(m.group(0))
            stack.append("fn")
            continue
        c = ty[i]
        if c == "(":
            stack.append("(")
        elif c == ")":
            if stack:
                stack.pop()
        elif not stack or "fn" not in stack:
            if c == "&" and not ty.startswith("&'", i):
                out += "&'a "
                i += 1
                continue
            if ty.startswith("'_", i):
                out += "'a"
                i += 2
                continue
        out += c
        i += 1
    return out


class Fn:
    def __init__(self, name, vis, params, indent):
        self.name, self.vis, self.params, self.indent = name, vis, params, indent
        self.sname = pascal(name) + "Args"


def transform(path, only):
    s = open(path).read()
    done, skipped = [], []
    for m in list(ALLOW.finditer(s))[::-1]:
        if ALLOW.match(s, m.end()):
            continue   # the lower of two allows on one fn: handled with the upper one
        head = re.match(r"((?:[ \t]*(?:#\[|///)[^\n]*\n)*)([ \t]*)((?:pub(?:\([\w:]+\))? )?)(?:const )?(?:async )?fn (\w+)"
                        r"(<[^>]*>)?\(", s[m.end():])
        if not head:
            skipped.append((s[m.end():m.end() + 60].split("\n")[0], "not a fn"))
            continue
        name = head.group(4)
        if only and name not in only:
            continue
        p0 = m.end() + head.end()
        depth, i = 1, p0
        code = code_only(s)
        while depth:
            if code[i] == "(":
                depth += 1
            elif code[i] == ")":
                depth -= 1
            i += 1
        ptext = s[p0:i - 1]
        # split on the comment-free text, keep each parameter's comments
        ctext = code_only(ptext)
        cuts, depth, k = [], 0, 0
        while k < len(ctext):
            if ctext.startswith("->", k):
                k += 2
                continue
            ch = ctext[k]
            if ch in "([{<":
                depth += 1
            elif ch in ")]}>":
                depth -= 1
            elif ch == "," and depth == 0:
                cuts.append(k)
            k += 1
        bounds = [0] + [c + 1 for c in cuts] + [len(ptext)]
        raw = [ptext[bounds[j]:bounds[j + 1] - (1 if j < len(cuts) else 0)] for j in range(len(bounds) - 1)]
        params, notes = [], []
        for r in raw:
            com = [ln.strip()[2:].strip() for ln in r.split("\n") if ln.strip().startswith("//")]
            body = " ".join(ln for ln in r.split("\n") if not ln.strip().startswith("//")).strip()
            if body:
                params.append(body)
                notes.append(com)
        gen_a = head.group(5) and re.fullmatch(r"<\s*'a\s*>", head.group(5))
        if head.group(5) and not gen_a:
            skipped.append((name, "generic"))
            continue
        # a method keeps `self`; an `impl Trait` parameter stays a
        # parameter after `args` (the struct would need a generic)
        selfp = params[0] if params and re.match(r"(&\s*('\w+\s+)?(mut\s+)?)?(mut\s+)?self\b", params[0]) else None
        # neither lint fires (clippy.toml: <= 5 params, self included, <= 2 bools): the allow just goes
        if len(params) <= 5 and sum(bool(re.search(r":\s*bool\s*$", p)) for p in params) < 3:
            s = s[:m.start()] + ALLOW.sub("", s[m.start():m.end()]) + s[m.end():]
            skipped.append((name, "allow dropped: the lint does not fire"))
            continue
        rest = list(zip(params, notes))[1 if selfp else 0:]
        parsed = []
        for p, com in rest:
            pm = re.match(r"(?s)(mut\s+)?(\w+)\s*:\s*(.+)$", p)
            if not pm:
                break
            parsed.append((bool(pm.group(1)), pm.group(2), " ".join(pm.group(3).split()), com,
                           "impl " in pm.group(3)))
        else:
            ind = head.group(2)
            vis = head.group(3).strip()
            sname = pascal(name) + "Args"
            fields = [(n, lifetimed(t), com) for _, n, t, com, kept in parsed if not kept]
            lt = gen_a or any("'a" in t for _, t, _ in fields)
            fvis = (vis + " ") if vis else ""
            # a method's struct goes above its `impl` block, at the
            # impl's own indent
            sind = ind
            if selfp:
                im = list(re.finditer(r"(?m)^([ \t]*)impl\b[^\n]*\{[ \t]*$", s[:m.start()]))
                if not im:
                    skipped.append((name, "method outside an impl"))
                    continue
                sind = im[-1].group(1)
                anchor = im[-1].start()
                ty = re.match(r"[ \t]*impl\b(?:<[^>]*>)?\s*(?:[\w:<>', ]+\s+for\s+)?(?:\w+::)*(\w+)",
                              s[anchor:]).group(1)
            link = f"{ty}::{name}" if selfp else name
            struct = (f"{sind}/// Arguments of [`{link}`].\n"
                      f"{sind}{fvis}struct {sname}{'<' + chr(39) + 'a>' if lt else ''} {{\n"
                      + "".join("".join(f"{sind}    /// {c}\n" for c in com) + f"{sind}    {fvis}{n}: {t},\n"
                                for n, t, com in fields)
                      + f"{sind}}}\n\n")
            # body prologue
            ob = s.index("{", i)
            pat = ", ".join(("mut " if mu else "") + n for mu, n, _, _, kept in parsed if not kept)
            prologue = f"\n{ind}    let {sname} {{ {pat} }} = args;"
            sig = ((selfp + ", ") if selfp else "") + \
                f"args: {sname}{('<' + chr(39) + 'a>' if gen_a else '<' + chr(39) + '_>') if lt else ''}" + \
                "".join(f", {'mut ' if mu else ''}{n}: {t}" for mu, n, t, _, kept in parsed if kept)
            s = s[:i - 1 - len(ptext)] + sig + s[i - 1:ob + 1] + prologue + s[ob + 1:]
            # struct above the fn's doc comment and attributes; every
            # worklist allow on the fn removed
            docs = re.search(r"(?m)(?:^[ \t]*(?:///|#\[)[^\n]*\n)*\Z", s[:m.start()])
            attrs_after = s[m.end():m.end() + head.start(2)]
            # the fn's `#[cfg(..)]` applies to its struct too
            cfgs = "".join(re.findall(r"(?m)^[ \t]*#\[cfg(?:_attr)?\([^\n]*\]\n",
                                      s[docs.start():m.start()] + attrs_after))
            cfgs = "".join(l.strip() and f"{sind}{l.strip()}\n" for l in cfgs.split("\n"))
            struct = struct.replace(f"{sind}{fvis}struct ", f"{cfgs}{sind}{fvis}struct ", 1)
            tail = ALLOW.sub("", s[docs.start():m.start()]) + ALLOW.sub("", attrs_after)
            s = s[:docs.start()] + tail + s[m.end() + head.start(2):]
            if selfp:
                idocs = re.search(r"(?m)(?:^[ \t]*(?:///|#\[)[^\n]*\n)*\Z", s[:anchor])
                s = s[:idocs.start()] + struct + s[idocs.start():]
            else:
                s = s[:docs.start()] + struct + s[docs.start():]
            done.append(Fn(name, vis, parsed, ind))
            continue
        skipped.append((name, "pattern parameter"))
    open(path, "w").write(s)
    return done, skipped


def check(pkg):
    cmd = ["cargo", "check", "--release", "--all-targets", "--message-format=json"]
    cmd += ["-p", pkg] if pkg else ["--workspace"]
    p = subprocess.run(cmd, capture_output=True, text=True)
    out = []
    for l in p.stdout.splitlines():
        try:
            m = json.loads(l)
        except ValueError:
            continue
        if m.get("reason") == "compiler-message" and m["message"]["level"] == "error":
            out.append(m["message"])
    return out


def call_args(text, start):
    """(open paren, close paren) of the call whose `(` follows start."""
    code = code_only(text)
    o = code.index("(", start)
    depth, i = 1, o + 1
    while depth:
        if code[i] in "([{":
            depth += 1
        elif code[i] in ")]}":
            depth -= 1
        i += 1
    return o, i - 1


def fix_calls(fns, errs, touched):
    by = {f.name: f for f in fns}
    sites = {}
    imports = {}
    for m in errs:
        code = (m.get("code") or {}).get("code")
        if code == "E0061":
            mm = re.search(r"this (?:function|method) takes \d+ arguments? but", m["message"])
            for sp in m["spans"]:
                if sp["is_primary"] and mm:
                    sites.setdefault(sp["file_name"], set()).add((sp["line_start"], sp["column_start"]))
        elif code in ("E0422", "E0433", "E0412"):
            mm = re.search(r"`(\w+Args)`", m["message"])
            if mm and any(f.sname == mm.group(1) for f in fns):
                for c in m.get("children", []):
                    for sp in c.get("spans", []):
                        r = sp.get("suggested_replacement") or ""
                        um = re.search(r"use ([\w:]+);", r)
                        if um and um.group(1).endswith("::" + mm.group(1)):
                            imports.setdefault(m["spans"][0]["file_name"], set()).add(um.group(1))
                            break
                    else:
                        continue
                    break
    changed = False
    # E0106 in a new struct: rustc's own lifetime suggestion
    sugg = {}
    for m in errs:
        if (m.get("code") or {}).get("code") != "E0106":
            continue
        for c in m.get("children", []):
            sp = next((x for x in c.get("spans", []) if x.get("suggested_replacement") is not None), None)
            if sp:
                sugg.setdefault(sp["file_name"], set()).add(
                    (sp["line_start"], sp["column_start"], sp["column_end"], sp["suggested_replacement"]))
                break
    for f, eds in sugg.items():
        text = open(f).read()
        touched.setdefault(f, text)
        lines = text.split("\n")
        for ln, a, b, r in sorted(eds, key=lambda e: (e[0], -e[1])):
            lines[ln - 1] = lines[ln - 1][:a - 1] + r + lines[ln - 1][b - 1:]
        open(f, "w").write("\n".join(lines))
        changed = True
    # a struct the caller cannot name: import it next to the function's
    # own `use` (or `pub use` re-export, anywhere in the workspace)
    for m in errs:
        code = (m.get("code") or {}).get("code")
        mm = re.search(r"`(\w+Args)`", m["message"])
        if code not in ("E0422", "E0412", "E0425", "E0433") or not mm:
            continue
        fn = next((x for x in fns if x.sname == mm.group(1)), None)
        if not fn:
            continue
        targets = [m["spans"][0]["file_name"]]
        if " in crate " in m["message"]:
            reexp = re.compile(rf"(?m)^\s*pub(\([\w:]+\))? use [\w:]+(::\{{[^}}]*)?\b{fn.name}\b")
            targets = [os.path.join(d, n) for d, _, ns in os.walk("crates") for n in ns
                       if n.endswith(".rs") and reexp.search(open(os.path.join(d, n)).read())]
        for f in targets:
            text = open(f).read()
            new = re.sub(rf"(?m)^(\s*(?:pub(?:\([\w:]+\))? )?use [\w:]+::){fn.name};",
                         rf"\1{{{fn.name}, {fn.sname}}};", text)
            if new == text:
                new = re.sub(rf"(?m)^(\s*(?:pub(?:\([\w:]+\))? )?use [\w:]+::\{{[^}}]*?)\b{fn.name}\b(?![^}}]*\b{fn.sname}\b)",
                             rf"\1{fn.name}, {fn.sname}", text)
            if new != text:
                touched.setdefault(f, text)
                open(f, "w").write(new)
                changed = True
    for f, locs in sites.items():
        text = open(f).read()
        touched.setdefault(f, text)
        offs = [0]
        for l in text.split("\n"):
            offs.append(offs[-1] + len(l) + 1)
        for ln, col in sorted(locs, reverse=True):
            pos = offs[ln - 1] + col - 1
            mm = re.match(r"((?:[\w]+::)*)(\w+)", text[pos:])
            if not mm or mm.group(2) not in by:
                print(f"site? {f}:{ln}:{col} {text[pos:pos+40]!r}")
                continue
            fn = by[mm.group(2)]
            o, c = call_args(text, pos + mm.end() - 1)
            args = split_top(text[o + 1:c])
            if len(args) != len(fn.params):
                print(f"arity? {f}:{ln} {fn.name} {len(args)}!={len(fn.params)}: {args}")
                continue
            prefix = mm.group(1)
            pairs = list(zip(fn.params, args))
            fields = ", ".join(field(p[1], a) for p, a in pairs if not p[4])
            kept = "".join(f", {a}" for p, a in pairs if p[4])
            text = text[:o + 1] + f"{prefix}{fn.sname} {{ {fields} }}{kept}" + text[c:]
            changed = True
        open(f, "w").write(text)
    for f, uses in imports.items():
        text = open(f).read()
        touched.setdefault(f, text)
        lines = text.split("\n")
        at = max((i for i, l in enumerate(lines) if re.match(r"\s*use [\w:{}, *]+;$", l)), default=0)
        ind = re.match(r"\s*", lines[at]).group(0)
        for u in sorted(uses):
            if f"use {u};" not in text:
                lines.insert(at + 1, f"{ind}use {u};")
                changed = True
        open(f, "w").write("\n".join(lines))
    return changed


def main():
    a = sys.argv[1:]
    pkg = a[a.index("--crate") + 1] if "--crate" in a else None
    if "--crate" in a:
        del a[a.index("--crate"):a.index("--crate") + 2]
    path, only = a[0], set(a[1:])
    touched = {path: open(path).read()}
    fns, skipped = transform(path, only)
    for n, why in skipped:
        print(f"skipped {n}: {why}")
    if not fns:
        return
    left = []
    for _ in range(12):
        errs = check(pkg)
        if not errs:
            break
        if not fix_calls(fns, errs, touched):
            left = errs
            break
    else:
        left = check(pkg)
    if left:
        for r in [m["rendered"] for m in left][:10]:
            print(r)
        for f, t in touched.items():
            open(f, "w").write(t)
        sys.exit(f"restored {len(touched)} files")
    subprocess.run(["cargo", "fmt"], capture_output=True)
    print(f"{path}: {len(fns)} functions take a params struct: " + ", ".join(f.name for f in fns))


if __name__ == "__main__":
    main()
