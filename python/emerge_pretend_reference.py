#!/usr/bin/env python3
"""Python reference implementation for the `emerge --pretend` pilot slice
(see PORTING/PROMPT.md and PORTING/rust/portage-repo/src/lib.rs for the
full scope writeup). Mirrors the exact same restricted v1 algorithm as the
Rust side so the two can be contract-tested against each other,
argv-for-argv and byte-for-byte on stdout, the same way every other pilot
slice is.

Overlays (see find_repos/list_candidates): candidates for a given
category/package are gathered from every repos.conf repo with a
location, main plus any overlays -- mirroring real portdbapi.cp_list,
which does the same (an overlay isn't consulted only if the main repo
has nothing). Repos are sorted ascending by (priority, name), matching
real portage's own prepos_order, so a tie between two repos providing
the identical version is broken toward the higher-priority one (see
_best_candidate). A repo's priority is its explicit repos.conf value if
present, else -1000 for the main repo (real portage's own default) or 0
for anything else.

USE/ACCEPT_KEYWORDS/package.mask/.unmask/.accept_keywords/.use (see
resolve_config) come from a real profile chain + make.conf + package.*,
not a hardcoded stand-in -- mirroring PORTING/rust/portage-profile/src/lib.rs
exactly (own implementation, not a wrapper around real config.py; see that
crate's doc comment for the full algorithm and its documented scope cuts:
no cross-repo profile parents, no USE_EXPAND (including package.use's
USE_EXPAND-prefix shorthand), only the `defaults`/`conf` USE_ORDER layers,
user-level package.mask/.unmask/.accept_keywords/.use only (no repo/
profile-level stacking), and the real config.py quirk where `${VAR}`
substitution excludes USE across profile levels). Matching a candidate
against a package.mask/.unmask/.accept_keywords/.use entry reuses the real
portage.dep.Atom(allow_wildcard=True) + match_from_list directly, since --
unlike the Rust side, whose v1 Atom grammar rejects wildcard atoms outright
and needs a separate bounded fallback -- real Atom already handles
"*/*"/"category/*"/"*/package" correctly via its own extended_syntax path
(verified empirically to agree with the Rust side's bounded matcher for
exactly those forms). package.use (see effective_use_flags) is applied per
package, not globally: each package's own DEPEND/RDEPEND are flattened
against its own effective USE set (base config["use_flags"] plus any
matching package.use entry's tokens), never leaking into a sibling or
dependency's own resolution.

Dependency recursion (see resolve_pretend_graph) walks DEPEND+RDEPEND via
the real portage.dep.use_reduce(flat=True), with its own documented scope
cuts mirrored exactly from portage-repo/src/lib.rs's resolve_pretend_graph
doc comment: || (any-of) groups resolve every alternative rather than
picking one (flat mode discards group boundaries, so there's no reliable
way to identify "the first" alternative from its output), cycles/
duplicates (by exact atom text) are deduped via a visited set, and a
dependency's own deps are only walked if it would newly merge or upgrade.
Two different SLOTs of the same package are genuinely separate,
independent entries -- real portage allows multiple slots to coexist in
one merge list (e.g. dev-lang/python:3.11 and :3.12 side by side), so
this is normal, not a conflict. A slot conflict only exists when two
atoms land on the IDENTICAL slot but need incompatible versions -- the
second atom's own constraint doesn't accept the version the first one
already resolved for that slot (see resolve_pretend_graph's
resolved_slots dict and its returned "slot_conflicts" list). Blocker
atoms (!/!!) are matched (see resolve_blockers) against installed
packages and this same graph's own New/Upgrade set -- reusing the real
match_from_list directly, since it ignores an atom's blocker marker
entirely (verified empirically) -- purely for reporting: no attempt is
made to resolve or enforce a blocker or slot conflict, matching real
--pretend's own "calculate and show, don't touch anything" behavior.

This is NOT a wrapper around the real `emerge` binary (unlike the
Python-side harnesses for versions/atom/use_reduce, which wrap real
production code): the whole point of this slice is that config.py's and
depgraph.py's real machinery is deliberately not being exercised yet, so
there is no real code to wrap for the top-level resolution algorithm this
script implements. It does reuse real portage code where it exists at the
right granularity: portage.versions.vercmp for version ordering,
portage.dep.Atom/match_from_list for atom parsing and matching, and
portage.dep.use_reduce for DEPEND/RDEPEND flattening.

Usage mirrors the real emerge CLI (and the Rust multicall's `emerge`
applet) directly:
    emerge_pretend_reference.py --pretend <category/package>

Config/target roots come from the real PORTAGE_CONFIGROOT/ROOT environment
variables, defaulting to "/" -- see lib/portage/const.py.
"""

