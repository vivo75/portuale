#!/usr/bin/env python3
"""Keep `EmergeOpts` whole in `pretend::run` (#336 decision 33).

Usage: consolidate_opts.py run      -- step 1: `run` reads `opts.x`
       consolidate_opts.py structs  -- step 2: Args structs take `opts`
       consolidate_opts.py unstruct -- step 3: <= 5 fields: plain parameters
A second argument names the struct kept whole: `opts` (EmergeOpts, the
default) or `plan` (ResolvePlan).

Step 1 replaces `let EmergeOpts { .. } = match parse_emerge_args(..)`
with `let mut opts = ..` and lets the compiler name every use left
unbound (E0425): each becomes `opts.x` (`x: opts.x` in a struct
literal). A name a later `let` shadows still binds there, so only the
uses that meant the option change.

Step 2: in each `<Fname>Args` struct, the fields the call in `run` fills
with `opts.x` / `&opts.x` / `&mut opts.x` go; the struct gets one
`opts` field and the function destructures it, so its body is unchanged.
"""

import json
import os
import re
import subprocess
import sys

sys.path.insert(0, os.path.dirname(__file__))
import extract_block as xb  # noqa: E402

D = "crates/portuale/src/pretend/"
MOD = D + "mod.rs"


# var -> (type, file, lifetime-generic, `let` it replaces in run)
WHOLE = {
    "opts": ("EmergeOpts", "args.rs", True, r"    let EmergeOpts \{.*?\} = match parse_emerge_args\(args\) \{",
             "    let mut opts = match parse_emerge_args(args) {"),
    "plan": ("ResolvePlan", "resolve_plan.rs", False, r"    let ResolvePlan \{.*?\} = match plan_resolve\(",
             "    let plan = match plan_resolve("),
}
VAR = "opts"


def opts_fields(var=None):
    ty, f, _, _, _ = WHOLE[var or VAR]
    a = open(D + f).read()
    m = re.search(rf"pub\(crate\) struct {ty}(<'a>)? \{{(.*?)\n\}}", a, re.S)
    return dict(re.findall(r"pub\(crate\) (\w+): ([^\n]+),", m.group(2)))


def check():
    p = subprocess.run(["cargo", "check", "--release", "--all-targets", "-p", "portuale",
                        "--message-format=json"], capture_output=True, text=True)
    out = []
    for l in p.stdout.splitlines():
        try:
            m = json.loads(l)
        except ValueError:
            continue
        if m.get("reason") == "compiler-message" and m["message"]["level"] == "error":
            out.append(m["message"])
    return out


def opener(code, pos):
    """The innermost bracket still open at pos."""
    depth = 0
    for i in range(pos - 1, -1, -1):
        c = code[i]
        if c in ")]}":
            depth += 1
        elif c in "([{":
            if depth == 0:
                return c
            depth -= 1
    return None


def step_run():
    fields = opts_fields()
    s = open(MOD).read()
    _, _, _, pat, repl = WHOLE[VAR]
    s, n = re.subn(pat, repl, s, flags=re.S)
    assert n == 1
    open(MOD, "w").write(s)
    for _ in range(30):
        errs = check()
        edits = []
        for m in errs:
            if (m.get("code") or {}).get("code") not in ("E0425", "E0423"):
                continue
            mm = re.search(r"cannot find value `(\w+)`|expected value, found (?:module|crate|function) `(\w+)`",
                           m["message"])
            if mm:
                mm = re.match(r"(\w+)", mm.group(1) or mm.group(2))
            if not mm or mm.group(1) not in fields:
                continue
            for sp in m["spans"]:
                if sp["is_primary"] and sp["file_name"].endswith("pretend/mod.rs"):
                    edits.append((sp["line_start"], sp["column_start"], sp["column_end"], mm.group(1)))
        if not edits:
            break
        text = open(MOD).read()
        code = xb.code_only(text)
        offs = [0]
        for l in text.split("\n"):
            offs.append(offs[-1] + len(l) + 1)
        lines = text.split("\n")
        for ln, a, b, name in sorted(set(edits), key=lambda e: (e[0], -e[1])):
            line = lines[ln - 1]
            short = re.fullmatch(rf"\s*{name},?\s*", line) is not None and opener(code, offs[ln - 1]) == "{"
            rep = f"{name}: {VAR}.{name}" if short else f"{VAR}.{name}"
            lines[ln - 1] = line[:a - 1] + rep + line[b - 1:]
        open(MOD, "w").write("\n".join(lines))
    left = [m["rendered"] for m in check()]
    print(f"{len(left)} errors left")
    for r in left[:12]:
        print(r)


