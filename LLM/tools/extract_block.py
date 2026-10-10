#!/usr/bin/env python3
"""Move a statement range of a big function into a new function
(#336 Phase 6.3: `pretend::run` phases D-S and the other function cuts).

Usage: extract_block.py SRC START END FNAME TARGET_RS [--doc TEXT] [--crate PKG]
                        [--try ERR] [--ret TYPE] [--loop LABEL --indent N] [--diverges]
                        [--value LABEL TYPE PAT] [--own A,B] [--keep]

SRC lines START..END (1-based, inclusive) must be whole statements of the
host function's top-level block (4-space indent). They move verbatim into
`pub(crate) fn FNAME`, appended to TARGET_RS. A new TARGET_RS next to a
`mod.rs` host is created with `use crate::<module>::*;` and declared in the
host (`mod x;` + `pub(crate) use x::*;`). Everything about the signature
comes from the compiler:

1. inputs: with no parameters, E0425/E0423 inside the new function name
   the host locals it reads;
2. input types: with each parameter typed `()`, the call site's E0308
   labels give each type (rustc caps them per call, so known ones are
   typed and the check repeats); a body label is the fallback;
3. modes: Copy-looking types go by value, others by `&` (`&[T]`, `&str`,
   `&Path` for `Vec<T>`, `String`, `PathBuf`); E0594/E0596/E0384 make a
   parameter `&mut` (a Copy value is copied into a local of the same name
   and written back on fall-through; a non-Copy one gets `*x = ...` for
   plain assignments); E0382 in the host makes a by-value one `&`; a
   `&` one moved inside the range goes by value when the host never reads
   it again, else `&mut` + `std::mem::take` (only at a `return`);
4. outputs: `let` locals the range declares at its top level and the host
   uses after it are returned (a tuple when several) and re-bound at the
   call site under the same names (`mut`; drop the unused ones after);
   their types come from probing `let _: () = x;`;
5. more than 5 parameters (clippy too-many-arguments-threshold) become a
   `<Fname>Args` struct that the function destructures under the same
   names, so the moved body stays as it was;
6. bare type names rustc prints are imported from its own `use`
   suggestion, else from the one workspace crate that defines them.

With `--loop LABEL --indent N` the range is statements of the body of the
host's loop `'LABEL` at indent N: a `continue` to that loop becomes
`return ControlFlow::Break(())` and the call site `continue 'LABEL`s;
`return Err(..)` and `?` stay (`Result<_, ERR>`, needs `--try ERR`).
`--value LABEL TYPE PAT` (with --loop): START..END is the body of
`let PAT = 'LABEL: { .. };`; `break 'LABEL X` returns
`ControlFlow::Continue(X)` and the whole `let` becomes
`let ControlFlow::Continue(PAT) = f(..) else { continue 'loop; };`.
`--own A,B`: these parameters go by value (the range consumes them).
`--diverges`: the statement right after the range leaves the block (a
let-else's closing `continue`), so the host reads nothing the range moves.

A `return X` in the range ends the host (`--ret TYPE` names the host's
return type, default `ExitCode`): the function returns
`Option<ExitCode>` (no outputs) or `ControlFlow<ExitCode, outputs>` (needs
`use std::ops::ControlFlow;` in the host), and the call
site returns the code. With `--try ERR` (the host returns `Result<_, ERR>`),
a range using `?` returns `Result<outputs, ERR>` and the call site keeps
the `?`. On success `cargo clippy --fix` and `cargo fmt` run;
leftover errors are printed and nothing is reverted.
"""

import json
import os
import re
import subprocess
import sys

sys.path.insert(0, os.path.dirname(__file__))
from rust_items import code_mask  # noqa: E402

COPY = re.compile(
    r"^(bool|char|u8|u16|u32|u64|u128|usize|i8|i16|i32|i64|i128|isize|f32|f64"
    r"|Option<(bool|u32|u64|usize|i64|f64|char)>|&.*|Option<&.*>"
    r"|portage_repo::Deep|Deep|portage_repo::SolverKind|SolverKind"
    r"|ebuild_phases::ShellBackend|ShellBackend)$")


_derived_copy = {}


