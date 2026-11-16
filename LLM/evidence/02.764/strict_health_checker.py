#!/usr/bin/env python3
"""Check that a package-manager plan leaves a healthy system.

Input: the document printed by `portuale emerge --pretend -v --json`
(entries with `dependencies`, `use`, `iuse`, `required_use`, plus the
top-level `kept` list of installed packages the plan leaves alone).
Installed packages' own data (USE, dependencies, sub-slot) is read from the
VDB when one is given, or from fields embedded in the `kept` items.

Properties, from portuale's charter (PMS-based, no Portage involved):

1. Dependencies hold in the final set: every RDEPEND/PDEPEND atom of every
   package is matched, no blocker matches, REQUIRED_USE holds, and no
   slot holds two packages.
2. Order is legal: every DEPEND/BDEPEND/IDEPEND atom of a merged package is
   matched by a package present when it is merged (installed, or merged
   earlier), and that package is usable (PMS 8.1): its RDEPEND closure is
   met by then too, and no DEPEND/BDEPEND blocker matches a package present
   at that moment. A need only a later package of the same cycle can meet
   (build-time and runtime edges between merges) is noted as an
   unserializable cycle, not reported as a failure.
4. A removal (the `{"removal": ...}` document of `-C`, `--depclean`,
   `--prune` with `--pretend --json`) keeps every shared library that an
   ELF file staying installed still links against: the dynamic linker
   still finds it after the removal, or it is in the `preserved` list.
   Dependencies the removal leaves unmatched are warnings (`removal-deps`),
   since the charter lets the user remove them. Needs the VDB.

The installed state (`--installed ROOT`): after a real merge, the VDB and the
files under ROOT are checked for properties 1 and 4, with no plan: every
installed package's runtime and post-merge dependencies are matched, no blocker
matches, REQUIRED_USE holds, no slot holds two packages, and every shared
library an installed ELF file needs is found by the dynamic linker (a
preserved library is a file on disk, so it is found). `--baseline FILE` is the
output of the same check before the merge: its findings are marked
`pre_existing`, so only what the merge introduced fails the run. Order (2) and
newest version (3) are not checked here: a final state does not record them.

With a VDB, the system before the plan is the VDB itself, not the plan's
`kept` list, so an installed package the plan silently drops is seen.

The atom matcher, version comparison and dependency-string evaluator below
are written from PMS and share no code with portuale or Portage.

Usage: health_checker.py [--vdb /var/db/pkg] PLAN.json...
       health_checker.py --installed ROOT [--baseline BEFORE.jsonl]
Prints one JSON object per plan: {"plan", "findings": [...], "refused"},
where `refused` says why portuale itself would not run the plan (its
findings are then expected, not failures). Exit 1 when a plan portuale
would run has a failure it introduces.
"""

from __future__ import annotations

import dataclasses
import functools
import io
import json
import os
import posixpath
import re
import sys
import tarfile
import time
from urllib.parse import urlparse
from dataclasses import asdict, dataclass, field
from pathlib import Path

MERGE_BOUND = {"new", "upgrade", "downgrade", "reinstall"}
RUNTIME_CLASSES = ("RDEPEND", "PDEPEND")
BUILD_CLASSES = ("DEPEND", "BDEPEND")
DEP_CLASSES = ("DEPEND", "BDEPEND", "RDEPEND", "PDEPEND", "IDEPEND")


# Notes and warnings: recorded, never a failure. `removal-deps` is a
# dependency a removal leaves unmatched, which the charter allows;
# `private-library` is a library installed outside the linker's search path
# (a JDK, systemd, a vendored application), which its own program finds.
# `world-unneeded` is `mrg --check-world`'s note that another slot serves a slot.
NOT_FAILURES = frozenset({"cycle-noted", "checker-limitation", "removal-deps",
                          "private-library", "world-unneeded"})


@dataclass(frozen=True)
class Finding:
    """One broken property: which package, which atom, and why."""

    property: str  # runtime-deps, blocker, required-use, slot, build-order, linked-library, ...
    package: str
    atom: str
    reason: str
    pre_existing: bool = False

    @property
    def is_failure(self) -> bool:
        return self.property not in NOT_FAILURES


# --- versions (PMS 3.2, 3.3) ------------------------------------------------

_VERSION = re.compile(
    r"^(\d+(?:\.\d+)*)([a-z]?)((?:_(?:alpha|beta|pre|rc|p)\d*)*)(?:-r(\d+))?$")
_SUFFIX = re.compile(r"_(alpha|beta|pre|rc|p)(\d*)")
_SUFFIX_RANK = {"alpha": 0, "beta": 1, "pre": 2, "rc": 3, "p": 5}
_NO_SUFFIX_RANK = 4


def is_version(text: str) -> bool:
    return _VERSION.match(text) is not None


def _cmp(a, b) -> int:
    return (a > b) - (a < b)


def vercmp(a: str, b: str) -> int:
    """-1, 0 or 1 as version `a` is lower, equal or higher than `b`."""
    ma, mb = _VERSION.match(a), _VERSION.match(b)
    if not ma or not mb:
        raise ValueError(f"not a version: {a!r} / {b!r}")
    na, nb = ma.group(1).split("."), mb.group(1).split(".")
    result = _cmp(int(na[0]), int(nb[0]))
    for x, y in zip(na[1:], nb[1:]):
        if result:
            return result
        if x.startswith("0") or y.startswith("0"):
            result = _cmp(x.rstrip("0"), y.rstrip("0"))
        else:
            result = _cmp(int(x), int(y))
    result = result or _cmp(len(na), len(nb))
    result = result or _cmp(ma.group(2), mb.group(2))
    if result:
        return result
    sa = [(_SUFFIX_RANK[s], int(n or 0)) for s, n in _SUFFIX.findall(ma.group(3))]
    sb = [(_SUFFIX_RANK[s], int(n or 0)) for s, n in _SUFFIX.findall(mb.group(3))]
    sa.append((_NO_SUFFIX_RANK, 0))
    sb.append((_NO_SUFFIX_RANK, 0))
    for x, y in zip(sa, sb):
        if x != y:
            return _cmp(x, y)
    return _cmp(int(ma.group(4) or 0), int(mb.group(4) or 0))


def without_revision(version: str) -> str:
    return re.sub(r"-r\d+$", "", version)


# --- atoms (PMS 8.3) ---------------------------------------------------------

@dataclass(frozen=True)
class UseDep:
    flag: str
    kind: str  # "on", "off", "eq", "neq", "if-on", "if-off"
    default: bool | None  # (+) / (-): the target's state when it lacks the flag


@dataclass(frozen=True)
class Atom:
    text: str
    blocker: bool
    strong: bool  # `!!` blocks everything matching; `!` blocks one package (PMS 8.3)
    op: str  # "", "<", "<=", "=", "~", ">=", ">"
    cp: tuple[str, str]
    version: str
    glob: bool  # `=cat/pkg-1.2*`
    slot: str  # "" (any), or the slot name
    subslot: str
    use_deps: tuple[UseDep, ...]


_ATOM = re.compile(
    r"^(?P<blocker>!!?)?(?P<op><=|>=|<|>|=|~)?(?P<cat>[\w+][\w+.-]*)/(?P<rest>[^:\[]+)"
    r"(?::(?P<slot>[^:\[]*))?(?:::(?P<repo>[\w-]+))?(?:\[(?P<use>[^\]]*)\])?$")
_USE_DEP = re.compile(r"^(?P<neg>[!-])?(?P<flag>[\w+][\w+@.-]*)(?:\((?P<default>[+-])\))?(?P<mod>[=?])?$")


def parse_atom(text: str) -> Atom:
    """A PMS dependency atom; raises ValueError on anything else."""
    m = _ATOM.match(text)
    if not m:
        raise ValueError(f"not an atom: {text}")
    op, rest = m.group("op") or "", m.group("rest")
    glob = op == "=" and rest.endswith("*")
    rest = rest.removesuffix("*") if glob else rest
    pkg, version = rest, ""
    if op:
        pkg, _, version = rest.rpartition("-")
        if re.fullmatch(r"r\d+", version):
            pkg, _, base = pkg.rpartition("-")
            version = f"{base}-{version}"
        if not pkg or not is_version(version):
            raise ValueError(f"bad version in atom: {text}")
    slot = (m.group("slot") or "").rstrip("=")
    slot, _, subslot = slot.partition("/")
    blocker = m.group("blocker") or ""
    return Atom(
        text=text,
        blocker=bool(blocker),
        strong=blocker == "!!",
        op=op,
        cp=(m.group("cat"), pkg),
        version=version,
        glob=glob,
        slot="" if slot == "*" else slot,
        subslot=subslot,
        use_deps=tuple(_parse_use_dep(u) for u in (m.group("use") or "").split(",") if u),
    )


