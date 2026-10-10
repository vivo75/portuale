#!/usr/bin/env python3
"""#336 Phase 5: rewrite positional `resolve_pretend_graph(...)` calls as
named-field `ResolveRequest` construction.

Usage: migrate_resolve_calls.py FILE [--stats] [--apply]

Each call's 44 arguments are matched positionally to the parameter names
of `resolve_pretend_graph`; the call becomes

    ResolveRequest {
        <every field whose argument differs from the neutral default>,
        ..ResolveRequest::new(config_root, root, atoms, config, distdir)
    }
    .resolve()

`--stats` prints, per parameter, how often each argument text occurs, so
the neutral defaults can be checked against real usage.
"""

import collections
import re
import sys

PARAMS = [
    "config_root", "root", "atoms", "config", "newuse", "changed_use", "nodeps",
    "update", "deep", "excluded", "with_bdeps", "changed_deps", "changed_slot",
    "with_test_deps", "changed_deps_report", "selective",
    "autounmask_suggest_keywords", "autounmask_suggest_use",
    "autounmask_suggest_license", "autounmask_suggest_masks", "usepkg",
    "usepkgonly", "binpkg_respect_use", "usepkg_exclude", "usepkg_include",
    "rebuilt_binaries", "rebuilt_binaries_timestamp", "newrepo", "buildpkgonly",
    "root_deps_running_root", "distdir", "empty", "getbinpkg",
    "ignore_built_slot_operator_deps", "backtrack_max", "reinstall_atoms",
    "rebuild_if_new_slot", "rebuild_if_unbuilt", "rebuild_if_new_rev",
    "rebuild_if_new_ver", "rebuild_exclude", "rebuild_ignore", "dynamic_deps",
    "complete",
]
CTOR = ["config_root", "root", "atoms", "config", "distdir"]
# Neutral defaults: what `ResolveRequest::new` sets, i.e. emerge's own
# defaults when no option is given (--with-bdeps, --selective,
# --rebuild-if-new-slot and --dynamic-deps on; portuale's --backtrack 10).
NEUTRAL = {p: "false" for p in PARAMS}
NEUTRAL.update({
    "with_bdeps": "true", "selective": "true", "rebuild_if_new_slot": "true",
    "dynamic_deps": "true",
    "deep": "Deep::NotRequested", "excluded": "&[]", "usepkg_exclude": "&[]",
    "usepkg_include": "&[]", "rebuilt_binaries_timestamp": "None",
    "root_deps_running_root": "None", "backtrack_max": "10",
    "reinstall_atoms": "&[]", "rebuild_exclude": "&[]", "rebuild_ignore": "&[]",
})
# Fields that are owned in the struct but borrowed in the old signature.
SLICES = {"excluded", "usepkg_exclude", "usepkg_include", "reinstall_atoms",
          "rebuild_exclude", "rebuild_ignore"}


def split_args(text):
    """Split a call's argument text on top-level commas."""
    out, depth, cur, i, in_str = [], 0, [], 0, False
    while i < len(text):
        c = text[i]
        if in_str:
            cur.append(c)
            if c == "\\":
                cur.append(text[i + 1]); i += 2; continue
            if c == '"':
                in_str = False
        elif c == '"':
            in_str = True; cur.append(c)
        elif c in "([{":
            depth += 1; cur.append(c)
        elif c in ")]}":
            depth -= 1; cur.append(c)
        elif c == "," and depth == 0:
            out.append("".join(cur).strip()); cur = []
        else:
            cur.append(c)
        i += 1
    tail = "".join(cur).strip()
    if tail:
        out.append(tail)
    return out


def find_calls(src):
    for m in re.finditer(r"\bresolve_pretend_graph\(", src):
        if src[max(0, m.start() - 7):m.start()] == "pub fn ":
            continue
        line = src[src.rfind("\n", 0, m.start()) + 1:m.start()]
        if "//" in line or line.count('"') % 2:
            continue  # inside a comment or a string literal
        i, depth, in_str = m.end(), 1, False
        while depth:
            c = src[i]
            if in_str:
                if c == "\\":
                    i += 1
                elif c == '"':
                    in_str = False
            elif c == '"':
                in_str = True
            elif c == "(":
                depth += 1
            elif c == ")":
                depth -= 1
            i += 1
        yield m.start(), i, split_args(src[m.end():i - 1])


def to_owned(field, arg):
    """Turn a borrowed argument expression into the struct's owned type."""
    if field in ("config_root", "root", "distdir"):
        return arg
    if field in SLICES or field == "atoms":
        return f"({arg}).to_vec()" if not arg.startswith("&[") else f"vec![{arg[2:-1]}]" if arg != "&[]" else "Vec::new()"
    if field == "root_deps_running_root":
        return arg if arg == "None" else f"({arg}).map(std::path::Path::to_path_buf)"
    return arg


def split_comment(arg):
    """Separate leading `//` lines and inline `/* */` labels from the value."""
    lines = arg.split("\n")
    comments = [l.strip() for l in lines if l.strip().startswith("//")]
    value = " ".join(l.strip() for l in lines if not l.strip().startswith("//"))
    value = re.sub(r"/\*.*?\*/\s*", "", value).strip()
    # a comment that only names the position adds nothing next to the field
    comments = [c for c in comments if not re.fullmatch(r"//\s*[-a-z_ ]+", c)]
    return comments, value


def rewrite(args, indent):
    pad = " " * indent
    fields = []
    for name, raw in zip(PARAMS, args):
        comments, arg = split_comment(raw)
        if name in CTOR or (arg == NEUTRAL[name] and not comments):
            continue
        fields.extend(f"{pad}    {c}" for c in comments)
        value = to_owned(name, arg)
        fields.append(f"{pad}    {name}," if value == name else f"{pad}    {name}: {value},")
    a = {n: split_comment(v)[1] for n, v in zip(PARAMS, args)}
    ctor = f"ResolveRequest::new({a['config_root']}, {a['root']}, {a['atoms']}, {a['config']}, {a['distdir']})"
    if not fields:
        return f"{ctor}\n{pad}.resolve()"
    return "ResolveRequest {\n" + "\n".join(fields) + f"\n{pad}    ..{ctor}\n{pad}}}\n{pad}.resolve()"


def main(path, flags):
    src = open(path).read()
    calls = list(find_calls(src))
    bad = [(s, len(a)) for s, _, a in calls if len(a) != len(PARAMS)]
    if bad:
        sys.exit(f"calls with an unexpected argument count: {bad[:5]}")
    print(f"{len(calls)} calls", file=sys.stderr)
    if "--stats" in flags:
        for i, name in enumerate(PARAMS):
            c = collections.Counter(a[i] for _, _, a in calls)
            print(f"{name:34s} " + "  ".join(f"{v!r}x{n}" for v, n in c.most_common(4)))
    if "--apply" in flags:
        out, last = [], 0
        for start, end, args in calls:
            line_start = src.rfind("\n", 0, start) + 1
            indent = len(src[line_start:start]) - len(src[line_start:start].lstrip())
            out.append(src[last:start])
            out.append(rewrite(args, indent))
            last = end
        out.append(src[last:])
        open(path, "w").write("".join(out))


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