import configparser
import os
import re
import sys
from collections import deque

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "..", "lib"))

from portage.dep import Atom, match_from_list, use_reduce
from portage.exception import InvalidAtom, InvalidDependString
from portage.versions import vercmp


class ResolutionError(Exception):
    pass


def _config_root():
    return os.environ.get("PORTAGE_CONFIGROOT") or "/"


def _root():
    return os.environ.get("ROOT") or "/"


def find_repos(config_root):
    """Parses repos.conf and returns every [reponame] section that has a
    location (the main repo plus any overlays) as a list of dicts with
    "name"/"location"/"priority", sorted ascending by (priority, name) --
    matching real portage's own prepos_order (see
    lib/portage/repository/config.py), which is also the order
    list_candidates below iterates them in, so a tie between two repos
    providing the identical version is broken toward the higher-priority
    one. Mirrors portage-repo/src/lib.rs's find_repos exactly."""
    repos_conf = os.path.join(config_root, "etc", "portage", "repos.conf")
    if os.path.isdir(repos_conf):
        files = sorted(
            os.path.join(repos_conf, name)
            for name in os.listdir(repos_conf)
            if os.path.isfile(os.path.join(repos_conf, name))
        )
    elif os.path.isfile(repos_conf):
        files = [repos_conf]
    else:
        raise ResolutionError(f"no repos.conf found at {repos_conf}")

    parser = configparser.ConfigParser()
    parser.read(files)

    main_repo = parser.defaults().get("main-repo")
    if not main_repo:
        raise ResolutionError("no [DEFAULT] main-repo in repos.conf")

    repos = []
    for name in parser.sections():
        location = parser.get(name, "location", fallback=None)
        if location is None:
            continue
        # Real repos.conf always uses absolute locations; relative ones
        # are a pilot/testing convenience -- see the matching comment in
        # portage-repo/src/lib.rs.
        if not os.path.isabs(location):
            location = os.path.join(config_root, location)
        # An explicit "priority" wins; otherwise the main repo defaults
        # to -1000 (real portage's own default -- see
        # lib/portage/repository/config.py) and every other repo to 0.
        priority_str = parser.get(name, "priority", fallback=None)
        try:
            priority = int(priority_str) if priority_str is not None else None
        except ValueError:
            priority = None
        if priority is None:
            priority = -1000 if name == main_repo else 0
        repos.append({"name": name, "location": location, "priority": priority})

    if not any(r["name"] == main_repo for r in repos):
        raise ResolutionError(f'no location for repo "{main_repo}" in repos.conf')

    repos.sort(key=lambda r: (r["priority"], r["name"]))
    return repos


def _read_config_lines(path):
    """Reads every non-comment, non-blank, trimmed line from `path`, which
    may be a single file or a directory of files merged in sorted-filename
    order. A missing path yields an empty list, not an error."""

    def read_file_lines(file_path):
        with open(file_path) as f:
            return [
                line.strip()
                for line in f
                if line.strip() and not line.strip().startswith("#")
            ]

    lines = []
    if os.path.isdir(path):
        for name in sorted(os.listdir(path)):
            file_path = os.path.join(path, name)
            if os.path.isfile(file_path):
                lines.extend(read_file_lines(file_path))
    elif os.path.isfile(path):
        lines.extend(read_file_lines(path))
    return lines


def _load_package_mask(config_root):
    path = os.path.join(config_root, "etc", "portage", "package.mask")
    result = []
    for line in _read_config_lines(path):
        if line.startswith("-"):
            removed = line[1:]
            result = [x for x in result if x != removed]
        else:
            result.append(line)
    return result


def _load_package_unmask(config_root):
    path = os.path.join(config_root, "etc", "portage", "package.unmask")
    return [line for line in _read_config_lines(path) if not line.startswith("-")]


def _load_package_accept_keywords(config_root):
    path = os.path.join(config_root, "etc", "portage", "package.accept_keywords")
    result = []
    for line in _read_config_lines(path):
        parts = line.split()
        atom, keywords = parts[0], parts[1:]
        if not keywords:
            continue
        result.append((atom, keywords))
    return result


def _load_package_use(config_root):
    path = os.path.join(config_root, "etc", "portage", "package.use")
    result = []
    for line in _read_config_lines(path):
        parts = line.split()
        atom, tokens = parts[0], parts[1:]
        if not tokens:
            continue
        result.append((atom, tokens))
    return result


