"""Properties any correct `emerge --pretend` output must have, checked with
no expected value (`docs/second_python_copy_removal.md` §1 and §2).

`check_json` checks one `--json` result; `check_cross_mode` checks that
plain, `--tree`, `--quiet` and `--json` describe the same graph. Both
return a list of human-readable violations (empty when the output is
consistent). Used by `tests/test_output_invariants.py` over the contract
cases and the harvested corpus, and by `TEST/compare/check-invariants.py`
over L0 output.
"""

from __future__ import annotations

import json
import re
from collections import Counter
from pathlib import Path

MERGE_BOUND = {"new", "upgrade", "downgrade", "reinstall"}

# Dependency variables that force merge order. Real's `_serialize_tasks`
# `ignore_priority` ladder (DepPriorityNormalRange) may drop `RDEPEND`
# (runtime) and `PDEPEND` (runtime_post) edges when a pass stalls; only
# buildtime edges (DEPEND/BDEPEND/IDEPEND, and `:=` slot-operator deps)
# are never ignored.
HARD_DEP_VARS = {"DEPEND", "BDEPEND", "IDEPEND"}
DEP_VARS = ("DEPEND", "RDEPEND", "BDEPEND", "IDEPEND", "PDEPEND")

_VERSION = re.compile(
    r"-(\d+(?:\.\d+)*[a-z]?(?:_(?:alpha|beta|pre|rc|p)\d*)*(?:-r\d+)?)$"
)
_LINE = re.compile(
    r"^\[(?P<kind>ebuild|binary|nomerge)\s*(?P<flags>[^\]]*)\](?P<indent>\s+)(?P<cpv>\S+)(?P<rest>.*)$"
)
_USE_VAR = re.compile(r'(?<![\w-])([A-Z][A-Z0-9_]*)="([^"]*)"')


def split_cpv(cpv: str) -> tuple[str, str, str]:
    """`cat/pkg-1.0-r1:slot::repo` -> (cat, pkg, version). A binary row's
    trailing build id (`pkg-1.0-3`) is dropped: a version never ends in a
    bare `-N`, and a package name never ends in `-<version>`."""
    cpv = cpv.split("::", 1)[0].split(":", 1)[0]
    cat, _, pv = cpv.partition("/")
    build_id = re.match(r"^(.*)-\d+$", pv)
    if build_id and _VERSION.search(build_id.group(1)):
        pv = build_id.group(1)
    m = _VERSION.search(pv)
    if not m:
        return cat, pv, ""
    return cat, pv[: m.start()], m.group(1)


def parse_json_stdout(stdout: str):
    """The `--json` document, or None when the command printed none (a
    non-resolver action, a CLI error, ...). Duplicate object keys are
    reported through the `_dup_keys` list on the returned dict."""
    dups: list[str] = []

    def hook(pairs):
        seen = Counter(k for k, _ in pairs)
        dups.extend(k for k, n in seen.items() if n > 1)
        return dict(pairs)

    for line in stdout.splitlines():
        if line.startswith('{"entries":'):
            doc = json.loads(line, object_pairs_hook=hook)
            doc["_dup_keys"] = dups
            return doc
    return None


def parse_plain(stdout: str) -> list[dict]:
    """Merge-list rows of plain / `--tree` / `--quiet` output."""
    rows = []
    for line in stdout.splitlines():
        m = _LINE.match(line)
        if not m:
            continue
        cat, pkg, version = split_cpv(m.group("cpv"))
        rows.append({
            "kind": m.group("kind"),
            "flags": m.group("flags"),
            "depth": (len(m.group("indent")) - 1) // 2,
            "cp": (cat, pkg),
            "version": version,
            "rest": m.group("rest"),
        })
    return rows


def _installed(root: Path | None, cp: tuple[str, str], installed=None) -> bool:
    if installed is not None:
        return cp in installed
    if root is None:
        return True  # cannot tell; don't flag
    vdb = root / "var" / "db" / "pkg" / cp[0]
    if not vdb.is_dir():
        return False
    return any(_VERSION.sub("", d.name) == cp[1] for d in vdb.iterdir())


def parse_vdb_list(text: str) -> set[tuple[str, str]]:
    """`cat/pkg-version` lines (the L0 run's `vdb-list.txt` snapshot) ->
    the set of installed `(cat, pkg)` pairs -- what `_installed` checks
    with a real `root`, without needing the container's vdb on the host."""
    cps: set[tuple[str, str]] = set()
    for line in text.splitlines():
        line = line.strip()
        if not line or "/" not in line:
            continue
        cat, _, pv = line.partition("/")
        cps.add((cat, _VERSION.sub("", pv)))
    return cps