def _parse_use_dep(text: str) -> UseDep:
    m = _USE_DEP.match(text)
    if not m:
        raise ValueError(f"bad USE dependency: {text}")
    neg, mod = m.group("neg"), m.group("mod")
    kind = {
        (None, None): "on", ("-", None): "off", (None, "="): "eq",
        ("!", "="): "neq", (None, "?"): "if-on", ("!", "?"): "if-off",
    }.get((neg, mod))
    if kind is None:
        raise ValueError(f"bad USE dependency: {text}")
    default = {"+": True, "-": False}.get(m.group("default"))
    return UseDep(m.group("flag"), kind, default)


# --- packages ----------------------------------------------------------------

@dataclass
class Package:
    category: str
    package: str
    version: str
    slot: str
    subslot: str = ""
    use: frozenset = frozenset()
    iuse: frozenset = frozenset()
    dependencies: dict = field(default_factory=dict)
    required_use: str = ""
    binary: bool = False
    known: bool = True  # False when nothing beyond name and slot could be read

    @property
    def cp(self) -> tuple[str, str]:
        return (self.category, self.package)

    @property
    def cpv(self) -> str:
        return f"{self.category}/{self.package}-{self.version}"


def _flag_state(pkg: Package, use_dep: UseDep) -> bool:
    if use_dep.flag not in pkg.iuse and use_dep.default is not None:
        return use_dep.default
    return use_dep.flag in pkg.use


def _use_deps_hold(atom: Atom, pkg: Package, parent_use: frozenset) -> bool:
    for dep in atom.use_deps:
        state, parent = _flag_state(pkg, dep), dep.flag in parent_use
        wanted = {
            "on": True, "off": False, "eq": parent, "neq": not parent,
            "if-on": True if parent else None, "if-off": False if not parent else None,
        }[dep.kind]
        if wanted is not None and state != wanted:
            return False
    return True


def _version_holds(atom: Atom, version: str) -> bool:
    if not atom.op:
        return True
    if atom.glob:
        return version.startswith(atom.version)
    if atom.op == "~":
        return vercmp(without_revision(version), without_revision(atom.version)) == 0
    c = vercmp(version, atom.version)
    return {"<": c < 0, "<=": c <= 0, "=": c == 0, ">=": c >= 0, ">": c > 0}[atom.op]


def matches(atom: Atom, pkg: Package, parent_use: frozenset = frozenset()) -> bool:
    """Whether `pkg` matches `atom` (blocker flag ignored), with the
    parent's USE for conditional USE dependencies."""
    if atom.cp != pkg.cp or not _version_holds(atom, pkg.version):
        return False
    if atom.slot and atom.slot != pkg.slot:
        return False
    if atom.subslot and atom.subslot != (pkg.subslot or pkg.slot):
        return False  # PMS 7.2: no sub-slot means the sub-slot is the slot
    return _use_deps_hold(atom, pkg, parent_use)


# --- dependency strings (PMS 8.2) -------------------------------------------
# A parsed string is a list of nodes: ("atom", Atom), ("all", [nodes]),
# ("any", [nodes]), ("one", [nodes]), ("most", [nodes]),
# ("if", flag, negated, [nodes]), ("flag", flag, negated) -- the last
# three kinds and "one"/"most" appear only in REQUIRED_USE.

_GROUPS = {"||": "any", "^^": "one", "??": "most"}


@functools.lru_cache(maxsize=None)
def parse_dep_string(text: str, required_use: bool = False) -> list:
    """Parse a dependency string (or a REQUIRED_USE string) into nodes.
    Cached: callers must not mutate the result."""
    tokens = text.split()
    nodes, pos = _parse_seq(tokens, 0, required_use)
    if pos != len(tokens):
        raise ValueError(f"unbalanced ')' in: {text}")
    return nodes


def _parse_seq(tokens: list[str], pos: int, required_use: bool) -> tuple[list, int]:
    nodes = []
    while pos < len(tokens) and tokens[pos] != ")":
        token = tokens[pos]
        if token == "(":
            children, pos = _parse_group(tokens, pos, required_use)
            nodes.append(("all", children))
        elif token in _GROUPS:
            children, pos = _parse_group(tokens, pos + 1, required_use)
            nodes.append((_GROUPS[token], children))
        elif token.endswith("?"):
            children, pos = _parse_group(tokens, pos + 1, required_use)
            nodes.append(("if", token[:-1].lstrip("!"), token.startswith("!"), children))
        elif required_use:
            nodes.append(("flag", token.lstrip("!"), token.startswith("!")))
            pos += 1
        else:
            nodes.append(("atom", parse_atom(token)))
            pos += 1
    return nodes, pos


def _parse_group(tokens: list[str], pos: int, required_use: bool) -> tuple[list, int]:
    """Parse `( ... )` starting at `pos`; returns the children and the
    position after `)`."""
    if pos >= len(tokens) or tokens[pos] != "(":
        raise ValueError(f"expected '(' at token {pos}")
    children, pos = _parse_seq(tokens, pos + 1, required_use)
    if pos >= len(tokens):
        raise ValueError("missing ')'")
    return children, pos + 1


def reduce_use(nodes: list, use: frozenset) -> list:
    """Drop the conditionals `use` turns off and splice in the ones it turns on."""
    out = []
    for node in nodes:
        if node[0] == "if":
            _, flag, negated, children = node
            if (flag in use) != negated:
                out.extend(reduce_use(children, use))
        elif node[0] in ("all", "any", "one", "most"):
            out.append((node[0], reduce_use(node[1], use)))
        else:
            out.append(node)
    return out


def render(node) -> str:
    if node[0] == "atom":
        return node[1].text
    if node[0] == "flag":
        return ("!" if node[2] else "") + node[1]
    prefix = {"all": "", "any": "|| ", "one": "^^ ", "most": "?? "}[node[0]]
    return prefix + "( " + " ".join(render(c) for c in node[1]) + " )"


def required_use_holds(text: str, use: frozenset) -> list[str]:
    """The top-level REQUIRED_USE items `use` breaks (empty when it holds)."""
    nodes = reduce_use(parse_dep_string(text, required_use=True), use)
    return [render(n) for n in nodes if not _required_use_node(n, use)]


def _required_use_node(node, use: frozenset) -> bool:
    if node[0] == "flag":
        return (node[1] in use) != node[2]
    values = [_required_use_node(c, use) for c in node[1]]
    if not values or node[0] == "all":
        return all(values)
    count = sum(values)
    return {"any": count >= 1, "one": count == 1, "most": count <= 1}[node[0]]


# --- satisfaction ------------------------------------------------------------

def _plain_atoms(nodes: list) -> list[Atom]:
    """Every non-blocker atom in a reduced tree, alternatives included."""
    out = []
    for node in nodes:
        if node[0] == "atom":
            if not node[1].blocker:
                out.append(node[1])
        else:
            out.extend(_plain_atoms(node[1]))
    return out


Index = dict  # (category, package) -> [Package]


def index(packages: list[Package]) -> Index:
    out: Index = {}
    for p in packages:
        out.setdefault(p.cp, []).append(p)
    return out


def _satisfied(node, available: Index, parent: Package) -> bool:
    if node[0] == "atom":
        atom = node[1]
        if atom.blocker:
            return True  # blockers are checked on the final set only
        return any(matches(atom, p, parent.use) for p in available.get(atom.cp, ()))
    values = [_satisfied(c, available, parent) for c in node[1]]
    return any(values) if node[0] == "any" and values else all(values)


def _blocker_hits(nodes: list, final: Index, owner: Package) -> list[tuple[Atom, Package]]:
    hits = []
    for node in nodes:
        if node[0] == "atom":
            atom = node[1]
            if atom.blocker:
                hits += [(atom, p) for p in final.get(atom.cp, ())
                         if p.cpv != owner.cpv and matches(atom, p, owner.use)]
        elif node[0] == "all":
            hits += _blocker_hits(node[1], final, owner)
    return hits


def _reduced(pkg: Package, dep_class: str) -> list:
    """The class's dependency tree under the package's USE; empty when the
    string does not parse (`check_plan` reports that once)."""
    try:
        return reduce_use(parse_dep_string(pkg.dependencies.get(dep_class, "")), pkg.use)
    except ValueError:
        return []


# --- loading a plan ----------------------------------------------------------

def _split_slot(slot: str) -> tuple[str, str]:
    main, _, sub = slot.partition("/")
    return main, sub


@functools.lru_cache(maxsize=None)
def read_vdb(vdb: Path | None, category: str, package: str, version: str) -> dict | None:
    """One installed package's VDB record as a `kept`-style dict, or None.
    Cached: callers must not mutate the result."""
    if vdb is None:
        return None
    d = vdb / category / f"{package}-{version}"
    if not d.is_dir():
        return None

    def read(key: str) -> str:
        f = d / key
        return f.read_text(errors="replace").strip() if f.is_file() else ""

    iuse = read("IUSE_EFFECTIVE") or read("IUSE")
    return {
        "slot": read("SLOT"),
        "use": read("USE").split(),
        "iuse": [f.lstrip("+-") for f in iuse.split()],
        "dependencies": {c: read(c) for c in DEP_CLASSES if read(c)},
        "required_use": read("REQUIRED_USE"),
    }