def _strip_version_prefix(dir_name, package):
    """A directory entry is only accepted as "<package>-<version>" if what
    follows the prefix looks like a version (starts with a digit) --
    otherwise a package like "foo" would wrongly absorb a sibling
    package's directory like "foo-bar-2.0". See the matching comment in
    portage-repo/src/lib.rs."""
    prefix = package + "-"
    if not dir_name.startswith(prefix):
        return None
    rest = dir_name[len(prefix) :]
    if rest[:1].isdigit():
        return rest
    return None


def read_md5_cache(repo_location, category, pf):
    path = os.path.join(repo_location, "metadata", "md5-cache", category, pf)
    result = {}
    with open(path) as f:
        for line in f:
            key, sep, value = line.rstrip("\n").partition("=")
            if sep:
                result[key] = value
    return result


def list_candidates(repos, category, package):
    """Lists every version of category/package that has an ebuild in ANY
    of `repos`, with metadata (KEYWORDS, SLOT) from each repo's own
    md5-cache -- mirroring real portdbapi.cp_list, which gathers
    candidates from every configured repo the same way, not just the
    first one that has the package. `repos` is iterated in the order
    given (see find_repos's ascending (priority, name) sort), and each
    resulting candidate remembers which repo it came from (repo_location/
    repo_priority) -- needed once there's more than one, both to re-read
    that exact package's own DEPEND/RDEPEND later and to break a
    same-version tie toward the higher-priority repo. Mirrors
    portage-repo/src/lib.rs's list_candidates exactly."""
    candidates = []
    for repo in repos:
        pkg_dir = os.path.join(repo["location"], category, package)
        if not os.path.isdir(pkg_dir):
            continue
        for name in os.listdir(pkg_dir):
            if not name.endswith(".ebuild"):
                continue
            stem = name[: -len(".ebuild")]
            version = _strip_version_prefix(stem, package)
            if version is None:
                continue
            try:
                metadata = read_md5_cache(repo["location"], category, stem)
            except OSError:
                continue
            keywords = metadata.get("KEYWORDS", "").split()
            slot = metadata.get("SLOT", "0").split("/")[0]
            candidates.append(
                {
                    "version": version,
                    "keywords": keywords,
                    "slot": slot,
                    "repo_location": repo["location"],
                    "repo_priority": repo["priority"],
                }
            )
    return candidates


def _matches_config_entry(entry, candidate_str, category, package):
    """Whether `entry` (a package.mask/.unmask/.accept_keywords line)
    matches this candidate. See the module doc comment: unlike the Rust
    side, this reuses real Atom(allow_wildcard=True) + match_from_list
    directly rather than needing a separate bounded wildcard fallback."""
    try:
        atom = Atom(entry, allow_wildcard=True)
    except InvalidAtom:
        return False
    return bool(match_from_list(atom, [candidate_str]))


def is_visible(candidate, category, package, config):
    """A candidate is visible if it isn't masked (matches a package.mask
    entry and no package.unmask entry) and its KEYWORDS intersect the
    accepted set -- the global config["accept_keywords"], plus any extra
    keywords contributed by a matching package.accept_keywords entry,
    with a "**" token in such an entry meaning "accept unconditionally"
    for matching candidates (even ones with empty/no KEYWORDS)."""
    candidate_str = f"{category}/{package}-{candidate['version']}:{candidate['slot']}"

    masked = any(
        _matches_config_entry(m, candidate_str, category, package)
        for m in config["package_mask"]
    ) and not any(
        _matches_config_entry(u, candidate_str, category, package)
        for u in config["package_unmask"]
    )
    if masked:
        return False

    accept_any = False
    extra_keywords = set()
    for atom, keywords in config["package_accept_keywords"]:
        if _matches_config_entry(atom, candidate_str, category, package):
            if "**" in keywords:
                accept_any = True
            extra_keywords.update(keywords)
    if accept_any:
        return True

    return bool((config["accept_keywords"] | extra_keywords) & set(candidate["keywords"]))


def effective_use_flags(base, package_use, candidate_str, category, package):
    """The USE flags in effect for one specific package: `base` with every
    matching package.use entry's tokens layered on top, in file order, via
    the same incremental -flag/flag/+flag semantics USE itself uses (see
    _apply_incremental). Applied per package, mirroring
    portage-repo/src/lib.rs's effective_use_flags exactly -- a package.use
    entry never affects any other package's own resolution."""
    use_flags = set(base)
    for entry, tokens in package_use:
        if _matches_config_entry(entry, candidate_str, category, package):
            _apply_incremental(" ".join(tokens), use_flags)
    return use_flags


def _max_version(versions):
    best = versions[0]
    for v in versions[1:]:
        if (vercmp(v, best) or 0) > 0:
            best = v
    return best


