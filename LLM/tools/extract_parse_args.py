#!/usr/bin/env python3
"""Extract `pretend::run`'s option state + parse loop (#336 Phase 6,
split-map 3.2 phases B and C) into `pretend/args.rs`.

Usage: extract_parse_args.py MOD_RS FIRST_LINE LAST_LINE

FIRST_LINE..LAST_LINE (1-based, inclusive) is the region of `run` from the
first option `let mut` to the closing `}` of the `while i < args.len()`
loop. The region moves verbatim into `parse_emerge_args(args)`, with each
`return X` turned into `return Err(X)`. Every function-level `let mut`
local of the region that `run` reads after the region becomes a field of
`EmergeOpts`; `run` gets them back under the same names by destructuring,
so nothing after the region changes. Fields are all bound `mut`; the
compiler's unused_mut warnings say which `mut`s to drop afterwards.
"""

import os
import re
import sys

LET = re.compile(r"^    let mut (\w+)(?::\s*([^=]+?))?\s*=\s*(.*);$")
RET = re.compile(r"^(\s*(?:.*=>\s*)?)return (.+?)([;,])\s*$")


def infer(init):
    if init in ("true", "false"):
        return "bool"
    if init == "portage_repo::SolverKind::Portage":
        return "portage_repo::SolverKind"
    if init == "portage_repo::Deep::NotRequested":
        return "portage_repo::Deep"
    if init == "ebuild_phases::ShellBackend::default()":
        return "ebuild_phases::ShellBackend"
    return None


def code_part(line):
    i = line.find("//")
    return line if i < 0 else line[:i]


def main(path, first, last):
    lines = open(path).read().split("\n")
    region = lines[first - 1:last]
    assert region[0].startswith("    let mut "), region[0]
    assert region[-1] == "    }", region[-1]
    # end of `run`: the first column-0 `}` after the region
    run_end = next(i for i in range(last, len(lines)) if lines[i] == "}")
    after = "\n".join(code_part(l) for l in lines[last:run_end])
    fields = []
    for l in region:
        m = LET.match(l)
        if not m:
            continue
        name, ty, init = m.group(1), m.group(2), m.group(3)
        if name == "i" or not re.search(rf"(?<![\w.]){name}\b", after):
            continue
        ty = ty.strip() if ty else infer(init)
        if ty is None:
            sys.exit(f"no type for {name} = {init}")
        fields.append((name, ty.replace("Vec<&str>", "Vec<&'a str>")))
    body = []
    for l in region:
        c = code_part(l)
        m = RET.match(c)
        if m and "return" in c:
            l = f"{m.group(1)}return Err({m.group(2)}){m.group(3)}" + l[len(c):]
        body.append(l)
    names = [n for n, _ in fields]
    struct = (
        "/// The options `run` reads, parsed from argv (`EMERGE_DEFAULT_OPTS`\n"
        "/// first, so argv wins for last-wins options).\n"
        "pub(crate) struct EmergeOpts<'a> {\n"
        + "".join(f"    pub(crate) {n}: {t},\n" for n, t in fields)
        + "}\n"
    )
    func = (
        "/// Parses `args` into [`EmergeOpts`]. `Err` carries the exit code of an\n"
        "/// option error, already reported.\n"
        "pub(crate) fn parse_emerge_args(args: &[String]) -> Result<EmergeOpts<'_>, ExitCode> {\n"
        + "\n".join(body)
        + "\n\n    Ok(EmergeOpts {\n"
        + "".join(f"        {n},\n" for n in names)
        + "    })\n}\n"
    )
    args_rs = os.path.join(os.path.dirname(path), "args.rs")
    open(args_rs, "w").write(
        "//! Parsing `emerge` arguments into [`EmergeOpts`].\n\n"
        "use crate::pretend::*;\n\n" + struct + "\n" + func)
    call = (
        ["    let EmergeOpts {"]
        + [f"        mut {n}," for n in names]
        + ["    } = match parse_emerge_args(args) {",
           "        Ok(opts) => opts,",
           "        Err(code) => return code,",
           "    };"]
    )
    lines[first - 1:last] = call
    open(path, "w").write("\n".join(lines))
    print(f"{len(fields)} fields, {len(region)} lines moved to {args_rs}")


if __name__ == "__main__":
    main(sys.argv[1], int(sys.argv[2]), int(sys.argv[3]))
