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

from portage.dep import Atom, check_required_use, match_from_list, use_reduce
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


def _parse_package_license_lines(lines):
    """package.license/package.properties/package.accept_restrict: each
    line is "<atom-or-wildcard> <token...>". Same shape as
    package.accept_keywords, reused directly for all three real files.
    Mirrors portage-profile/src/lib.rs's parse_package_license_lines
    exactly."""
    return _parse_package_accept_keywords_lines(lines)


def _parse_license_groups_lines(lines):
    """license_groups (real LicenseManager._read_license_groups): each
    non-comment, non-blank line is "<group-name> <license or @group
    ...>" (grabdict format -- no "-atom"/removal semantics at all).
    Later sources *extend* (never replace) whatever the same group name
    already has, matching real
    self._license_groups.setdefault(k, []).extend(v) exactly. Mirrors
    portage-profile/src/lib.rs's parse_license_groups_lines exactly."""
    groups = {}
    for line in lines:
        parts = line.split()
        if not parts:
            continue
        name, members = parts[0], parts[1:]
        groups.setdefault(name, []).extend(members)
    return groups


def _expand_license_token(token, groups, traversed=None):
    """Expands a single ACCEPT_LICENSE/package.license token against
    `groups`: a plain license name (or "*"/"-*", real portage's own
    symbolic wildcard tokens) passes through unchanged; an "@group-name"
    token (optionally "-"-negated) expands to every one of that group's
    own members, each recursively expanded the same way -- negation
    applies to every expanded member, not just the group reference
    itself. `traversed` guards against a circular group reference (a
    group already being expanded higher up this same call stack is left
    as its own literal "@group-name" text instead of recursing
    infinitely), same for a genuinely undefined group name. Mirrors real
    LicenseManager._expandLicenseToken and portage-profile/src/lib.rs's
    expand_license_token exactly (deliberately silent here, unlike real
    portage's own writemsg, same as every other real-portage-warning-only
    path this pilot already skips silently)."""
    if traversed is None:
        traversed = set()
    negate = token.startswith("-")
    license_name = token[1:] if negate else token
    if not license_name.startswith("@"):
        return [token]
    group_name = license_name[1:]
    if group_name in traversed:
        result = [f"@{group_name}"]
    elif group_name in groups:
        traversed.add(group_name)
        result = []
        for member in groups[group_name]:
            # Real portage: a group's own member list is never itself
            # allowed to contain a "-"-negated entry.
            if not member.startswith("-"):
                result.extend(_expand_license_token(member, groups, traversed))
        traversed.discard(group_name)
    else:
        result = [f"@{group_name}"]
    if negate:
        result = [f"-{t}" for t in result]
    return result


def _expand_license_tokens(tokens, groups):
    """Expands every token in `tokens` against `groups`, in order.
    Mirrors real LicenseManager.expandLicenseTokens and
    portage-profile/src/lib.rs's expand_license_tokens exactly."""
    expanded = []
    for t in tokens:
        expanded.extend(_expand_license_token(t, groups))
    return expanded


def _parse_package_use_lines(lines, use_expand_shorthand=False):
    """A line with no tokens after the atom is a documented no-op,
    matching _parse_package_accept_keywords_lines. Purely additive across
    sources, like package.accept_keywords and unlike package.mask/
    .unmask: real portage's own package.use consumption only ever
    .extend()s a growing token list per source, never removes a previous
    entry.

    use_expand_shorthand, when True, ports real
    UseManager._parse_user_files_to_extatomdict's own "VIDEO_CARDS:
    nvidia intel" syntax: a token ending in ":" sets a
    lowercase(name) + "_" prefix applied to every following token on
    that same line (a leading "-" stays outside the new prefix), reset
    back to none at the start of every line. Callers pass False for
    repo-level/profile-level lines: confirmed by reading
    UseManager.__init__, only the user-level source ever applies this
    shorthand at all -- see portage-profile/src/lib.rs's own
    parse_package_use_lines doc comment for the full grounding. Mirrors
    that function exactly."""
    result = []
    for line in lines:
        parts = line.split()
        atom, raw_tokens = parts[0], parts[1:]
        prefix = ""
        tokens = []
        for tok in raw_tokens:
            if use_expand_shorthand and tok.endswith(":"):
                prefix = tok[:-1].lower() + "_"
                continue
            if not prefix:
                tokens.append(tok)
            elif tok.startswith("-"):
                tokens.append(f"-{prefix}{tok[1:]}")
            else:
                tokens.append(f"{prefix}{tok}")
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
                    "repo_name": repo["name"],
                    # Read alongside KEYWORDS/SLOT at zero extra I/O cost
                    # (the same metadata dict) -- see is_visible's own
                    # license-masking check. Mirrors portage-repo's own
                    # Candidate.license/.iuse exactly.
                    "license": metadata.get("LICENSE", ""),
                    "iuse": metadata.get("IUSE", ""),
                    "properties": metadata.get("PROPERTIES", ""),
                    "restrict": metadata.get("RESTRICT", ""),
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


def _license_struct_has_masked(struct, acceptable):
    """Whether `struct` (real `use_reduce(license_str, uselist=use,
    opconvert=True)` -- a `||` group's own members are flat, e.g.
    `['||', 'MIT', 'BSD']`, not double-nested; a plain sub-group
    directly inside a `||`'s own member list stays a genuine nested
    list instead) has at least one required-but-unaccepted license.
    Mirrors real `LicenseManager._getMaskedLicenses`, as a bool rather
    than the full "list every masked license" diagnostic real portage's
    own mask-display machinery uses (this pilot has no mask-reason
    display to feed it). Mirrors portage-repo/src/lib.rs's
    tree_has_masked_license/node_has_masked_license exactly (structural
    difference only: this walks real use_reduce's own list-of-str-or-
    list shape directly, since Python already has the real function to
    call -- see the Rust side's own LicenseNode doc comment for why it
    needed a bespoke parser instead)."""
    if not struct:
        return False
    if struct[0] == "||":
        for element in struct[1:]:
            if isinstance(element, list):
                if element and not _license_struct_has_masked(element, acceptable):
                    return False
            elif element in acceptable:
                return False
        return True
    for element in struct:
        if isinstance(element, list):
            if element and _license_struct_has_masked(element, acceptable):
                return True
        elif element not in acceptable:
            return True
    return False


def _use_flags_if_conditional(value_str, candidate, category, package, candidate_str, config):
    """This candidate's own effective USE, only actually resolved if
    `value_str` (a LICENSE/PROPERTIES/RESTRICT string) contains a "?" at
    all -- real use_reduce's own "if '?' in license_str" optimization,
    shared by every metadata key that needs this same "resolve USE, but
    only when it could possibly matter" treatment. Mirrors
    portage-repo/src/lib.rs's use_flags_if_conditional exactly."""
    if "?" not in value_str:
        return set()
    return effective_use_flags(
        config["use_flags"],
        config["package_use"],
        config["package_use_force"],
        config["package_use_mask"],
        config["use_stable_force"],
        config["use_stable_mask"],
        config["package_use_stable_force"],
        config["package_use_stable_mask"],
        candidate["keywords"],
        config["accept_keywords"],
        config["package_accept_keywords"],
        candidate_str,
        category,
        package,
    )


def _resolve_accept_tokens(global_accept, package_accept, candidate_str, category, package):
    """This candidate's own effective ACCEPT_LICENSE/ACCEPT_PROPERTIES/
    ACCEPT_RESTRICT-style symbolic token list: `global_accept`, with
    every matching `package_accept` entry's own tokens layered on top,
    in atom-specificity order -- real _getPkgAcceptLicense's own
    accept_license.extend(x) loop over ordered_by_atom_specificity
    matches (and its _getMissingProperties/_getMissingRestrict
    siblings, which do the identical thing for their own accept lists).
    Mirrors portage-repo/src/lib.rs's resolve_accept_tokens exactly."""
    matching = [
        (atom, tokens)
        for atom, tokens in package_accept
        if _matches_config_entry(atom, candidate_str, category, package)
    ]
    matching.sort(key=lambda et: _atom_specificity(et[0]))
    accept_tokens = list(global_accept)
    for _atom, tokens in matching:
        accept_tokens.extend(tokens)
    return accept_tokens


def _resolve_acceptable_tokens(accept_tokens, all_mentioned):
    """Resolves `accept_tokens` (symbolic -- "*"/"-*"/"-token"/"token")
    into a concrete acceptable-token set, given `all_mentioned` (every
    token the candidate's own metadata value could possibly mention,
    real matchall=1 semantics). Shared by _license_accepted/
    _metadata_key_accepted -- real getMissingLicenses/
    _getMissingProperties/_getMissingRestrict all use this identical
    algorithm, just for a different metadata key. Mirrors
    portage-repo/src/lib.rs's resolve_acceptable_tokens exactly."""
    acceptable = set()
    for token in accept_tokens:
        if token == "*":
            acceptable.update(all_mentioned)
        elif token == "-*":
            acceptable.clear()
        elif token.startswith("-"):
            acceptable.discard(token[1:])
        else:
            acceptable.add(token)
    return acceptable


def _license_accepted(candidate, category, package, candidate_str, config):
    """Whether `candidate`'s own declared LICENSE is fully accepted --
    real Package.py's own `settings._getMissingLicenses` check (via
    LicenseManager.getMissingLicenses/_getPkgAcceptLicense). A LICENSE
    string real use_reduce can't parse is treated as masked (not
    visible) rather than accepted -- matching the "can't tell, so
    exclude" precedent this pilot's own _reinstall_flags_for_use_change
    already establishes for an unreadable candidate. Mirrors
    portage-repo/src/lib.rs's license_accepted exactly."""
    license_str = candidate.get("license", "")
    if not license_str.strip():
        return True

    use_flags = _use_flags_if_conditional(
        license_str, candidate, category, package, candidate_str, config
    )
    accept_tokens = _resolve_accept_tokens(
        config["accept_license"], config["package_license"], candidate_str, category, package
    )
    try:
        all_mentioned = {
            t for t in use_reduce(license_str, matchall=True, flat=True) if t != "||"
        }
    except InvalidDependString:
        return False
    acceptable = _resolve_acceptable_tokens(accept_tokens, all_mentioned)

    try:
        struct = use_reduce(license_str, uselist=list(use_flags), opconvert=True)
    except InvalidDependString:
        return False
    return not _license_struct_has_masked(struct, acceptable)


def _metadata_key_accepted(
    value_str, candidate, category, package, candidate_str, config, global_accept, package_accept
):
    """Whether every token in `value_str` (a candidate's own real
    PROPERTIES/RESTRICT metadata) is accepted -- real
    _getMissingProperties/_getMissingRestrict, ported as a bool. Unlike
    LICENSE (which needs "||"-group *structure*), PROPERTIES/RESTRICT
    have no any-of semantics at all: real config.py's own comment says
    it plainly, "ACCEPT_PROPERTIES works like ACCEPT_LICENSE, without
    groups" -- every flattened token individually needs to be accepted,
    so this calls real use_reduce with flat=True directly, the same way
    the Rust side reuses use_reduce_flat directly instead of its own
    bespoke LicenseNode tree. Mirrors portage-repo/src/lib.rs's
    metadata_key_accepted exactly."""
    if not value_str.strip():
        return True

    use_flags = _use_flags_if_conditional(
        value_str, candidate, category, package, candidate_str, config
    )
    accept_tokens = _resolve_accept_tokens(
        global_accept, package_accept, candidate_str, category, package
    )
    try:
        all_mentioned = {t for t in use_reduce(value_str, matchall=True, flat=True) if t != "||"}
    except InvalidDependString:
        return False
    acceptable = _resolve_acceptable_tokens(accept_tokens, all_mentioned)

    try:
        flat = use_reduce(value_str, uselist=list(use_flags), flat=True)
    except InvalidDependString:
        return False
    return all(t in acceptable for t in flat)