def installed_candidates(root, category, package):
    """Lists every installed (version, slot) pair for category/package,
    reading each entry's SLOT file (defaulting to "0" if missing, same
    fallback as list_candidates). Used for blocker matching, which needs
    slots to support slotted blocker atoms -- installed_versions below
    doesn't need this and stays a plain version list for its existing
    callers. Mirrors portage-repo/src/lib.rs's installed_candidates."""
    cat_dir = os.path.join(root, "var", "db", "pkg", category)
    if not os.path.isdir(cat_dir):
        return []
    candidates = []
    for name in os.listdir(cat_dir):
        entry_dir = os.path.join(cat_dir, name)
        if not os.path.isdir(entry_dir):
            continue
        version = _strip_version_prefix(name, package)
        if version is None:
            continue
        try:
            with open(os.path.join(entry_dir, "SLOT")) as f:
                slot = f.read().strip().split("/")[0] or "0"
        except OSError:
            slot = "0"
        candidates.append((version, slot))
    return candidates


def installed_versions(root, category, package):
    return [version for version, _slot in installed_candidates(root, category, package)]


_VAR_REF_RE = re.compile(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}")
_KEY_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")


def _substitute(value, scalars):
    """Substitutes ${VARNAME} references against `scalars`, matching
    bash's default (unset-as-empty) behavior for unknown variables."""
    return _VAR_REF_RE.sub(lambda m: scalars.get(m.group(1), ""), value)


def _parse_kv_line(line):
    """Parses one KEY="value" / KEY='value' / KEY=value line. Returns
    None for comments, blank lines, or anything that isn't a simple
    assignment."""
    line = line.strip()
    if not line or line.startswith("#") or "=" not in line:
        return None
    key, _, value = line.partition("=")
    key = key.strip()
    if not _KEY_RE.match(key):
        return None
    value = value.strip()
    if len(value) >= 2 and (
        (value[0] == '"' and value[-1] == '"') or (value[0] == "'" and value[-1] == "'")
    ):
        value = value[1:-1]
    return key, value


def _apply_incremental(tokens, target_set):
    """Applies real incremental-variable token semantics: "-*" clears
    everything accumulated so far, "-flag" removes, "flag"/"+flag" adds."""
    for tok in tokens.split():
        if tok == "-*":
            target_set.clear()
        elif tok.startswith("-"):
            target_set.discard(tok[1:])
        elif tok.startswith("+"):
            if tok[1:]:
                target_set.add(tok[1:])
        else:
            target_set.add(tok)


def _process_config_lines(text, scalars, use_flags, accept_keywords):
    for line in text.splitlines():
        parsed = _parse_kv_line(line)
        if parsed is None:
            continue
        key, raw_value = parsed
        value = _substitute(raw_value, scalars)
        if key == "USE":
            _apply_incremental(value, use_flags)
        elif key == "ACCEPT_KEYWORDS":
            _apply_incremental(value, accept_keywords)
        scalars[key] = value


def _read_parent_lines(profile_dir):
    parent_path = os.path.join(profile_dir, "parent")
    if not os.path.isfile(parent_path):
        return []
    with open(parent_path) as f:
        return [line.strip() for line in f if line.strip() and not line.strip().startswith("#")]


def _visit_profile(directory, visited, chain):
    canon = os.path.realpath(directory)
    if not os.path.isdir(canon):
        raise ResolutionError(f"resolving profile {directory}: not a directory")
    if canon in visited:
        return
    visited.add(canon)
    for parent in _read_parent_lines(canon):
        if ":" in parent:
            raise ResolutionError(
                f'cross-repo profile parent "{parent}" (referenced from {canon}) '
                "is out of v1 scope"
            )
        _visit_profile(os.path.join(canon, parent), visited, chain)
    chain.append(canon)


def _resolve_profile_chain(leaf):
    visited = set()
    chain = []
    _visit_profile(leaf, visited, chain)
    return chain