def is_copy(ty):
    """COPY's list, or a plain type name the workspace derives `Copy` for."""
    if COPY.match(ty):
        return True
    name = ty.split("::")[-1]
    if not re.fullmatch(r"[A-Z]\w*", name):
        return False
    if name not in _derived_copy:
        _derived_copy[name] = False
        for dirpath, _, files in os.walk("crates"):
            for f in files:
                if f.endswith(".rs") and re.search(
                        rf"#\[derive\([^)]*\bCopy\b[^)]*\)\]\s*(#\[[^\]]*\]\s*)*pub(\([\w:]+\))? (enum|struct) {name}\b",
                        open(os.path.join(dirpath, f)).read()):
                    _derived_copy[name] = True
    return _derived_copy[name]


def closure_spans(text, mask):
    """[start, end) of every closure body `{...}` in text (code only)."""
    code = "".join(c if m else " " for c, m in zip(text, mask))
    spans = []
    for m in re.finditer(r"(?:^|[(,=\s])(?:move\s+)?\|[^|\n]*\|\s*(?:->\s*[^{\n]+?)?\s*\{", code):
        # skip `a | b {` bit-or and `x || {`: the char before the first `|`
        # must open an expression position
        k = m.start()
        while k < len(code) and code[k] != "|":
            k += 1
        prev = code[:k].rstrip()[-1:] if code[:k].rstrip() else "("
        if prev not in "(,=>" and not code[:k].rstrip().endswith(("move", "=>")):
            continue
        b = m.end() - 1
        depth, e = 0, b
        while e < len(code):
            if code[e] == "{":
                depth += 1
            elif code[e] == "}":
                depth -= 1
                if depth == 0:
                    break
            e += 1
        spans.append((b, e))
    return spans


def wrap_returns(text, ctor):
    """`return X;` / `=> return X,` -> `return ctor(X)`; token-based. A
    `return` inside a closure body belongs to the closure and is kept."""
    mask = code_mask(text)
    inside = closure_spans(text, mask)
    out, i, n = [], 0, len(text)
    pat = re.compile(r"\breturn\b")
    while i < n:
        m = pat.search(text, i)
        if not m:
            out.append(text[i:])
            break
        k = m.start()
        if not mask[k] or any(a < k < b for a, b in inside):
            out.append(text[i:m.end()])
            i = m.end()
            continue
        j = m.end()
        while j < n and text[j] in " \n":
            j += 1
        if text[j] == ";":
            # a bare `return;` is a closure's or loop helper's, not the host's
            out.append(text[i:j])
            i = j
            continue
        depth, e = 0, j
        while e < n:
            c = text[e]
            if mask[e]:
                if c in "([{":
                    depth += 1
                elif c in ")]}":
                    if depth == 0:
                        break
                    depth -= 1
                elif c in ";," and depth == 0:
                    break
            e += 1
        out.append(text[i:j] + ctor + "(" + text[j:e].rstrip() + ")")
        i = e
    return "".join(out)


def loop_controls(text, label):
    """[start, end) of each `continue` in text that targets the host loop
    `'label` (labelled, or bare outside any loop the range opens). A `break`
    aimed at the host loop is not supported."""
    mask = code_mask(text)
    code = code_only(text)
    stack, spans, sep = [], [], -1
    tok = re.compile(r"\b(continue|break)\b(\s*'(\w+))?")
    i = 0
    while i < len(code):
        c = code[i]
        if mask[i] and c == "{":
            header = code[sep + 1:i]
            stack.append(bool(re.search(r"(^|[^\w])(loop|while)\b|(^|[^\w])for\b[^{]*\bin\b", header)))
            sep = i
        elif mask[i] and c == "}":
            if stack:
                stack.pop()
            sep = i
        elif mask[i] and c == ";":
            sep = i
        elif mask[i] and (c == "c" or c == "b") and (i == 0 or not (code[i - 1].isalnum() or code[i - 1] == "_")):
            m = tok.match(code, i)
            if m:
                lab = m.group(3)
                host = lab == label if lab else not any(stack)
                if host:
                    assert m.group(1) == "continue", f"`break` to the host loop at {i}"
                    spans.append((i, m.end()))
                i = m.end()
                continue
        i += 1
    return spans


def find_type(name, skip="portuale"):
    """`use <crate>::Name;` for a pub type defined in exactly one library
    crate of the workspace (the crates re-export their types at the root)."""
    hits = set()
    for d in os.listdir("crates"):
        if d == skip:
            continue
        for dirpath, _, files in os.walk(os.path.join("crates", d, "src")):
            for f in files:
                if f.endswith(".rs") and re.search(
                        rf"(?m)^pub (struct|enum|type|trait) {name}\b",
                        open(os.path.join(dirpath, f)).read()):
                    hits.add(d.replace("-", "_"))
    return f"use {hits.pop()}::{name};" if len(hits) == 1 else None