def is_visible(candidate, category, package, config):
    """A candidate is visible if it isn't masked (matches a package.mask
    entry and no package.unmask entry), its KEYWORDS intersect the
    accepted set -- the global config["accept_keywords"], plus any extra
    keywords contributed by a matching package.accept_keywords entry,
    with a "**" token in such an entry meaning "accept unconditionally"
    for matching candidates (even ones with empty/no KEYWORDS) -- and its
    own declared LICENSE/PROPERTIES/RESTRICT are all fully accepted (see
    _license_accepted/_metadata_key_accepted) -- real Package.py's own
    _masks dict collects package.mask, LICENSE, PROPERTIES, and RESTRICT
    as four independent masking reasons the same way."""
    candidate_str = (
        f"{category}/{package}-{candidate['version']}:{candidate['slot']}"
        f"::{candidate['repo_name']}"
    )

    masked = any(
        _matches_config_entry(m, candidate_str, category, package)
        for m in config["package_mask"]
    ) and not any(
        _matches_config_entry(u, candidate_str, category, package)
        for u in config["package_unmask"]
    )
    if masked:
        return False

    if not _license_accepted(candidate, category, package, candidate_str, config):
        return False

    if not _metadata_key_accepted(
        candidate.get("properties", ""),
        candidate,
        category,
        package,
        candidate_str,
        config,
        config["accept_properties"],
        config["package_properties"],
    ):
        return False

    if not _metadata_key_accepted(
        candidate.get("restrict", ""),
        candidate,
        category,
        package,
        candidate_str,
        config,
        config["accept_restrict"],
        config["package_accept_restrict"],
    ):
        return False

    return _keywords_accepted(
        candidate["keywords"],
        candidate_str,
        category,
        package,
        config["accept_keywords"],
        config["package_accept_keywords"],
    )


def _keywords_accepted(
    keywords, candidate_str, category, package, accept_keywords, package_accept_keywords
):
    """The keyword-matching half of is_visible (everything except the
    package.mask/.unmask check), factored out so _is_stable below can
    reuse it against an artificially-unstabilized keyword list instead
    of a candidate's own real keywords -- real KeywordsManager.isStable/
    getMissingKeywords share this exact same matching logic with real
    visibility checking too, just against a different input keyword
    set, not a separate algorithm.

    Grounded against real KeywordsManager.getMissingKeywords/
    _getEgroups: a package.accept_keywords entry doesn't just *add*
    keywords on top of the global ACCEPT_KEYWORDS set -- real
    _getEgroups folds "-token"/"-*" removals too, over the *combined*
    list (global keywords first, then each matching entry's own tokens,
    in atom-specificity order), so a more-specific package.accept_
    keywords line can revoke a keyword the global set already granted,
    not just add new ones. Ported here via _specificity_ordered_flags
    (already established for package.use.mask/.force's own identical
    "specificity-ordered incremental fold" shape) seeded with
    accept_keywords itself, rather than a "union everything a matching
    entry ever mentions, ignore any '-' prefix" accumulation. "**" is
    folded in exactly like any other token now (removable by a later
    "-*"/"-**"), rather than a separate unconditional-accept pre-scan
    that ignored fold order entirely -- once folded, its presence in the
    final accepted set still means "accept any KEYWORDS state, even
    empty," the same real '"**" in pgroups' unconditional-match rule
    this pilot already documented. Deliberately unchanged: a bare
    package.accept_keywords atom with no keyword list at all stays a
    documented no-op (real accept_keywords_defaults substitution is a
    separate mechanism, not negation, and was already out of scope
    before this). Mirrors portage-repo/src/lib.rs's keywords_accepted
    exactly."""
    accepted = _specificity_ordered_flags(
        package_accept_keywords, candidate_str, category, package, seed=accept_keywords
    )
    if "**" in accepted:
        return True
    return bool(accepted & set(keywords))


def _is_stable(keywords, candidate_str, category, package, accept_keywords, package_accept_keywords):
    """Whether `keywords` (a candidate's own KEYWORDS) count as "stable"
    for the purposes of use.stable.mask/.force/package.use.stable.mask/
    .force -- ported from real KeywordsManager.isStable: NOT a raw "no
    ~ prefix" check. A candidate counts as stable if replacing every one
    of its own keywords with its "~"-prefixed unstable form would make
    it invisible under the current ACCEPT_KEYWORDS/package.accept_keywords
    -- real portage's own comment explains why: "this guarantees that
    the effective use.force/mask settings for a particular ebuild do not
    change when that ebuild is stabilized." Reuses _keywords_accepted
    (the same matching logic is_visible itself uses) against that
    artificially-unstabilized list. Mirrors portage-repo/src/lib.rs's
    is_stable exactly."""
    unstable = [k if k.startswith("~") else f"~{k}" for k in keywords]
    return not _keywords_accepted(
        unstable, candidate_str, category, package, accept_keywords, package_accept_keywords
    )


def effective_use_flags(
    base,
    package_use,
    package_use_force,
    package_use_mask,
    use_stable_force,
    use_stable_mask,
    package_use_stable_force,
    package_use_stable_mask,
    keywords,
    accept_keywords,
    package_accept_keywords,
    candidate_str,
    category,
    package,
):
    """The USE flags in effect for one specific package: `base` with every
    matching package.use entry's tokens layered on top, in file order, via
    the same incremental -flag/flag/+flag semantics USE itself uses (see
    _apply_incremental), THEN package.use.force/package.use.mask layered
    on top of that (force winning first, then mask -- see
    _specificity_ordered_flags for how a conflict between multiple
    matching mask/force entries is resolved), THEN, only if this candidate
    counts as "stable" (_is_stable), use_stable_force/package_use_stable_force
    and use_stable_mask/package_use_stable_mask -- the .stable. variants of
    the sources already applied above, ported from real getUseMask/
    getUseForce's own per-package branch (which appends the stable variant
    right alongside the ordinary one at each accumulation step, but only
    when stable). Applied per package, mirroring
    portage-repo/src/lib.rs's effective_use_flags exactly -- a package.use
    entry never affects any other package's own resolution."""
    use_flags = set(base)
    for entry, tokens in package_use:
        if _matches_config_entry(entry, candidate_str, category, package):
            _apply_incremental(" ".join(tokens), use_flags)

    stable = _is_stable(
        keywords, candidate_str, category, package, accept_keywords, package_accept_keywords
    )

    use_flags |= _specificity_ordered_flags(
        package_use_force, candidate_str, category, package
    )
    if stable:
        use_flags |= use_stable_force
        use_flags |= _specificity_ordered_flags(
            package_use_stable_force, candidate_str, category, package
        )
    use_flags -= _specificity_ordered_flags(
        package_use_mask, candidate_str, category, package
    )
    if stable:
        use_flags -= use_stable_mask
        use_flags -= _specificity_ordered_flags(
            package_use_stable_mask, candidate_str, category, package
        )
    return use_flags


def _atom_specificity(entry):
    """Simplified port of real best_match_to_list's own specificity
    ranking table (used by ordered_by_atom_specificity). Mirrors
    portage-repo/src/lib.rs's atom_specificity exactly, including its
    own documented simplifications: comparison operators (>,<,>=,<=)
    all share one tier without real portage's own "closest version wins
    a tie" refinement, and every wildcard entry this pilot's own grammar
    can produce falls into real portage's lowest extended-syntax tier,
    since it never has a slot or "=*" glob of its own."""
    try:
        atom = Atom(entry, allow_wildcard=True)
    except InvalidAtom:
        return -2
    if atom.extended_syntax:
        return -2
    op_values = {"=": 6, "~": 5, "=*": 4, ">": 2, "<": 2, ">=": 2, "<=": 2, None: 1}
    op_val = op_values.get(atom.operator, 1)
    slot_val = 3 if atom.slot is not None else -(10**9)
    return max(op_val, slot_val)


def _specificity_ordered_flags(entries, candidate_str, category, package, seed=None):
    """Computes the final per-candidate flag set from `entries` (raw
    package.use.mask/.force/package.accept_keywords (atom, tokens)
    pairs): filters to entries whose atom actually matches
    `candidate_str`, orders the matches from least to most specific
    (_atom_specificity), then applies each one's own tokens via the same
    incremental semantics package.use itself uses, onto `seed` (an empty
    set if not given) -- so a more-specific atom's own "-flag" can
    cancel a less-specific atom's own mask/force (or, for
    _keywords_accepted's own use below, even a keyword `seed` itself
    already contains). Mirrors portage-repo/src/lib.rs's
    specificity_ordered_flags exactly (Python's own list.sort() is
    stable, matching Rust's sort_by_key, so ties keep their original
    file/stacking order). `seed` is empty for every package.use.mask/
    .force caller -- _keywords_accepted is the one caller that seeds it
    with something real, mirroring real KeywordsManager.
    getMissingKeywords's own "pgroups = global_accept_keywords.split();
    pgroups.extend(unmaskgroups)" (seed first, then fold in
    package-specific contributions) exactly."""
    matching = [
        (entry, tokens)
        for entry, tokens in entries
        if _matches_config_entry(entry, candidate_str, category, package)
    ]
    matching.sort(key=lambda et: _atom_specificity(et[0]))
    flags = set() if seed is None else set(seed)
    for _entry, tokens in matching:
        _apply_incremental(" ".join(tokens), flags)
    return flags


def _read_vdb_flag_set(root, category, package, version, filename):
    """Reads <root>/var/db/pkg/<category>/<package>-<version>/<filename>
    (a vdb aux file, e.g. USE or IUSE -- same directory SLOT/CATEGORY
    already come from) as a set of flag names, one per whitespace-
    separated token, with any "+"/"-" IUSE default-marker prefix
    stripped. A missing file is an empty set, not an error. Mirrors
    portage-repo/src/lib.rs's read_vdb_flag_set exactly."""
    path = os.path.join(root, "var", "db", "pkg", category, f"{package}-{version}", filename)
    try:
        with open(path) as f:
            text = f.read()
    except OSError:
        text = ""
    return {tok.lstrip("+-") for tok in text.split()}


def _reinstall_flags_for_use_change(root, category, package, candidate, config, newuse):
    """--newuse/--changed-use: ports both the "newuse" and "elif
    changed_use" branches of real depgraph.py's _reinstall_for_flags --
    whether `candidate` (a version already installed) needs reinstalling
    because its currently-effective USE differs from what the vdb
    recorded at merge time. Returns the sorted list of flags that
    triggered it, or None if nothing did. Only ever called when at least
    one of newuse/changed_use is set; if both are, newuse wins (see
    resolve_pretend's own docstring). Mirrors portage-repo/src/lib.rs's
    reinstall_flags_for_use_change exactly, including which branch gets
    which term -- see that function's own doc comment for the full
    algorithm writeup."""
    version = candidate["version"]
    orig_use = _read_vdb_flag_set(root, category, package, version, "USE")
    orig_iuse = _read_vdb_flag_set(root, category, package, version, "IUSE")

    try:
        metadata = read_md5_cache(candidate["repo_location"], category, f"{package}-{version}")
    except OSError:
        return None
    if not metadata.get("IUSE"):
        return None
    cur_iuse = {tok.lstrip("+-") for tok in metadata["IUSE"].split()}
    candidate_str = f"{category}/{package}-{version}:{candidate['slot']}::{candidate['repo_name']}"
    cur_use = effective_use_flags(
        config["use_flags"],
        config["package_use"],
        config["package_use_force"],
        config["package_use_mask"],
        config["use_stable_force"],
        config["use_stable_mask"],
        config["package_use_stable_force"],
        config["package_use_stable_mask"],
        candidate["keywords"],
        config["accept_keywords"],
        config["package_accept_keywords"],
        candidate_str,
        category,
        package,
    )

    # Shared term (the *entire* --changed-use formula on its own):
    # (orig_iuse∩orig_use) ^ (cur_iuse∩cur_use).
    flags = (orig_iuse & orig_use) ^ (cur_iuse & cur_use)

    if newuse:
        # --newuse adds (orig_iuse ^ cur_iuse) - forced_flags on top --
        # forced_flags (config's own "use_force" union "use_mask") is
        # subtracted here and only here, matching real portage's own
        # "flags -= forced_flags" line, which sits between the "^=" and
        # the final "|=".
        forced_flags = config["use_force"] | config["use_mask"]
        flags |= (orig_iuse ^ cur_iuse) - forced_flags

    return sorted(flags) if flags else None