def _process_make_conf_file(path, config_root, scalars, use_flags, accept_keywords, visited_sources):
    """Resolves "source <path>" against config_root as if it were "/"
    (chroot-style), matching PORTAGE_CONFIGROOT/ROOT semantics elsewhere
    in this pilot. A missing sourced file is silently skipped."""
    if not os.path.isfile(path):
        return
    canon = os.path.realpath(path)
    if canon in visited_sources:
        return
    visited_sources.add(canon)
    with open(canon) as f:
        text = f.read()
    for line in text.splitlines():
        trimmed = line.strip()
        if trimmed.startswith("source "):
            sourced = trimmed[len("source ") :].strip()
            if os.path.isabs(sourced):
                resolved = os.path.join(config_root, sourced.lstrip("/"))
            else:
                resolved = os.path.join(os.path.dirname(canon), sourced)
            _process_make_conf_file(
                resolved, config_root, scalars, use_flags, accept_keywords, visited_sources
            )
            continue
        parsed = _parse_kv_line(trimmed)
        if parsed is None:
            continue
        key, raw_value = parsed
        value = _substitute(raw_value, scalars)
        if key == "USE":
            _apply_incremental(value, use_flags)
        elif key == "ACCEPT_KEYWORDS":
            _apply_incremental(value, accept_keywords)
        scalars[key] = value


def resolve_config(config_root):
    """Computes real USE/ACCEPT_KEYWORDS/package.mask/.unmask/
    .accept_keywords: the profile chain rooted at
    <config_root>/etc/portage/make.profile (if it exists), then
    <config_root>/etc/portage/make.conf (if it exists) as the final,
    highest-priority USE/ACCEPT_KEYWORDS layer, then package.*. Own
    implementation (not a wrapper around real config.py), mirroring
    portage-profile/src/lib.rs's resolve_config exactly -- see that
    crate's doc comment for the full algorithm and its documented scope
    cuts. Returns a dict with keys "use_flags", "accept_keywords",
    "package_mask", "package_unmask", "package_accept_keywords"."""
    use_flags = set()
    accept_keywords = set()
    scalars = {}

    make_profile = os.path.join(config_root, "etc", "portage", "make.profile")
    if os.path.exists(make_profile):
        for level in _resolve_profile_chain(make_profile):
            make_defaults = os.path.join(level, "make.defaults")
            if not os.path.isfile(make_defaults):
                continue
            # Real config.py quirk: USE is excluded from cross-level
            # substitution -- see the module doc comment.
            scalars.pop("USE", None)
            with open(make_defaults) as f:
                text = f.read()
            _process_config_lines(text, scalars, use_flags, accept_keywords)

    make_conf = os.path.join(config_root, "etc", "portage", "make.conf")
    if os.path.isfile(make_conf):
        _process_make_conf_file(make_conf, config_root, scalars, use_flags, accept_keywords, set())

    return {
        "use_flags": use_flags,
        "accept_keywords": accept_keywords,
        "package_mask": _load_package_mask(config_root),
        "package_unmask": _load_package_unmask(config_root),
        "package_accept_keywords": _load_package_accept_keywords(config_root),
        "package_use": _load_package_use(config_root),
    }


def _best_candidate(candidates):
    """Picks the best of `candidates` by version, breaking a tie on an
    identical version toward the higher-priority repo -- mirroring
    portage-repo/src/lib.rs's max_by(vercmp_ordering(...).then(repo_priority))
    exactly, since more than one repo can now provide the identical
    version."""
    best = candidates[0]
    for c in candidates[1:]:
        cmp = vercmp(c["version"], best["version"]) or 0
        if cmp > 0 or (cmp == 0 and c["repo_priority"] > best["repo_priority"]):
            best = c
    return best


def resolve_pretend(repos, root, atom_str, config):
    """The single-atom v1 resolution decision: find the best visible
    candidate matching `atom_str` (any atom portage-dep's v1 grammar
    supports -- operator, slot, not just a bare category/package) across
    all of `repos` (the main repo and any overlays -- see find_repos),
    compare it against what's installed. Returns a tuple whose first
    element is the outcome tag: "new", "upgrade", "already_installed", or
    "no_visible_candidate"."""
    atom = _parse_atom(atom_str)
    if atom is None:
        raise ResolutionError(f'invalid atom "{atom_str}"')
    category, package = atom.cp.split("/", 1)

    candidates = list_candidates(repos, category, package)
    visible = [c for c in candidates if is_visible(c, category, package, config)]
    if not visible:
        return ("no_visible_candidate",)

    # Reuses the real match_from_list rather than re-deriving
    # version/slot matching rules here, mirroring portage-repo's Rust
    # side exactly.
    candidate_strs = [
        f"{category}/{package}-{c['version']}:{c['slot']}" for c in visible
    ]
    by_str = dict(zip(candidate_strs, visible))
    matched = [by_str[m] for m in match_from_list(atom_str, candidate_strs) if m in by_str]
    if not matched:
        return ("no_visible_candidate",)
    best = _best_candidate(matched)["version"]

    installed = installed_versions(root, category, package)
    if best in installed:
        return ("already_installed", best)
    if installed:
        return ("upgrade", _max_version(installed), best)
    return ("new", best)