def split_pv(pv: str) -> tuple[str, str] | None:
    """`name-1.0-r1` -> (`name`, `1.0-r1`), or None."""
    m = re.match(r"^(.+?)-(\d[^-]*(?:-r\d+)?)$", pv)
    return (m.group(1), m.group(2)) if m and is_version(m.group(2)) else None


def split_cpv(cpv: str) -> tuple[str, str, str]:
    category, _, pv = cpv.partition("/")
    return (category, *split_pv(pv))


@functools.lru_cache(maxsize=None)
def installed_cpvs(vdb: Path) -> tuple[tuple[str, str, str], ...]:
    """(category, package, version) of every package in the VDB."""
    return tuple((d.parent.name, *split_pv(d.name)) for d in sorted(vdb.glob("*/*"))
                 if d.is_dir() and split_pv(d.name))


def _package(category, package, version, slot, data: dict | None, binary=False) -> Package:
    known = data is not None and "dependencies" in data
    data = data or {}
    main, sub = _split_slot(data.get("slot") or slot or "")
    return Package(
        category=category, package=package, version=version, slot=main,
        subslot=data.get("sub_slot") or sub,
        use=frozenset(data.get("use", ())), iuse=frozenset(data.get("iuse", ())),
        dependencies=dict(data.get("dependencies", {})),
        required_use=data.get("required_use", ""), binary=binary, known=known)


@dataclass
class Plan:
    merges: list[tuple[int, Package]]  # (merge order, package)
    kept: list[Package]
    replaced: list[tuple[int, Package]]  # installed, removed at that merge order
    dropped: list[Package] = field(default_factory=list)  # in the VDB, not kept or replaced

    @property
    def final(self) -> list[Package]:
        return self.kept + [p for _, p in self.merges]

    @property
    def initial(self) -> list[Package]:
        return self.kept + [p for _, p in self.replaced] + self.dropped

    def available_at(self, order: int) -> list[Package]:
        """What is installed when the merge at `order` starts."""
        return (self.kept + [p for o, p in self.replaced if o >= order]
                + [p for o, p in self.merges if o < order])


def load_plan(doc: dict, vdb: Path | None = None) -> Plan:
    merges, replaced = [], []
    for e in doc["entries"]:
        cat, pkg, order = e["category"], e["package"], e["merge_order"]
        outcome = e["outcome"]
        old = {"upgrade": e.get("from_version"), "downgrade": e.get("from_version"),
               "reinstall": e.get("version"), "uninstall": e.get("version")}.get(outcome)
        if old:
            replaced.append((order, _package(cat, pkg, old, e.get("slot"),
                                             read_vdb(vdb, cat, pkg, old))))
        if outcome in MERGE_BOUND:
            merges.append((order, _package(cat, pkg, e["version"], e.get("slot"), e,
                                           binary=e.get("source") == "binary")))
    kept = [_package(k["category"], k["package"], k["version"], k["slot"],
                     k if "dependencies" in k else read_vdb(vdb, k["category"], k["package"], k["version"]))
            for k in doc.get("kept", [])]
    known = {p.cpv for p in kept} | {p.cpv for _, p in replaced}
    dropped = [] if vdb is None else [
        _package(c, p, v, "", read_vdb(vdb, c, p, v)) for c, p, v in installed_cpvs(vdb)
        if f"{c}/{p}-{v}" not in known]
    return Plan(merges, kept, replaced, dropped)


# --- the checks --------------------------------------------------------------

def check_final_state(plan: Plan) -> list[Finding]:
    """Property 1 on the set the plan leaves installed."""
    final, initial = index(plan.final), index(plan.initial)
    kept_ids = {id(p) for p in plan.kept}
    findings = []
    for pkg in plan.final:
        was_there = id(pkg) in kept_ids
        for dep_class in RUNTIME_CLASSES:
            for node in _reduced(pkg, dep_class):
                if not _satisfied(node, final, pkg):
                    findings.append(Finding(
                        "runtime-deps", pkg.cpv, render(node),
                        f"{dep_class} not matched by any package in the final set",
                        pre_existing=was_there and not _satisfied(node, initial, pkg)))
            for atom, hit in _blocker_hits(_reduced(pkg, dep_class), final, pkg):
                findings.append(Finding(
                    "blocker", pkg.cpv, atom.text, f"blocks {hit.cpv}, which stays installed",
                    pre_existing=was_there and hit in initial.get(hit.cp, ())))
        if pkg.required_use:
            for item in required_use_holds(pkg.required_use, pkg.use):
                findings.append(Finding(
                    "required-use", pkg.cpv, item,
                    f"REQUIRED_USE broken by USE=\"{' '.join(sorted(pkg.use))}\"",
                    pre_existing=was_there))
    by_slot: dict = {}
    for pkg in plan.final:
        by_slot.setdefault((pkg.cp, pkg.slot), []).append(pkg.cpv)
    initial_cpvs = {p.cpv for p in plan.initial}
    for (cp, slot), cpvs in sorted(by_slot.items()):
        # A second build of the same version is one package, not two
        # (charter 2a); two different versions in one slot still fail.
        if len(set(cpvs)) > 1:
            findings.append(Finding("slot", "/".join(cp), f"{'/'.join(cp)}:{slot}",
                                    "slot holds " + ", ".join(sorted(cpvs)),
                                    pre_existing=initial_cpvs.issuperset(cpvs)))
    return findings


def _build_atoms(pkg: Package) -> list:
    classes = ("IDEPEND",) if pkg.binary else BUILD_CLASSES + ("IDEPEND",)
    return [(c, node) for c in classes for node in _reduced(pkg, c)]


def _merges_by_cp(plan: Plan) -> dict:
    out: dict = {}
    for order, pkg in plan.merges:
        out.setdefault(pkg.cp, []).append((order, pkg))
    return out


def _matching_merges(by_cp: dict, nodes: list, parent: Package) -> set[int]:
    """Orders of the merges matching any plain atom of `nodes`."""
    return {o for a in _plain_atoms(nodes) for o, p in by_cp.get(a.cp, ())
            if matches(a, p, parent.use)}


def _build_graph(plan: Plan) -> dict[int, set[int]]:
    """order -> orders of the merges that must be usable before it: those its
    build-time atoms name (to build it) and those its RDEPEND names (for it
    to be usable), unless an installed package meets the atom already. A
    cycle in this graph cannot be serialized."""
    by_cp, initial = _merges_by_cp(plan), index(plan.initial)
    graph = {}
    for order, pkg in plan.merges:
        nodes = [n for _, n in _build_atoms(pkg)] + _reduced(pkg, "RDEPEND")
        needs = [[("atom", a) for a in atoms] for atoms in _choices(nodes)
                 if not any(matches(a, p, pkg.use) for a in atoms for p in initial.get(a.cp, ()))]
        graph[order] = {o for n in needs for o in _matching_merges(by_cp, n, pkg)} - {order}
    return graph


def _components(graph: dict[int, set[int]]) -> dict[int, int]:
    """Strongly connected component id per node (Tarjan, iterative)."""
    index, low, comp, stack, on_stack, counter = {}, {}, {}, [], set(), [0]
    for root in graph:
        if root in index:
            continue
        work = [(root, iter(sorted(graph[root])))]
        index[root] = low[root] = counter[0]
        counter[0] += 1
        stack.append(root)
        on_stack.add(root)
        while work:
            node, children = work[-1]
            child = next(children, None)
            if child is None:
                work.pop()
                if work:
                    low[work[-1][0]] = min(low[work[-1][0]], low[node])
                if low[node] == index[node]:
                    while True:
                        member = stack.pop()
                        on_stack.discard(member)
                        comp[member] = node
                        if member == node:
                            break
            elif child not in index:
                index[child] = low[child] = counter[0]
                counter[0] += 1
                stack.append(child)
                on_stack.add(child)
                work.append((child, iter(sorted(graph.get(child, ())))))
            elif child in on_stack:
                low[node] = min(low[node], index[child])
    return comp