def _read_vdb_string(root, category, package, version, filename):
    """Reads <root>/var/db/pkg/<category>/<package>-<version>/<filename>
    as a raw string (e.g. DEPEND/RDEPEND), unlike _read_vdb_flag_set
    which splits into a flag-name set -- a dependency string needs to
    stay intact for use_reduce to parse (||/USE-conditional groups, not
    just bare tokens). A missing file is an empty string, not an error.
    Mirrors portage-repo/src/lib.rs's read_vdb_string exactly."""
    path = os.path.join(root, "var", "db", "pkg", category, f"{package}-{version}", filename)
    try:
        with open(path) as f:
            return f.read()
    except OSError:
        return ""


def _flat_dep_atoms(depstr, use_flags):
    """Flattens `depstr` (one or more concatenated dependency-string
    keys) against `use_flags`, into a set of dependency-atom tokens
    ("||" markers dropped) suitable for order-independent equality
    comparison. None if `depstr` doesn't parse at all. Mirrors
    portage-repo/src/lib.rs's flat_dep_atoms exactly."""
    try:
        flat = use_reduce(depstr, flat=True, uselist=use_flags)
    except InvalidDependString:
        return None
    return {tok for tok in flat if tok != "||"}


def _deps_changed(root, repos, category, package, version, with_bdeps):
    """--changed-deps: whether `version`'s own vdb-recorded dependency
    strings differ from the repo's own *current* ebuild for that exact
    version, once both are flattened against the *same* input -- the
    installed package's own recorded USE (real depgraph.py's own
    _changed_deps: uselist=pkg.use.enabled, used for *both* sides of the
    comparison, so a difference driven purely by a USE change is never
    what this detects -- that's --newuse/--changed-use's own job, and
    can fire independently of (or alongside) this one). Which keys are
    compared respects with_bdeps exactly like _enqueue_dependencies's
    own dep-key list does.

    KNOWN, DOCUMENTED SCOPE CUT: real _changed_deps compares real
    *structured* use_reduce output (||-group boundaries preserved) key
    by key, so a dependency moved between two of the five keys with the
    same net atom set, or a pure ||-group restructuring with the same
    underlying atoms, would count as "changed" there but not here. This
    pilot's own dependency-recursion machinery is flat-only everywhere
    else too (use_reduce(..., flat=True)), so this reuses that same flat
    comparison rather than building bespoke structured-tree machinery
    just for this one feature. Also unaddressed: real strip_libc_deps (a
    libc-specific special case this pilot has no fixture or machinery
    for anywhere else) -- no observable effect in this pilot's own
    fixture tree.

    A vdb-side dependency string that fails to parse counts as "changed"
    unconditionally, matching real portage's own "except
    InvalidDependString: changed = True"; a repo-side one that fails to
    parse instead reports "unchanged" (False), the same tolerant
    "can't tell, don't crash" fallback _enqueue_dependencies already
    uses for its own unreadable-metadata cases. Mirrors
    portage-repo/src/lib.rs's deps_changed exactly."""
    dep_keys = (
        ("DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND")
        if with_bdeps
        else ("RDEPEND", "PDEPEND", "IDEPEND")
    )

    installed_use = _read_vdb_flag_set(root, category, package, version, "USE")

    vdb_depstr = " ".join(
        s
        for s in (_read_vdb_string(root, category, package, version, k) for k in dep_keys)
        if s
    )

    repo_candidates = [c for c in list_candidates(repos, category, package) if c["version"] == version]
    if not repo_candidates:
        return False
    resolved = max(repo_candidates, key=lambda c: c["repo_priority"])
    try:
        metadata = read_md5_cache(resolved["repo_location"], category, f"{package}-{version}")
    except OSError:
        return False
    repo_depstr = " ".join(metadata[k] for k in dep_keys if metadata.get(k))

    repo_atoms = _flat_dep_atoms(repo_depstr, installed_use)
    if repo_atoms is None:
        return False
    vdb_atoms = _flat_dep_atoms(vdb_depstr, installed_use)
    if vdb_atoms is None:
        return True
    return vdb_atoms != repo_atoms


def _use_deps_satisfied(atom, iuse, enabled):
    """Ports real match_from_list's own USE-dep post-pass (its
    "if mydep.unevaluated_atom.use:" block, lib/portage/dep/__init__.py
    lines 3143-3188) -- NOT called from match_from_list itself, since
    real match_from_list skips this same block entirely for a
    plain-string candidate (its own "hasattr(x, 'use')" guard), which is
    exactly what this pilot's own candidate strings always are -- see
    portage-repo/src/lib.rs's own use_deps_satisfied doc comment for the
    full architecture writeup this mirrors. Called separately, after
    match_from_list's own version/slot/repo filtering, once a real
    candidate's own IUSE/effective-USE is in hand.

    `atom` is a real Atom (so `atom.use` is a real `_use_dep` object --
    `.required`/`.enabled`/`.disabled`/`.missing_enabled`/
    `.missing_disabled` used directly, not re-derived); `iuse` is the
    candidate's own declared IUSE (a set of flag names, `+`/`-` default
    markers already stripped); `enabled` is its own effective USE set.

    Real behavior, faithfully ported, not simplified: a use-dep flag
    with no `(+)`/`(-)` default marker -- of ANY form, including the
    four conditional ones -- must be a real, declared IUSE flag on the
    candidate, or the atom doesn't match this candidate at all (real
    `.required`, checked before anything else). Only the two
    *unconditional* forms, `flag` and `-flag` (real `.enabled`/
    `.disabled`, which real `_use_dep.__init__` populates solely from
    these two), actually constrain the candidate's own enabled/disabled
    state; a `(+)`/`(-)` default only ever matters for a flag missing
    from this candidate's own IUSE, standing in for "as if
    enabled/disabled". The four conditional forms (`flag?`/`!flag?`/
    `flag=`/`!flag=`) impose NO enabled/disabled constraint here at all
    -- genuine real match_from_list behavior (it never reads their own
    `.conditional` structure), not a pilot simplification: evaluating a
    conditional use-dep needs the *atom-owning* package's own USE state,
    a completely different mechanism this pilot doesn't have and
    match_from_list itself doesn't either."""
    use = atom.use
    if use is None:
        return True

    if any(flag not in iuse for flag in use.required):
        return False

    missing_enabled = frozenset(flag for flag in use.missing_enabled if flag not in iuse)
    missing_disabled = frozenset(flag for flag in use.missing_disabled if flag not in iuse)

    if use.enabled:
        if any(f in missing_disabled for f in use.enabled):
            return False
        need_enabled = use.enabled - enabled
        if need_enabled and any(f not in missing_enabled for f in need_enabled):
            return False

    if use.disabled:
        if any(f in missing_enabled for f in use.disabled):
            return False
        need_disabled = use.disabled & enabled
        if need_disabled and any(f not in missing_disabled for f in need_disabled):
            return False

    return True


def _candidate_iuse_and_use(candidate, category, package, config):
    """`candidate`'s own current IUSE (read fresh from its own md5-cache
    entry -- the current tree's metadata, not the vdb) and its own
    effective (computed) USE set. Used by resolve_pretend's own USE-dep
    filtering (_use_deps_satisfied); a missing md5-cache entry, or a
    missing IUSE key within one (a real, valid "declares no USE flags at
    all" state, same "absence is real, not an error" precedent
    _read_vdb_flag_set already sets), returns (set(), set()) rather than
    raising -- the caller treats that as "can't tell, so this use-dep
    can't be satisfied by a declared flag" via the ordinary matching
    logic, not a separate error path. Mirrors portage-repo/src/lib.rs's
    candidate_iuse_and_use exactly."""
    try:
        metadata = read_md5_cache(
            candidate["repo_location"], category, f"{package}-{candidate['version']}"
        )
    except OSError:
        return (set(), set())
    iuse = {tok.lstrip("+-") for tok in metadata.get("IUSE", "").split()}
    candidate_str = (
        f"{category}/{package}-{candidate['version']}:{candidate['slot']}::"
        f"{candidate['repo_name']}"
    )
    use_flags = effective_use_flags(
        config["use_flags"],
        config["package_use"],
        config["package_use_force"],
        config["package_use_mask"],
        config["use_stable_force"],
        config["use_stable_mask"],
        config["package_use_stable_force"],
        config["package_use_stable_mask"],
        candidate["keywords"],
        config["accept_keywords"],
        config["package_accept_keywords"],
        candidate_str,
        category,
        package,
    )
    return (iuse, use_flags)


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


def _process_config_lines(text, scalars, use_flags, accept_keywords, use_expand):
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
        elif key == "USE_EXPAND":
            _apply_incremental(value, use_expand)
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