def _dep_vars_for_atom(repo_roots: list[Path], child_cp: tuple[str, str],
                       cat: str, pkg: str, version: str) -> set[str]:
    """Which dependency variables of the owner's md5-cache entry name
    `child_cp` (any atom form; conditionals ignored -- a mention in any
    branch marks the variable). Empty when the entry is absent."""
    found: set[str] = set()
    # `cat/pkg`, `cat/pkg:slot`, `>=cat/pkg-1.2` match; `cat/pkg-other`
    # (a longer package name) does not: a `-` boundary must start a
    # version digit.
    atom = re.compile(
        rf"(?<![\w/.-]){re.escape(child_cp[0])}/{re.escape(child_cp[1])}"
        rf"(?=$|[^\w.+-]|-\d)")
    for repo in repo_roots:
        entry = repo / "metadata" / "md5-cache" / cat / f"{pkg}-{version}"
        if not entry.is_file():
            continue
        for line in entry.read_text(errors="replace").splitlines():
            for var in DEP_VARS:
                if line.startswith(var + "=") and atom.search(line[len(var) + 1:]):
                    found.add(var)
    return found


def _cyclic_edges(edges: set[tuple[int, int]], n: int) -> set[tuple[int, int]]:
    """Edges (child -> owner) that lie on a cycle: both ends in one SCC."""
    graph: dict[int, list[int]] = {i: [] for i in range(n)}
    for a, b in edges:
        graph[a].append(b)
    index, low, on_stack, stack, comp = {}, {}, set(), [], {}
    counter = [0]

    def strong(v):
        work = [(v, iter(graph[v]))]
        index[v] = low[v] = counter[0]
        counter[0] += 1
        stack.append(v)
        on_stack.add(v)
        while work:
            node, it = work[-1]
            advanced = False
            for w in it:
                if w not in index:
                    index[w] = low[w] = counter[0]
                    counter[0] += 1
                    stack.append(w)
                    on_stack.add(w)
                    work.append((w, iter(graph[w])))
                    advanced = True
                    break
                if w in on_stack:
                    low[node] = min(low[node], index[w])
            if advanced:
                continue
            work.pop()
            if work:
                low[work[-1][0]] = min(low[work[-1][0]], low[node])
            if low[node] == index[node]:
                while True:
                    w = stack.pop()
                    on_stack.discard(w)
                    comp[w] = node
                    if w == node:
                        break

    for v in range(n):
        if v not in index:
            strong(v)
    return {(a, b) for a, b in edges if comp[a] == comp[b]}