def _order_finding(plan: Plan, component, order: int, pkg: Package, node,
                   what: str, owner: Package | None = None, later=None) -> Finding:
    """A build-time need of `pkg` (merged at `order`) that is not met when it
    starts: noted as a cycle when a later package of its cycle meets it.
    `owner` is the package whose dependency `node` is (default: `pkg`);
    `later` the merges that would meet it (default: those matching `node`)."""
    if later is None:
        later = sorted((o, p) for o, p in plan.merges if o > order
                       and o in _matching_merges(_merges_by_cp(plan), [node], owner or pkg))
    if any(component.get(o) == component[order] for o, _ in later):
        return Finding("cycle-noted", pkg.cpv, render(node),
                       f"{what} is only met by a later package of the same cycle")
    second = _second_build(plan, order, pkg, node, owner or pkg)
    if second is not None:
        return Finding("cycle-noted", pkg.cpv, render(node),
                       f"{what} is met only after an identical rebuild of the same "
                       f"package (#{second} > #{order})")
    if later:
        return Finding("build-order", pkg.cpv, render(node),
                       f"{what} is met only by {later[0][1].cpv}, merged later "
                       f"(#{later[0][0]} > #{order}): merge it first")
    return Finding("build-order", pkg.cpv, render(node),
                   f"{what} is not matched by anything installed or merged before it")


def _second_build(plan: Plan, order: int, pkg: Package, node,
                 owner: Package) -> int | None:
    """Charter 2a: the order of a later merge of the identical cpv in the
    identical slot that already meets `node`, or None. A package the plan
    builds twice to break a cycle excuses the need its first build missed;
    a later build of another version never does."""
    # Matching against available_at excuses the order finding only: the
    # second build changes no version in the final set, so property 1 stays
    # with check_final_state either way.
    for later, other in sorted(plan.merges, key=lambda m: m[0]):
        if (later > order and other.cpv == pkg.cpv and other.slot == pkg.slot
                and _satisfied(node, index(plan.available_at(later)), owner)):
            return later
    return None


def _choices(nodes: list) -> list[list[Atom]]:
    """The atoms of a reduced tree as lists of alternatives, one of each
    list to be met (an `all` group splits, an `any` group is one list)."""
    out = []
    for node in nodes:
        if node[0] == "atom":
            out += [] if node[1].blocker else [[node[1]]]
        elif node[0] == "all":
            out += _choices(node[1])
        else:
            out.append(_plain_atoms([node]))
    return out


@dataclass
class _Moment:
    """The system when one merge starts, for the usability walk."""

    available: Index
    initial: Index
    merged: set  # id() of the packages the plan merges
    gaps: dict = field(default_factory=dict)  # id(package) -> its gap, or None

    def providers(self, atoms: list[Atom], parent: Package) -> list[Package]:
        return [p for a in atoms for p in self.available.get(a.cp, ()) if matches(a, p, parent.use)]

    def gap(self, pkg: Package) -> tuple[Package, tuple] | None:
        """Why `pkg` is not usable now (PMS 8.1): the first package in its
        RDEPEND closure with an RDEPEND atom the plan left unmet, and that
        atom. An atom an installed package already missed before the plan
        does not count; a runtime cycle counts as usable. A need met only by
        a merge the plan orders later (a late rebuild for a sub-slot pin, a
        compat package, a soname change the checker cannot see) is still
        reported: that ordering reason is outside the plan, so the finding
        is a checker limitation, not a plan defect (backlog #737)."""
        if id(pkg) in self.gaps:
            return self.gaps[id(pkg)]
        self.gaps[id(pkg)] = None  # a cycle back here counts as usable
        found = None
        for node in _reduced(pkg, "RDEPEND"):
            if not _satisfied(node, self.available, pkg):
                if id(pkg) in self.merged or _satisfied(node, self.initial, pkg):
                    found = (pkg, node)
                    break
                continue
            for atoms in _choices([node]):
                sub = [self.gap(p) for p in self.providers(atoms, pkg)]
                if sub and all(sub):
                    found = sub[0]
                    break
            if found:
                break
        self.gaps[id(pkg)] = found
        return found


def _unusable(node, moment: _Moment, pkg: Package) -> tuple[Package, tuple] | None:
    """When no package meeting build-time `node` of `pkg` is usable: the
    first gap found."""
    for atoms in _choices([node]):
        gaps = [moment.gap(p) for p in moment.providers(atoms, pkg)]
        if gaps and all(gaps):
            return gaps[0]
    return None


def _build_blockers(pkg: Package, available: Index, plan: Plan, order: int) -> list[Finding]:
    """DEPEND/BDEPEND blockers of `pkg` matching a package present when it builds.

    A weak (`!`) blocker matching a package the plan itself replaces later is
    only noted: the final set decides, and the hit is gone by then. A strong
    (`!!`) blocker always fails."""
    findings = []
    for dep_class in BUILD_CLASSES:
        for atom, hit in _blocker_hits(_reduced(pkg, dep_class), available, pkg):
            if not atom.strong and any(r is hit and ro > order for ro, r in plan.replaced):
                findings.append(Finding(
                    "cycle-noted", pkg.cpv, atom.text,
                    f"weak blocker {atom.text} matches {hit.cpv}, which the plan replaces later"))
            else:
                findings.append(Finding(
                    "blocker", pkg.cpv, atom.text,
                    f"build-time blocker: {hit.cpv} is installed when it builds"))
    return findings


def check_order(plan: Plan) -> list[Finding]:
    """Property 2: build-time atoms are matched, by usable packages, when
    each merge starts."""
    component = _components(_build_graph(plan))
    merged = {id(p) for _, p in plan.merges}
    initial = index(plan.initial)
    findings = []
    for order, pkg in plan.merges:
        available = index(plan.available_at(order))
        moment = _Moment(available, initial, merged)
        if not pkg.binary:
            findings += _build_blockers(pkg, available, plan, order)
        for dep_class, node in _build_atoms(pkg):
            if not _satisfied(node, available, pkg):
                findings.append(_order_finding(plan, component, order, pkg, node, dep_class))
                continue
            hit = _unusable(node, moment, pkg)
            if hit:
                provider, unmet = hit
                # An installed copy the plan rebuilds later is fixed by that rebuild.
                rebuild = [(o, p) for o, p in plan.merges if o > order and any(
                    r is provider and ro == o for ro, r in plan.replaced)]
                findings.append(_order_finding(
                    plan, component, order, pkg, unmet,
                    f"{dep_class} {render(node)} is not usable yet: RDEPEND of {provider.cpv}",
                    provider, rebuild or None))
    return findings


# --- property 3: each merged package is the best visible version ------------
# Visibility (PMS 5.2, 7.3; Devmanual "Keywording"): a version is visible when
# no package.mask atom matches it (unless package.unmask does), one of its
# KEYWORDS is accepted, and its LICENSE is.

_ASSIGN = re.compile(r"""^[ \t]*(?:export[ \t]+)?(\w+)=("(?:[^"\\]|\\.)*"|'[^']*'|\S*)"""
                     r"""|^[ \t]*(?:source|\.)[ \t]+(\S+)""", re.M | re.S)


def read_make_vars(path: Path, env: dict, root: Path) -> dict:
    """The assignments of a make.conf-style file, `${VAR}` expanded against
    `env` and the file itself; `source` lines are followed."""
    out = {}
    text = path.read_text(errors="replace") if path.is_file() else ""
    for m in _ASSIGN.finditer(text):
        if m.group(3):
            out.update(read_make_vars(root / m.group(3).lstrip("/"), {**env, **out}, root))
            continue
        value = m.group(2)
        if value[:1] in "\"'":
            value = value[1:-1]
        both = {**env, **out}
        out[m.group(1)] = re.sub(r"\$\{(\w+)\}|\$(\w+)",
                                 lambda v: both.get(v.group(1) or v.group(2), ""), value)
    return out


def stack(tokens: list[str], text: str) -> list[str]:
    """Apply one incremental layer (`-*` clears, `-x` removes)."""
    for t in text.split():
        if t == "-*":
            tokens = []
        elif t.startswith("-"):
            tokens = [x for x in tokens if x != t[1:]]
        elif t not in tokens:
            tokens = tokens + [t]
    return tokens


def read_lines(path: Path) -> list[str]:
    """The non-comment lines of a config file, or of every file under a
    config directory (hidden and backup files skipped)."""
    if path.is_dir():
        return [ln for f in sorted(path.rglob("*")) if f.is_file()
                and not f.name.startswith(".") and not f.name.endswith("~") for ln in read_lines(f)]
    lines = path.read_text(errors="replace").splitlines() if path.is_file() else []
    return [ln.split("#")[0].strip() for ln in lines if ln.split("#")[0].strip()]


@dataclass
class Candidate:
    """One ebuild of the repositories, or one binary package of the binary
    index, with what visibility needs."""

    pkg: Package
    keywords: list[str]
    license: str
    iuse_raw: list[str]
    repo: str
    binary: bool = False  # an entry of the binary index, not an ebuild
    build_id: str = ""