def _process_make_conf_file(
    path, config_root, scalars, use_flags, accept_keywords, use_expand, visited_sources
):
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
                resolved,
                config_root,
                scalars,
                use_flags,
                accept_keywords,
                use_expand,
                visited_sources,
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
        elif key == "USE_EXPAND":
            _apply_incremental(value, use_expand)
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
    "package_mask", "package_unmask", "package_accept_keywords",
    "package_use", "system_packages", "use_force", "use_mask",
    "package_use_force", "package_use_mask", "use_expand", "use_stable_force",
    "use_stable_mask", "package_use_stable_force", "package_use_stable_mask".

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
    use_expand = set()
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
        _process_config_lines(text, scalars, use_flags, accept_keywords, use_expand)

    make_conf = os.path.join(config_root, "etc", "portage", "make.conf")
    if os.path.isfile(make_conf):
        _process_make_conf_file(
            make_conf, config_root, scalars, use_flags, accept_keywords, use_expand, set()
        )

    # USE_EXPAND (PMS 7.3.4; real config.py's own regenerate(), "Do the
    # USE calculation last because it depends on USE_EXPAND"): now that
    # every profile level's own make.defaults plus make.conf have been
    # read, use_expand holds the final, incrementally-stacked set of
    # USE_EXPAND variable NAMES (e.g. "VIDEO_CARDS"). Each named
    # variable's own current VALUE -- read from scalars, the same
    # last-level-wins mechanism every other non-USE/ACCEPT_KEYWORDS
    # variable already uses (a deliberate simplification of real
    # portage's own genuinely-incremental per-USE_EXPAND-variable
    # behavior -- extending the pre-existing "no incremental merge
    # outside USE/ACCEPT_KEYWORDS" cut to these variables too, not a new
    # one) -- is expanded into lowercase-prefixed pseudo-USE-flags via
    # the exact same _apply_incremental token semantics USE itself
    # already uses, folded directly into use_flags. Mirrors
    # portage-profile/src/lib.rs's resolve_config exactly, including its
    # own doc comment's list of deliberately out-of-scope USE_EXPAND
    # corners (USE_EXPAND_UNPREFIXED, IUSE-aware wildcard expansion,
    # USE_EXPAND_HIDDEN/_IMPLICIT, and package.use's own USE_EXPAND-prefix
    # shorthand, a separate, not-yet-ported follow-up).
    for var in use_expand:
        value = scalars.get(var)
        if value is None:
            continue
        prefix = var.lower()
        prefixed_tokens = []
        for tok in value.split():
            if tok.startswith("-"):
                prefixed_tokens.append(f"-{prefix}_{tok[1:]}")
            elif tok.startswith("+"):
                prefixed_tokens.append(f"{prefix}_{tok[1:]}")
            else:
                prefixed_tokens.append(f"{prefix}_{tok}")
        _apply_incremental(" ".join(prefixed_tokens), use_flags)

    # use.mask/use.force: every profile level's own file (in chain
    # order), stacked with the same "-atom" removal semantics
    # package.mask uses (see _stack_mask_lines) -- mirrors
    # portage-profile/src/lib.rs's resolve_config exactly, including its
    # own "use.mask"/"use.force" doc comment: applied last, after every
    # other real accumulation source above -- force-add every use.force
    # flag, THEN force-remove every use.mask flag, so a flag in both
    # ends up masked, not forced.
    usemask_sources = [_read_config_lines(os.path.join(level, "use.mask")) for level in chain]
    useforce_sources = [_read_config_lines(os.path.join(level, "use.force")) for level in chain]
    use_force = set(_stack_mask_lines(useforce_sources))
    use_mask = set(_stack_mask_lines(usemask_sources))
    use_flags |= use_force
    use_flags -= use_mask

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
    # Repo-level/profile-level lines are parsed separately from
    # user-level ones (rather than one concatenated pass, like every
    # other package.use.* file here) only because of the
    # USE_EXPAND-prefix shorthand's own real user-only restriction --
    # see _parse_package_use_lines's own docstring.
    repo_and_profile_use_lines = _read_config_lines(
        os.path.join(main_repo_location, "profiles", "package.use")
    )
    for level in chain:
        repo_and_profile_use_lines.extend(
            _read_config_lines(os.path.join(level, "package.use"))
        )
    user_use_lines = _read_config_lines(
        os.path.join(config_root, "etc", "portage", "package.use")
    )

    # package.use.mask/package.use.force: repo-level (main repo only, no
    # masters) plus every profile level's own file (in chain order) -- NO
    # user-level source at all, unlike package.use: confirmed by reading
    # UseManager.__init__'s own file/variable table (the "user config"
    # section lists only "package.use -> _pusedict", nothing for
    # mask/force). Flat-concatenated the same way use_lines already is;
    # which entry actually wins when more than one matches the same
    # candidate is decided later, at application time -- see
    # effective_use_flags's own docstring (atom-specificity ordering).
    # Mirrors portage-profile/src/lib.rs's resolve_config exactly.
    use_force_lines = _read_config_lines(
        os.path.join(main_repo_location, "profiles", "package.use.force")
    )
    use_mask_lines = _read_config_lines(
        os.path.join(main_repo_location, "profiles", "package.use.mask")
    )
    for level in chain:
        use_force_lines.extend(_read_config_lines(os.path.join(level, "package.use.force")))
        use_mask_lines.extend(_read_config_lines(os.path.join(level, "package.use.mask")))

    # use.stable.mask/use.stable.force (PMS 5+; real
    # eapi_supports_stable_use_forcing_and_masking's own EAPI floor,
    # always recognized here -- this pilot's established "no EAPI
    # parametrization" precedent): profile-chain only, same
    # "-atom"-removal stacking use_force/use_mask already get --
    # deliberately NOT folded into use_flags here, since "stable" is a
    # per-candidate property (depends on that candidate's own KEYWORDS)
    # with no meaningful "global" value -- real getUseForce's own
    # pkg=None case never even looks at the stable variant at all.
    # effective_use_flags applies these conditionally, once it knows a
    # specific candidate's own stability. Mirrors
    # portage-profile/src/lib.rs's resolve_config exactly, including its
    # own deliberate simplification of not also adding the repo-level
    # sourcing real per-package getUseForce(pkg) has for the *non-stable*
    # global file (which use_force/use_mask never had here either).
    use_stable_force_sources = [
        _read_config_lines(os.path.join(level, "use.stable.force")) for level in chain
    ]
    use_stable_mask_sources = [
        _read_config_lines(os.path.join(level, "use.stable.mask")) for level in chain
    ]
    use_stable_force = set(_stack_mask_lines(use_stable_force_sources))
    use_stable_mask = set(_stack_mask_lines(use_stable_mask_sources))

    # package.use.stable.mask/package.use.stable.force: repo-level (main
    # repo only) plus every profile level's own file (in chain order) --
    # NO user-level source at all, mirroring package.use.force/.mask's
    # own confirmed sourcing exactly. No shorthand either, same reasoning.
    use_stable_force_lines = _read_config_lines(
        os.path.join(main_repo_location, "profiles", "package.use.stable.force")
    )
    use_stable_mask_lines = _read_config_lines(
        os.path.join(main_repo_location, "profiles", "package.use.stable.mask")
    )
    for level in chain:
        use_stable_force_lines.extend(
            _read_config_lines(os.path.join(level, "package.use.stable.force"))
        )
        use_stable_mask_lines.extend(
            _read_config_lines(os.path.join(level, "package.use.stable.mask"))
        )

    # packages (@system): every profile level's own file, in chain order,
    # stacked with the same "-atom" removal semantics package.mask uses
    # (see _stack_mask_lines) -- mirrors portage-profile/src/lib.rs's
    # resolve_config exactly, including its own "packages" doc comment:
    # only *after* stacking are the "*"-prefixed lines kept (with the "*"
    # stripped) as the real @system atom list; every other stacked line
    # is a "known but not system" hint with no @system-set meaning of its
    # own. No repo-level or user-level source exists for this file in
    # real portage at all.
    packages_sources = [
        _read_config_lines(os.path.join(level, "packages")) for level in chain
    ]
    system_packages = [
        line[1:] for line in _stack_mask_lines(packages_sources) if line.startswith("*")
    ]

    # license_groups: every profile level's own file, in chain order,
    # plus the user-level one -- see _parse_license_groups_lines's own
    # docstring for the "extend, don't stack/replace" semantics. Read
    # before ACCEPT_LICENSE/package.license below, both of which need
    # the full, final group map to expand "@group" tokens against.
    # Mirrors portage-profile/src/lib.rs's resolve_config exactly.
    license_groups = {}
    for level in chain:
        for name, members in _parse_license_groups_lines(
            _read_config_lines(os.path.join(level, "license_groups"))
        ).items():
            license_groups.setdefault(name, []).extend(members)
    for name, members in _parse_license_groups_lines(
        _read_config_lines(os.path.join(config_root, "etc", "portage", "license_groups"))
    ).items():
        license_groups.setdefault(name, []).extend(members)

    # ACCEPT_LICENSE: last-level-wins scalar (see this pilot's own
    # pre-existing "any variable other than USE/ACCEPT_KEYWORDS is a
    # plain last-level-wins scalar" cut, extended here rather than
    # inventing a new, ACCEPT_LICENSE-specific incremental mechanism) --
    # scalars already holds whatever the profile chain + make.conf left
    # it as. Real portage's own default when ACCEPT_LICENSE is never set
    # anywhere at all -- "* -@EULA" -- is replicated exactly (real
    # config.py's own accept_license_str = " ".join(mysplit) or
    # "* -@EULA"). Mirrors portage-profile/src/lib.rs's resolve_config
    # exactly.
    accept_license_str = scalars.get("ACCEPT_LICENSE", "* -@EULA")
    accept_license = _expand_license_tokens(accept_license_str.split(), license_groups)

    # package.license: user-level only -- real portage's own rare,
    # opt-in "profile-license" profile-format (a profile-level source)
    # and its own "*/*"-line "extract into the global ACCEPT_LICENSE"
    # quirk (extract_global_changes) are both deliberately NOT
    # replicated (see portage-profile/src/lib.rs's own package_license
    # doc comment). Mirrors portage-profile/src/lib.rs's resolve_config
    # exactly.
    package_license_lines = _read_config_lines(
        os.path.join(config_root, "etc", "portage", "package.license")
    )
    package_license = [
        (atom, _expand_license_tokens(tokens, license_groups))
        for atom, tokens in _parse_package_license_lines(package_license_lines)
    ]

    # ACCEPT_PROPERTIES/ACCEPT_RESTRICT: last-level-wins scalars, real
    # "*" default (see portage-profile/src/lib.rs's own
    # accept_properties doc comment) -- no "@group" expansion for
    # either, unlike ACCEPT_LICENSE/package.license just above.
    accept_properties = scalars.get("ACCEPT_PROPERTIES", "*").split()
    accept_restrict = scalars.get("ACCEPT_RESTRICT", "*").split()

    # package.properties/package.accept_restrict: user-level only, same
    # "atom + raw tokens" shape package.license already reads (reused
    # directly -- see _parse_package_license_lines's own docstring).
    package_properties = _parse_package_license_lines(
        _read_config_lines(os.path.join(config_root, "etc", "portage", "package.properties"))
    )
    package_accept_restrict = _parse_package_license_lines(
        _read_config_lines(
            os.path.join(config_root, "etc", "portage", "package.accept_restrict")
        )
    )

    return {
        "use_flags": use_flags,
        "accept_keywords": accept_keywords,
        "package_mask": _stack_mask_lines(mask_sources),
        "package_unmask": _stack_mask_lines(unmask_sources),
        "package_accept_keywords": _parse_package_accept_keywords_lines(accept_keywords_lines),
        "package_use": (
            _parse_package_use_lines(repo_and_profile_use_lines)
            + _parse_package_use_lines(user_use_lines, use_expand_shorthand=True)
        ),
        "system_packages": system_packages,
        "use_force": use_force,
        "use_mask": use_mask,
        "package_use_force": _parse_package_use_lines(use_force_lines),
        "package_use_mask": _parse_package_use_lines(use_mask_lines),
        "use_expand": use_expand,
        "use_stable_force": use_stable_force,
        "use_stable_mask": use_stable_mask,
        "package_use_stable_force": _parse_package_use_lines(use_stable_force_lines),
        "package_use_stable_mask": _parse_package_use_lines(use_stable_mask_lines),
        "license_groups": license_groups,
        "accept_license": accept_license,
        "package_license": package_license,
        "accept_properties": accept_properties,
        "package_properties": package_properties,
        "accept_restrict": accept_restrict,
        "package_accept_restrict": package_accept_restrict,
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


def resolve_pretend(
    repos,
    root,
    atom_str,
    config,
    newuse=False,
    changed_use=False,
    update=False,
    excluded=(),
    changed_deps=False,
    with_bdeps=True,
):
    """The single-atom v1 resolution decision: find the best visible
    candidate matching `atom_str` (any atom portage-dep's v1 grammar
    supports -- operator, slot, not just a bare category/package) across
    all of `repos` (the main repo and any overlays -- see find_repos),
    compare it against what's installed. Returns a tuple whose first
    element is the outcome tag: "new", "upgrade", "reinstall",
    "already_installed", or "no_visible_candidate". `newuse`/
    `changed_use` each enable their own reinstall check (see
    _reinstall_flags_for_use_change) for an already-installed match,
    `newuse` winning if both are set; False for both reproduces this
    function's behavior from before either existed exactly.

    `update` (--update/-u) mirrors real depgraph.py's own
    `avoid_update`/`dont_miss_updates` (lib/_emerge/depgraph.py, lines
    7814 and 8448): `"--update" not in myopts` is real portage's
    *default*, under which an already-installed version that itself
    still satisfies the atom is returned immediately, without ever
    searching for a newer one -- real emerge does NOT offer an upgrade
    just because `emerge cat/pkg` was run with no other flags. Ported
    below as an early return, checked before the "always resolve to the
    single best visible candidate" logic that already existed: if not
    `update` and some installed version both matches `atom_str` and
    still has a visible candidate (mask/keyword-filtered above), the
    highest such version (repo-priority tie-broken exactly like
    _best_candidate) is used as-is, `newuse`/`changed_use` included.
    Requiring a *visible* candidate, not just checking the vdb directly,
    is deliberate: it's what lets an installed version that's since
    become masked fall through to the ordinary best-visible-candidate
    path below unchanged, matching real portage's own "enable upgrade or
    downgrade to a version with visible KEYWORDS when the installed
    version is masked" comment right above its own avoid_update check.

    `excluded` (--exclude/-X) is a list of raw atom/wildcard-atom
    strings (real WildcardPackageSet, ported here as the same
    _matches_config_entry two-tier matcher package.mask/.unmask already
    uses). Checked in two places, mirroring real depgraph.py's own
    scattered excluded_pkgs.findAtomForPackage call sites: (1) if an
    installed version matches both `atom_str` and an exclude atom, it's
    returned as already_installed immediately, before update/newuse/
    changed_use ever get a say -- ported from _want_update_pkg's and
    _replace_installed_atom's own excluded-check-first pattern; (2) an
    excluded candidate is never eligible to be selected as the
    new/upgrade "best visible candidate" either -- if every remaining
    candidate is excluded and none is already installed, this resolves
    to no_visible_candidate, the same outcome an atom with no eligible
    candidate for any other reason already gets. Deliberately NOT
    replicated: real depgraph.py's own ~18 excluded_pkgs call sites
    cover many more specific interaction points this pilot doesn't
    implement at all -- these two checks cover the dominant real-world
    use ("pin an installed package so --update/--deep never touch it")
    and the new/upgrade selection case, not every real edge case.

    `changed_deps` (--changed-deps) is an independent, freely-combinable
    reinstall trigger alongside newuse/changed_use -- see _deps_changed's
    own docstring for the real depgraph.py::_changed_deps behavior it
    ports. `with_bdeps` (--with-bdeps) only affects which dependency keys
    _deps_changed itself compares; see resolve_pretend_graph's own
    docstring for the full with_bdeps grounding.
    Mirrors portage-repo/src/lib.rs's resolve_pretend exactly."""
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
        f"{category}/{package}-{c['version']}:{c['slot']}::{c['repo_name']}" for c in visible
    ]
    by_str = dict(zip(candidate_strs, visible))
    matched = [by_str[m] for m in match_from_list(atom_str, candidate_strs) if m in by_str]

    # USE deps (dev-libs/foo[bar]/[-bar], (+)/(-) defaults -- PMS 8.3.4):
    # a post-filter on top of match_from_list's own version/slot/repo
    # matching, exactly where real portage's own match_from_list applies
    # its equivalent USE-dep post-pass too -- see _use_deps_satisfied's
    # own docstring for the ported algorithm and why match_from_list
    # itself doesn't do this. Mirrors portage-repo/src/lib.rs exactly.
    if atom.use:
        matched = [
            c
            for c in matched
            if _use_deps_satisfied(atom, *_candidate_iuse_and_use(c, category, package, config))
        ]

    if not matched:
        return ("no_visible_candidate",)

    installed = installed_versions(root, category, package)

    # --exclude/-X: an installed version matching an exclude atom is
    # left exactly as-is, unconditionally, before --update/--newuse/
    # --changed-use ever get a say -- see this function's own docstring.
    if excluded:
        installed_matched = [c for c in matched if c["version"] in installed]
        if installed_matched:
            installed_best = _best_candidate(installed_matched)
            installed_str = (
                f"{category}/{package}-{installed_best['version']}:"
                f"{installed_best['slot']}::{installed_best['repo_name']}"
            )
            if any(
                _matches_config_entry(ex, installed_str, category, package) for ex in excluded
            ):
                return ("already_installed", installed_best["version"])

    if not update:
        installed_matched = [c for c in matched if c["version"] in installed]
        if installed_matched:
            installed_best = _best_candidate(installed_matched)
            changed_flags = (
                _reinstall_flags_for_use_change(
                    root, category, package, installed_best, config, newuse
                )
                if newuse or changed_use
                else None
            ) or []
            deps_changed_flag = changed_deps and _deps_changed(
                root, repos, category, package, installed_best["version"], with_bdeps
            )
            if changed_flags or deps_changed_flag:
                return (
                    "reinstall",
                    installed_best["version"],
                    changed_flags,
                    deps_changed_flag,
                )
            return ("already_installed", installed_best["version"])

    # --exclude/-X: an excluded candidate is never eligible to become
    # the new/upgrade "best visible candidate" either -- see this
    # function's own docstring. Any already-installed match was already
    # handled (and returned) above, so nothing here can silently drop an
    # installed-and-excluded version -- only a not-yet-installed one can
    # end up filtered out entirely.
    if excluded:
        matched = [
            c
            for c in matched
            if not any(
                _matches_config_entry(
                    ex,
                    f"{category}/{package}-{c['version']}:{c['slot']}::{c['repo_name']}",
                    category,
                    package,
                )
                for ex in excluded
            )
        ]
        if not matched:
            return ("no_visible_candidate",)

    best = _best_candidate(matched)

    if best["version"] in installed:
        changed_flags = (
            _reinstall_flags_for_use_change(root, category, package, best, config, newuse)
            if newuse or changed_use
            else None
        ) or []
        deps_changed_flag = changed_deps and _deps_changed(
            root, repos, category, package, best["version"], with_bdeps
        )
        if changed_flags or deps_changed_flag:
            return ("reinstall", best["version"], changed_flags, deps_changed_flag)
        return ("already_installed", best["version"])
    if installed:
        return ("upgrade", _max_version(installed), best["version"])
    return ("new", best["version"])


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
        for category, package, outcome, _blockers, slot, _use_display, _required_by in entries:
            if (category, package) != target_key:
                continue
            if outcome[0] == "new":
                version = outcome[1]
            elif outcome[0] == "upgrade":
                version = outcome[2]
            elif outcome[0] == "reinstall":
                version = outcome[1]
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