def check_json(doc: dict, root: Path | None = None,
               repo_roots: list[Path] | None = None,
               args: list[str] | None = None,
               installed: set[tuple[str, str]] | None = None,
               dep_vars: dict[tuple, set[str]] | None = None) -> list[str]:
    """`args` is the command line; `--rebuild-if-*` reinstalls are seeds
    (real adds them as rebuild arguments, so they have no parent).
    `installed` is a `parse_vdb_list` set (the L0 run cannot expose a
    `root`); `dep_vars` maps `(child_cp, owner_cp)` to the owner's
    dependency variables naming the child (L0's `dep-classes.tsv`),
    letting the soft-edge exemption work without repo access."""
    problems: list[str] = []
    rebuild_seeds = any(a.startswith("--rebuild-if-") for a in args or [])

    def is_seed(e):
        return e["requested"] or (
            rebuild_seeds and e["outcome"] == "reinstall" and not e["required_by"])

    entries = doc["entries"]
    if doc.get("_dup_keys"):
        problems.append(f"duplicate JSON keys: {sorted(set(doc['_dup_keys']))}")

    by_cp: dict[tuple[str, str], list[int]] = {}
    for i, e in enumerate(entries):
        by_cp.setdefault((e["category"], e["package"]), []).append(i)
        if e["merge_order"] != i:
            problems.append(f"entry {i} has merge_order {e['merge_order']}")

    # required_by: non-empty for non-seeds, each owner an entry or installed.
    for e in entries:
        name = f"{e['category']}/{e['package']}"
        if not is_seed(e) and not e["required_by"]:
            problems.append(f"{name}: not requested and required_by is empty")
        for owner in e["required_by"]:
            ocp = (owner["category"], owner["package"])
            if ocp not in by_cp and not _installed(root, ocp, installed):
                problems.append(f"{name}: owner {ocp[0]}/{ocp[1]} is neither an entry nor installed")

    # Reachability from the seeds (requested entries; an installed owner
    # that is not itself an entry is an external root).
    reached = {i for i, e in enumerate(entries) if is_seed(e)}
    changed = True
    while changed:
        changed = False
        for i, e in enumerate(entries):
            if i in reached:
                continue
            for owner in e["required_by"]:
                ocp = (owner["category"], owner["package"])
                if ocp not in by_cp or any(j in reached for j in by_cp[ocp]):
                    reached.add(i)
                    changed = True
                    break
    for i, e in enumerate(entries):
        if i not in reached:
            problems.append(f"{e['category']}/{e['package']}: unreachable from the seeds")

    # No duplicate cat/pkg:slot unless a slot conflict is reported for it.
    conflicted = {(c["category"], c["package"]) for c in doc["slot_conflicts"]}
    slots = Counter(
        (e["category"], e["package"], e["slot"], e.get("builds_against_running_root"))
        for e in entries if e["outcome"] in MERGE_BOUND
    )
    for (cat, pkg, slot, _running_root), n in slots.items():
        if n > 1 and (cat, pkg) not in conflicted:
            problems.append(f"{cat}/{pkg}:{slot} appears {n} times with no slot conflict reported")

    # Merge order respects required_by edges, except cycles, PDEPEND and
    # dependencies an installed instance satisfies.
    if doc.get("aborted") is None:
        bound = [i for i, e in enumerate(entries) if e["outcome"] in MERGE_BOUND]
        edges = set()
        for i in bound:
            for owner in entries[i]["required_by"]:
                for j in by_cp.get((owner["category"], owner["package"]), []):
                    if entries[j]["outcome"] in MERGE_BOUND and j != i:
                        edges.add((i, j))
        cyclic = _cyclic_edges(edges, len(entries))
        owners_of: dict[int, list[int]] = {}
        for i, j in edges:
            owners_of.setdefault(i, []).append(j)
        for i, owners in owners_of.items():
            e = entries[i]
            # A multi-slot owner is satisfied by any instance merging later.
            by_owner_cp: dict[tuple[str, str], list[int]] = {}
            for j in owners:
                by_owner_cp.setdefault((entries[j]["category"], entries[j]["package"]), []).append(j)
            child_cp = (e["category"], e["package"])
            # Real `_serialize_tasks` may ignore an edge whose dependency an
            # installed instance already satisfies (`DepPriority.satisfied`),
            # so a child with any installed instance is not held to it.
            # `_installed(root=None)` degrades to "cannot tell", which must
            # not exempt here: the original `root is not None and ...` guard.
            if installed is not None:
                installed_child = child_cp in installed
            else:
                installed_child = root is not None and _installed(root, child_cp)
            installed_child = installed_child or any(
                entries[k]["outcome"] != "new" for k in by_cp[child_cp])
            if installed_child:
                continue
            for ocp, js in by_owner_cp.items():
                if any(j > i for j in js) or any((i, j) in cyclic for j in js):
                    continue
                # Another merge-bound instance of a multi-instance child cp
                # before the owner satisfies the cp-level edge: the JSON
                # aggregates `required_by` by cp, while real's constraint is
                # per package instance (e.g. the clang:21 entry carrying an
                # edge that clang:22 already satisfied).
                if len(by_cp[child_cp]) > 1 and any(
                        k != i and entries[k]["outcome"] in MERGE_BOUND
                        and any(k < j for j in js)
                        for k in by_cp[child_cp]):
                    continue
                o = entries[js[0]]
                vars_found: set[str] = set()
                if dep_vars is not None:
                    vars_found = dep_vars.get((child_cp, ocp), set())
                elif repo_roots:
                    for j in js:
                        vars_found |= _dep_vars_for_atom(
                            repo_roots, child_cp, entries[j]["category"],
                            entries[j]["package"], entries[j]["version"])
                # A soft edge (RDEPEND/PDEPEND only) may legally be
                # relaxed by the scheduler's ignore ladder. Unknown
                # metadata stays strict.
                if vars_found and not (vars_found & HARD_DEP_VARS):
                    continue
                problems.append(
                    f"merge order: {e['category']}/{e['package']} (#{i}) merges after "
                    f"its owner {ocp[0]}/{ocp[1]} (#{max(js)})"
                )
    return problems


def check_unparsed_dep_tokens(stderr: str) -> list[str]:
    """§4: `PORTUALE_REPORT_UNPARSED_DEP_TOKENS` makes `emerge` report how
    many dependency tokens it skipped as unparseable; that must be 0."""
    m = re.search(r"^portuale: unparsed dependency tokens: (\d+)$", stderr, re.M)
    if m is None:
        return ["no unparsed-dependency-token report on stderr"]
    return [] if m.group(1) == "0" else [f"{m.group(1)} dependency token(s) dropped as unparseable"]


