#!/usr/bin/env python3
"""#336 Phase 5: give `resolve_pretend` named arguments.

Usage: migrate_select_calls.py FILE

Replaces the 29 positional parameters of `resolve_pretend` with
`(source: &SelectSource, atom_str: &str, opts: &SelectOptions)`, inserting
both structs, a `Default` (emerge's defaults), and
`ResolveCtx::select_options`. The body is left untouched: it destructures
both structs into the old local names first. Every call is rewritten with
named fields; a call that copies a field from `ctx`/`bp` the way
`select_options` does leaves it out.
"""

import re
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from migrate_resolve_calls import split_args, split_comment  # noqa: E402

PARAMS = ["repos", "root", "atom_str", "config", "newuse", "changed_use", "update",
          "excluded", "changed_deps", "with_bdeps", "changed_slot", "selective",
          "is_top_level", "usepkg", "usepkgonly", "binpkg_respect_use", "usepkg_exclude",
          "usepkg_include", "rebuilt_binaries", "rebuilt_binaries_timestamp", "newrepo",
          "empty", "getbinpkg", "autounmask_keywords", "autounmask_use",
          "autounmask_license", "autounmask_masks", "extra_constraints", "local_binpkg"]
SOURCE = ["repos", "root", "config", "local_binpkg"]
OPTS = [p for p in PARAMS if p not in SOURCE and p != "atom_str"]
DEFAULT = {p: "false" for p in OPTS}
DEFAULT.update({"excluded": "&[]", "usepkg_exclude": "&[]", "usepkg_include": "&[]",
                "rebuilt_binaries_timestamp": "None", "extra_constraints": "&[]",
                "with_bdeps": "true", "selective": "true"})
# what `ResolveCtx::select_options(bp)` copies, per field
FROM_CTX = {p: f"ctx.{p}" for p in OPTS}
FROM_CTX.update({f"autounmask_{k}": f"bp.autounmask_suggest_{k}"
                 for k in ("keywords", "use", "license", "masks")})
FROM_CTX.update({"is_top_level": None, "extra_constraints": None})
TYPES = {"excluded": "&'a [String]", "usepkg_exclude": "&'a [String]",
         "usepkg_include": "&'a [String]", "rebuilt_binaries_timestamp": "Option<u64>",
         "extra_constraints": "&'a [String]"}

DEFS = '''/// Where [`resolve_pretend`] selects from: the repos, the target root,
/// its config and the local binary-package index.
#[derive(Clone, Copy)]
pub struct SelectSource<'a> {
    pub repos: &'a [RepoConfig],
    pub root: &'a Path,
    pub config: &'a portage_profile::Config,
    pub local_binpkg: &'a std::sync::Arc<BinaryIndex>,
}

/// The emerge options that shape one [`resolve_pretend`] selection.
/// `Default` is emerge with no option given (`--with-bdeps` and
/// `--selective` on, everything else off) for a dependency atom.
#[derive(Debug, Clone, Copy)]
pub struct SelectOptions<'a> {
''' + "".join(f"    pub {p}: {TYPES.get(p, 'bool')},\n" for p in OPTS) + '''}

impl Default for SelectOptions<'_> {
    fn default() -> Self {
        Self {
''' + "".join(f"            {p}: {DEFAULT[p].replace('&[]', '&[]')},\n" for p in OPTS) + '''        }
    }
}

impl ResolveCtx<'_> {
    /// The selection options this walk passes for every atom, with
    /// `bp`'s autounmask switches; callers set `is_top_level` and
    /// `extra_constraints` per atom.
    fn select_options(&self, bp: &BacktrackParams) -> SelectOptions<'_> {
        SelectOptions {
''' + "".join(f"            {p}: {FROM_CTX[p].replace('ctx.', 'self.')},\n"
              for p in OPTS if FROM_CTX[p]) + '''            is_top_level: false,
            extra_constraints: &[],
        }
    }
}

'''


def calls(src):
    for m in re.finditer(r"\bresolve_pretend\(", src):
        line = src[src.rfind("\n", 0, m.start()) + 1:m.start()]
        if "//" in line or line.count('"') % 2 or "fn " in line:
            continue
        i, depth = m.end(), 1
        while depth:
            depth += (src[i] == "(") - (src[i] == ")")
            i += 1
        yield m.start(), i, split_args(src[m.end():i - 1])


def rewrite(args, pad):
    vals = dict(zip(PARAMS, (split_comment(a) for a in args)))
    uses_ctx = vals["repos"][1] == "&ctx.repos"
    src = ", ".join(f"{k}: {vals[k][1]}" if vals[k][1] != k else k for k in SOURCE)
    fields = []
    for p in OPTS:
        comments, v = vals[p]
        base = FROM_CTX[p] if uses_ctx and FROM_CTX[p] else DEFAULT[p]
        if v == base and not comments:
            continue
        fields += [f"{pad}        {c}" for c in comments]
        fields.append(f"{pad}        {p}," if v == p else f"{pad}        {p}: {v},")
    base = "ctx.select_options(bp)" if uses_ctx else "SelectOptions::default()"
    opts = ("&SelectOptions {\n" + "\n".join(fields) + f"\n{pad}        ..{base}\n{pad}    }}"
            if fields else f"&{base}")
    return (f"resolve_pretend(\n{pad}    &SelectSource {{ {src} }},\n"
            f"{pad}    {vals['atom_str'][1]},\n{pad}    {opts},\n{pad})")


def main(path):
    s = open(path).read()
    found = list(calls(s))
    bad = [len(a) for _, _, a in found if len(a) != len(PARAMS)]
    if bad:
        sys.exit(f"unexpected argument counts: {bad}")
    out, last = [], 0
    for start, end, args in found:
        ls = s.rfind("\n", 0, start) + 1
        pad = " " * (len(s[ls:start]) - len(s[ls:start].lstrip()))
        out += [s[last:start], rewrite(args, pad)]
        last = end
    s = "".join(out) + s[last:]
    # signature
    k = s.index("pub fn resolve_pretend(")
    e = s.index(") -> Result<PretendOutcome, Error> {", k)
    s = (s[:k] + "pub fn resolve_pretend(\n    source: &SelectSource,\n    atom_str: &str,\n"
         "    opts: &SelectOptions,\n" + s[e:])
    body = s.index("{", s.index(") -> Result<PretendOutcome, Error>", k)) + 1
    destructure = ("\n    let SelectSource { repos, root, config, local_binpkg } = *source;\n"
                   "    let SelectOptions {\n" + "".join(f"        {p},\n" for p in OPTS) +
                   "    } = *opts;")
    s = s[:body] + destructure + s[body:]
    # struct definitions go before the function's doc comment
    doc = k
    while s[s.rfind("\n", 0, doc - 1) + 1:doc].lstrip().startswith(("///", "#[")):
        doc = s.rfind("\n", 0, doc - 1) + 1
    s = s[:doc] + DEFS + s[doc:]
    open(path, "w").write(s)
    print(f"{len(found)} calls rewritten")


if __name__ == "__main__":
    main(sys.argv[1])