def pascal(name):
    return "".join(p.capitalize() for p in name.split("_"))


def lifetimed(ty):
    return re.sub(r"&(?!')", "&'a ", ty).replace("'_", "'a")


def code_only(text):
    return "".join(c if m else " " for c, m in zip(text, code_mask(text)))


class Cut:
    def __init__(self, src, start, end, fname, target, doc, pkg, err=None, ret="ExitCode",
                 loop=None, indent=4, diverges=False, value=None, own=()):
        self.src, self.start, self.end = src, start, end
        self.fname, self.target, self.doc, self.pkg = fname, target, doc, pkg
        self.orig_src = open(src).read()
        self.orig_target = open(target).read() if os.path.exists(target) else None
        self.pristine = (self.orig_src, self.orig_target)
        if self.orig_target is None and os.path.dirname(target) == os.path.dirname(src) \
                and os.path.basename(src) == "mod.rs":
            self.declare(os.path.basename(target)[:-3])
        lines = self.orig_src.split("\n")
        self.region = lines[self.start - 1:self.end]
        body = "\n".join(self.region)
        self.ind = " " * indent
        self.loop = loop.lstrip("'") if loop else None
        self.own = set(own)
        # --value LABEL TYPE PAT: START..END is the body of
        # `let PAT = 'LABEL: { ... };` (the lines right inside it); the
        # whole `let` is replaced and `break 'LABEL X` returns X
        self.value = value
        self.site = (self.start - 1, self.end + 1)
        if value:
            lines0 = self.orig_src.split("\n")
            assert re.match(rf"^\s*let .* = '{value[0]}: \{{$", lines0[self.start - 2]), lines0[self.start - 2]
            assert lines0[self.end].strip() == "};", lines0[self.end]
            assert self.loop, "--value needs --loop"
        self.ret_ty = ret
        if self.loop:
            # inside the host loop: `return Err(..)` and `?` stay as they are
            # (the function returns `Result<_, err>`), a `continue` to the
            # host loop becomes `return ControlFlow::Break(())`
            code = code_only(body)
            self.fallible = "?" in code or re.search(r"\breturn\b", code) is not None
            assert not self.fallible or err, "--loop with `?`/`return` needs --try ERR"
            assert not re.search(r"\breturn\s+(?!Err\()", code), "only `return Err(..)` is kept"
            ctl = loop_controls(body, self.loop)
            self.ctl = bool(ctl)
            brk = "return Ok(ControlFlow::Break(()))" if self.fallible else "return ControlFlow::Break(())"
            for a, b in reversed(ctl):
                body = body[:a] + brk + body[b:]
            if self.value:
                ok = (lambda x: f"Ok({x})") if self.fallible else (lambda x: x)
                body, n = re.subn(rf"\bbreak '{self.value[0]}\s+(.*?);",
                                  lambda m: "return " + ok(f"ControlFlow::Continue({m.group(1)})") + ";", body)
                assert n and f"'{self.value[0]}" not in code_only(body), "multi-line `break 'label` value"
                # a nested branch's tail expression that is the block value
                body = re.sub(rf"(?m)^(\s+){re.escape(self.value[2])}$",
                              lambda m: m.group(1) + "return " + ok(f"ControlFlow::Continue({self.value[2]})"),
                              body)
                self.ctl = True
            self.returns = False
            self.loop_err = err
        self.raw_body = body
        if not self.loop:
            self.returns = wrap_returns(body, "Some") != body
        # `?` in the range: the function returns `Result<_, err>` and the
        # call site keeps the `?` (needs --try ERR, the host's error type)
        self.err = err if err and "?" in code_only(body) and not self.loop else None
        assert not (self.err and self.returns), "`return` and `?` in one range"
        # outputs: top-level `let` names of the range read after it
        host_end = next(i for i in range(self.end, len(lines)) if lines[i] == "}")
        after = code_only("\n".join(lines[self.end:host_end]))
        if diverges:
            # the range is followed by `continue`/`return`: nothing after
            # it in the host reads what it binds or moves
            after = ""
        self.after = after
        names = []
        for l in self.region:
            m = re.match(rf"^{self.ind}let (?:mut )?([a-z_]\w*)\b(?!\()", l)
            if m and m.group(1) not in names and re.search(rf"(?<![\w.]){m.group(1)}\b", after):
                names.append(m.group(1))
        self.outs = [] if value else [[n, None] for n in names]
        self.params = []          # [name, type, mode] mode: val|ref|mut
        self.imports = []         # `use` lines for bare type names

    def declare(self, stem):
        """Add `mod stem;` + `pub(crate) use stem::*;` to the host mod.rs,
        shifting START/END by the lines added above them."""
        lines = self.orig_src.split("\n")

        def place(pattern, text):
            idx = [i for i, l in enumerate(lines) if re.match(pattern, l)
                   and not lines[i - 1].startswith("#[")]
            after = [i for i in idx if lines[i] < text]
            lines.insert((after[-1] if after else idx[0] - 1) + 1, text)
            return idx

        if not any(re.match(r"^mod \w+;$", l) for l in lines):
            # first child module: `mod` + glob re-export after the imports
            at = max(i for i, l in enumerate(lines[:self.start]) if l.startswith("use ")) + 1
            lines[at:at] = ["", f"mod {stem};", f"pub(crate) use {stem}::*;"]
            self.start += 3
            self.end += 3
            self.orig_src = "\n".join(lines)
            return
        uses = place(r"^pub(\(crate\))? use \w+::\*;$", f"pub(crate) use {stem}::*;")
        place(r"^mod \w+;$", f"mod {stem};")
        assert uses[-1] + 1 < self.start - 1
        self.start += 2
        self.end += 2
        self.orig_src = "\n".join(lines)

    def module_path(self):
        d = os.path.dirname(self.src)
        return d[d.index("/src") + 5:].replace("/", "::")

    def ret_type(self, typed):
        if self.loop:
            ts = [t if typed and t else "()" for _, t in self.outs]
            o = None if not ts else ts[0] if len(ts) == 1 else pascal(self.fname) + "Out"
            inner = (f"ControlFlow<(), {o}>" if o else "ControlFlow<()>") if self.ctl else o
            if self.value:
                inner = f"ControlFlow<(), {self.value[1]}>"
            return f"Result<{inner or '()'}, {self.loop_err}>" if self.fallible else inner
        if self.err:
            ts = [t if typed and t else "()" for _, t in self.outs]
            o = "()" if not ts else ts[0] if len(ts) == 1 else pascal(self.fname) + "Out"
            return f"Result<{o}, {self.err}>"
        if self.outs:
            ts = [t if typed and t else "()" for _, t in self.outs]
            o = ts[0] if len(ts) == 1 else pascal(self.fname) + "Out"
            return f"ControlFlow<{self.ret_ty}, {o}>" if self.returns else o
        return f"Option<{self.ret_ty}>" if self.returns else None

    def render(self, typed, probe=False):
        lines = self.orig_src.split("\n")
        ps = self.params
        copy_mut = {n for n, t, m in ps if typed and m == "mut" and is_copy(t)}
        use_struct = len(ps) > 5 and typed
        sname = pascal(self.fname) + "Args"
        if use_struct:
            call_args = sname + " {\n" + "".join(
                f"            {n}{'' if m in ('val', 'own') else ': &mut ' + n if m == 'mut' else ': &' + n},\n"
                for n, _, m in ps) + "        }"
        else:
            call_args = ", ".join(
                n if m in ("val", "own") or not typed else ("&mut " + n if m == "mut" else "&" + n)
                for n, _, m in ps)
        call = f"{self.fname}({call_args})"
        # the early-return value is named after its type: ExitCode -> code
        rv = re.findall(r"[A-Z][a-z]*", self.ret_ty)[-1].lower()
        onames = [n for n, _ in self.outs]
        oname = pascal(self.fname) + "Out"
        pat = (onames[0] if len(onames) == 1 else oname + " { " + ", ".join(onames) + " }") if onames else None
        mpat = (("mut " + onames[0]) if len(onames) == 1
                else oname + " { " + ", ".join("mut " + n for n in onames) + " }") if onames else None
        ind = self.ind
        if self.loop:
            ce = call + ("?" if self.fallible else "")
            if self.value:
                site = [f"{ind[:-4]}let ControlFlow::Continue({self.value[2]}) = {ce} else {{",
                        f"{ind[:-4]}    continue '{self.loop};", f"{ind[:-4]}}};"]
            elif self.ctl and onames:
                site = [f"{ind}let ControlFlow::Continue({mpat}) = {ce} else {{",
                        f"{ind}    continue '{self.loop};", f"{ind}}};"]
            elif self.ctl:
                site = [f"{ind}if {ce}.is_break() {{", f"{ind}    continue '{self.loop};", f"{ind}}}"]
            elif onames:
                site = [f"{ind}let {mpat} = {ce};"]
            else:
                site = [f"{ind}{ce};"]
        elif self.err:
            site = [f"    let {mpat} = {call}?;"] if onames else [f"    {call}?;"]
        elif onames and self.returns:
            site = [f"    let {mpat} = match {call} {{", "        ControlFlow::Continue(out) => out,",
                    f"        ControlFlow::Break({rv}) => return {rv},", "    };"]
        elif onames:
            site = [f"    let {mpat} = {call};"]
        elif self.returns:
            site = [f"    if let Some({rv}) = {call} {{", f"        return {rv};", "    }"]
        else:
            site = [f"    {call};"]
        if self.value:
            new_src = lines[:self.start - 2] + site + lines[self.end + 1:]
        else:
            new_src = lines[:self.start - 1] + site + lines[self.end:]
        rt = self.ret_type(typed)
        ret = f" -> {rt}" if rt else ""
        writeback = "".join(f"    *{n}_out = {n};\n" for n, _, _ in ps if n in copy_mut)
        if probe:
            tail = "\n" + "".join(f"    let _: () = {n};\n" for n in onames) + "    todo!()\n"
        elif self.value:
            tail = "\n" + writeback
        elif self.loop:
            val = pat
            if self.ctl:
                val = f"ControlFlow::Continue({pat or '()'})"
            if self.fallible:
                val = f"Ok({val or '()'})"
            tail = "\n" + writeback + (f"    {val}\n" if val else "")
        elif self.err:
            tail = "\n" + writeback + "    Ok(" + (pat or "()") + ")\n"
        elif probe:
            tail = "\n" + "".join(f"    let _: () = {n};\n" for n in onames) + "    todo!()\n"
        elif onames:
            tail = "\n" + writeback + ("    ControlFlow::Continue(" + pat + ")\n" if self.returns else "    " + pat + "\n")
        elif self.returns:
            tail = "\n" + writeback + "    None\n"
        else:
            tail = "\n" + writeback
        body = wrap_returns(self.raw_body, "ControlFlow::Break" if onames else "Some") if self.returns else self.raw_body

        def ptype(t, m):
            if not typed:
                return "()"
            if m == "ref":
                t = re.sub(r"^Vec<(.*)>$", r"[\1]", t)
                t = {"String": "str", "PathBuf": "Path",
                     "std::path::PathBuf": "std::path::Path"}.get(t, t)
            return t if m in ("val", "own") else ("&mut " + t if m == "mut" else "&" + t)
        struct = ""
        if len(onames) > 1:
            struct += (f"/// What [`{self.fname}`] hands back to its caller.\n"
                       f"pub(crate) struct {oname} {{\n" + "".join(
                           f"    pub(crate) {n}: {t if typed and t else '()'},\n" for n, t in self.outs)
                       + "}\n\n")
        if use_struct:
            fields_txt = "".join(f"    pub(crate) {n}: {lifetimed(ptype(t, m))},\n" for n, t, m in ps)
            lt = "'a" in fields_txt
            struct += (f"/// The values [`{self.fname}`] reads from its caller.\n"
                       f"pub(crate) struct {sname}{'<' + chr(39) + 'a>' if lt else ''} {{\n"
                       + fields_txt + "}\n\n")
            sig = f"pub(crate) fn {self.fname}(args: {sname}{'<' + chr(39) + '_>' if lt else ''}){ret}"
            fields = ", ".join(n if n not in copy_mut else f"{n}: {n}_out" for n, _, _ in ps)
            prologue = f"    let {sname} {{ {fields} }} = args;\n"
        else:
            sig = f"pub(crate) fn {self.fname}(" + ", ".join(
                f"{n + '_out' if n in copy_mut else n}: {ptype(t, m)}" for n, t, m in ps) + f"){ret}"
            prologue = ""
        prologue += "".join(f"    let mut {n} = *{n}_out;\n" for n, _, _ in ps if n in copy_mut)
        if typed:
            for n, t, m in ps:
                conv = ("to_vec" if t.startswith("Vec<") else "to_path_buf" if t.endswith("PathBuf")
                        else "to_string" if t == "String" else None)
                if m == "ref" and conv:
                    body = re.sub(rf"(?<![\w.]){n}\.clone\(\)", f"{n}.{conv}()", body)
                    view = {"to_vec": "as_slice", "to_path_buf": "as_path", "to_string": "as_str"}[conv]
                    body = re.sub(rf"(?<![\w.]){n}\.{view}\(\)", n, body)
            for n, _, m in ps:
                if m == "mut" and n not in copy_mut:
                    body = re.sub(rf"(?m)^(\s*){n}(\s*[+\-]?=[^=])", rf"\1*{n}\2", body)
                    body = re.sub(rf"&mut\s+{n}\b(?![.\[])", f"&mut *{n}", body)
        doc = f"/// {self.doc}\n" if self.doc else ""
        fn = f"{struct}{doc}{sig} {{\n{prologue}{body}{tail}}}\n"
        glob = f"use crate::{self.module_path()}::*;\n"
        base = self.orig_target
        if base is None:
            base = (f"//! {self.doc}\n\n" if self.doc else "") + glob
        missing = [u for u in self.imports if u not in base]
        if missing:
            base = base.replace(glob, glob + "".join(u + "\n" for u in missing), 1)
        open(self.src, "w").write("\n".join(new_src))
        open(self.target, "w").write(base.rstrip("\n") + "\n\n" + fn)

    def check(self):
        p = subprocess.run(["cargo", "check", "--release", "--all-targets", "-p", self.pkg,
                            "--message-format=json"], capture_output=True, text=True)
        msgs = []
        for l in p.stdout.splitlines():
            try:
                m = json.loads(l)
            except ValueError:
                continue
            if m.get("reason") == "compiler-message" and m["message"]["level"] == "error":
                msgs.append(m["message"])
        return msgs

    def fn_start(self):
        t = open(self.target).read().split("\n")
        return next(i for i, l in enumerate(t)
                    if re.match(rf"pub\(crate\) fn {self.fname}\(", l)) + 1

    @staticmethod
    def labels(m):
        """(span, expression text, label) for the labelled spans of an error
        and of its child notes (rustc moves per-argument labels there when
        several arguments of one call mismatch)."""
        for s in m["spans"]:
            if s.get("label") and s.get("text"):
                t0 = s["text"][0]
                expr = t0["text"][t0["highlight_start"] - 1:t0["highlight_end"] - 1]
                yield s, expr, s["label"]
        for c in m.get("children", []):
            for s in c.get("spans", []):
                if s.get("text"):
                    t0 = s["text"][0]
                    expr = t0["text"][t0["highlight_start"] - 1:t0["highlight_end"] - 1]
                    yield s, expr, c["message"]

    def mode_fixes(self, errs):
        """Turn the parameters rustc says are written or moved into `&mut`
        or `&`; True when a mode changed."""
        changed = False
        for m in errs:
            code = (m.get("code") or {}).get("code")
            mm = re.search(r"`(\w+)", m["message"])
            p = next((p for p in self.params if mm and p[0] == mm.group(1)), None)
            if p and code in ("E0594", "E0596", "E0384") and p[2] != "mut":
                p[2] = "mut"
                changed = True
            elif p and code == "E0507" and p[2] in ("ref", "mut") \
                    and not re.search(rf"(?<![\w.]){p[0]}\b", self.after):
                # moved out of (a field of) a borrowed parameter the host
                # never reads again: hand the whole value over
                p[2] = "own"
                changed = True
            elif p and code == "E0382" and p[2] == "val":
                p[2] = "ref"
                changed = True
            elif code == "E0308":
                # a `&` parameter moved into a returned value: make it
                # `&mut` so apply_suggestions can `std::mem::take` it
                for sp in m["spans"]:
                    t0 = (sp.get("text") or [{}])[0]
                    expr = t0.get("text", "")[t0.get("highlight_start", 1) - 1:t0.get("highlight_end", 1) - 1]
                    q = next((q for q in self.params if q[0] == expr and q[2] in ("ref", "mut")), None)
                    if not (sp.get("is_primary") and q and re.search(r"found `&", sp.get("label") or "")):
                        continue
                    if not re.search(rf"(?<![\w.]){expr}\b", self.after):
                        # the host never reads it again: hand it over
                        q[2] = "own"
                        changed = True
                    elif q[2] == "ref":
                        q[2] = "mut"
                        changed = True
        return changed

    def add_imports(self, errs):
        added = False
        for m in errs:
            mt = re.search(r"cannot find type `(\w+)`", m["message"])
            if mt:
                mm = re.search(r"^\s*\d*\s*\+ (use [\w:]+;)$", m["rendered"], re.M)
                u = mm.group(1) if mm else find_type(mt.group(1), self.pkg)
                if u and u not in self.imports:
                    self.imports.append(u)
                    added = True
        return added

    def apply_suggestions(self, errs):
        """Apply the remove-`&` / add-`*` suggestions rustc attaches to errors
        in TARGET_RS; the body text changes, so the edit goes to raw_body."""
        tgt = os.path.abspath(self.target)
        text = open(self.target).read().split("\n")
        edits = []
        for m in errs:
            for c in m.get("children", []):
                if not re.search(r"consider (removing the leading `&`-reference|dereferencing the borrow"
                                 r"|removing the borrow)", c["message"]):
                    continue
                for s in c["spans"]:
                    if os.path.abspath(s["file_name"]) != tgt or s.get("suggested_replacement") is None:
                        continue
                    if s["line_start"] != s["line_end"]:
                        continue
                    edits.append((s["line_start"], s["column_start"], s["column_end"],
                                  s["suggested_replacement"]))
        # a `&mut` parameter moved into the value a `return` hands back
        # (`Box::new(grown)`): the host returns right after, so taking it
        # leaves nothing the host still reads
        muts = {n for n, _, m in self.params if m == "mut"}
        for m in errs:
            if (m.get("code") or {}).get("code") != "E0308":
                continue
            for s in m["spans"]:
                if not s.get("is_primary") or os.path.abspath(s["file_name"]) != tgt \
                        or s["line_start"] != s["line_end"]:
                    continue
                expr = s["text"][0]["text"][s["text"][0]["highlight_start"] - 1:
                                            s["text"][0]["highlight_end"] - 1]
                if expr in muts and re.search(r"found `&mut ", s.get("label") or ""):
                    line = text[s["line_start"] - 1]
                    short = re.fullmatch(rf"\s*{expr},?\s*", line) is not None
                    edits.append((s["line_start"], s["column_start"], s["column_end"],
                                  (f"{expr}: " if short else "") + f"std::mem::take({expr})"))
        # `Vec<String>::contains(&x)` with `x` now a `&str` parameter
        strs = {n for n, t, m in self.params if m == "ref" and t == "String"}
        for m in errs:
            if (m.get("code") or {}).get("code") != "E0308" or "found `&&str`" not in m["rendered"]:
                continue
            for s in m["spans"]:
                if not s.get("is_primary") or os.path.abspath(s["file_name"]) != tgt:
                    continue
                line = text[s["line_start"] - 1]
                for n in strs:
                    k = line.find(f".contains(&{n})")
                    if k >= 0:
                        edits.append((s["line_start"], k + 1, k + len(f".contains(&{n})") + 1,
                                      f".iter().any(|s| s == {n})"))
        if not edits:
            return False
        for ln, a, b, rep in sorted(set(edits), key=lambda e: (e[0], -e[1])):
            line = text[ln - 1]
            text[ln - 1] = line[:a - 1] + rep + line[b - 1:]
        open(self.target, "w").write("\n".join(text))
        # keep raw_body in sync: re-read the fn body from the file
        t = "\n".join(text)
        start = t.index(f"fn {self.fname}(")
        open_brace = t.index("{\n", t.index(")", start))
        self.patched_target = t
        return True

    def run(self):
        tgt = os.path.abspath(self.target)
        # 1. inputs
        self.render(typed=False)
        fstart = self.fn_start()
        names = []
        for m in self.check():
            code = (m.get("code") or {}).get("code")
            for s in m["spans"]:
                if not (s["is_primary"] and os.path.abspath(s["file_name"]) == tgt
                        and s["line_start"] >= fstart):
                    continue
                mm = None
                if code == "E0425":
                    mm = re.search(r"cannot find (value|function) `(\w+)`", m["message"])
                    if mm and mm.group(1) == "function":
                        sys.exit(f"needs a host-local fn: {mm.group(2)}")
                    mm = mm and mm.group(2)
                elif code == "E0423":
                    mm = re.search(r"expected value, found (?:module|crate) `(\w+)`", m["message"])
                    mm = mm and mm.group(1)
                if mm and mm not in names:
                    names.append(mm)
        self.params = [[n, "()", "val"] for n in names]
        # 2. input types
        types, fallback = {}, {}
        for _ in range(12):
            for p in self.params:
                p[1], p[2] = types.get(p[0], "()"), "val"
            self.render(typed=True)
            before = len(types)
            errs = self.check()
            added = self.add_imports(errs)
            for m in errs:
                if (m.get("code") or {}).get("code") != "E0308":
                    continue
                for s, expr, label in self.labels(m):
                    mm = re.search(r"expected `\(\)`, found (?:[a-z]+ )?`(.+)`$", label)
                    if mm and os.path.abspath(s["file_name"]) == os.path.abspath(self.src):
                        types.setdefault(expr, mm.group(1))
                    mm = re.search(r"expected (?:[a-z]+ )?`(.+)`, found `\(\)`$", label)
                    if mm:
                        fallback.setdefault(expr, mm.group(1))
            if all(p[0] in types for p in self.params) or (len(types) == before and not added):
                break
        for p in self.params:
            t = types.get(p[0]) or fallback.get(p[0])
            if t is None:
                sys.exit(f"no type for {p[0]}: types={types} fallback={fallback}")
            p[1], p[2] = t, ("val" if is_copy(t) else "own" if p[0] in self.own else "ref")
            # mutated in the body: a mutating method, `&mut x`, or an assignment
            code = code_only(self.raw_body)
            n = re.escape(p[0])
            if re.search(rf"(?<![\w.]){n}\s*\.\s*(push|push_str|extend|insert|clear|retain|sort|sort_by"
                         rf"|sort_by_key|sort_unstable|dedup|truncate|remove|append|drain|pop|entry"
                         rf"|get_mut|iter_mut|swap|reverse)\b", code) \
                    or re.search(rf"&mut\s+{n}\b", code) \
                    or re.search(rf"(?m)^\s*{n}\s*([+\-*/|&]?=)(?!=)", code):
                p[2] = "mut"
        # 3. output types
        if self.outs:
            self.render(typed=True, probe=True)
            for m in self.check():
                for s, expr, label in self.labels(m):
                    mm = re.search(r"expected `\(\)`, found (?:[a-z]+ )?`(.+)`$", label)
                    for o in self.outs:
                        if mm and expr == o[0] and o[1] is None:
                            o[1] = mm.group(1)
            for o in self.outs:
                if o[1] is None:
                    sys.exit(f"no type for output {o[0]}")
        # 4-5 repeat: borrow errors (E0594/E0596) only show once the type
        # errors step 5 fixes are gone, and a new mode re-renders the body
        for _round in range(3):
            # 4. modes and imports
            for _ in range(8):
                self.render(typed=True)
                errs = self.check()
                changed = self.add_imports(errs)
                changed = self.mode_fixes(errs) or changed
                if not changed:
                    break
            # 5. rustc's own fixes inside the new fn: `&x` -> `x` where `x` is
            # already a reference, `x` -> `*x` where a value is expected
            for _ in range(6):
                errs = self.check()
                if not self.apply_suggestions(errs):
                    break
            if not self.mode_fixes(self.check()):
                break
        left = [m["rendered"] for m in self.check()]
        print(f"{self.fname}: {len(self.params)} params "
              + ", ".join(f"{n}:{m}" for n, _, m in self.params)
              + (f"; returns {', '.join(f'{n}: {t}' for n, t in self.outs)}" if self.outs else ""))
        if left:
            print(f"{len(left)} compile errors left:")
            for r in left[:15]:
                print(r)
            sys.exit(1)
        subprocess.run(["cargo", "clippy", "--fix", "--allow-dirty", "--allow-staged", "--release",
                        "--all-targets", "-p", self.pkg], capture_output=True)
        subprocess.run(["cargo", "fmt"], capture_output=True)