def resolve_pretend_graph(
    config_root,
    root,
    atoms,
    config,
    newuse=False,
    changed_use=False,
    nodeps=False,
    update=False,
    deep=0,
    excluded=(),
    with_bdeps=True,
    changed_deps=False,
):
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
    only ever non-empty/non-None for New/Upgrade/Reinstall entries. See
    the module doc comment for the recursion's documented scope cuts.
    `newuse`/`changed_use` each enable their own reinstall check (see
    resolve_pretend) for an already-installed package, walking its
    dependencies too when it triggers -- False for both reproduces this
    function's behavior from before either existed exactly. `nodeps`
    (--nodeps/-O) disables the
    dependency walk entirely, for every entry, not just top-level atoms:
    only `atoms` themselves are ever resolved, ported from real
    create_depgraph_params.py popping "recurse" out of myparams (which
    depgraph.py's own dependency-walk checks for and returns early
    without). Each resolved entry's own USE display is still computed
    (real portage's -v output shows a package's own USE regardless of
    whether its dependencies get walked), but no DEPEND/RDEPEND/etc is
    ever read, so no dependency atom is ever queued and no blocker is
    ever collected. `update` (--update/-u) is threaded uniformly to every
    atom this BFS resolves, top-level and dependency alike, via
    resolve_pretend -- see that function's own docstring for the real
    avoid_update/dont_miss_updates behavior it ports. `deep` (--deep/-D)
    gates only whether an AlreadyInstalled package's own further
    dependencies get walked -- `0` (the default) means never, matching
    real portage's own default (deep=0, permanently "too deep" at every
    depth, since create_depgraph_params.py only sets myparams["deep"] at
    all when --deep's own value is present and non-zero); `True` means
    unlimited depth (a bare --deep); a positive int bounds it to that
    many levels past a directly-requested top-level atom (depth 0) --
    exactly real portage's own three `myoptions.deep` shapes, reused
    directly rather than wrapped in a dedicated type, unlike the Rust
    side's own `Deep` enum (see _deep_recurses_at). It has no effect at
    all on New/Upgrade/Reinstall packages (already always walked, `deep`
    or not) and is itself ignored outright when `nodeps` disables the
    dependency walk entirely. `excluded` (--exclude/-X, see
    resolve_pretend's own docstring) is threaded uniformly to every atom
    this BFS resolves, top-level and dependency alike, same whole-graph-
    uniform application every other flag above already gets -- including
    an AlreadyInstalled package reached only via --deep's own walk,
    since that dependency atom re-enters this same BFS loop and
    resolve_pretend call like any other, with no special case needed.
    `with_bdeps` (--with-bdeps, real depgraph.py's own "if pkg.built and
    not removal_action: ... else: ignore_build_time_deps = True") skips
    DEPEND/BDEPEND -- never RDEPEND/PDEPEND/IDEPEND -- for an
    AlreadyInstalled package's own dependency walk under --deep when
    False; like `deep` immediately above, it has no effect at all on
    New/Upgrade/Reinstall packages, since real portage only ever drops
    build-time deps for an already-built package. `True` covers both
    real `y` and the real default `auto` (depgraph.py itself only ever
    tests `in ("y", "auto")`, never distinguishing the two); see
    _enqueue_dependencies's own docstring for where the distinction is
    actually made. Mirrors portage-repo/src/lib.rs's
    resolve_pretend_graph exactly.

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
    # Each queued atom carries its own depth (0 for a directly-requested
    # top-level atom, parent's depth + 1 for anything reached only via a
    # dependency string) -- only consulted by _deep_recurses_at below,
    # for deciding whether an AlreadyInstalled package's own further
    # dependencies get walked; every other outcome ignores it entirely --
    # and the (category, package) that pushed it, if any (None for a
    # directly-requested top-level atom), only consulted by
    # required_by_map below, for each entry's own required_by.
    queue = deque((a, 0, None) for a in atoms)
    pending_blockers = []
    # (category, package) -> set of every distinct owner that reached it
    # via a dependency string, accumulated separately from the BFS's own
    # dedup/recursion decisions below (visited_atoms/resolved_slots/
    # other_outcomes) so a diamond dependency's second (deduped) owner
    # still gets recorded even though it never triggers a new
    # resolution -- merged into entries in a single post-pass at the
    # end, the same "accumulate now, merge once the whole graph is
    # known" shape pending_blockers/resolve_blockers already use.
    # Pilot-specific, no real portage equivalent -- see run()'s own
    # --json handling for why it exists at all.
    required_by_map = {}

    while queue:
        current_atom_str, depth, owner = queue.popleft()
        atom = _parse_atom(current_atom_str)
        if atom is None:
            continue
        if atom.blocker:
            continue
        category, package = atom.cp.split("/", 1)
        key = (category, package)
        if owner is not None:
            required_by_map.setdefault(key, set()).add(owner)
        if current_atom_str in visited_atoms:
            continue
        visited_atoms.add(current_atom_str)

        outcome = resolve_pretend(
            repos,
            root,
            current_atom_str,
            config,
            newuse,
            changed_use,
            update,
            excluded,
            changed_deps,
            with_bdeps,
        )

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
        elif outcome[0] == "reinstall":
            version = outcome[1]
        else:
            # AlreadyInstalled / NoVisibleCandidate: no slot to key a
            # repeat by, so dedup on category/package alone, same as v1
            # always did before slot-aware resolution existed.
            if key in other_outcomes:
                continue
            other_outcomes.add(key)
            # --deep: an AlreadyInstalled package's own further
            # dependencies are walked too, once `deep` allows recursion
            # at this package's own depth (see _deep_recurses_at) --
            # never for NoVisibleCandidate (no version to look anything
            # up by), and never when `nodeps` disables the dependency
            # walk entirely, matching every other outcome's own `nodeps`
            # handling further below.
            if outcome[0] == "already_installed" and not nodeps and _deep_recurses_at(deep, depth):
                _enqueue_dependencies(
                    repos,
                    category,
                    package,
                    outcome[1],
                    config,
                    depth + 1,
                    queue,
                    pending_blockers,
                    key,
                    outcome[1],
                    with_bdeps,
                )
            entries.append((category, package, outcome, [], None, [], []))
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
        repo_name = resolved["repo_name"]

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
                existing_outcome[1]
                if existing_outcome[0] in ("new", "reinstall")
                else existing_outcome[2]
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
        entries.append((category, package, outcome, [], slot, [], []))

        pf = f"{package}-{version}"
        try:
            metadata = read_md5_cache(repo_location, category, pf)
        except OSError:
            continue
        candidate_str = f"{category}/{package}-{version}:{slot}::{repo_name}"
        use_flags = effective_use_flags(
            config["use_flags"],
            config["package_use"],
            config["package_use_force"],
            config["package_use_mask"],
            config["use_stable_force"],
            config["use_stable_mask"],
            config["package_use_stable_force"],
            config["package_use_stable_mask"],
            resolved["keywords"],
            config["accept_keywords"],
            config["package_accept_keywords"],
            candidate_str,
            category,
            package,
        )
        # REQUIRED_USE (PMS 7.3.4/8.2): checked once, here, right after a
        # candidate is newly resolved -- real depgraph.py's own "NOTE:
        # REQUIRED_USE checks are delayed until after package selection"
        # (a genuine *post*-selection check, no part of matching/
        # visibility at all). A violation is FATAL to the whole run
        # regardless of whether this candidate was reached as a
        # top-level atom or a dependency deep in the graph -- real
        # portage's own severity for this -- unlike a dependency's own
        # NoVisibleCandidate (report, don't fail). Calls the real
        # portage.dep.check_required_use directly (pinned to eapi="8",
        # same reasoning as required_use_harness.py's own docstring) --
        # mirrors portage-repo/src/lib.rs's own ported algorithm
        # (portage_required_use::check_required_use) exactly, verified
        # to agree via the shared required-use-harness contract suite.
        required_use = metadata.get("REQUIRED_USE", "").strip()
        if required_use:
            iuse_set = {tok.lstrip("+-") for tok in metadata.get("IUSE", "").split()}
            try:
                satisfied = bool(
                    check_required_use(
                        required_use, use_flags, lambda flag: flag in iuse_set, eapi="8"
                    )
                )
            except InvalidDependString as e:
                raise ResolutionError(
                    f"REQUIRED_USE for {category}/{package}-{version} is invalid: {e}"
                ) from e
            if not satisfied:
                normalized = " ".join(required_use.split())
                raise ResolutionError(
                    f'REQUIRED_USE not satisfied for {category}/{package}-{version}: '
                    f'"{normalized}"'
                )

        # IUSE's own "+flag"/"-flag" default markers only matter for
        # resolving a flag's default when nothing else decides it --
        # already handled upstream, wherever use_flags itself came from --
        # so display only needs the bare flag name, paired with whatever
        # use_flags (the real resolved set) says. Computed (and shown by
        # --pretend -v) regardless of nodeps below -- real portage's own
        # USE display is about the package's own metadata, unrelated to
        # whether its dependencies get walked. Mirrors
        # portage-repo/src/lib.rs's resolve_pretend_graph exactly.
        if metadata.get("IUSE"):
            display = sorted(
                (flag.lstrip("+-"), flag.lstrip("+-") in use_flags)
                for flag in metadata["IUSE"].split()
            )
            entries[entry_idx] = (category, package, outcome, [], slot, display, [])

        # --nodeps: skip this package's own DEPEND/RDEPEND/etc entirely --
        # see this function's own docstring.
        if nodeps:
            continue

        depstr = " ".join(
            metadata[k]
            for k in ("DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND")
            if metadata.get(k)
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
            queue.append((tok, depth + 1, key))

    # Merge required_by_map into entries in a single post-pass, mirroring
    # portage-repo/src/lib.rs's own identical final loop (run before
    # resolve_blockers below, same order) -- entries are tuples
    # (immutable), so this rebuilds each one rather than mutating in
    # place.
    entries = [
        (category, package, outcome, blockers, slot, use_display, sorted(required_by_map.get((category, package), ())))
        for category, package, outcome, blockers, slot, use_display, _required_by in entries
    ]

    # setdefault (not a dict comprehension) so the *first* entry for a
    # given owner wins when the same category/package appears more than
    # once (multiple slots) -- mirrors portage-repo/src/lib.rs's
    # `entries.iter_mut().find(...)`, which also attaches to the first
    # match.
    blockers_by_owner = {}
    for category, package, _o, blockers, _slot, _use_display, _required_by in entries:
        blockers_by_owner.setdefault((category, package), blockers)
    for owner_key, conflict in resolve_blockers(root, pending_blockers, entries):
        blockers_by_owner[owner_key].append(conflict)

    return {"entries": entries, "slot_conflicts": slot_conflicts}


def _deep_recurses_at(deep, depth):
    """Whether an already-installed, already-satisfied package sitting at
    `depth` (0 for a directly-requested top-level atom) should have its
    own further dependencies walked. Mirrors real depgraph.py's own
    `recurse = deep is True or not self._too_deep(self._depth_increment(depth, n=1))`:
    `deep=0` (the default) is never satisfied, regardless of depth;
    `deep is True` always is; a positive int is satisfied while
    `depth < deep`, so a dependency discovered this way lands at
    `depth + 1 <= deep`. Mirrors portage-repo/src/lib.rs's
    Deep::recurses_at exactly."""
    if deep is True:
        return True
    return depth < deep


def _enqueue_dependencies(
    repos,
    category,
    package,
    version,
    config,
    child_depth,
    queue,
    pending_blockers,
    owner_key,
    owner_version,
    with_bdeps=True,
):
    """Reads `category/package-version`'s own DEPEND+RDEPEND+BDEPEND+
    PDEPEND+IDEPEND metadata (from whichever repo actually carries this
    exact version) and enqueues each flattened dependency token -- into
    `pending_blockers` if it's a blocker atom, `queue` (at `child_depth`)
    otherwise -- exactly the same lookup-and-flatten steps
    resolve_pretend_graph's own main loop already takes for a freshly
    resolved New/Upgrade/Reinstall candidate, factored out here so
    --deep's AlreadyInstalled walk can reuse it without duplicating that
    logic. Silently does nothing if `version` can't be found in any
    repo, or its md5-cache entry can't be read -- matching the same
    tolerance the main loop already has for those cases.

    Deliberate simplification: real portage reads an AlreadyInstalled
    package's metadata from the vdb's own installed-time snapshot, not
    the repo's *current* ebuild -- this pilot has no vdb-metadata reader
    (installed_versions only checks presence, never reads DEPEND/USE/
    etc), so this reuses the repo's current metadata for that version
    instead, same as every other candidate lookup in this pilot already
    does.

    `with_bdeps` (real --with-bdeps, see resolve_pretend_graph's own
    docstring for the full grounding): when False, DEPEND and BDEPEND are
    left out of the dep-key list entirely -- RDEPEND/PDEPEND/IDEPEND are
    unaffected. This is the one place that distinction is ever made;
    resolve_pretend_graph's own main loop (the New/Upgrade/Reinstall
    path this function mirrors) deliberately doesn't take a with_bdeps
    parameter at all, since real portage only ever drops build-time deps
    for an already-built package. Mirrors portage-repo/src/lib.rs's
    enqueue_dependencies exactly."""
    repo_candidates = [c for c in list_candidates(repos, category, package) if c["version"] == version]
    if not repo_candidates:
        return
    resolved = max(repo_candidates, key=lambda c: c["repo_priority"])
    slot = resolved["slot"]
    repo_location = resolved["repo_location"]
    repo_name = resolved["repo_name"]

    pf = f"{package}-{version}"
    try:
        metadata = read_md5_cache(repo_location, category, pf)
    except OSError:
        return
    candidate_str = f"{category}/{package}-{version}:{slot}::{repo_name}"
    use_flags = effective_use_flags(
        config["use_flags"],
        config["package_use"],
        config["package_use_force"],
        config["package_use_mask"],
        config["use_stable_force"],
        config["use_stable_mask"],
        config["package_use_stable_force"],
        config["package_use_stable_mask"],
        resolved["keywords"],
        config["accept_keywords"],
        config["package_accept_keywords"],
        candidate_str,
        category,
        package,
    )

    dep_keys = (
        ("DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND")
        if with_bdeps
        else ("RDEPEND", "PDEPEND", "IDEPEND")
    )
    depstr = " ".join(metadata[k] for k in dep_keys if metadata.get(k))
    try:
        flat_deps = use_reduce(depstr, flat=True, uselist=use_flags)
    except InvalidDependString:
        return
    for tok in flat_deps:
        if tok == "||":
            continue
        dep_atom = _parse_atom(tok)
        if dep_atom is not None and dep_atom.blocker:
            pending_blockers.append(
                {
                    "atom_str": tok,
                    "strong": bool(dep_atom.blocker.overlap.forbid),
                    "target_category": dep_atom.cp.split("/", 1)[0],
                    "target_package": dep_atom.cp.split("/", 1)[1],
                    "owner_key": owner_key,
                    "owner_version": owner_version,
                }
            )
            continue
        queue.append((tok, child_depth, owner_key))


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
# genuinely unknown/misspelled flag. Only --pretend/-p, --verbose/-v,
# --newuse/-N, --changed-use/-U, --nodeps/-O, --onlydeps/-o,
# --update/-u, --deep/-D, --exclude/-X, --deselect/-W, --with-bdeps,
# --changed-deps, and --help/-h are actually implemented (see run()
# below); every table here exists purely for recognition, not behavior.
# Mirrors
# PORTING/rust/multicall/src/emerge_options.rs's own copy of these same
# three tables exactly, so both sides report identical text for
# identical input (verified by the shared contract suite).
#
# KNOWN, DOCUMENTED SCOPE CUTS (see emerge_options.rs for the full
# writeup): no short-flag bundling ("-pv" isn't recognized as "-p" +
# "-v"); "category" (boolean/value/action) is tracked for accurate
# enumeration only -- run() reports and exits immediately on any
# recognized-but-unimplemented option, so it never needs to parse or
# skip over that option's own argument.

_BOOLEAN_OPTIONS = [
    ("--alphabetical", None),
    ("--ask-enter-invalid", None),
    ("--buildpkgonly", "-B"),
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
    ("--nobindeps", None),
    ("--noreplace", "-n"),
    ("--nospinner", None),
    ("--oneshot", "-1"),
    ("--quiet-repo-display", None),
    ("--quiet-unmerge-warn", None),
    ("--resume", "-r"),
    ("--searchdesc", "-S"),
    ("--skipfirst", None),
    ("--tree", "-t"),
    ("--unordered-display", None),
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
    ("--changed-deps-report", None),
    ("--changed-slot", None),
    ("--config-root", None),
    ("--color", None),
    ("--complete-graph", None),
    ("--complete-graph-if-new-use", None),
    ("--complete-graph-if-new-ver", None),
    ("--depclean-lib-check", None),
    ("--dynamic-deps", None),
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
    subset doesn't at all -- wildcards, build-ids. portage-dep's Atom
    struct (see its own source) has no fields for either, so a
    Rust-side parse_atom call on the same text returns None outright --
    the same "invalid atom" outcome as genuinely malformed input, not
    the "blocker, not a valid target" outcome _is_blocker_atom below
    covers. Operator, plain slot, slot operator (":=" / ":*" /
    ":slot="), USE deps ("[bar]" and every other PMS 8.3.4 form), AND
    the "::reponame" repo constraint (PMS 3.1.5) are all representable
    on the Rust side, so none of them are checked here."""
    return a.extended_syntax or a.build_id is not None


def _json_escape(s):
    """Escapes `s` for embedding in a JSON string literal (quote,
    backslash, and control characters -- category/package/version/atom
    text from this pilot's own inputs never needs anything fancier).
    Hand-rolled, not json.dumps, so this side's --json output is
    byte-for-byte identical to pretend.rs's own hand-rolled
    json_escape/json_string -- both build the exact same string via the
    exact same field order, rather than two different serializers that
    merely happen to agree. See run()'s own --json handling for why
    --json exists at all (it's NOT a port of any real emerge behavior)."""
    out = []
    for c in s:
        if c == '"':
            out.append('\\"')
        elif c == "\\":
            out.append("\\\\")
        elif c == "\n":
            out.append("\\n")
        elif c == "\r":
            out.append("\\r")
        elif c == "\t":
            out.append("\\t")
        elif ord(c) < 0x20:
            out.append(f"\\u{ord(c):04x}")
        else:
            out.append(c)
    return "".join(out)


def _json_string(s):
    return f'"{_json_escape(s)}"'


def _json_bool(b):
    return "true" if b else "false"


def _entry_to_json(category, package, outcome, blockers, slot, use_display, required_by, top_level_pkgs, verbose):
    """One JSON object per entry -- a structured mirror of the plain-text
    "[ebuild ...]"/"already installed"/blocker lines in run(), plus two
    fields no plain-text line carries at all: "requested" (was this
    exact category/package one of the atoms given directly, as opposed
    to reached only via a dependency string) and "required_by" (which
    package(s), if any, pulled it in that way). "source" is always
    "ebuild": this pilot has no binary-package support anywhere (no
    --usepkg/--getbinpkg, no binpkg reading at all), so nothing else is
    ever possible -- included so a consumer doesn't have to assume it,
    not because this pilot actually distinguishes binary from source.
    Deliberately NOT affected by --onlydeps's own suppression (a
    display-only concern for the plain-text loop in run()): --json
    always dumps the whole resolved graph, letting a consumer filter on
    "requested" itself if they want the --onlydeps view. Mirrors
    pretend.rs's own entry_to_json exactly, field for field, in the same
    order."""
    requested = (category, package) in top_level_pkgs
    fields = [
        f'"category":{_json_string(category)}',
        f'"package":{_json_string(package)}',
    ]
    tag = outcome[0]
    fields.append(f'"outcome":{_json_string(tag)}')
    if tag in ("new", "already_installed"):
        fields.append(f'"version":{_json_string(outcome[1])}')
    elif tag == "upgrade":
        fields.append(f'"version":{_json_string(outcome[2])}')
        fields.append(f'"from_version":{_json_string(outcome[1])}')
    elif tag == "reinstall":
        fields.append(f'"version":{_json_string(outcome[1])}')
        changed_use = ",".join(_json_string(f) for f in outcome[2])
        fields.append(f'"changed_use":[{changed_use}]')
        fields.append(f'"changed_deps":{_json_bool(outcome[3])}')
    fields.append(f'"slot":{_json_string(slot) if slot is not None else "null"}')
    if tag != "no_visible_candidate":
        fields.append('"source":"ebuild"')
    fields.append(f'"requested":{_json_bool(requested)}')
    required_by_json = ",".join(
        f'{{"category":{_json_string(c)},"package":{_json_string(p)}}}' for c, p in required_by
    )
    fields.append(f'"required_by":[{required_by_json}]')
    if verbose and use_display:
        use_flags = ",".join(f"{_json_string(flag)}:{_json_bool(enabled)}" for flag, enabled in use_display)
        fields.append(f'"use_flags":{{{use_flags}}}')
    blockers_json = ",".join(
        f'{{"atom":{_json_string(b["atom_str"])},"strong":{_json_bool(b["strong"])},'
        f'"matched_category":{_json_string(b["matched_category"])},'
        f'"matched_package":{_json_string(b["matched_package"])},'
        f'"matched_version":{_json_string(b["matched_version"])}}}'
        for b in blockers
    )
    fields.append(f'"blockers":[{blockers_json}]')
    return "{" + ",".join(fields) + "}"


def _slot_conflict_to_json(c):
    return (
        f'{{"category":{_json_string(c["category"])},"package":{_json_string(c["package"])},'
        f'"slot":{_json_string(c["slot"])},"resolved_version":{_json_string(c["resolved_version"])},'
        f'"conflicting_atom":{_json_string(c["conflicting_atom"])}}}'
    )


def _print_json(entries, slot_conflicts, top_level_pkgs, verbose):
    """The whole --json output: {"entries": [...], "slot_conflicts": [...]},
    one line, no pretty-printing (a pilot-specific convenience format,
    not a stable schema -- see run()'s own --json handling). Mirrors
    pretend.rs's own print_json exactly."""
    entries_json = ",".join(
        _entry_to_json(category, package, outcome, blockers, slot, use_display, required_by, top_level_pkgs, verbose)
        for category, package, outcome, blockers, slot, use_display, required_by in entries
    )
    conflicts_json = ",".join(_slot_conflict_to_json(c) for c in slot_conflicts)
    print(f'{{"entries":[{entries_json}],"slot_conflicts":[{conflicts_json}]}}')


def _report_option(token):
    """Reports and returns the exit code for a single option/action token
    ("-x" or "--long", never a positional atom) that isn't --pretend/-p,
    --verbose/-v, --newuse/-N, --changed-use/-U, --nodeps/-O, or
    --onlydeps/-o -- shared between a standalone token and one character
    of a decomposed short-flag bundle, so both produce identical
    messages for the same underlying flag. Mirrors pretend.rs's
    report_option exactly."""
    found = _lookup_option(token)
    if found is not None:
        category, canonical = found
        kind = "action" if category == "action" else "option"
        print(
            f'emerge (pilot v1): {kind} "{canonical}" is a real emerge {kind}, '
            "but is not implemented in this pilot (only --pretend/-p, "
            "--verbose/-v, --newuse/-N, --changed-use/-U, --nodeps/-O, "
            "--onlydeps/-o, --update/-u, --deep/-D, --exclude/-X, "
            "--deselect/-W, --with-bdeps, --changed-deps, and --help/-h "
            "are implemented so far; see PROMPT.md)",
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
    print("   -N, --newuse    reinstall an already-installed package if its USE has changed")
    print("   -U, --changed-use  like -N, but ignores newly added/removed IUSE flags entirely")
    print("   -O, --nodeps    do not resolve or show any dependency, only the given atoms")
    print("   -o, --onlydeps  show only the given atoms' dependencies, not the atoms themselves")
    print(
        "   -u, --update    upgrade to a newer visible version even if the installed one satisfies the atom"
    )
    print(
        "   -D, --deep[=N]  also recurse into an already-installed package's own dependencies (optionally, only N levels deep)"
    )
    print(
        "   -X, --exclude ATOMS  leave any matching already-installed package as-is, and never install a matching new one (repeatable, space-separated)"
    )
    print(
        '   -W, --deselect  a standalone action: report which world favorites ATOMS would remove (never writes; requires --pretend)'
    )
    print(
        "       --with-bdeps y|n  include (y, the default) or skip (n) DEPEND/BDEPEND when --deep walks an already-installed package's own dependencies"
    )
    print(
        "       --changed-deps[=y|n]  reinstall an already-installed package whose own vdb-recorded dependencies differ from the current ebuild's"
    )
    print("   -h, --help      show this message and exit")
    print(
        "       --json      dump the whole resolved graph as one line of JSON instead "
        "of the lines above (pilot-specific, not a real emerge option)"
    )
    print()
    print(
        "Every other real emerge option/action is recognized by name (see "
        "lib/_emerge/main.py) but not implemented -- using one reports which "
        "option or action it is, instead of a generic error."
    )
    print("See PORTING/README.md and PORTING/PROMPT.md for this pilot's current scope.")


def _read_world_atoms(root):
    """Reads <root>/var/lib/portage/world (real portage's own WORLD_FILE
    -- lib/portage/const.py) into a list of atom strings, one per line.
    A missing file is not an error -- an empty, or never-yet-created,
    world is a real, valid state, not a mistake.

    KNOWN, DOCUMENTED SCOPE CUT: only plain atom lines are read, via a
    leading "@" check. Real portage's own world file may also contain
    "@some-set" lines (added by a prior "emerge --noreplace @some-set"),
    and real @world is itself defined as the union of this file's own
    atoms with any such referenced sets (see WorldSelectedSet in
    lib/portage/_sets/files.py) -- resolving those recursively would
    need general set-recursion machinery this pilot doesn't have, so a
    "@"-prefixed line here is simply skipped rather than expanded.
    @system (the profile's own "packages" file -- see resolve_config's
    own "system_packages" key) is a separate, different mechanism with
    its own expansion in run() below, not handled by this function at
    all. Only the literal token "@world" triggers *this* expansion.
    Mirrors pretend.rs's read_world_atoms exactly."""
    path = os.path.join(root, "var", "lib", "portage", "world")
    try:
        with open(path) as f:
            text = f.read()
    except FileNotFoundError:
        return []
    return [
        line
        for line in (raw.strip() for raw in text.splitlines())
        if line and not line.startswith("#") and not line.startswith("@")
    ]


def _run_deselect(targets, root):
    """Ports real action_deselect (lib/_emerge/actions.py, lines
    1740-1835) exactly: needs no repo/config resolution at all, only the
    world file and the vdb. Each target is expanded into its own
    actually-installed category/package:slot form(s) -- a bare package
    name (no "/") via real portage's own "null category" mechanism,
    scanning the world file for a same-named atom to borrow its category
    from, then an installed_candidates (vardb.match-equivalent) lookup
    either way -- and each expanded form is matched against every
    world-file atom. Unlike pretend.rs's own run_deselect, which hand-
    rolls a narrower category/package(+slot) equality check as a
    documented scope cut, this reuses the real match_from_list directly
    (the same "why re-derive it" reasoning as _matches_config_entry
    above) -- both give identical results across every case this pilot's
    own contract suite exercises (plain atoms, slot-restricted atoms),
    since neither exercises the version-range/USE-dep territory where
    the two would actually diverge. A "@"-prefixed world entry is never
    matched, consistent with _read_world_atoms's own pre-existing cut for
    @world itself. Mirrors pretend.rs's run_deselect exactly."""
    world_atoms = _read_world_atoms(root)

    expanded = set()
    for target in targets:
        if "/" in target:
            candidate_atom_strs = [target]
        else:
            candidate_atom_strs = []
            for w in world_atoms:
                a = _parse_atom(w)
                if a is not None and a.cp.split("/", 1)[1] == target:
                    candidate_atom_strs.append(f"{a.cp.split('/', 1)[0]}/{target}")

        for atom_str in candidate_atom_strs:
            atom = _parse_atom(atom_str)
            if atom is None:
                print(f"emerge: invalid atom {atom_str!r}", file=sys.stderr)
                return 1
            category, package = atom.cp.split("/", 1)
            for version, slot in installed_candidates(root, category, package):
                candidate_str = f"{category}/{package}-{version}:{slot}"
                if match_from_list(atom_str, [candidate_str]):
                    expanded.add((category, package, slot))

    discard = []
    for world_atom_str in world_atoms:
        w = _parse_atom(world_atom_str)
        if w is None:
            continue
        for cat, pkg, slot in expanded:
            if w.cp == f"{cat}/{pkg}" and (w.slot is None or w.slot == slot):
                discard.append(world_atom_str)
                break

    if not discard:
        print('>>> No matching atoms found in "world" favorites file...')
    else:
        for atom in sorted(discard):
            print(f'>>> Would remove {atom} from "world" favorites file...')
    return 0


def _reinstall_reason(changed_flags, deps_changed):
    """The "(reinstall for ...)" note's own reason text, real portage
    treating --newuse/--changed-use and --changed-deps as independent,
    freely-combinable triggers. `changed_flags` is only ever empty when
    `deps_changed` alone triggered this outcome (resolve_pretend's own
    construction guarantees at least one is non-trivial). Pilot-invented
    wording either way, same as the pre-existing "changed USE: ..." text
    -- real portage's own default --pretend output shows no such
    itemized reason at all. Mirrors pretend.rs's own reinstall_reason
    exactly."""
    if changed_flags and not deps_changed:
        return f"changed USE: {', '.join(changed_flags)}"
    if not changed_flags and deps_changed:
        return "changed dependencies"
    if changed_flags and deps_changed:
        return f"changed USE: {', '.join(changed_flags)}; changed dependencies"
    raise AssertionError(
        "resolve_pretend only ever constructs a reinstall outcome with a "
        "non-empty changed_flags or deps_changed=True"
    )


def run(args):
    if _wants_help(args):
        _print_help()
        return 0

    atom_args = []
    pretend = False
    verbose = False
    newuse = False
    changed_use = False
    nodeps = False
    onlydeps = False
    update = False
    deep = 0
    excluded = []
    json_output = False
    deselect = False
    with_bdeps = True
    changed_deps = False

    i = 0
    while i < len(args):
        arg = args[i]
        if arg in ("--pretend", "-p"):
            pretend = True
            i += 1
        elif arg in ("--newuse", "-N"):
            newuse = True
            i += 1
        elif arg in ("--changed-use", "-U"):
            changed_use = True
            i += 1
        elif arg in ("--nodeps", "-O"):
            nodeps = True
            i += 1
        elif arg in ("--onlydeps", "-o"):
            onlydeps = True
            i += 1
        elif arg in ("--update", "-u"):
            update = True
            i += 1
        elif arg in ("--deep", "-D"):
            # Peeks at the next token, consuming it only if it parses as
            # a non-negative integer -- see pretend.rs's module doc
            # comment (real valid_integers's own __contains__, checked
            # by insert_optional_args before optparse ever sees the
            # value). A bare --deep/-D, or one followed by anything that
            # doesn't parse this way, means unlimited depth, matching
            # real myoptions.deep == "True".
            nxt = args[i + 1] if i + 1 < len(args) else None
            if nxt is not None and nxt.isdigit():
                deep = int(nxt)
                i += 2
            else:
                deep = True
                i += 1
        elif arg.startswith("--deep="):
            # argparse's own native "="-form -- a separate mechanism from
            # the optional-next-token one above, so a non-numeric value
            # here is a real, immediate parse error (matching real
            # parser.error("Invalid --deep parameter: ...")), unlike a
            # non-numeric *next token* above, which just means "no value
            # given" and is left alone.
            value = arg[len("--deep=") :]
            if value.isdigit():
                deep = int(value)
                i += 1
            else:
                print(f'emerge: invalid --deep parameter: "{value}"', file=sys.stderr)
                return 2
        elif arg in ("--exclude", "-X"):
            # Real "action": "append" -- repeatable, each occurrence's
            # own value is itself a *space-separated* atom list (real
            # bin/emerge's own help text: "A space separated list of
            # package names or slot atoms"), so both accumulate: multiple
            # --exclude/-X occurrences, and multiple atoms within one
            # occurrence's value. Unlike --deep/-D's own optional value,
            # this one is required -- a missing value is a real,
            # immediate usage error, not "no value given, fall back to a
            # default."
            if i + 1 >= len(args):
                print('emerge: option "--exclude" requires an argument', file=sys.stderr)
                return 2
            excluded.extend(args[i + 1].split())
            i += 2
        elif arg.startswith("--exclude="):
            excluded.extend(arg[len("--exclude=") :].split())
            i += 1
        elif arg == "--json":
            # NOT a real emerge option at all -- real portage has no
            # structured-output mode for --pretend. Pilot-specific, so
            # deliberately not routed through _lookup_option's real-CLI-
            # surface tables at all (unlike every other flag here), and
            # given no short alias (nothing to bundle).
            json_output = True
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
        elif arg in ("--deselect", "-W"):
            # Real "--deselect": y_or_n, the same optional-value shape
            # "--verbose"/"-v" has above -- but unlike "--verbose", a
            # bare "--deselect"/"-W" turns this whole invocation into a
            # different, standalone action (see _run_deselect's own
            # docstring) rather than modifying ordinary --pretend
            # resolution -- real main.py's own "if myaction is None and
            # myoptions.deselect is True: myaction = 'deselect'".
            nxt = args[i + 1] if i + 1 < len(args) else None
            if nxt == "y":
                deselect = True
                i += 2
            elif nxt == "n":
                deselect = False
                i += 2
            else:
                deselect = True
                i += 1
        elif arg == "--deselect=y":
            deselect = True
            i += 1
        elif arg == "--deselect=n":
            deselect = False
            i += 1
        elif arg == "--with-bdeps":
            # Real "argument_options" with "choices": ("y", "n") --
            # unlike --exclude (arbitrary text) or --deep/--verbose
            # (either an optional peek, or values beyond y/n), this is a
            # REQUIRED, closed-choice value: a missing value is a real,
            # immediate usage error (same shape as --exclude's own), and
            # a value that's neither "y" nor "n" is *also* a real,
            # immediate usage error (real argparse's own choices
            # validation) -- there's no "not given at all" default to
            # silently fall back to for either failure mode.
            if i + 1 >= len(args):
                print('emerge: option "--with-bdeps" requires an argument', file=sys.stderr)
                return 2
            value = args[i + 1]
            if value == "y":
                with_bdeps = True
                i += 2
            elif value == "n":
                with_bdeps = False
                i += 2
            else:
                print(
                    f'emerge: option "--with-bdeps": invalid choice: "{value}" '
                    '(choose from "y", "n")',
                    file=sys.stderr,
                )
                return 2
        elif arg.startswith("--with-bdeps="):
            value = arg[len("--with-bdeps=") :]
            if value == "y":
                with_bdeps = True
                i += 1
            elif value == "n":
                with_bdeps = False
                i += 1
            else:
                print(
                    f'emerge: option "--with-bdeps": invalid choice: "{value}" '
                    '(choose from "y", "n")',
                    file=sys.stderr,
                )
                return 2
        elif arg == "--changed-deps":
            # Real "--changed-deps": y_or_n (default_arg_opts), the same
            # optional-value shape "--verbose"/"-v" and "--deselect"/"-W"
            # already have -- no short alias, though (real main.py
            # declares none). Unlike --deselect, this stays an ordinary
            # --pretend modifier, not a standalone action.
            nxt = args[i + 1] if i + 1 < len(args) else None
            if nxt == "y":
                changed_deps = True
                i += 2
            elif nxt == "n":
                changed_deps = False
                i += 2
            else:
                changed_deps = True
                i += 1
        elif arg == "--changed-deps=y":
            changed_deps = True
            i += 1
        elif arg == "--changed-deps=n":
            changed_deps = False
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
                elif c == "N":
                    newuse = True
                elif c == "U":
                    changed_use = True
                elif c == "O":
                    nodeps = True
                elif c == "o":
                    onlydeps = True
                elif c == "u":
                    update = True
                elif c == "D":
                    deep = True
                elif c == "W":
                    deselect = True
                elif c == "X":
                    # Unlike every other bundle-compatible short flag
                    # here, -X's own value is *required*, not optional --
                    # there's no sensible "just default it" behavior the
                    # way a bundled -v/-D has, so this pilot deliberately
                    # doesn't support bundling -X at all, with a specific
                    # message instead of a misleading generic one.
                    print(
                        "emerge: -X (--exclude) requires an argument and can't be "
                        "bundled with other short flags in this pilot",
                        file=sys.stderr,
                    )
                    return 2
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

    if deselect:
        return _run_deselect(atom_args, _root())

    if not atom_args:
        print(
            "emerge (pilot v1): expected a package atom, e.g. "
            "`emerge --pretend cat/pkg`",
            file=sys.stderr,
        )
        return 2

    try:
        # resolve_config needs the main repo's own location for
        # package.mask/.unmask's repo-level source (see its own
        # docstring) -- found via the same find_repos repos.conf parsing
        # resolve_pretend_graph uses internally a few lines down; called
        # again here since it's cheap and keeps this mirroring the Rust
        # side's own pretend.rs exactly. Resolved before @world/@system
        # expansion below: @system's own atom list lives in config's
        # "system_packages" key, so config must already exist by the
        # time a "@system" token is seen.
        main_repo = next(r for r in find_repos(_config_root()) if r["is_main"])
        config = resolve_config(_config_root(), main_repo["location"])
    except ResolutionError as e:
        print(f"emerge: {e}", file=sys.stderr)
        return 1

    # "@world"/"@system" each expand to their own real atom list, in
    # place, at whichever position they appear -- see _read_world_atoms's
    # own docstring for @world's exact scope (plain atoms only; nested
    # "@set" references stay unimplemented), and resolve_config's own
    # docstring for @system's. Only these two literal tokens trigger
    # expansion -- any other "@"-prefixed token falls through to the
    # ordinary atom-parsing path below and gets a clear "invalid atom"
    # error, not a silent no-op.
    expanded_atoms = []
    for atom_arg in atom_args:
        if atom_arg == "@world":
            expanded_atoms.extend(_read_world_atoms(_root()))
        elif atom_arg == "@system":
            expanded_atoms.extend(config["system_packages"])
        else:
            expanded_atoms.append(atom_arg)
    atom_args = expanded_atoms

    if not atom_args:
        print(
            "emerge (pilot v1): no package atoms to resolve (the target list, "
            "after expanding any @world/@system, is empty)",
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
        result = resolve_pretend_graph(
            _config_root(),
            _root(),
            atom_args,
            config,
            newuse,
            changed_use,
            nodeps,
            update,
            deep,
            excluded,
            with_bdeps,
            changed_deps,
        )
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

    if json_output:
        _print_json(entries, result["slot_conflicts"], top_level_pkgs, verbose)
        return 0

    for category, package, outcome, blockers, _slot, use_display, _required_by in entries:
        tag = outcome[0]
        # --onlydeps (man/emerge.1: "Only merge (or pretend to merge) the
        # dependencies of the packages specified, not the packages
        # themselves"): a directly-requested (top-level) atom's own line
        # is suppressed -- whatever its outcome -- while its dependencies
        # (reached the same as always, since resolve_pretend_graph's own
        # recursion is entirely unaffected by this flag) print normally.
        onlydeps_suppressed = onlydeps and (category, package) in top_level_pkgs
        if tag == "new":
            if not onlydeps_suppressed:
                print(f"[ebuild  N] {category}/{package}-{outcome[1]}{use_suffix(use_display)}")
            print_blockers(category, package, outcome[1], blockers)
        elif tag == "upgrade":
            if not onlydeps_suppressed:
                print(
                    f"[ebuild  U] {category}/{package}-{outcome[2]} (upgrade from {outcome[1]})"
                    f"{use_suffix(use_display)}"
                )
            print_blockers(category, package, outcome[2], blockers)
        elif tag == "reinstall":
            changed_flags = outcome[2]
            deps_changed_flag = outcome[3]
            if not onlydeps_suppressed:
                reason = _reinstall_reason(changed_flags, deps_changed_flag)
                print(
                    f"[ebuild  r] {category}/{package}-{outcome[1]} "
                    f"(reinstall for {reason}){use_suffix(use_display)}"
                )
            print_blockers(category, package, outcome[1], blockers)
        elif tag == "already_installed":
            # Already-satisfied dependencies aren't shown, matching real
            # emerge's usual "don't clutter the list" behavior -- only a
            # directly-requested (top-level) atom gets its own
            # "is already installed; nothing to do" line, and --onlydeps
            # suppresses that too, same as every other outcome above.
            if (category, package) in top_level_pkgs and not onlydeps_suppressed:
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