def _binary_metadata(path: Path) -> dict | None:
    """The metadata of a binary package file (`SLOT`, `USE`, `KEYWORDS`, ...):
    the xpak of a `.tbz2`, the `metadata.tar` of a `.gpkg.tar`. None when it
    cannot be read."""
    try:
        if path.name.endswith(".tbz2"):
            data = path.read_bytes()
            if data[-4:] != b"STOP":
                return None
            xpak = data[-8 - int.from_bytes(data[-8:-4], "big"):-8]
            idx_len, data_len = (int.from_bytes(xpak[8 + 4 * i:12 + 4 * i], "big") for i in (0, 1))
            index, body = xpak[16:16 + idx_len], xpak[16 + idx_len:16 + idx_len + data_len]
            out, pos = {}, 0
            while pos < len(index):
                size = int.from_bytes(index[pos:pos + 4], "big")
                name = index[pos + 4:pos + 4 + size].decode()
                off, length = (int.from_bytes(index[pos + 4 + size + 4 * i:pos + 8 + size + 4 * i], "big")
                               for i in (0, 1))
                out[name] = " ".join(body[off:off + length].decode(errors="replace").split())
                pos += 12 + size
            return out
        with tarfile.open(path) as outer:
            member = next(m for m in outer.getmembers() if "/metadata.tar" in m.name)
            raw = outer.extractfile(member).read()
        suffix = member.name.rpartition("metadata.tar")[2]
        if suffix == ".zst":
            from compression import zstd
            raw = zstd.decompress(raw)
        with tarfile.open(fileobj=io.BytesIO(raw)) as inner:  # transparent gz/xz/bz2
            return {m.name.rpartition("/")[2]: " ".join(inner.extractfile(m).read().decode(
                errors="replace").split()) for m in inner.getmembers() if m.isfile()
                and "/metadata/" in m.name}
    except (OSError, ValueError, StopIteration, ImportError, tarfile.TarError, AttributeError):
        return None


class Visibility:
    """An independent model of which ebuilds are visible on a system, read
    from its repositories and configuration (no package manager involved)."""

    def __init__(self, root: Path):
        self.root = root
        etc = root / "etc/portage"
        self.repos = self._repos(etc)
        chain = self._profile_chain((etc / "make.profile").resolve()) + [etc / "profile"]
        env: dict = {}
        layers = [root / "usr/share/portage/config/make.globals"] + [
            d / "make.defaults" for d in chain] + [etc / "make.conf"]
        values = []
        for layer in layers:
            values.append(read_make_vars(layer, env, root))
            env.update(values[-1])
        values.append(os.environ)
        pkgdir = [v["PKGDIR"] for v in values if v.get("PKGDIR")]
        self.pkgdir = root / (pkgdir[-1] if pkgdir else "var/cache/binpkgs").lstrip("/")
        self.binhosts = self._binhosts(etc, values)
        self.accept_keywords = []
        for v in values:
            self.accept_keywords = stack(self.accept_keywords, v.get("ACCEPT_KEYWORDS", ""))
        # Incremental too, but with groups: kept as written, see `_license_ok`.
        self.accept_license = [t for v in values for t in v.get("ACCEPT_LICENSE", "").split()]
        self.masks = []  # (atom text, the repo it applies to or "")
        for name, location in self.repos.items():
            own = "" if name == "gentoo" else name
            self.masks += [(a, own) for a in read_lines(location / "profiles/package.mask")]
        for d in chain + [etc]:
            for line in read_lines(d / "package.mask"):
                self.masks = ([m for m in self.masks if m[0] != line[1:]] if line.startswith("-")
                              else self.masks + [(line, "")])
        self.unmasks = [a for d in chain + [etc] for a in read_lines(d / "package.unmask")]
        self.package_keywords = [ln.split() for d in chain + [etc] for f in (
            "package.accept_keywords", "package.keywords") for ln in read_lines(d / f)]
        self.package_license = [ln.split() for ln in read_lines(etc / "package.license")]
        self.groups = {}
        for location in self.repos.values():
            for line in read_lines(location / "profiles/license_groups"):
                name, *members = line.split()
                self.groups.setdefault(name, members)

    def _repos(self, etc: Path) -> dict[str, Path]:
        out, name = {}, None
        for line in read_lines(etc / "repos.conf"):
            if line.startswith("["):
                name = line.strip("[]")
            elif name and name != "DEFAULT" and line.replace(" ", "").startswith("location="):
                out[name] = self.root / line.split("=", 1)[1].strip().lstrip("/")
        return out

    def _profile_chain(self, profile: Path) -> list[Path]:
        chain = []
        for parent in read_lines(profile / "parent"):
            repo, sep, rel = parent.partition(":")
            base = (self.repos[repo] / "profiles") if sep and repo in self.repos else profile
            chain += self._profile_chain((base / (rel if sep else parent)).resolve())
        return chain + [profile]

    @functools.lru_cache(maxsize=None)
    def candidates(self, cp: tuple[str, str]) -> list[Candidate] | None:
        """Every ebuild of `cp` in the repositories, or None when one has
        ebuilds without a metadata cache (not modelled)."""
        out = []
        for name, location in self.repos.items():
            if not (location / cp[0] / cp[1]).is_dir():
                continue
            cache = location / "metadata/md5-cache" / cp[0]
            ebuilds = list((location / cp[0] / cp[1]).glob("*.ebuild"))
            entries = [cache / e.name.removesuffix(".ebuild") for e in ebuilds]
            if any(not e.is_file() for e in entries):
                return None
            for e in entries:
                meta = dict(ln.split("=", 1) for ln in e.read_text(errors="replace").splitlines() if "=" in ln)
                main, sub = _split_slot(meta.get("SLOT", "0"))
                iuse = meta.get("IUSE", "").split()
                pkg = Package(cp[0], cp[1], split_pv(e.name)[1], main, sub,
                              iuse=frozenset(f.lstrip("+-") for f in iuse),
                              dependencies={c: meta[c] for c in DEP_CLASSES if meta.get(c)},
                              required_use=meta.get("REQUIRED_USE", ""))
                out.append(Candidate(pkg, meta.get("KEYWORDS", "").split(), meta.get("LICENSE", ""),
                                     iuse, name))
        return out

    def _binhosts(self, etc: Path, values: list[dict]) -> list[str]:
        """The `sync-uri` of every `binrepos.conf` section, then the words of
        `PORTAGE_BINHOST` (the last layer that sets it wins)."""
        uris = [line.partition("=")[2].strip() for line in read_lines(etc / "binrepos.conf")
                if line.replace(" ", "").startswith("sync-uri=")]
        binhost = [v["PORTAGE_BINHOST"] for v in values if v.get("PORTAGE_BINHOST")]
        return uris + (binhost[-1].split() if binhost else [])

    def _binary_indexes(self) -> list[Path]:
        """The `Packages` files that list binary packages: the local one, then
        the cached copy of every remote index (`EROOT/var/cache/edb/binhost/
        <host>/<path>`, where Portage keeps what it fetched; the checker never
        uses the network) and, for a `file://` binhost, its own directory."""
        found = [self.pkgdir / "Packages"]
        for uri in self.binhosts:
            url = urlparse(uri)
            found.append(self.root / "var/cache/edb/binhost" / (url.hostname or "")
                         / url.path.lstrip("/") / "Packages")
            if url.scheme == "file":
                found.append(Path(url.path) / "Packages")
        return [f for f in dict.fromkeys(found) if f.is_file()]

    @functools.cached_property
    def _binary_pool(self) -> dict[tuple[str, str], list[Candidate]] | None:
        """Every binary package the checker can read, by `(category, name)`:
        the stanzas of the binary indexes, then the packages of PKGDIR that no
        stanza lists (their own metadata). None when there is nothing to read."""
        metas = []
        for index in self._binary_indexes():
            for block in index.read_text(errors="replace").split("\n\n"):
                meta = dict(line.partition(": ")[::2] for line in block.splitlines() if ": " in line)
                if "/" in meta.get("CPV", ""):
                    metas.append({k: v.strip() for k, v in meta.items()})
        listed = {(m["CPV"], m.get("BUILD_ID", "")) for m in metas}
        for path, cpv, build_id in self._unlisted_binaries(listed):
            if (meta := _binary_metadata(path)) is not None:
                metas.append(dict(meta, CPV=cpv, BUILD_ID=build_id, REPO=meta.get("REPO", "")))
        if not metas and not self._binary_indexes():
            return None
        pool: dict[tuple[str, str], list[Candidate]] = {}
        seen = set()
        for meta in metas:
            cat, _, pv = meta["CPV"].partition("/")
            split = split_pv(pv)
            key = (cat, split and split[0])
            if not split or (meta["CPV"], meta.get("BUILD_ID", ""), meta.get("REPO", "")) in seen:
                continue
            seen.add((meta["CPV"], meta.get("BUILD_ID", ""), meta.get("REPO", "")))
            main, sub = _split_slot(meta.get("SLOT", "0"))
            iuse = meta.get("IUSE", "").split()
            pkg = Package(cat, split[0], split[1], main, sub,
                          use=frozenset(meta.get("USE", "").split()),
                          iuse=frozenset(f.lstrip("+-") for f in iuse),
                          dependencies={c: meta[c] for c in DEP_CLASSES if meta.get(c)},
                          required_use=meta.get("REQUIRED_USE", ""), binary=True)
            pool.setdefault(key, []).append(Candidate(
                pkg, meta.get("KEYWORDS", "").split(), meta.get("LICENSE", ""), iuse,
                meta.get("REPO", ""), True, meta.get("BUILD_ID", "")))
        return pool

    def _unlisted_binaries(self, listed: set):
        """`(file, cat/pn-ver, build id)` of the `.tbz2`/`.gpkg.tar` files under
        PKGDIR that no index stanza names. The name gives category, package
        and version (`<cat>/<pn>-<ver>.tbz2`, or `<cat>/<pn>/<pn>-<ver>-<id>.`
        with multiple instances); the slot comes from the file's metadata."""
        if not self.pkgdir.is_dir():
            return
        for path in sorted(self.pkgdir.rglob("*")):
            stem = next((path.name.removesuffix(x) for x in (".tbz2", ".gpkg.tar")
                         if path.name.endswith(x)), None)
            rel = path.relative_to(self.pkgdir).parts
            if stem is None or len(rel) not in (2, 3):
                continue
            build_id = ""
            if len(rel) == 3:
                stem, _, build_id = stem.rpartition("-")
            if (split := split_pv(stem)) is None or (len(rel) == 3 and split[0] != rel[1]):
                continue
            cpv = f"{rel[0]}/{stem}"
            if (cpv, build_id) not in listed and (cpv, "") not in listed:
                yield path, cpv, build_id

    def binary_candidates(self, cp: tuple[str, str]) -> list[Candidate] | None:
        """Every binary package of `cp` the checker can read: the local binary
        index, the cached copies of remote indexes and the PKGDIR files outside
        any index; None when there is none of them."""
        pool = self._binary_pool
        return None if pool is None else pool.get(cp, [])

    def _matching(self, lines: list[list[str]], pkg: Package) -> list[str]:
        """The tokens of the per-package lines whose atom matches `pkg`."""
        out = []
        for atom, *tokens in lines:
            try:
                if matches(parse_atom(atom), pkg):
                    out.append(tokens)
            except ValueError:
                pass
        return out

    def _masked(self, cand: Candidate) -> bool:
        def hit(text: str) -> bool:
            try:
                return matches(parse_atom(text), cand.pkg) and (
                    "::" not in text or text.split("::")[1].split("[")[0] == cand.repo)
            except ValueError:
                return False
        return (any(hit(a) for a, repo in self.masks if repo in ("", cand.repo))
                and not any(hit(a) for a in self.unmasks))

    def _keyword_ok(self, cand: Candidate) -> bool:
        accepted = self.accept_keywords
        for tokens in self._matching(self.package_keywords, cand.pkg):
            default = [f"~{k}" for k in self.accept_keywords if k[:1] not in "~-"]
            accepted = stack(accepted, " ".join(tokens or default))
        if "**" in accepted:
            return True
        return any(k in accepted or (k.startswith("~") and "~*" in accepted)
                   or (k[:1] not in "~-" and "*" in accepted)
                   for k in cand.keywords if not k.startswith("-"))

    def _license_ok(self, cand: Candidate, use: frozenset) -> bool:
        tokens = self.accept_license + [
            t for ts in self._matching(self.package_license, cand.pkg) for t in ts]
        allowed, denied, everything = set(), set(), False
        for token in tokens:
            names = self._group(token.lstrip("-"))
            if token == "-*":
                allowed, denied, everything = set(), set(), False
            elif token == "*":
                everything, denied = True, set()
            elif token.startswith("-"):
                allowed, denied = allowed - names, denied | names
            else:
                allowed, denied = allowed | names, denied - names

        def holds(node) -> bool:
            if node[0] == "flag":
                return node[1] in allowed or (everything and node[1] not in denied)
            values = [holds(c) for c in node[1]]
            return any(values) if node[0] == "any" else all(values)
        try:
            nodes = reduce_use(parse_dep_string(cand.license, required_use=True), use)
        except ValueError:
            return False
        return all(holds(n) for n in nodes)

    def _group(self, token: str, depth: int = 0) -> set[str]:
        if not token.startswith("@") or depth > 8:
            return {token}
        return {x for m in self.groups.get(token[1:], ()) for x in self._group(m, depth + 1)}

    def mask_reason(self, cand: Candidate, use: frozenset) -> str:
        """Why `cand` is not visible ("" when it is)."""
        if self._masked(cand):
            return "package.mask"
        if not self._keyword_ok(cand):
            return "keywords"
        return "" if self._license_ok(cand, use) else "license"