def main():
    a = sys.argv[1:]
    doc = a[a.index("--doc") + 1] if "--doc" in a else ""
    pkg = a[a.index("--crate") + 1] if "--crate" in a else "portuale"
    err = a[a.index("--try") + 1] if "--try" in a else None
    ret = a[a.index("--ret") + 1] if "--ret" in a else "ExitCode"
    loop = a[a.index("--loop") + 1] if "--loop" in a else None
    indent = int(a[a.index("--indent") + 1]) if "--indent" in a else 4
    value = tuple(a[a.index("--value") + 1:a.index("--value") + 4]) if "--value" in a else None
    own = a[a.index("--own") + 1].split(",") if "--own" in a else ()
    cut = Cut(a[0], int(a[1]), int(a[2]), a[3], a[4], doc, pkg, err, ret, loop, indent,
              "--diverges" in a, value, own)
    try:
        cut.run()
    except SystemExit as e:
        if e.code not in (None, 0) and "--keep" not in a:
            src, tgt = cut.pristine
            open(cut.src, "w").write(src)
            if tgt is None:
                if os.path.exists(cut.target):
                    os.remove(cut.target)
            else:
                open(cut.target, "w").write(tgt)
            print("restored the original files (pass --keep to inspect the failed state)")
        raise


if __name__ == "__main__":
    main()