def resolve_blockers(root, pending, entries):
    """Matches each `pending` blocker's target category/package against
    both currently-installed candidates (installed_candidates) and this
    graph's own resolved New/Upgrade set (entries, which may now hold
    more than one slot for the same category/package -- every one of
    them is a real candidate, not just the first), reusing the real
    match_from_list exactly as every other atom-vs-candidate check in
    this module does (it ignores an atom's blocker marker entirely --
    verified empirically -- so a "!"/"!!"-prefixed atom string matches
    candidates by category/package/version/slot exactly like a normal
    one). A match against the owner package's own resolved version is
    dropped defensively (a package blocking itself is nonsensical, but
    cheap to guard against). Returns (owner_key, conflict_dict) pairs.
    Mirrors portage-repo/src/lib.rs's resolve_blockers exactly."""
    conflicts = []
    for pb in pending:
        target_key = (pb["target_category"], pb["target_package"])
        candidates = list(
            installed_candidates(root, pb["target_category"], pb["target_package"])
        )
        for category, package, outcome, _blockers, slot in entries:
            if (category, package) != target_key:
                continue
            if outcome[0] == "new":
                version = outcome[1]
            elif outcome[0] == "upgrade":
                version = outcome[2]
            else:
                continue
            if slot is None:
                continue
            if (version, slot) not in candidates:
                candidates.append((version, slot))
        candidate_strs = [
            f"{pb['target_category']}/{pb['target_package']}-{v}:{s}" for v, s in candidates
        ]
        matched = match_from_list(pb["atom_str"], candidate_strs)
        by_str = dict(zip(candidate_strs, candidates))
        for m in matched:
            matched_version, _matched_slot = by_str[m]
            if target_key == pb["owner_key"] and matched_version == pb["owner_version"]:
                continue
            conflicts.append(
                (
                    pb["owner_key"],
                    {
                        "atom_str": pb["atom_str"],
                        "strong": pb["strong"],
                        "matched_category": pb["target_category"],
                        "matched_package": pb["target_package"],
                        "matched_version": matched_version,
                    },
                )
            )
    return conflicts