@functools.lru_cache(maxsize=None)
def visibility(root: Path) -> Visibility:
    return Visibility(root)


def candidate_use(cand: Candidate, merged: Package) -> frozenset:
    """The USE a candidate would get in place of `merged`: its flags as the
    plan set them on `merged`, IUSE defaults for new ones, and the implicit
    flags (`elibc_*`, `kernel_*`, ...) of `merged`."""
    own = {f.lstrip("+-") for f in cand.iuse_raw if (
        f.lstrip("+-") in merged.use if f.lstrip("+-") in merged.iuse else f.startswith("+"))}
    return frozenset(own | (merged.use - merged.iuse))


def _constraints(plan: Plan, merged: Package, request: list[Atom]) -> tuple[list, list]:
    """The atoms other packages (and the request) place on `merged`'s
    package that `merged` meets, with their owners, and the blockers of
    the others."""
    needs, blockers = [], []
    for pkg in plan.final:
        if pkg is merged:
            continue
        classes = DEP_CLASSES if any(pkg is m for _, m in plan.merges) else RUNTIME_CLASSES
        for c in classes:
            for node in _reduced(pkg, c):
                for atom in _all_atoms([node]):
                    if atom.cp != merged.cp:
                        continue
                    if atom.blocker:
                        blockers.append((atom, pkg))
                    elif matches(atom, merged, pkg.use):
                        needs.append((atom, pkg))
    needs += [(a, None) for a in request if a.cp == merged.cp and matches(a, merged)]
    return needs, blockers


def _all_atoms(nodes: list) -> list[Atom]:
    """Every atom of a reduced tree, alternatives and blockers included."""
    return [a for n in nodes for a in ([n[1]] if n[0] == "atom" else _all_atoms(n[1]))]


def _fits(cand: Candidate, use: frozenset, merged: Package, plan: Plan, needs, blockers) -> str:
    """"yes" when `cand` can take `merged`'s place with nothing else in the
    plan changed, "no" when the others' atoms or blockers forbid it,
    "unknown" when it would need more changes (its own dependencies)."""
    pkg = dataclasses.replace(cand.pkg, use=use)
    if any(not matches(a, pkg, owner.use if owner else frozenset()) for a, owner in needs):
        return "no"
    if any(matches(a, pkg, owner.use) for a, owner in blockers):
        return "no"
    others = index([p for p in plan.final if p is not merged] + [pkg])
    if _blocker_hits([n for c in DEP_CLASSES for n in _reduced(pkg, c)], others, pkg):
        return "no"
    unmet = [n for c in DEP_CLASSES for n in _reduced(pkg, c) if not _satisfied(n, others, pkg)]
    broken = pkg.required_use and required_use_holds(pkg.required_use, use)
    return "unknown" if unmet or broken else "yes"


def request_atoms(doc: dict, root: Path) -> list[Atom]:
    """The plain atoms of the request, with `@world`/`@selected` read from
    the world file (other sets name no versions worth modelling)."""
    out = []
    for arg in doc.get("request", ()):
        texts = read_lines(root / "var/lib/portage/world") if arg in ("@world", "@selected") else [arg]
        for text in texts:
            try:
                out.append(parse_atom(text))
            except ValueError:
                pass
    return out