def step_structs():
    fields = opts_fields()
    mod = open(MOD).read()
    files = {f: open(D + f).read() for f in os.listdir(D) if f.endswith(".rs") and f != "mod.rs"}
    for lit in re.finditer(r"(\w+)\((\w+Args) \{\n(.*?)\n(\s*)\}\)", mod, re.S):
        fname, sname, body = lit.group(1), lit.group(2), lit.group(3)
        taken, muts = [], []
        for fm in re.finditer(rf"(?m)^\s*(\w+): (&(?:mut )?)?{VAR}\.(\w+),$", body):
            if fm.group(1) == fm.group(3) and fm.group(1) in fields:
                taken.append(fm.group(1))
                if fm.group(2) == "&mut ":
                    muts.append(fm.group(1))
        if not taken:
            continue
        src = next(f for f, t in files.items() if re.search(rf"pub\(crate\) struct {sname}\b", t))
        t = files[src]
        # struct definition
        sd = re.search(rf"pub\(crate\) struct {sname}(<[^>]*>)? \{{\n(.*?)\n\}}", t, re.S)
        kept = [l for l in sd.group(2).split("\n")
                if not re.match(rf"\s*pub\(crate\) ({'|'.join(taken)}):", l)]
        ty, _, lt, _, _ = WHOLE[VAR]
        old = sd.group(1) or ""
        if muts:
            gen = "<'a, 'o>"
            ofield = f"    pub(crate) {VAR}: &'a mut {ty}{'<' + chr(39) + 'o>' if lt else ''},"
        else:
            gen = "<'a, 'o>" if "'o" in old else "<'a>"
            ofield = f"    pub(crate) {VAR}: &'a {ty}{'<' + chr(39) + 'a>' if lt else ''},"
        t = t[:sd.start()] + f"pub(crate) struct {sname}{gen} {{\n" + "\n".join([ofield] + kept) + "\n}" + t[sd.end():]
        nlt = gen.count("'")
        t = re.sub(rf"(args: {sname})(<[^>]*>)?", rf"\1<{', '.join([chr(39) + '_'] * nlt)}>", t)
        # destructuring in the function
        dm = re.search(rf"let {sname} \{{(.*?)\}} = args;", t, re.S)
        items = [x.strip() for x in dm.group(1).split(",") if x.strip()]
        alias = {}
        rest = []
        for it in items:
            n, _, a = it.partition(":")
            n = n.strip()
            if n in taken:
                alias[n] = a.strip() or n
            else:
                rest.append(it)
        pats = []
        for n in taken:
            a = alias.get(n, n)
            if n in muts:
                pats.append(f"{n}: ref mut {a}" if a != n else f"ref mut {n}")
            elif xb.is_copy(fields[n]):
                pats.append(n if a == n else f"{n}: {a}")
            else:
                pats.append(f"ref {n}" if a == n else f"{n}: ref {a}")
        amp = "&mut " if muts else "&"
        new = (f"let {sname} {{ {', '.join([VAR] + rest)} }} = args;\n"
               f"    let {amp}{ty} {{ {', '.join(pats)}, .. }} = {VAR};")
        t = t[:dm.start()] + new + t[dm.end():]
        files[src] = t
        # call site
        nb = "\n".join(l for l in body.split("\n")
                       if not re.match(rf"\s*({'|'.join(taken)}): &?(mut )?{VAR}\.\w+,$", l))
        ind = re.match(r"\s*", body).group(0)
        nb = f"{ind}{VAR}: {'&mut ' if muts else '&'}{VAR},\n" + nb
        mod = mod.replace(lit.group(0), f"{fname}({sname} {{\n{nb}\n{lit.group(4)}}})", 1)
        print(f"{sname}: {len(taken)} fields -> {VAR}{' (mut: ' + ','.join(muts) + ')' if muts else ''}")
    open(MOD, "w").write(mod)
    for f, t in files.items():
        if t != open(D + f).read():
            open(D + f, "w").write(t)
    subprocess.run(["cargo", "fmt"])
    left = [m["rendered"] for m in check()]
    print(f"{len(left)} errors left")
    for r in left[:20]:
        print(r)


def unlifetime(ty):
    """A struct field type as a plain parameter type: `&'a T` -> `&T`,
    `X<'a>` -> `X<'_>`."""
    ty = re.sub(r"&'\w+ ", "&", ty)
    return re.sub(r"'(?!_)\w+", "'_", ty)


def step_unstruct(limit=5):
    """Args structs left with <= limit fields become plain parameters."""
    mod = open(MOD).read()
    for f in sorted(os.listdir(D)):
        if not f.endswith(".rs") or f == "mod.rs":
            continue
        t = open(D + f).read()
        for sd in list(re.finditer(r"(/// The values \[`\w+`\] reads from its caller\.\n)?"
                                   r"pub\(crate\) struct (\w+Args)(<[^>]*>)? \{\n(.*?)\n\}\n\n", t, re.S)):
            sname = sd.group(2)
            flds = re.findall(r"pub\(crate\) (\w+): ([^\n]+),", sd.group(4))
            if len(flds) > limit:
                continue
            t = t.replace(sd.group(0), "", 1)
            dm = re.search(rf"let {sname} \{{(.*?)\}} = args;\n", t, re.S)
            alias = {}
            for it in [x.strip() for x in dm.group(1).split(",") if x.strip()]:
                n, _, a = it.partition(":")
                alias[n.strip()] = a.strip() or n.strip()
            t = t[:dm.start()] + t[dm.end():]
            params = ", ".join(f"{alias[n]}: {unlifetime(ty)}" for n, ty in flds)
            t, k = re.subn(rf"\(\s*args: {sname}(<[^>]*>)?,?\s*\)", f"({params})", t)
            assert k == 1, sname
            # call site
            cm = re.search(rf"\({sname} \{{(.*?)\}}\)", mod, re.S)
            vals = {}
            for it in [x.strip() for x in cm.group(1).split(",") if x.strip()]:
                n, _, v = it.partition(":")
                vals[n.strip()] = v.strip() or n.strip()
            mod = mod[:cm.start()] + "(" + ", ".join(vals[n] for n, _ in flds) + ")" + mod[cm.end():]
            print(f"{sname}: {len(flds)} plain parameters")
        open(D + f, "w").write(t)
    open(MOD, "w").write(mod)
    subprocess.run(["cargo", "fmt"])
    left = [m["rendered"] for m in check()]
    print(f"{len(left)} errors left")
    for r in left[:12]:
        print(r)


if __name__ == "__main__":
    if len(sys.argv) > 2:
        VAR = sys.argv[2]
    {"run": step_run, "structs": step_structs, "unstruct": step_unstruct}[sys.argv[1]]()