def resolve_pretend_graph(config_root, root, atom_str, config):
    """Recursively resolves `atom_str` and -- for packages that would
    newly merge or upgrade -- its DEPEND+RDEPEND atoms, breadth-first.
    Returns a dict with keys "entries" (a list of (category, package,
    outcome, blockers, slot) tuples, one per distinct category/package/
    slot combination visited, in discovery order -- unlike a package name
    alone, two DIFFERENT slots of the same package are both real,
    independent entries, mirroring how real portage genuinely allows
    multiple slots of the same package to coexist in one merge list) and
    "slot_conflicts" (a list of conflict dicts -- see below). `blockers`
    is a list of conflict dicts (see resolve_blockers), `slot` is the
    resolved SLOT string; both are only ever non-empty/non-None for
    New/Upgrade entries. See the module doc comment for the recursion's
    documented scope cuts.

    A slot conflict is when two different atoms land on the identical
    category/package/slot but need incompatible versions -- the second
    atom's own constraint doesn't accept the version the first one
    already caused to be resolved (and recursed into). This is distinct
    from two atoms simply requesting different slots (not a conflict at
    all -- see above). Purely informational, same "report, don't enforce"
    spirit as blockers: real portage's own depgraph treats an unresolved
    slot conflict as fatal; this pilot instead reports it and keeps
    going, using whichever version was resolved first. Mirrors
    portage-repo/src/lib.rs's resolve_pretend_graph exactly."""
    repos = find_repos(config_root)

    # Guards against infinite requeuing (e.g. a dependency cycle): the
    # exact same atom text is only ever resolved once -- deliberately
    # coarser than the (category, package, slot) dedup below, which
    # exists to decide whether a given slot has already been fully
    # resolved, not just to guarantee termination.
    visited_atoms = set()
    # (category, package, slot) -> index into entries, for New/Upgrade
    # outcomes only. The first atom to resolve a given slot "wins" (its
    # version is what gets recursed into); every later atom landing on
    # the same slot is checked against that already-resolved version
    # (see slot_conflicts) instead of triggering a second, independent
    # resolution.
    resolved_slots = {}
    # (category, package) -> already added an AlreadyInstalled/
    # NoVisibleCandidate entry for it -- neither outcome carries a slot
    # to usefully key repeats by.
    other_outcomes = set()

    entries = []
    slot_conflicts = []
    queue = deque([atom_str])
    pending_blockers = []

    while queue:
        current_atom_str = queue.popleft()
        atom = _parse_atom(current_atom_str)
        if atom is None:
            continue
        if atom.blocker:
            continue
        if current_atom_str in visited_atoms:
            continue
        visited_atoms.add(current_atom_str)
        category, package = atom.cp.split("/", 1)
        key = (category, package)

        outcome = resolve_pretend(repos, root, current_atom_str, config)

        if outcome[0] == "new":
            version = outcome[1]
        elif outcome[0] == "upgrade":
            version = outcome[2]
        else:
            # AlreadyInstalled / NoVisibleCandidate: no slot to key a
            # repeat by, so dedup on category/package alone, same as v1
            # always did before slot-aware resolution existed.
            if key in other_outcomes:
                continue
            other_outcomes.add(key)
            entries.append((category, package, outcome, [], None))
            continue

        # The resolved version may have come from any of `repos` (not
        # necessarily the main one), so re-derive which repo it actually
        # lives in -- reusing list_candidates rather than threading a
        # repo location back out of resolve_pretend's outcome tuple,
        # since more than one repo could in principle carry the identical
        # version, tie-broken the same way resolve_pretend itself does.
        repo_candidates = [c for c in list_candidates(repos, category, package) if c["version"] == version]
        if not repo_candidates:
            continue
        resolved = max(repo_candidates, key=lambda c: c["repo_priority"])
        slot = resolved["slot"]
        repo_location = resolved["repo_location"]

        slot_key = (category, package, slot)
        if slot_key in resolved_slots:
            # This exact category/package/slot was already resolved by
            # an earlier atom. If the current atom's own constraint
            # doesn't accept that already-resolved version, it's a real
            # slot conflict -- report it and move on, without a second,
            # independent resolution or any attempt to reconcile the two.
            existing_idx = resolved_slots[slot_key]
            existing_outcome = entries[existing_idx][2]
            existing_version = (
                existing_outcome[1] if existing_outcome[0] == "new" else existing_outcome[2]
            )
            existing_str = f"{category}/{package}-{existing_version}:{slot}"
            satisfied = bool(match_from_list(current_atom_str, [existing_str]))
            if not satisfied:
                slot_conflicts.append(
                    {
                        "category": category,
                        "package": package,
                        "slot": slot,
                        "resolved_version": existing_version,
                        "conflicting_atom": current_atom_str,
                    }
                )
            continue
        resolved_slots[slot_key] = len(entries)
        entries.append((category, package, outcome, [], slot))

        pf = f"{package}-{version}"
        try:
            metadata = read_md5_cache(repo_location, category, pf)
        except OSError:
            continue
        depstr = " ".join(metadata[k] for k in ("DEPEND", "RDEPEND") if metadata.get(k))
        candidate_str = f"{category}/{package}-{version}:{slot}"
        use_flags = effective_use_flags(
            config["use_flags"], config["package_use"], candidate_str, category, package
        )
        try:
            flat_deps = use_reduce(depstr, flat=True, uselist=use_flags)
        except InvalidDependString:
            continue
        for tok in flat_deps:
            if tok == "||":
                continue
            dep_atom = _parse_atom(tok)
            if dep_atom is not None and dep_atom.blocker:
                pending_blockers.append(
                    {
                        "atom_str": tok,
                        # blocker.overlap.forbid is real portage's own
                        # strong-vs-weak signal (see
                        # lib/_emerge/resolver/output.py's "hard blocking"
                        # vs "soft blocking"), not the "!!" prefix text.
                        "strong": bool(dep_atom.blocker.overlap.forbid),
                        "target_category": dep_atom.cp.split("/", 1)[0],
                        "target_package": dep_atom.cp.split("/", 1)[1],
                        "owner_key": key,
                        "owner_version": version,
                    }
                )
                continue
            queue.append(tok)

    # setdefault (not a dict comprehension) so the *first* entry for a
    # given owner wins when the same category/package appears more than
    # once (multiple slots) -- mirrors portage-repo/src/lib.rs's
    # `entries.iter_mut().find(...)`, which also attaches to the first
    # match.
    blockers_by_owner = {}
    for category, package, _o, blockers, _slot in entries:
        blockers_by_owner.setdefault((category, package), blockers)
    for owner_key, conflict in resolve_blockers(root, pending_blockers, entries):
        blockers_by_owner[owner_key].append(conflict)

    return {"entries": entries, "slot_conflicts": slot_conflicts}


def _parse_atom(atom_str):
    """Uses the real Atom parser (same grammar the Rust side's portage-dep
    crate was verified against) so the accept/reject boundary matches
    exactly, not just the happy path. Returns an Atom, or None if it
    doesn't parse at all (distinct from parsing but using a feature v1
    doesn't support -- see _is_bare_atom -- mirroring how the Rust side
    separates "invalid atom" from "only a bare atom is supported")."""
    try:
        return Atom(atom_str, allow_wildcard=True)
    except InvalidAtom:
        return None