def check_best_versions(doc: dict, vdb: Path | None) -> list[Finding]:
    """Property 3: no merged package has a higher visible version (in its
    slot, or in any slot for a new package nothing pins to one) that every
    constraint of the plan also allows. The candidates are the ebuilds of the
    repositories and the binary packages the checker can read (the local
    index, the cached index of each binhost, the files of PKGDIR), as far as
    the run's `options` (`--usepkg`, `--usepkgonly`) let it use them."""
    if vdb is None or "request" not in doc:
        return [Finding("checker-limitation", "", "", "property 3 needs the VDB and `-v --json`")]
    root = vdb.parents[2]
    world = visibility(root)
    plan = load_plan(doc, vdb)
    request = request_atoms(doc, root)
    sets = [a for a in doc["request"] if a.startswith("@")]
    named = {a.cp for a in request_atoms({"request": [a for a in doc["request"] if a not in sets]}, root)}
    options = doc.get("options")  # absent from a plan printed before it was added
    findings = []
    if options and options.get("getbinpkg") and not world.binhosts:
        findings.append(Finding("checker-limitation", "", "",
                                "property 3: --getbinpkg without a binhost in binrepos.conf "
                                "or PORTAGE_BINHOST: no remote index to read"))
    for order, merged in plan.merges:
        entry = next(e for e in doc["entries"] if e["merge_order"] == order)
        if not merged.known:
            continue
        mine_pool = world.binary_candidates(merged.cp) if merged.binary else world.candidates(merged.cp)
        mine = [c for c in mine_pool or () if c.pkg.version == merged.version
                and (not merged.binary or not entry.get("build_id")
                     or c.build_id == str(entry["build_id"]))]
        if mine_pool is None or not mine:
            what = "no binary index the checker can read has it" if merged.binary \
                else "the repositories' metadata cache does not have it"
            findings.append(Finding("checker-limitation", merged.cpv, "", f"property 3: {what}"))
            continue
        reasons = {world.mask_reason(c, merged.use) for c in mine}
        if "" not in reasons:
            findings.append(Finding("visibility", merged.cpv, "",
                                    f"not visible: masked by {', '.join(sorted(reasons))}"))
            continue
        # A rebuild at the installed version is what a request without
        # --update means, unless the package was named on the command line.
        if entry["outcome"] == "reinstall" and merged.cp not in named:
            continue
        needs, blockers = _constraints(plan, merged, request)
        free = entry["outcome"] == "new" and all(not a.slot for a, _ in needs) and \
            sum(p.cp == merged.cp for p in plan.final + plan.initial) == 1
        cands, notes = _usable_candidates(world, merged, options)
        findings += _better_version(world, plan, merged, cands, needs, blockers, free)
        findings += notes
    return findings


def _usable_candidates(world, merged: Package, options: dict | None):
    """The candidates the run could have picked instead of `merged`, and the
    notes about the ones it cannot tell about. Ebuilds count unless
    `--usepkgonly`; binary packages count with `--usepkg` or when `merged`
    is one. Without `options` (an older plan), a higher ebuild for a merged
    binary is only a note: the run may have been `--usepkgonly`."""
    usepkgonly = bool(options and options.get("usepkgonly"))
    usepkg = bool(options and (options.get("usepkg") or usepkgonly))
    ebuilds = world.candidates(merged.cp) or []
    binaries = world.binary_candidates(merged.cp) or []
    notes = []
    if merged.binary:
        if options is None:
            for cand in ebuilds:
                if vercmp(cand.pkg.version, merged.version) > 0 and cand.pkg.slot == merged.slot \
                        and not world.mask_reason(cand, candidate_use(cand, merged)):
                    notes.append(Finding(
                        "checker-limitation", merged.cpv, f"={cand.pkg.cpv}",
                        "property 3: a higher ebuild is visible, but the plan does not say "
                        "whether `--usepkgonly` ruled it out"))
                    break
            return binaries, notes
        return binaries + ([] if usepkgonly else ebuilds), notes
    return ebuilds + (binaries if usepkg else []), notes


def _better_version(world, plan, merged, cands, needs, blockers, free) -> list[Finding]:
    """A finding for the highest visible candidate above `merged` that fits
    the plan; a limitation record when one might, but needs other changes."""
    unknown = None
    higher = [c for c in cands if vercmp(c.pkg.version, merged.version) > 0
              and (c.pkg.slot == merged.slot or free)]
    for cand in sorted(higher, key=functools.cmp_to_key(lambda a, b: vercmp(b.pkg.version, a.pkg.version))):
        use = candidate_use(cand, merged)
        if cand.binary:
            # `--binpkg-respect-use`: a binary whose USE differs from the one
            # the package would get is not a candidate.
            flags = cand.pkg.iuse
            if {f for f in cand.pkg.use if f in flags} != {f for f in use if f in flags}:
                continue
            use = cand.pkg.use
        if world.mask_reason(cand, use):
            continue
        fit = _fits(cand, use, merged, plan, needs, blockers)
        if fit == "yes":
            return [Finding("best-version", merged.cpv, f"={cand.pkg.cpv}",
                            f"{cand.pkg.version}{' binary package' if cand.binary else ''} "
                            f"(slot {cand.pkg.slot}) is visible and meets every "
                            f"atom the plan places on {merged.category}/{merged.package}")]
        unknown = unknown or (fit == "unknown" and cand)
    if unknown:
        return [Finding("checker-limitation", merged.cpv, f"={unknown.pkg.cpv}",
                        "property 3: a higher visible version would need other changes")]
    return []


# --- property 4: a removal keeps the libraries remaining programs link -------

@dataclass(frozen=True)
class Elf:
    """One `NEEDED.ELF.2` line: an ELF file, its soname, where it looks for
    libraries and which ones it needs, and its ABI (multilib category)."""

    path: str
    soname: str
    runpaths: tuple[str, ...]
    needed: tuple[str, ...]
    abi: str


@dataclass
class Installed:
    """What the VDB records about one installed package's files."""

    files: set[str]  # every non-directory path of CONTENTS
    elfs: list[Elf]


def read_elfs(text: str) -> list[Elf]:
    out = []
    for line in text.splitlines():
        f = line.split(";")
        if len(f) < 5:
            continue
        origin = str(Path(f[1]).parent)
        runpaths = tuple(r.replace("${ORIGIN}", origin).replace("$ORIGIN", origin)
                         for r in f[3].split(":") if r)
        abi = f[5] if len(f) > 5 and f[5] else f[0]
        out.append(Elf(f[1], f[2], runpaths, tuple(n for n in f[4].split(",") if n), abi))
    return out


def read_files(text: str) -> set[str]:
    """Paths of the `obj` and `sym` lines of a CONTENTS text."""
    out = set()
    for line in text.splitlines():
        if line.startswith("obj "):
            out.add(line[4:].rsplit(" ", 2)[0])
        elif line.startswith("sym "):
            out.add(line[4:].split(" -> ")[0])
    return out


@functools.lru_cache(maxsize=None)
def read_installed(vdb: Path) -> dict[str, Installed]:
    """cpv -> files and ELF data of every installed package."""
    out = {}
    for cat, pkg, version in installed_cpvs(vdb):
        d = vdb / cat / f"{pkg}-{version}"
        text = {k: (d / k).read_text(errors="replace") if (d / k).is_file() else ""
                for k in ("CONTENTS", "NEEDED.ELF.2")}
        out[f"{cat}/{pkg}-{version}"] = Installed(read_files(text["CONTENTS"]),
                                                  read_elfs(text["NEEDED.ELF.2"]))
    return out


def ld_paths(root: Path) -> list[str]:
    """The dynamic linker's search directories: /etc/ld.so.conf (with its
    `include` globs), then the default ones."""
    def read(conf: Path) -> list[str]:
        out = []
        lines = conf.read_text(errors="replace").splitlines() if conf.is_file() else []
        for line in (ln.split("#")[0].strip() for ln in lines):
            if line.startswith("include "):
                for pattern in line.split()[1:]:
                    base = conf.parent if not pattern.startswith("/") else root
                    for inc in sorted(base.glob(pattern.lstrip("/"))):
                        out += read(inc)
            elif line.startswith("/"):
                out.append(line)
        return out
    return read(root / "etc/ld.so.conf") + ["/lib64", "/usr/lib64", "/lib", "/usr/lib"]


class Removal:
    """The file system before and after a removal, as the VDB and the disk
    describe it. Paths are compared with their directories resolved, so a
    `/lib64 -> usr/lib64` link names one file twice."""

    def __init__(self, root: Path, installed: dict[str, Installed], removed: set[str],
                 preserved: set[str]):
        self.root, self.real_root = root, root.resolve()
        self.dirs = ld_paths(root)
        self.abi = {self.canon(e.path): e.abi for p in installed.values() for e in p.elfs}
        staying = {self.canon(f) for c, p in installed.items() if c not in removed for f in p.files}
        self.gone = {self.canon(f) for c in removed for f in installed[c].files} - staying
        self.gone -= {self.canon(f) for f in preserved}
        self.owner = {self.canon(f): c for c in removed for f in installed[c].files}

    def inside(self, disk: Path) -> str:
        """An absolute path on this machine, as a path under the root."""
        try:
            rel = str(disk.relative_to(self.real_root))
        except ValueError:
            return str(disk)  # a link leaving the root: keep it as written
        return "/" if rel == "." else "/" + rel

    @functools.lru_cache(maxsize=None)
    def canon(self, path: str) -> str:
        head, _, tail = path.rpartition("/")
        return self.inside((self.root / head.lstrip("/")).resolve()).rstrip("/") + "/" + tail

    def exists(self, path: str, after: bool, depth: int = 0) -> bool:
        """Whether `path` (and the links it goes through) is there."""
        if after and path in self.gone:
            return False
        disk = self.root / path.lstrip("/")
        if not disk.is_symlink():
            return disk.exists()
        target = self.canon(str(Path(path).parent / disk.readlink()))
        return depth < 16 and self.exists(target, after, depth + 1)

    @functools.lru_cache(maxsize=None)
    def lookup(self, runpaths: tuple, abi: str, soname: str, after: bool) -> str | None:
        """Where the dynamic linker finds `soname` for an ELF file of `abi`
        with these runpaths: the first search directory holding a file of
        that ABI by that name."""
        for d in runpaths + tuple(self.dirs):
            path = self.canon(f"{d.rstrip('/')}/{soname}")
            if self.exists(path, after):
                real = self.inside((self.root / path.lstrip("/")).resolve())
                if self.abi.get(real, abi) == abi:
                    return path
        return None