def check_plain_use(stdout: str) -> list[str]:
    """Each USE / USE_EXPAND flag name appears once per merge-list row."""
    problems = []
    for row in parse_plain(stdout):
        for var, value in _USE_VAR.findall(row["rest"]):
            names = [re.sub(r"^[({]?-?|[*%]*[)}]?[*%]*$", "", tok) for tok in value.split()]
            dup = sorted(n for n, c in Counter(names).items() if c > 1)
            if dup:
                problems.append(f"{row['cp'][0]}/{row['cp'][1]}: {var} repeats {dup}")
    return problems


def check_summary(stdout: str) -> list[str]:
    """The `-v` `Total: N packages (...)` counters equal the merge list."""
    m = re.search(r"^Total: (\d+) packages?(?: \(([^)]*)\))?", stdout, re.M)
    if not m:
        return []
    rows = [r for r in parse_plain(stdout) if r["kind"] != "nomerge"]
    counts = Counter()
    for r in rows:
        f = r["flags"]
        if "N" in f:
            counts["in new slot" if "S" in f else "new"] += 1
        elif "U" in f:
            counts["downgrade" if "D" in f else "upgrade"] += 1
        elif "R" in f or "r" in f:
            counts["reinstall"] += 1
    problems = []
    total = int(m.group(1))
    if total != sum(counts.values()):
        problems.append(f"Total: {total} but {sum(counts.values())} merge rows")
    stated = Counter()
    for part in (m.group(2) or "").split(", "):
        pm = re.match(r"(\d+) (upgrade|downgrade|new|in new slot|reinstall)s?$", part)
        if pm:
            stated[pm.group(2)] = int(pm.group(1))
    for kind in ("upgrade", "downgrade", "new", "in new slot", "reinstall"):
        if stated[kind] != counts[kind]:
            problems.append(f"Total says {stated[kind]} {kind}, merge list has {counts[kind]}")
    return problems


def check_cross_mode(plain: str, tree: str, quiet: str, doc: dict,
                     args: list[str] | None = None) -> list[str]:
    """`args` is the command line: `--autounmask-only` prints no merge list
    by design, and `--onlydeps` keeps the suppressed arguments in `--json`
    (as `requested`) but not in the displayed list."""
    problems = []
    args = args or []
    if "--autounmask-only" in args:
        return problems
    onlydeps = "--onlydeps" in args or any(
        a.startswith("-") and not a.startswith("--") and "o" in a for a in args)

    def rows_key(rows):
        return sorted((r["cp"], r["version"]) for r in rows if r["kind"] != "nomerge")

    want = sorted(
        ((e["category"], e["package"]), e["version"])
        for e in doc["entries"]
        if e["outcome"] in MERGE_BOUND and not (onlydeps and e["requested"])
    )
    for mode, text in (("plain", plain), ("--tree", tree), ("--quiet", quiet)):
        got = rows_key(parse_plain(text))
        if got != want:
            missing = sorted(set(want) - set(got))
            extra = sorted(set(got) - set(want))
            problems.append(f"{mode} merge list != --json entries (missing {missing}, extra {extra})")

    # `--tree` nesting: a row's nearest shallower row must be one of its
    # owners, possibly through owners that are not displayed (an installed
    # package, an `--onlydeps`-suppressed argument). A row with no
    # shallower row must have such a hidden owner.
    required_by: dict[tuple[str, str], set[tuple[str, str]]] = {}
    for e in doc["entries"]:
        required_by.setdefault((e["category"], e["package"]), set()).update(
            (o["category"], o["package"]) for o in e["required_by"])
    tree_rows = parse_plain(tree)
    shown = {r["cp"] for r in tree_rows}

    def hidden_path_to(cp, target):
        seen, todo = set(), [cp]
        while todo:
            for owner in required_by.get(todo.pop(), ()):
                if owner == target:
                    return True
                if owner not in shown and owner not in seen:
                    seen.add(owner)
                    todo.append(owner)
        return False

    def has_hidden_owner(cp):
        return any(o not in shown for o in required_by.get(cp, ()))

    stack: list[tuple[int, tuple[str, str]]] = []
    for r in tree_rows:
        while stack and stack[-1][0] >= r["depth"]:
            stack.pop()
        name = f"{r['cp'][0]}/{r['cp'][1]}"
        if r["depth"] > 0 and r["kind"] != "nomerge":
            if not stack:
                if not has_hidden_owner(r["cp"]):
                    problems.append(f"--tree: {name} at depth {r['depth']} has no parent row")
            elif not hidden_path_to(r["cp"], stack[-1][1]):
                p = stack[-1][1]
                problems.append(
                    f"--tree nests {name} under {p[0]}/{p[1]}, which is not in its required_by")
        stack.append((r["depth"], r["cp"]))
    return problems
