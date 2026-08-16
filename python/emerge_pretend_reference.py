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
package, not globally: each package's own dependency strings are flattened
against its own effective USE set (base config["use_flags"] plus any
matching package.use entry's tokens), never leaking into a sibling or
dependency's own resolution.

Dependency recursion (see resolve_pretend_graph) walks all five real
dependency-string keys -- DEPEND, RDEPEND, BDEPEND, PDEPEND, IDEPEND --
concatenated and flattened together with no distinction between them:
real portage's own merge ordering treats these differently, but that's
meaningless for a --pretend-only pilot with no real merge ordering to
begin with, so v1 treats all five uniformly as "a dependency this package
needs, resolve and report it". Flattening itself uses the real
portage.dep.use_reduce(flat=True), with its own documented scope cuts
mirrored exactly from portage-repo/src/lib.rs's resolve_pretend_graph doc
comment: || (any-of) groups resolve every alternative rather than picking
one (flat mode discards group boundaries, so there's no reliable way to
identify "the first" alternative from its output), cycles/duplicates (by
exact atom text) are deduped via a visited set, and a dependency's own
deps are only walked if it would newly merge or upgrade.
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
portage.dep.use_reduce for dependency-string flattening.

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
    "name"/"location"/"priority"/"is_main", sorted ascending by
    (priority, name) -- matching real portage's own prepos_order (see
    lib/portage/repository/config.py), which is also the order
    list_candidates below iterates them in, so a tie between two repos
    providing the identical version is broken toward the higher-priority
    one. "is_main" (whether this is repos.conf's [DEFAULT] main-repo) is
    needed by resolve_config's own repo-level package.mask/.unmask
    source. Mirrors portage-repo/src/lib.rs's find_repos exactly."""
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
        repos.append(
            {
                "name": name,
                "location": location,
                "priority": priority,
                "is_main": name == main_repo,
            }
        )

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


def _stack_mask_lines(sources):
    """Stacks ordered package.mask/.unmask lines from multiple sources
    (earlier sources first) with real portage's own -atom removal
    semantics -- see MaskManager.py's stack_lists(incremental=1): a
    -atom line removes the exact matching atom text added by ANY earlier
    source in this same stack, not just within its own source (e.g. a
    user-level -atom in package.mask can remove an atom the repo or a
    profile level added). Shared between package.mask and
    package.unmask, which real portage stacks identically -- unlike this
    pilot's previous, user-level-only package.unmask handling, which
    treated a leading "-" there as meaningless; it's meaningful once
    more than one source can contribute an unmask entry. Mirrors
    portage-profile/src/lib.rs's stack_mask_lines exactly."""
    result = []
    for lines in sources:
        for line in lines:
            if line.startswith("-"):
                removed = line[1:]
                result = [x for x in result if x != removed]
            else:
                result.append(line)
    return result


def _parse_package_accept_keywords_lines(lines):
    """A line with no keyword tokens after the atom is a documented v1
    no-op -- see portage-profile/src/lib.rs's
    parse_package_accept_keywords_lines for why this is a simplification
    only for the profile-level source (real portage gives a bare
    profile-level entry an implicit derived "~arch" meaning a bare
    user-level entry never gets), kept simple and symmetric between the
    two here rather than adding a profile-only special case."""
    result = []
    for line in lines:
        parts = line.split()
        atom, keywords = parts[0], parts[1:]
        if not keywords:
            continue
        result.append((atom, keywords))
    return result


def _parse_package_use_lines(lines):
    """A line with no tokens after the atom is a documented no-op,
    matching _parse_package_accept_keywords_lines. Purely additive across
    sources, like package.accept_keywords and unlike package.mask/
    .unmask: real portage's own package.use consumption only ever
    .extend()s a growing token list per source, never removes a previous
    entry. Mirrors portage-profile/src/lib.rs's parse_package_use_lines
    exactly."""
    result = []
    for line in lines:
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


def resolve_config(config_root, main_repo_location):
    """Computes real USE/ACCEPT_KEYWORDS/package.mask/.unmask/
    .accept_keywords: the profile chain rooted at
    <config_root>/etc/portage/make.profile (if it exists), then
    <config_root>/etc/portage/make.conf (if it exists) as the final,
    highest-priority USE/ACCEPT_KEYWORDS layer, then package.*. Own
    implementation (not a wrapper around real config.py), mirroring
    portage-profile/src/lib.rs's resolve_config exactly -- see that
    crate's doc comment for the full algorithm and its documented scope
    cuts. Returns a dict with keys "use_flags", "accept_keywords",
    "package_mask", "package_unmask", "package_accept_keywords".

    main_repo_location (the main repo's own tree root -- see
    find_repos/is_main) is needed for package.mask/.unmask's repo-level
    source, <main_repo_location>/profiles/package.mask -- real portage's
    most common real-world masking source. It's stacked together with
    every profile level's own package.mask/.unmask (in chain order) and
    the user-level /etc/portage files, exactly matching real
    MaskManager.py's three-source stack (see _stack_mask_lines). An
    overlay repo's own repo-level package.mask/.unmask stays deliberately
    out of scope, same as the rest of overlays' per-repo config -- only
    the one main repo's is read here."""
    use_flags = set()
    accept_keywords = set()
    scalars = {}

    make_profile = os.path.join(config_root, "etc", "portage", "make.profile")
    chain = _resolve_profile_chain(make_profile) if os.path.exists(make_profile) else []
    for level in chain:
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

    mask_sources = [
        _read_config_lines(os.path.join(main_repo_location, "profiles", "package.mask"))
    ]
    unmask_sources = [
        _read_config_lines(os.path.join(main_repo_location, "profiles", "package.unmask"))
    ]
    for level in chain:
        mask_sources.append(_read_config_lines(os.path.join(level, "package.mask")))
        unmask_sources.append(_read_config_lines(os.path.join(level, "package.unmask")))
    mask_sources.append(
        _read_config_lines(os.path.join(config_root, "etc", "portage", "package.mask"))
    )
    unmask_sources.append(
        _read_config_lines(os.path.join(config_root, "etc", "portage", "package.unmask"))
    )

    # package.accept_keywords: profile-chain (in chain order), then
    # user-level -- mirrors portage-profile/src/lib.rs's resolve_config
    # exactly (see its own comment for why, grounded in real
    # KeywordsManager.getPKeywords: no repo-level source exists for this
    # file in real portage at all, and it's purely additive -- no "-atom"
    # removal -- so concatenating lines before parsing is equivalent to
    # parsing each source separately and concatenating the results).
    accept_keywords_lines = []
    for level in chain:
        accept_keywords_lines.extend(
            _read_config_lines(os.path.join(level, "package.accept_keywords"))
        )
    accept_keywords_lines.extend(
        _read_config_lines(
            os.path.join(config_root, "etc", "portage", "package.accept_keywords")
        )
    )

    # package.use: repo-level, then every profile level's own package.use
    # (in chain order), then user-level -- same file-location convention
    # package.mask/package.accept_keywords both already use, and purely
    # additive like package.accept_keywords. Mirrors
    # portage-profile/src/lib.rs's resolve_config exactly, including the
    # same deliberate, confirmed-with-the-user simplification (a flat
    # concatenation, not real portage's own repo/defaults/pkg USE_ORDER
    # layering -- see that crate's own doc comment for the full
    # reasoning).
    use_lines = _read_config_lines(
        os.path.join(main_repo_location, "profiles", "package.use")
    )
    for level in chain:
        use_lines.extend(_read_config_lines(os.path.join(level, "package.use")))
    use_lines.extend(
        _read_config_lines(os.path.join(config_root, "etc", "portage", "package.use"))
    )

    return {
        "use_flags": use_flags,
        "accept_keywords": accept_keywords,
        "package_mask": _stack_mask_lines(mask_sources),
        "package_unmask": _stack_mask_lines(unmask_sources),
        "package_accept_keywords": _parse_package_accept_keywords_lines(accept_keywords_lines),
        "package_use": _parse_package_use_lines(use_lines),
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
        for category, package, outcome, _blockers, slot, _use_display in entries:
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


def resolve_pretend_graph(config_root, root, atoms, config):
    """Recursively resolves every atom in `atoms` and -- for packages that
    would newly merge or upgrade -- its DEPEND+RDEPEND+BDEPEND+PDEPEND+
    IDEPEND atoms, breadth-first. Returns a dict with keys "entries" (a
    list of (category, package, outcome, blockers, slot, use_display)
    tuples, one per distinct
    category/package/slot combination visited, in discovery order --
    unlike a package name alone, two DIFFERENT slots of the same package
    are both real, independent entries, mirroring how real portage
    genuinely allows multiple slots of the same package to coexist in one
    merge list) and "slot_conflicts" (a list of conflict dicts -- see
    below). `blockers` is a list of conflict dicts (see resolve_blockers),
    `slot` is the resolved SLOT string, `use_display` is a sorted list of
    (flag, enabled) pairs for this package's own IUSE-declared flags (for
    --pretend -v's USE="..." display -- see run() below); all three are
    only ever non-empty/non-None for New/Upgrade entries. See the module
    doc comment for the recursion's documented scope cuts.

    `atoms` seeds the BFS queue together, in the order given, before any
    dependency is ever pushed -- so all of them are dequeued and resolved
    first (level-order guarantee), and the existing visited-atom/
    resolved-slot/blocker bookkeeping below handles sharing between them
    for free, same as a diamond dependency. A top-level atom with no
    visible candidate raises ResolutionError (fatal to the whole call,
    matching real portage's own depgraph.py "there are no ebuilds to
    satisfy" behavior) rather than being reported-and-continued the way a
    *dependency's* NoVisibleCandidate is; since top-level atoms are always
    dequeued in argv order before any dependency, the first bad one aborts
    before any later atom is even attempted.

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
    top_level = set(atoms)

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
    queue = deque(atoms)
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

        # A top-level atom (as opposed to a dependency reached while
        # recursing) with no visible candidate aborts the whole call --
        # matching real portage's own depgraph.py behavior for an
        # unsatisfiable target, not the "report and keep going" treatment
        # a dependency's own NoVisibleCandidate gets a few lines down.
        if current_atom_str in top_level and outcome[0] == "no_visible_candidate":
            raise ResolutionError(f'there are no ebuilds to satisfy "{current_atom_str}".')

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
            entries.append((category, package, outcome, [], None, []))
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
        entry_idx = len(entries)
        resolved_slots[slot_key] = entry_idx
        entries.append((category, package, outcome, [], slot, []))

        pf = f"{package}-{version}"
        try:
            metadata = read_md5_cache(repo_location, category, pf)
        except OSError:
            continue
        depstr = " ".join(
            metadata[k]
            for k in ("DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND")
            if metadata.get(k)
        )
        candidate_str = f"{category}/{package}-{version}:{slot}"
        use_flags = effective_use_flags(
            config["use_flags"], config["package_use"], candidate_str, category, package
        )
        # IUSE's own "+flag"/"-flag" default markers only matter for
        # resolving a flag's default when nothing else decides it --
        # already handled upstream, wherever use_flags itself came from --
        # so display only needs the bare flag name, paired with whatever
        # use_flags (the real resolved set) says. Mirrors
        # portage-repo/src/lib.rs's resolve_pretend_graph exactly.
        if metadata.get("IUSE"):
            display = sorted(
                (flag.lstrip("+-"), flag.lstrip("+-") in use_flags)
                for flag in metadata["IUSE"].split()
            )
            entries[entry_idx] = (category, package, outcome, [], slot, display)
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
    for category, package, _o, blockers, _slot, _use_display in entries:
        blockers_by_owner.setdefault((category, package), blockers)
    for owner_key, conflict in resolve_blockers(root, pending_blockers, entries):
        blockers_by_owner[owner_key].append(conflict)

    return {"entries": entries, "slot_conflicts": slot_conflicts}


def _parse_atom(atom_str):
    """Uses the real Atom parser (same grammar the Rust side's portage-dep
    crate was verified against) so the accept/reject boundary matches
    exactly, not just the happy path. Returns an Atom, or None if it
    doesn't parse at all (distinct from parsing but using a feature v1
    doesn't support -- see _has_unsupported_top_level_features --
    mirroring how the Rust side separates "invalid atom" from "not a
    valid emerge target")."""
    try:
        return Atom(atom_str, allow_wildcard=True)
    except InvalidAtom:
        return None


# Enumerates the real `emerge` CLI's full option surface (see
# lib/_emerge/main.py: the `options` list, `shortmapping` dict,
# `argument_options` dict, and `actions` frozenset), so that using any
# real emerge flag this pilot doesn't implement yet produces a clear
# "recognized, but not implemented" message -- distinct from a
# genuinely unknown/misspelled flag. Only --pretend/-p and --verbose/-v
# are actually implemented (see run() below); every table here exists
# purely for recognition, not behavior. Mirrors
# PORTING/rust/multicall/src/emerge_options.rs's own copy of these same
# three tables exactly, so both sides report identical text for
# identical input (verified by the shared contract suite).
#
# KNOWN, DOCUMENTED SCOPE CUTS (see emerge_options.rs for the full
# writeup): no short-flag bundling ("-pv" isn't recognized as "-p" +
# "-v"); "category" (boolean/value/action) is tracked for accurate
# enumeration only -- run() reports and exits immediately on any
# recognized-but-unimplemented option, so it never needs to parse or
# skip over that option's own argument; --help/-h is recognized as a
# real (unimplemented) action, not given its own pilot help text.

_BOOLEAN_OPTIONS = [
    ("--alphabetical", None),
    ("--ask-enter-invalid", None),
    ("--buildpkgonly", "-B"),
    ("--changed-use", "-U"),
    ("--columns", None),
    ("--debug", "-d"),
    ("--digest", None),
    ("--emptytree", "-e"),
    ("--verbose-conflicts", None),
    ("--fetchonly", "-f"),
    ("--fetch-all-uri", "-F"),
    ("--ignore-default-opts", None),
    ("--noconfmem", None),
    ("--newrepo", None),
    ("--newuse", "-N"),
    ("--nobindeps", None),
    ("--nodeps", "-O"),
    ("--noreplace", "-n"),
    ("--nospinner", None),
    ("--oneshot", "-1"),
    ("--onlydeps", "-o"),
    ("--quiet-repo-display", None),
    ("--quiet-unmerge-warn", None),
    ("--resume", "-r"),
    ("--searchdesc", "-S"),
    ("--skipfirst", None),
    ("--tree", "-t"),
    ("--unordered-display", None),
    ("--update", "-u"),
    ("--update-if-installed", None),
    ("--cols", None),
    ("--skip-first", None),
]

_VALUE_OPTIONS = [
    ("--alert", "-A"),
    ("--ask", "-a"),
    ("--autounmask", None),
    ("--autounmask-backtrack", None),
    ("--autounmask-continue", None),
    ("--autounmask-only", None),
    ("--autounmask-license", None),
    ("--autounmask-unrestricted-atoms", None),
    ("--autounmask-use", None),
    ("--autounmask-keep-keywords", None),
    ("--autounmask-keep-masks", None),
    ("--autounmask-write", None),
    ("--accept-properties", None),
    ("--accept-restrict", None),
    ("--backtrack", None),
    ("--binpkg-changed-deps", None),
    ("--buildpkg", "-b"),
    ("--buildpkg-exclude", None),
    ("--changed-deps", None),
    ("--changed-deps-report", None),
    ("--changed-slot", None),
    ("--config-root", None),
    ("--color", None),
    ("--complete-graph", None),
    ("--complete-graph-if-new-use", None),
    ("--complete-graph-if-new-ver", None),
    ("--deep", "-D"),
    ("--depclean-lib-check", None),
    ("--deselect", "-W"),
    ("--dynamic-deps", None),
    ("--exclude", "-X"),
    ("--fail-clean", None),
    ("--fuzzy-search", None),
    ("--ignore-built-slot-operator-deps", None),
    ("--ignore-soname-deps", None),
    ("--ignore-world", None),
    ("--implicit-system-deps", None),
    ("--jobs", "-j"),
    ("--jobs-tmpdir-require-free-gb", None),
    ("--keep-going", None),
    ("--load-average", "-l"),
    ("--misspell-suggestions", None),
    ("--with-bdeps", None),
    ("--with-bdeps-auto", None),
    ("--reinstall", None),
    ("--reinstall-atoms", None),
    ("--binpkg-respect-use", None),
    ("--getbinpkg", "-g"),
    ("--getbinpkgonly", "-G"),
    ("--getbinpkg-exclude", None),
    ("--getbinpkg-include", None),
    ("--usepkg-exclude", None),
    ("--usepkg-include", None),
    ("--onlydeps-with-ideps", None),
    ("--onlydeps-with-rdeps", None),
    ("--rebuild-exclude", None),
    ("--rebuild-ignore", None),
    ("--package-moves", None),
    ("--prefix", None),
    ("--pkg-format", None),
    ("--quickpkg-direct", None),
    ("--quickpkg-direct-root", None),
    ("--quiet", "-q"),
    ("--quiet-build", None),
    ("--quiet-fail", None),
    ("--read-news", None),
    ("--rebuild-if-new-slot", None),
    ("--rebuild-if-new-rev", None),
    ("--rebuild-if-new-ver", None),
    ("--rebuild-if-unbuilt", None),
    ("--rebuilt-binaries", None),
    ("--rebuilt-binaries-timestamp", None),
    ("--regex-search-auto", None),
    ("--root", None),
    ("--root-deps", None),
    ("--search-index", None),
    ("--search-similarity", None),
    ("--select", "-w"),
    ("--selective", None),
    ("--sync-submodule", None),
    ("--sysroot", None),
    ("--use-ebuild-visibility", None),
    ("--useoldpkg-atoms", None),
    ("--usepkg", "-k"),
    ("--usepkgonly", "-K"),
    ("--usepkg-exclude-live", None),
    ("--verbose-missing-ebuilds", None),
    ("--verbose-slot-rebuilds", None),
    ("--with-test-deps", None),
]

_ACTIONS = [
    ("--clean", None),
    ("--check-news", None),
    ("--config", None),
    ("--depclean", "-c"),
    # "--help"/"-h" deliberately excluded -- see the module doc comment.
    ("--info", None),
    ("--list-sets", None),
    ("--metadata", None),
    ("--moo", None),
    ("--prune", "-P"),
    ("--rage-clean", None),
    ("--regen", None),
    ("--search", "-s"),
    ("--status", None),
    ("--sync", None),
    ("--unmerge", "-C"),
    ("--version", "-V"),
]


def _find_option(table, name):
    for long, short in table:
        if name == long or (short is not None and name == short):
            return long
    return None


def _lookup_option(token):
    """Looks `token` (a single argv entry, e.g. "--deep", "-D", or
    "--deep=1") up across all three tables. Returns a (category,
    canonical_long_name) tuple, or None if it isn't any real emerge
    option/action this table knows about at all. Mirrors
    emerge_options.rs's lookup() exactly."""
    if token.startswith("--") and "=" in token:
        name = token.split("=", 1)[0]
    else:
        name = token
    canonical = _find_option(_BOOLEAN_OPTIONS, name)
    if canonical is not None:
        return ("boolean", canonical)
    canonical = _find_option(_VALUE_OPTIONS, name)
    if canonical is not None:
        return ("value", canonical)
    canonical = _find_option(_ACTIONS, name)
    if canonical is not None:
        return ("action", canonical)
    return None


def _has_unsupported_top_level_features(a):
    """Real portage.dep.Atom (used by _parse_atom, unlike Rust's own
    narrowed portage-dep crate) successfully parses grammar Rust's v1
    subset doesn't at all -- repo constraints, wildcards, build-ids.
    portage-dep's Atom struct (see its own source) has no fields for any
    of these, so a Rust-side parse_atom call on the same text returns
    None outright -- the same "invalid atom" outcome as genuinely
    malformed input, not the "blocker, not a valid target" outcome
    _is_blocker_atom below covers. Operator, plain slot, slot operator
    (":=" / ":*" / ":slot=" ), AND USE deps ("[bar]" and every other PMS
    8.3.4 form) are all representable on the Rust side, so none of them
    are checked here."""
    return a.repo is not None or a.extended_syntax or a.build_id is not None


def _report_option(token):
    """Reports and returns the exit code for a single option/action token
    ("-x" or "--long", never a positional atom) that isn't --pretend/-p
    or --verbose/-v -- shared between a standalone token and one
    character of a decomposed short-flag bundle, so both produce
    identical messages for the same underlying flag. Mirrors
    pretend.rs's report_option exactly."""
    found = _lookup_option(token)
    if found is not None:
        category, canonical = found
        kind = "action" if category == "action" else "option"
        print(
            f'emerge (pilot v1): {kind} "{canonical}" is a real emerge {kind}, '
            "but is not implemented in this pilot (only --pretend/-p, "
            "--verbose/-v, and --help/-h are implemented so far; see PROMPT.md)",
            file=sys.stderr,
        )
    else:
        print(f'emerge: unrecognized option "{token}"', file=sys.stderr)
    return 2


def _wants_help(args):
    """Whether --help/-h appears anywhere in args, including as one
    character of a short-flag bundle -- see pretend.rs's module doc
    comment on why this wins unconditionally, checked before anything
    else."""
    for arg in args:
        if (
            arg in ("--help", "-h")
            or (arg.startswith("-") and not arg.startswith("--") and "h" in arg[1:])
        ):
            return True
    return False


def _print_help():
    """A short, honest, pilot-specific summary -- not a port of real
    emerge's own _emerge/help.py (see pretend.rs's module doc comment for
    why). Mirrors pretend.rs's print_help exactly."""
    print("emerge (pilot v1): command-line interface to the Rust porting pilot")
    print()
    print("Usage:")
    print("   emerge --pretend [--verbose] <atom> [<atom> ...]")
    print("   emerge --help")
    print()
    print("Options:")
    print("   -p, --pretend   required: the only real merge calculation this pilot implements")
    print('   -v, --verbose   show USE="..." on each [ebuild ...] line (optionally: -v y|n)')
    print("   -h, --help      show this message and exit")
    print()
    print(
        "Every other real emerge option/action is recognized by name (see "
        "lib/_emerge/main.py) but not implemented -- using one reports which "
        "option or action it is, instead of a generic error."
    )
    print("See PORTING/README.md and PORTING/PROMPT.md for this pilot's current scope.")


def run(args):
    if _wants_help(args):
        _print_help()
        return 0

    atom_args = []
    pretend = False
    verbose = False

    i = 0
    while i < len(args):
        arg = args[i]
        if arg in ("--pretend", "-p"):
            pretend = True
            i += 1
        elif arg in ("--verbose", "-v"):
            # Peeks at the next token, consuming it only if it's exactly
            # "y"/"n" -- see pretend.rs's module doc comment on why (real
            # insert_optional_args behavior for a standalone, non-bundled
            # occurrence).
            nxt = args[i + 1] if i + 1 < len(args) else None
            if nxt == "y":
                verbose = True
                i += 2
            elif nxt == "n":
                verbose = False
                i += 2
            else:
                verbose = True
                i += 1
        elif arg == "--verbose=y":
            verbose = True
            i += 1
        elif arg == "--verbose=n":
            verbose = False
            i += 1
        elif not arg.startswith("-"):
            atom_args.append(arg)
            i += 1
        elif not arg.startswith("--") and len(arg) > 2:
            # Short-flag bundle (e.g. "-pv") -- decomposed one character
            # at a time, left to right; see pretend.rs's module doc
            # comment for how this differs from real emerge's own
            # recycling-based algorithm (same outcomes, different
            # internal order) and why a bundled -v never consumes a
            # value.
            for c in arg[1:]:
                if c == "p":
                    pretend = True
                elif c == "v":
                    verbose = True
                else:
                    return _report_option(f"-{c}")
            i += 1
        else:
            return _report_option(arg)

    if not pretend:
        print(
            "emerge (pilot v1): only --pretend is implemented "
            "(no real merges yet, see PROMPT.md)",
            file=sys.stderr,
        )
        return 2

    if not atom_args:
        print(
            "emerge (pilot v1): expected a package atom, e.g. "
            "`emerge --pretend cat/pkg`",
            file=sys.stderr,
        )
        return 2

    for atom_arg in atom_args:
        atom = _parse_atom(atom_arg)
        if atom is None or _has_unsupported_top_level_features(atom):
            print(f'emerge: invalid atom "{atom_arg}"', file=sys.stderr)
            return 1
        if atom.blocker:
            print(
                f'emerge (pilot v1): "{atom_arg}" is a blocker, not a valid emerge target',
                file=sys.stderr,
            )
            return 2

    top_level_pkgs = {tuple(_parse_atom(a).cp.split("/", 1)) for a in atom_args}

    try:
        # resolve_config needs the main repo's own location for
        # package.mask/.unmask's repo-level source (see its own
        # docstring) -- found via the same find_repos repos.conf parsing
        # resolve_pretend_graph uses internally a few lines down; called
        # again here since it's cheap and keeps this mirroring the Rust
        # side's own pretend.rs exactly.
        main_repo = next(r for r in find_repos(_config_root()) if r["is_main"])
        config = resolve_config(_config_root(), main_repo["location"])
        result = resolve_pretend_graph(_config_root(), _root(), atom_args, config)
    except ResolutionError as e:
        print(f"emerge: {e}", file=sys.stderr)
        return 1
    entries = result["entries"]

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

    def use_suffix(use_display):
        # "  USE=\"flag1 -flag2\"", matching real --pretend -v's own line
        # format, or "" when --verbose wasn't given or there's no
        # IUSE-declared flags at all. Real portage's own USE display
        # additionally colorizes and diffs against the previously
        # installed version's IUSE (*/% markers) and groups by
        # USE_EXPAND; this pilot shows none of that, just the plain
        # enabled/disabled set, alphabetically sorted.
        if not verbose or not use_display:
            return ""
        flags = [flag if enabled else f"-{flag}" for flag, enabled in use_display]
        return '  USE="{}"'.format(" ".join(flags))

    for category, package, outcome, blockers, _slot, use_display in entries:
        tag = outcome[0]
        if tag == "new":
            print(f"[ebuild  N] {category}/{package}-{outcome[1]}{use_suffix(use_display)}")
            print_blockers(category, package, outcome[1], blockers)
        elif tag == "upgrade":
            print(
                f"[ebuild  U] {category}/{package}-{outcome[2]} (upgrade from {outcome[1]})"
                f"{use_suffix(use_display)}"
            )
            print_blockers(category, package, outcome[2], blockers)
        elif tag == "already_installed":
            # Already-satisfied dependencies aren't shown, matching real
            # emerge's usual "don't clutter the list" behavior -- only a
            # directly-requested (top-level) atom gets its own
            # "is already installed; nothing to do" line.
            if (category, package) in top_level_pkgs:
                print(f"{category}/{package}-{outcome[1]} is already installed; nothing to do")
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