def _is_bare_atom(a):
    """v1 only supports a bare category/package atom -- no operator, no
    slot, no version, no USE deps, no blocker, no repo/wildcard/build-id."""
    return not (
        a.operator is not None
        or a.slot is not None
        or a.use is not None
        or a.repo is not None
        or a.extended_syntax
        or a.build_id is not None
        or a.blocker
    )


def run(args):
    atom_arg = None
    pretend = False

    for arg in args:
        if arg in ("--pretend", "-p"):
            pretend = True
        elif not arg.startswith("-"):
            if atom_arg is not None:
                print(
                    "emerge (pilot v1): only a single package atom is supported",
                    file=sys.stderr,
                )
                return 2
            atom_arg = arg
        else:
            print(
                f'emerge (pilot v1): unsupported option "{arg}" '
                "(only --pretend/-p is implemented)",
                file=sys.stderr,
            )
            return 2

    if not pretend:
        print(
            "emerge (pilot v1): only --pretend is implemented "
            "(no real merges yet, see PROMPT.md)",
            file=sys.stderr,
        )
        return 2

    if atom_arg is None:
        print(
            "emerge (pilot v1): expected a package atom, e.g. "
            "`emerge --pretend cat/pkg`",
            file=sys.stderr,
        )
        return 2

    atom = _parse_atom(atom_arg)
    if atom is None:
        print(f'emerge: invalid atom "{atom_arg}"', file=sys.stderr)
        return 1
    if not _is_bare_atom(atom):
        print(
            "emerge (pilot v1): only a bare category/package atom is "
            f'supported, got "{atom_arg}"',
            file=sys.stderr,
        )
        return 2
    try:
        config = resolve_config(_config_root())
        result = resolve_pretend_graph(_config_root(), _root(), atom_arg, config)
    except ResolutionError as e:
        print(f"emerge: {e}", file=sys.stderr)
        return 1
    entries = result["entries"]

    # resolve_pretend_graph's BFS always visits the requested atom first,
    # so entries[0] is the top-level package; its outcome keeps the exact
    # messages/exit codes the single-atom (no-deps) case always had.
    top_category, top_package, top_outcome, _top_blockers, _top_slot = entries[0]
    if top_outcome[0] == "no_visible_candidate":
        print(f'!!! no visible ebuild for "{top_category}/{top_package}"', file=sys.stderr)
        return 1
    if top_outcome[0] == "already_installed" and len(entries) == 1:
        print(
            f"{top_category}/{top_package}-{top_outcome[1]} is already installed; "
            "nothing to do"
        )
        return 0

    def print_blockers(category, package, owner_version, blockers):
        # Purely informational (see resolve_pretend_graph's doc comment):
        # v1 neither refuses nor changes the exit code for a blocker
        # match, strong or weak.
        for b in blockers:
            strength = "hard" if b["strong"] else "soft"
            print(
                f"[blocks] {category}/{package}-{owner_version} {strength} blocks "
                f"{b['matched_category']}/{b['matched_package']}-{b['matched_version']} "
                f'("{b["atom_str"]}")'
            )

    for category, package, outcome, blockers, _slot in entries:
        tag = outcome[0]
        if tag == "new":
            print(f"[ebuild  N] {category}/{package}-{outcome[1]}")
            print_blockers(category, package, outcome[1], blockers)
        elif tag == "upgrade":
            print(f"[ebuild  U] {category}/{package}-{outcome[2]} (upgrade from {outcome[1]})")
            print_blockers(category, package, outcome[2], blockers)
        elif tag == "already_installed":
            # Already-satisfied dependencies aren't shown, matching real
            # emerge's usual "don't clutter the list" behavior -- the
            # top-level already-installed case is handled above instead.
            pass
        else:
            print(
                f'!!! no visible ebuild for dependency "{category}/{package}"',
                file=sys.stderr,
            )

    # Purely informational, same as blockers -- see resolve_pretend_graph's
    # doc comment: v1 neither refuses nor changes the exit code for a slot
    # conflict.
    for c in result["slot_conflicts"]:
        print(
            f"[slot conflict] {c['category']}/{c['package']}:{c['slot']} resolved to "
            f"{c['category']}/{c['package']}-{c['resolved_version']}, which does not "
            f'satisfy "{c["conflicting_atom"]}"'
        )
    return 0


if __name__ == "__main__":
    sys.exit(run(sys.argv[1:]))