def check_removal_plan(doc: dict, vdb: Path | None) -> list[Finding]:
    """Property 4 on a pretend removal (`-C`, `--depclean`, `--prune` with
    `--json`): every library a remaining ELF file needs is still found after
    the removal, or preserved; a dependency the removal leaves unmatched is
    only a warning (the charter allows it)."""
    if vdb is None:
        return [Finding("checker-limitation", "", "", "a removal is checked against the VDB")]
    installed = read_installed(vdb)
    removed = {f"{r['category']}/{r['package']}-{r['version']}" for r in doc["removal"]}
    preserved = set(doc.get("preserved", ()))
    world = Removal(vdb.parents[2], installed, removed, preserved)
    kept_files = {world.canon(p) for p in preserved}
    findings = []
    for cpv, pkg in sorted(installed.items()):
        for elf in pkg.elfs:
            if cpv in removed and world.canon(elf.path) not in kept_files:
                continue
            for soname in elf.needed:
                before = world.lookup(elf.runpaths, elf.abi, soname, False)
                if before and not world.lookup(elf.runpaths, elf.abi, soname, True):
                    findings.append(Finding(
                        "linked-library", cpv, soname,
                        f"{elf.path} needs {soname}: {before} goes with "
                        f"{world.owner.get(before, 'the removal')} and is not preserved"))
    return findings + removal_dependency_warnings(vdb, removed)


def removal_dependency_warnings(vdb: Path, removed: set[str]) -> list[Finding]:
    """Run-time dependencies the removal leaves unmatched (allowed, warned)."""
    before = [_package(c, p, v, "", read_vdb(vdb, c, p, v)) for c, p, v in installed_cpvs(vdb)]
    after = [p for p in before if p.cpv not in removed]
    initial, final = index(before), index(after)
    return [Finding("removal-deps", pkg.cpv, render(node),
                    f"{dep_class} no longer matched after the removal")
            for pkg in after for dep_class in RUNTIME_CLASSES
            for node in _reduced(pkg, dep_class)
            if _satisfied(node, initial, pkg) and not _satisfied(node, final, pkg)]


def check_plan(doc: dict, vdb: Path | None = None) -> list[Finding]:
    """Every finding for one `--pretend -v --json` document."""
    plan = load_plan(doc, vdb)
    limits = [Finding("checker-limitation", p.cpv, "", "no dependency data for this package")
              for p in plan.final if not p.known]
    for pkg in plan.final:
        for dep_class, text in pkg.dependencies.items():
            try:
                parse_dep_string(text)
            except ValueError as err:
                limits.append(Finding("checker-limitation", pkg.cpv, dep_class, f"cannot parse: {err}"))
    return limits + check_final_state(plan) + check_order(plan)


def check_document(doc: dict, vdb: Path | None = None) -> list[Finding]:
    """Every finding for one document: a removal, or a merge plan."""
    if "removal" in doc:
        return check_removal_plan(doc, vdb)
    return check_plan(doc, vdb) + check_best_versions(doc, vdb)


def refusal(doc: dict) -> str:
    """Why portuale itself refuses to run this plan ("" when it would run it):
    a slot conflict, a needed configuration change, or an unsolvable blocker."""
    reasons = []
    if doc.get("slot_conflicts"):
        reasons.append("slot conflict")
    if any(doc.get(f"autounmask_{k}_changes") for k in ("keyword", "use", "license", "mask")):
        reasons.append("needs configuration changes")
    if any(b.get("unsolvable") for e in doc["entries"] for b in e.get("blockers", ())):
        reasons.append("unsolvable blocker")
    return ", ".join(reasons)


# --- the installed state after a real merge -----------------------------------

def installed_plan(vdb: Path) -> Plan:
    """The VDB as a plan that merges nothing: every installed package is kept."""
    kept = [_package(c, p, v, "", read_vdb(vdb, c, p, v)) for c, p, v in installed_cpvs(vdb)]
    return Plan([], kept, [], [])


def finding_key(f: Finding) -> tuple:
    return (f.property, f.package, f.atom, f.reason)


def check_installed(root: Path, baseline: frozenset = frozenset(),
                    vdb: Path | None = None) -> list[Finding]:
    """Properties 1 and 4 on the system installed under `root`. A finding whose
    key is in `baseline` (the same check run before the merge) is `pre_existing`.
    `vdb` overrides `root/var/db/pkg` (the mutation test reads a modified copy)."""
    vdb = vdb or root / "var/db/pkg"
    for cached in (read_vdb, read_installed, installed_cpvs):
        cached.cache_clear()  # the disk may have changed since an earlier call
    findings = check_final_state(installed_plan(vdb))
    installed = read_installed(vdb)
    world = Removal(root, installed, set(), set())
    searched = {posixpath.dirname(world.canon(f"{d.rstrip('/')}/x")) for d in world.dirs}
    # Libraries installed outside the search path: found by the program that
    # carries their directory in its own RUNPATH, not by the linker's default.
    private = {(e.abi, e.soname) for pkg in installed.values() for e in pkg.elfs
               if e.soname and posixpath.dirname(world.canon(e.path)) not in searched
               and world.exists(world.canon(e.path), False)}
    # An ABI this system cannot run (a vendored ppc64 or arm library) has no
    # library in the search path; its files are not programs that run here.
    runnable = {e.abi for pkg in installed.values() for e in pkg.elfs
                if e.soname and posixpath.dirname(world.canon(e.path)) in searched}
    for cpv, pkg in sorted(installed.items()):
        for elf in pkg.elfs:
            if elf.abi not in runnable:
                continue
            for soname in elf.needed:
                if world.lookup(elf.runpaths, elf.abi, soname, False):
                    continue
                if (elf.abi, soname) in private:
                    findings.append(Finding(
                        "private-library", cpv, soname,
                        f"{elf.path} needs {soname}, installed outside the linker's search path"))
                else:
                    findings.append(Finding(
                        "linked-library", cpv, soname,
                        f"{elf.path} needs {soname}, which the dynamic linker does not find"))
    return [dataclasses.replace(f, pre_existing=finding_key(f) in baseline) for f in findings]


def read_baseline(path: str) -> frozenset:
    """The finding keys of an earlier `--installed` run (its JSON line)."""
    keys = set()
    for line in Path(path).read_text().splitlines():
        if line.startswith("{"):
            keys |= {(f["property"], f["package"], f["atom"], f["reason"])
                     for f in json.loads(line).get("findings", ())}
    return frozenset(keys)


def parse_json_output(text: str) -> dict | None:
    """The `--json` document in a captured stdout, or None."""
    for line in text.splitlines():
        if line.startswith(('{"entries":', '{"removal":')):
            return json.loads(line)
    return None


def main(argv: list[str]) -> int:
    vdb = None
    if len(argv) >= 2 and argv[0] == "--vdb":
        vdb, argv = Path(argv[1]), argv[2:]
    if len(argv) >= 2 and argv[0] == "--installed":
        root, baseline = Path(argv[1]), frozenset()
        if len(argv) == 4 and argv[2] == "--baseline":
            baseline = read_baseline(argv[3])
        findings = check_installed(root, baseline)
        print(json.dumps({"installed": str(root), "findings": [
            dict(asdict(f), failure=f.is_failure) for f in findings]}, sort_keys=True))
        return 1 if any(f.is_failure and not f.pre_existing for f in findings) else 0
    failed = False
    for name in argv:
        doc = parse_json_output(Path(name).read_text(errors="replace"))
        record = {"plan": name}
        if doc is None:
            record["skipped"] = "no --json document"
        elif doc.get("aborted"):
            record["skipped"] = f"plan aborted: {doc['aborted'].get('reason')}"
        else:
            start = time.monotonic()
            findings = check_document(doc, vdb)
            record["seconds"] = round(time.monotonic() - start, 3)
            record["findings"] = [dict(asdict(f), failure=f.is_failure) for f in findings]
            record["refused"] = "" if "removal" in doc else refusal(doc)
            failed |= not record["refused"] and any(
                f.is_failure and not f.pre_existing for f in findings)
        print(json.dumps(record, sort_keys=True))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
