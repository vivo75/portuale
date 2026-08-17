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
only the `defaults`/`conf` USE_ORDER layers, `masters` (layout.conf repo
inheritance) still unimplemented, and the real config.py quirk where
`${VAR}` substitution excludes USE across profile levels). Matching a candidate
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

from portage.dep import Atom, check_required_use, match_from_list, paren_enclose, use_reduce
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


def _scope_repo_mask_lines(lines, repo_name):
    """Scopes each of `lines` (raw package.mask/.unmask entries, a
    leading "-" meaning removal) to `repo_name` by appending a "::name"
    suffix to the atom portion -- real append_repo's own "atoms without
    an explicit repo part get one; atoms that already have one are left
    alone" rule (lib/portage/util/__init__.py), applied here to an
    overlay's own repo-level entries so they can never silently
    mask/unmask a same-named package in a *different* repo. A leading
    "-" (removal) is preserved ahead of the atom, not swallowed into it
    -- "-cat/pkg" scopes to "-cat/pkg::name", matching real portage's own
    behavior of scoping a removal atom exactly like an addition one, so
    it can only ever cancel a same-repo-scoped entry. Mirrors
    portage-repo/src/lib.rs's scope_repo_mask_lines exactly."""
    result = []
    for line in lines:
        if line.startswith("-"):
            prefix, atom = "-", line[1:]
        else:
            prefix, atom = "", line
        if "::" in atom:
            result.append(line)
        else:
            result.append(f"{prefix}{atom}::{repo_name}")
    return result


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
    """A line with no keyword tokens after the atom is kept here with an
    *empty* token list, not dropped -- real portage gives a bare atom an
    implicit "~arch" meaning at *both* levels (confirmed by reading
    KeywordsManager.__init__ and getPKeywords -- the same
    accept_keywords_defaults formula either way: "~" + keyword for each
    plain, non-"~"/"-"-prefixed token in the current global
    ACCEPT_KEYWORDS). resolve_config fills in the actual defaults once
    config["accept_keywords"] is final; this function only preserves the
    bare atom itself. Mirrors portage-profile/src/lib.rs's
    parse_package_accept_keywords_lines exactly."""
    result = []
    for line in lines:
        parts = line.split()
        atom, keywords = parts[0], parts[1:]
        result.append((atom, keywords))
    return result


def _parse_package_license_lines(lines):
    """package.license/package.properties/package.accept_restrict: each
    line is "<atom-or-wildcard> <token...>". Same shape as
    package.accept_keywords, reused directly for all three real files --
    except for a bare atom's own meaning: none of these three files gets
    package.accept_keywords's own implicit "~arch"-default treatment in
    real portage, so a bare atom is filtered back out here as a genuine
    no-op. Mirrors portage-profile/src/lib.rs's parse_package_license_lines
    exactly."""
    return [
        (atom, keywords)
        for atom, keywords in _parse_package_accept_keywords_lines(lines)
        if keywords
    ]


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
    """A line with no tokens after the atom is a documented no-op --
    unlike package.accept_keywords, package.use has no
    accept_keywords_defaults-style implicit meaning for a bare atom in
    real portage either, so this one stays a genuine no-op. Purely
    additive across sources, like package.accept_keywords and unlike
    package.mask/.unmask: real portage's own package.use consumption
    only ever .extend()s a growing token list per source, never removes
    a previous entry.

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
            slot, sub_slot = _split_slot(metadata.get("SLOT", "0"))
            candidates.append(
                {
                    "version": version,
                    "keywords": keywords,
                    "slot": slot,
                    "sub_slot": sub_slot,
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
                    "source": "ebuild",
                    "binary_use": None,
                }
            )
    return candidates


def _read_packages_index(pkgdir):
    """Parses <pkgdir>/Packages (real bintree.py's own index file) into
    one dict per package entry -- NOT read_md5_cache's "KEY=value"
    format: real portage's own index format is "KEY: value"
    (colon-space, confirmed against getbinpkg.py's own PackageIndex
    writer and this machine's own real /var/cache/binpkgs/Packages),
    blank-line-separated blocks, first block is a global header (always
    skipped). Trusts the index outright (real "pkgdir-index-trusted"
    behavior) rather than re-deriving fields from the actual binpkg
    file. Missing file -> empty list, not an error. Mirrors
    portage-repo/src/lib.rs's read_packages_index exactly."""
    path = os.path.join(pkgdir, "Packages")
    try:
        with open(path) as f:
            text = f.read()
    except OSError:
        return []
    blocks = []
    current = {}
    for line in text.splitlines():
        if not line.strip():
            if current:
                blocks.append(current)
                current = {}
            continue
        if ": " in line:
            key, value = line.split(": ", 1)
            current[key] = value
    if current:
        blocks.append(current)
    return blocks[1:]


def list_binary_candidates(pkgdir, category, package):
    """Lists every binary-package build of category/package recorded in
    <pkgdir>/Packages -- real bindbapi, the "binary" half of depgraph's
    own candidate dbs list. A binary candidate's own CPV field
    (category/package-version) is matched the same way an ebuild
    filename already is: filtered to this category/package, then
    _strip_version_prefix peels the version off. Mirrors
    portage-repo/src/lib.rs's list_binary_candidates exactly, including
    its own deliberately-lower-than-any-real-repo repo_priority
    (float("-inf") here, i32::MIN there) so an identical-version ebuild
    naturally wins any tie via the existing repo_priority comparison,
    with no special-casing needed anywhere else."""
    candidates = []
    for entry in _read_packages_index(pkgdir):
        cpv = entry.get("CPV")
        if cpv is None or not cpv.startswith(f"{category}/"):
            continue
        pf = cpv[len(category) + 1 :]
        version = _strip_version_prefix(pf, package)
        if version is None:
            continue
        keywords = entry.get("KEYWORDS", "").split()
        slot, sub_slot = _split_slot(entry.get("SLOT", "0"))
        candidates.append(
            {
                "version": version,
                "keywords": keywords,
                "slot": slot,
                "sub_slot": sub_slot,
                "repo_location": "",
                "repo_priority": float("-inf"),
                "repo_name": "__binary__",
                "license": entry.get("LICENSE", ""),
                "iuse": entry.get("IUSE", ""),
                "properties": entry.get("PROPERTIES", ""),
                "restrict": entry.get("RESTRICT", ""),
                "source": "binary",
                "binary_use": set(entry.get("USE", "").split()),
            }
        )
    return candidates


def read_binary_metadata(pkgdir, category, package, version):
    """Re-reads <pkgdir>/Packages for category/package-version's own
    entry -- the binary-candidate counterpart to read_md5_cache, giving
    DEPEND/RDEPEND/etc once a binary candidate has actually been chosen.
    None if not found. Mirrors portage-repo/src/lib.rs's
    read_binary_metadata exactly."""
    want = f"{category}/{package}-{version}"
    for entry in _read_packages_index(pkgdir):
        if entry.get("CPV") == want:
            return entry
    return None


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
        candidate["iuse"],
        config["use_tokens"],
        config["package_use"],
        config["package_use_force"],
        config["package_use_mask"],
        config["use_force"],
        config["use_mask"],
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
        f"{category}/{package}-{candidate['version']}:{candidate['slot']}/{candidate['sub_slot']}"
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


def _keyword_masked_only(candidate, category, package, config):
    """--autounmask's own keyword-suggestion sub-feature (real
    --autounmask-keep-keywords=n, see resolve_pretend_graph's own
    docstring for the full on/off default-resolution logic this pilot
    ported): true iff candidate would be is_visible except for its own
    KEYWORDS -- every other check is_visible makes (package.mask,
    license, properties, restrict) passes, only _keywords_accepted
    fails. Duplicates is_visible's own body rather than refactoring it
    to return a reason -- real portage's own _get_masking_status is
    considerably more elaborate (distinguishing package.mask/license/
    keyword/REQUIRED_USE/etc. reasons, each with its own "unmask hint"),
    and this pilot only needs the single "keywords, and only keywords"
    question for its own deliberately narrow v1."""
    candidate_str = (
        f"{category}/{package}-{candidate['version']}:{candidate['slot']}/{candidate['sub_slot']}"
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

    return not _keywords_accepted(
        candidate["keywords"],
        candidate_str,
        category,
        package,
        config["accept_keywords"],
        config["package_accept_keywords"],
    )


def _suggested_keyword(candidate):
    """The keyword this pilot's own --autounmask v1 would suggest adding
    to package.accept_keywords for candidate -- the first of its own
    (non-"-"-prefixed) KEYWORDS tokens. Deliberately simpler than real
    portage's own _get_masking_status -- see _keyword_masked_only's own
    docstring for the full scope writeup this shares."""
    for token in candidate["keywords"]:
        if not token.startswith("-"):
            return token
    return None


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
    this pilot already documented. A bare package.accept_keywords atom
    with no keyword list at all no longer reaches this function empty:
    resolve_config already substitutes real accept_keywords_defaults's
    own implicit meaning -- the "~"-prefixed unstable form of every
    currently-accepted keyword -- before this function ever sees it, so
    it folds in through _specificity_ordered_flags exactly like any
    other entry's own explicit tokens would.

    A second real mechanism, previously unhandled: a literal "*"/"~*"
    token in the accepted set means "accept any stable keyword"/"accept
    any testing keyword" respectively -- distinct from "**" (accept even
    an *empty* KEYWORDS) and from a plain keyword name, which
    _apply_incremental would otherwise insert as an inert string that
    can never equal a real KEYWORDS entry. Ported from real
    _getMissingKeywords's own per-candidate-keyword loop
    (lib/portage/package/ebuild/_config/KeywordsManager.py, lines
    ~273-300): each of the candidate's own `keywords` is checked for a
    direct match first (short-circuiting immediately, same as real
    "match = True; break"); a "-"-prefixed one (explicit "not supported
    here", distinct from simply absent) never matches and is excluded
    from classification entirely, matching real portage's own elif
    chain; anything else is classified stable or testing ("~"-prefixed)
    for the final fallback -- "*" grants acceptance if *any* declared
    keyword was stable-classified, "~*" if any was testing-classified,
    matching real "(hastesting and '~*' in pgroups) or (hasstable and
    '*' in pgroups)" exactly (the third real disjunct, '"**" in
    pgroups', is the unconditional check already handled above). Mirrors
    portage-repo/src/lib.rs's keywords_accepted exactly."""
    accepted = _specificity_ordered_flags(
        package_accept_keywords, candidate_str, category, package, seed=accept_keywords
    )
    if "**" in accepted:
        return True

    has_stable = False
    has_testing = False
    for k in keywords:
        if k.startswith("-"):
            continue
        if k in accepted:
            return True
        if k.startswith("~"):
            has_testing = True
        else:
            has_stable = True
    return (has_testing and "~*" in accepted) or (has_stable and "*" in accepted)


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
    iuse,
    use_tokens,
    package_use,
    package_use_force,
    package_use_mask,
    use_force,
    use_mask,
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
    """The USE flags in effect for one specific package: `iuse`'s own
    +flag/-flag default markers (real "pkginternal", see below) seeded
    first, then `use_tokens` (the *ordered raw* USE= value strings from
    every profile level's own make.defaults plus make.conf, replayed via
    _apply_incremental directly -- not a pre-flattened set unioned on
    top, see the `iuse`'s own defaults paragraph below for why that
    distinction matters) with every matching package.use entry's tokens
    layered on top after that, in file order, via the same incremental
    -flag/flag/+flag semantics USE itself uses (see _apply_incremental),
    THEN package.use.force/package.use.mask layered on top of that (force
    winning first, then mask -- see _specificity_ordered_flags for how a
    conflict between multiple matching mask/force entries is resolved),
    THEN, only if this candidate counts as "stable" (_is_stable),
    use_stable_force/package_use_stable_force and use_stable_mask/
    package_use_stable_mask -- the .stable. variants of the sources
    already applied above, ported from real getUseMask/getUseForce's own
    per-package branch (which appends the stable variant right alongside
    the ordinary one at each accumulation step, but only when stable).
    Applied per package, mirroring portage-repo/src/lib.rs's
    effective_use_flags exactly -- a package.use entry never affects any
    other package's own resolution.

    `iuse`'s own defaults: found and grounded by comparing this pilot's
    own output against the real, installed system emerge on a real
    package (media-video/ffmpeg) -- REQUIRED_USE reported violated for a
    USE combination that's actually fully satisfied once IUSE's own +/-
    markers are honored (ffmpeg's own real IUSE declares
    +gpl/+dav1d/+drm/etc., none of which this pilot's prior
    effective_use_flags ever enabled, silently defaulting every one of
    them to disabled instead). Real config.py's own _setup_pkg_iuse
    (lib/portage/package/ebuild/config.py, ~line 1878) builds exactly
    this from a package's raw IUSE string -- "+flag" contributes a bare
    "flag" (enable) token, "-flag" contributes itself unchanged
    (disable), a markerless "flag" contributes nothing at all -- and
    stores it under self.configdict["pkginternal"]["USE"], a real, named
    USE_ORDER component (lib/_emerge/actions.py's own default,
    "env:pkg:conf:defaults:pkginternal:features:repo:env.d") -- confirmed
    by reading config.py's own self.uvlist construction (`for x in
    self["USE_ORDER"].split(":"): ...; self.uvlist.reverse()`):
    incremental application walks uvlist in *reversed* USE_ORDER, so
    pkginternal (position 5 of 8) is applied well *before* defaults
    (profile), conf (make.conf), and pkg (package.use) -- real portage's
    own actual precedence has every one of those three able to override
    an IUSE default; only env/env.d (real per-invocation/stacked-profile-
    env overrides, positions 8 and 1) sit even lower/higher than this
    pilot models at all. Ported here as the seed use_flags starts from,
    with use_tokens (defaults/conf) replayed directly on top via
    _apply_incremental -- NOT a plain set union of the already-flattened
    use_flags. An earlier version of this pilot did union a flattened
    base here, which meant base could only ever *add* a flag, never
    explicitly cancel an IUSE +default the way real defaults/conf
    genuinely can (real regenerate() runs one continuous incremental walk
    across the whole reversed uvlist -- pkginternal then defaults then
    conf then pkg -- so a -flag token in defaults/conf really does cancel
    an earlier pkginternal +flag, exactly like any other incremental
    variable). Replaying the ordered raw tokens instead of the flattened
    set closes that gap: resolve_config exposes both use_flags (the
    flattened result, still used elsewhere for e.g. --newuse comparisons)
    and use_tokens (the ordered raw values that produced it). The
    dominant real-world case -- an ebuild author sets a sensible IUSE
    default, and nothing else ever mentions the flag at all -- was
    already correct either way; this closes the narrower case where a
    profile or make.conf genuinely does mention it.

    use_force/use_mask (global use.force/use.mask): applied at the exact
    same position package_use_force/package_use_mask already are (below),
    NOT folded into use_tokens/use_flags early -- real regenerate()'s own
    self.useforce/self.usemask (which setcpv() sets to the *per-package*
    getUseForce(pkg)/getUseMask(pkg), i.e. global force/mask combined
    with the atom-scoped variant) is applied as the literal *last* step
    of its incremental USE walk, strictly after the "pkg" (package.use)
    tier -- so a package.use entry can never override a global
    use.force/use.mask decision, matching real portage."""
    # real pkginternal: only a token with an explicit "+"/"-" marker
    # contributes anything at all -- a markerless IUSE token (no declared
    # default) is a real, deliberate no-op here, matching real config.py's
    # own "if x.startswith('+'): ... elif x.startswith('-'): ..." (no
    # else branch at all).
    iuse_defaults = " ".join(
        tok for tok in iuse.split() if tok.startswith("+") or tok.startswith("-")
    )
    use_flags = set()
    _apply_incremental(iuse_defaults, use_flags)
    for token in use_tokens:
        _apply_incremental(token, use_flags)
    for entry, tokens in package_use:
        if _matches_config_entry(entry, candidate_str, category, package):
            _apply_incremental(" ".join(tokens), use_flags)

    stable = _is_stable(
        keywords, candidate_str, category, package, accept_keywords, package_accept_keywords
    )

    use_flags |= use_force
    use_flags |= _specificity_ordered_flags(
        package_use_force, candidate_str, category, package
    )
    if stable:
        use_flags |= use_stable_force
        use_flags |= _specificity_ordered_flags(
            package_use_stable_force, candidate_str, category, package
        )
    use_flags -= use_mask
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
    candidate_str = (
        f"{category}/{package}-{version}:{candidate['slot']}/{candidate['sub_slot']}"
        f"::{candidate['repo_name']}"
    )
    cur_use = effective_use_flags(
        metadata["IUSE"],
        config["use_tokens"],
        config["package_use"],
        config["package_use_force"],
        config["package_use_mask"],
        config["use_force"],
        config["use_mask"],
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


def _libc_provider_cps(root):
    """Real find_libc_deps(vardb, realized=False) (portage.dep.libc): the
    "cp" (category/package) identity of every atom virtual/libc's own
    installed (vdb) RDEPEND names, once flattened against its own
    installed USE -- empty if virtual/libc isn't installed at all, same
    as real vardb.match("virtual/libc") finding nothing. A simplified,
    one-level port of real expand_new_virt: real Gentoo's own
    virtual/libc RDEPEND is always a flat "|| ( sys-libs/glibc
    sys-libs/musl ... )" of real (non-virtual) packages, so this doesn't
    replicate expand_new_virt's own further case of recursing into a
    *second* virtual reached this way, which real virtual/libc never
    actually needs. Used by _deps_changed to strip libc atoms out of
    both sides of its own comparison before comparing -- real
    strip_libc_deps's whole purpose: practically every ebuild silently
    gains/loses an implicit libc dependency across revisions, and that's
    noise, not a real dependency change worth reporting. Mirrors
    portage-repo/src/lib.rs's libc_provider_cps exactly."""
    result = set()
    for version in installed_versions(root, "virtual", "libc"):
        use_flags = _read_vdb_flag_set(root, "virtual", "libc", version, "USE")
        rdepend = _read_vdb_string(root, "virtual", "libc", version, "RDEPEND")
        atoms = _flat_dep_atoms(rdepend, use_flags)
        if atoms is None:
            continue
        for atom_str in atoms:
            atom = _parse_atom(atom_str)
            if atom is not None:
                result.add(atom.cp)
    return result


def _strip_libc_atoms(atoms, libc_cps):
    """Removes any atom in `atoms` whose own "cp" is in `libc_cps` -- see
    _libc_provider_cps's own docstring. Mirrors
    portage-repo/src/lib.rs's strip_libc_atoms exactly."""
    if not libc_cps:
        return atoms
    result = set()
    for a in atoms:
        atom = _parse_atom(a)
        if atom is None or atom.cp not in libc_cps:
            result.add(a)
    return result


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
    just for this one feature.

    Both atom sets are filtered through _libc_provider_cps first (see
    its own docstring) -- real strip_libc_deps, closing the gap this
    docstring used to name explicitly as unaddressed.

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
    libc_cps = _libc_provider_cps(root)
    repo_atoms = _strip_libc_atoms(repo_atoms, libc_cps)
    vdb_atoms = _flat_dep_atoms(vdb_depstr, installed_use)
    if vdb_atoms is None:
        return True
    return _strip_libc_atoms(vdb_atoms, libc_cps) != repo_atoms


def _split_slot(raw):
    """Splits a raw SLOT string into (slot, sub_slot) -- real portage:
    SLOT="main/sub", sub_slot defaulting to the slot itself when no "/"
    is present (real portage.versions._pkg_str's own slot-parsing
    branch). An empty string (missing SLOT file/key) defaults to
    ("0", "0"), matching the same "0" fallback list_candidates/
    installed_candidates already use for a missing SLOT. Mirrors
    portage-repo/src/lib.rs's split_slot exactly."""
    if not raw:
        return ("0", "0")
    if "/" in raw:
        slot, sub_slot = raw.split("/", 1)
        return (slot, sub_slot)
    return (raw, raw)


def _read_vdb_slot(root, category, package, version):
    """Reads <root>/var/db/pkg/<category>/<package>-<version>/SLOT and
    splits it via _split_slot -- real vardbapi's own SLOT file is
    written verbatim from the same SLOT variable a repo's own ebuild
    declares, so this is the identical format list_candidates's own
    (main-slot-only) parsing already reads from the repo side, just with
    the sub-slot component kept too instead of discarded. Mirrors
    portage-repo/src/lib.rs's read_vdb_slot exactly."""
    return _split_slot(_read_vdb_string(root, category, package, version, "SLOT").strip())


def _slot_changed(root, repos, category, package, version):
    """--changed-slot: whether `version`'s own vdb-recorded SLOT
    (main+sub) differs from the repo's own *current* ebuild for that
    exact version. Real depgraph.py's own _changed_slot: "ebuild =
    self._equiv_ebuild(pkg); return ebuild is not None and (ebuild.slot,
    ebuild.sub_slot) != (pkg.slot, pkg.sub_slot)".

    KNOWN, DOCUMENTED SCOPE CUT: real portage's own consumers of
    _changed_slot live deep inside binary-package/slot-operator-rebuild
    scheduling this pilot has none of -- rejecting a matched installed
    candidate and, depending on context, either aborting the search or
    continuing to look for a binary package with the right SLOT. Ported
    here as simply another independent reinstall trigger instead, the
    same "report a reinstall" simplification --changed-deps already
    established -- captures the dominant real-world effect (a package
    whose SLOT metadata changed upstream, e.g. an ABI-bump SLOT="0" ->
    SLOT="0/2", gets flagged for reinstall) without replicating real
    portage's own considerably messier, binpkg-entangled control flow.
    Deliberately does not reuse a candidate's own already-parsed slot
    (list_candidates already truncates that to the main component only)
    -- re-reads the repo's own raw SLOT value directly instead, the same
    "re-read metadata this pilot's general candidate model doesn't
    carry" approach _deps_changed already uses for DEPEND/RDEPEND. A
    repo-side lookup that fails (version no longer in the tree,
    unreadable metadata) reports "unchanged" (False), the same tolerant
    fallback _deps_changed already uses, matching real
    "_equiv_ebuild(pkg) is None" -> False exactly. Mirrors
    portage-repo/src/lib.rs's slot_changed exactly."""
    vdb_slot = _read_vdb_slot(root, category, package, version)

    repo_candidates = [c for c in list_candidates(repos, category, package) if c["version"] == version]
    if not repo_candidates:
        return False
    resolved = max(repo_candidates, key=lambda c: c["repo_priority"])
    try:
        metadata = read_md5_cache(resolved["repo_location"], category, f"{package}-{version}")
    except OSError:
        return False
    repo_slot = _split_slot((metadata.get("SLOT") or "").strip())

    return vdb_slot != repo_slot


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
        f"{category}/{package}-{candidate['version']}:{candidate['slot']}/{candidate['sub_slot']}::"
        f"{candidate['repo_name']}"
    )
    use_flags = effective_use_flags(
        metadata.get("IUSE", ""),
        config["use_tokens"],
        config["package_use"],
        config["package_use_force"],
        config["package_use_mask"],
        config["use_force"],
        config["use_mask"],
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
    """Lists every installed (version, slot, sub_slot) triple for
    category/package, reading each entry's SLOT file via _split_slot
    (defaulting to ("0", "0") if missing, same fallback as
    list_candidates). Used for blocker matching, which needs slots
    (sub-slot included) to support slotted blocker atoms --
    installed_versions below doesn't need this and stays a plain version
    list for its existing callers. run_deselect itself only ever uses
    version/slot from this (real Atom(f"{pkg.cp}:{pkg.slot}") never
    includes sub-slot either), so adding sub_slot here doesn't change its
    behavior at all. Mirrors portage-repo/src/lib.rs's
    installed_candidates."""
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
                raw_slot = f.read().strip()
        except OSError:
            raw_slot = ""
        slot, sub_slot = _split_slot(raw_slot)
        candidates.append((version, slot, sub_slot))
    return candidates


def installed_versions(root, category, package):
    return [version for version, _slot, _sub_slot in installed_candidates(root, category, package)]


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


def _process_config_lines(
    text, scalars, use_flags, use_tokens, accept_keywords, use_expand, use_expand_unprefixed
):
    for line in text.splitlines():
        parsed = _parse_kv_line(line)
        if parsed is None:
            continue
        key, raw_value = parsed
        value = _substitute(raw_value, scalars)
        if key == "USE":
            _apply_incremental(value, use_flags)
            use_tokens.append(value)
        elif key == "ACCEPT_KEYWORDS":
            _apply_incremental(value, accept_keywords)
        elif key == "USE_EXPAND":
            _apply_incremental(value, use_expand)
        elif key == "USE_EXPAND_UNPREFIXED":
            _apply_incremental(value, use_expand_unprefixed)
        scalars[key] = value


def _read_parent_lines(profile_dir):
    parent_path = os.path.join(profile_dir, "parent")
    if not os.path.isfile(parent_path):
        return []
    with open(parent_path) as f:
        return [line.strip() for line in f if line.strip() and not line.strip().startswith("#")]


def _repo_containing(directory, repos):
    """Finds which of `repos` (each (name, location)) `directory` lives
    inside, via the longest matching location prefix -- mirrors real
    LocationsManager._addProfile's own intersecting_repos/max(key=len)
    logic, needed to resolve a same-repo ":path" profile parent
    shorthand. Mirrors portage-profile/src/lib.rs's repo_containing
    exactly."""
    best = None
    for name, location in repos:
        canon_loc = os.path.realpath(location)
        if directory.startswith(canon_loc) and (best is None or len(canon_loc) > len(best[1])):
            best = (name, canon_loc)
    return best


def _expand_parent_colon(parent, current_repo, repos, parents_file):
    """Expands a profile "parent" file line's real cross-repo ":path"/
    "reponame:path" syntax (LocationsManager._expand_parent_colon): a
    ":" with nothing before it means "this same repo" (current_repo),
    anything else before the ":" is another repo's own name, looked up
    in repos. Both forms expand to "<repo_location>/profiles/<rest>". A
    line with no ":" at all is returned unchanged. Real portage only
    allows this syntax when the current profile node's own repo
    declares profile-formats = portage-2 in layout.conf -- this pilot
    doesn't model layout.conf profile-formats at all, so it's always
    allowed here (see resolve_config's own docstring). Mirrors
    portage-profile/src/lib.rs's expand_parent_colon exactly."""
    colon = parent.find(":")
    if colon == -1:
        return parent
    if colon == 0:
        if current_repo is None:
            raise ResolutionError(
                f'parent "{parent}" not found: {parents_file} '
                "(not inside any known repo)"
            )
        repo_loc = current_repo[1]
        rest = parent[1:]
    else:
        repo_name = parent[:colon]
        repo_loc = next((loc for name, loc in repos if name == repo_name), None)
        if repo_loc is None:
            raise ResolutionError(
                f'parent "{parent}" not found: {parents_file} '
                f'(no repo named "{repo_name}")'
            )
        rest = parent[colon + 1 :]
    return os.path.join(repo_loc, "profiles", rest)


def _visit_profile(directory, repos, visited, chain):
    canon = os.path.realpath(directory)
    if not os.path.isdir(canon):
        raise ResolutionError(f"resolving profile {directory}: not a directory")
    if canon in visited:
        return
    visited.add(canon)
    current_repo = _repo_containing(canon, repos)
    parents_file = os.path.join(canon, "parent")
    for parent in _read_parent_lines(canon):
        expanded = _expand_parent_colon(parent, current_repo, repos, parents_file)
        _visit_profile(os.path.join(canon, expanded), repos, visited, chain)
    chain.append(canon)


def _resolve_profile_chain(leaf, repos):
    visited = set()
    chain = []
    _visit_profile(leaf, repos, visited, chain)
    return chain


def _process_make_conf_file(
    path,
    config_root,
    scalars,
    use_flags,
    use_tokens,
    accept_keywords,
    use_expand,
    use_expand_unprefixed,
    visited_sources,
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
                use_tokens,
                accept_keywords,
                use_expand,
                use_expand_unprefixed,
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
            use_tokens.append(value)
        elif key == "ACCEPT_KEYWORDS":
            _apply_incremental(value, accept_keywords)
        elif key == "USE_EXPAND":
            _apply_incremental(value, use_expand)
        elif key == "USE_EXPAND_UNPREFIXED":
            _apply_incremental(value, use_expand_unprefixed)
        scalars[key] = value


def resolve_config(config_root, main_repo_location, overlay_repos=(), main_repo_name=""):
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
    "package_use_force", "package_use_mask", "use_expand",
    "use_expand_unprefixed", "use_stable_force",
    "use_stable_mask", "package_use_stable_force", "package_use_stable_mask".

    main_repo_location (the main repo's own tree root -- see
    find_repos/is_main) is needed for package.mask/.unmask's repo-level
    source, <main_repo_location>/profiles/package.mask -- real portage's
    most common real-world masking source. It's stacked together with
    every profile level's own package.mask/.unmask (in chain order) and
    the user-level /etc/portage files, exactly matching real
    MaskManager.py's three-source stack (see _stack_mask_lines).

    overlay_repos (each overlay's own (name, location) pairs, e.g. every
    non-main entry from find_repos) supplies each overlay's own
    repo-level package.mask/.unmask too, real MaskManager.py's own
    repositories.repos_with_profiles() loop -- confirmed by reading it
    directly: it iterates every configured repo unconditionally, not
    just the main one. Each overlay's own lines are scoped with a
    "::reponame" suffix first (_scope_repo_mask_lines, real
    append_repo's own "atoms without an explicit repo part get one;
    atoms that already have one are left alone" rule, applied to a
    "-atom" removal line's own atom portion too) before being folded
    into the same stack -- otherwise an overlay's own mask entry would
    silently also mask a same-named package in the main repo or another
    overlay, which real portage's own scoping specifically prevents.
    Deliberately asymmetric, confirmed while implementing this: the main
    repo's own entries above stay unscoped, matching this pilot's own
    pre-existing (unchanged) behavior -- real portage scopes every
    repo's own repo-level entries this same way, including the main
    repo's, so a package.mask entry from main only masking main's own
    packages is a separate, distinct correctness question this slice
    doesn't also take on. Real masters (each repo's own package.mask --
    and ONLY package.mask, MaskManager.py's own package.unmask loop
    never consults masters at all -- stacks with its declared masters'
    own lines before repo-scoping) is now modeled to the extent every
    fixture here needs: a repo with no explicit "masters =" (this pilot
    doesn't parse that repos.conf key at all yet) implicitly masters the
    main repo alone, real config.py's own
    "repo.masters = (self.mainRepo(),)" default -- every overlay here
    gets exactly that. An explicit "masters =" override, or a
    multi-master chain, stays unimplemented. profiles/ (an overlay's own profile
    directory joining the active chain) and license_groups from an
    overlay are NOT part of this same "every repo, unconditionally"
    mechanism -- real LicenseManager's own profile_locations and the
    profile chain itself only ever include an overlay's own directories
    once the active chain's parent file uses reponame:path syntax to
    reach into it (_expand_parent_colon, main_repo_name below), which is
    exactly what makes them reachable: once a chain level's parent file
    names an overlay, every "for level in chain" loop below
    (license_groups included) reads from that overlay's own directory
    the same as any other chain level, with no separate code path
    needed.

    main_repo_name (the main repo's own name from repos.conf, e.g.
    find_repos's main entry) plus overlay_repos above together give
    _resolve_profile_chain every configured repo's own (name, location),
    needed to resolve a parent file's real cross-repo syntax
    (_expand_parent_colon, grounded against
    LocationsManager._expand_parent_colon): a bare ":some/path" means
    "this same repo" (whichever repo the current profile node's own
    directory belongs to -- _repo_containing), "reponame:some/path"
    means a different, named repo. Both expand to
    "<repo_location>/profiles/some/path"."""
    use_flags = set()
    use_tokens = []
    accept_keywords = set()
    use_expand = set()
    use_expand_unprefixed = set()
    scalars = {}

    all_repos = [(main_repo_name, main_repo_location)] + list(overlay_repos)

    make_profile = os.path.join(config_root, "etc", "portage", "make.profile")
    chain = _resolve_profile_chain(make_profile, all_repos) if os.path.exists(make_profile) else []
    for level in chain:
        make_defaults = os.path.join(level, "make.defaults")
        if not os.path.isfile(make_defaults):
            continue
        # Real config.py quirk: USE is excluded from cross-level
        # substitution -- see the module doc comment.
        scalars.pop("USE", None)
        with open(make_defaults) as f:
            text = f.read()
        _process_config_lines(
            text,
            scalars,
            use_flags,
            use_tokens,
            accept_keywords,
            use_expand,
            use_expand_unprefixed,
        )

    make_conf = os.path.join(config_root, "etc", "portage", "make.conf")
    if os.path.isfile(make_conf):
        _process_make_conf_file(
            make_conf,
            config_root,
            scalars,
            use_flags,
            use_tokens,
            accept_keywords,
            use_expand,
            use_expand_unprefixed,
            set(),
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
    # already uses, folded directly into use_flags. USE_EXPAND_UNPREFIXED
    # (real config.py's own companion mechanism -- no prefix at all,
    # applied in the loop right below this one) IS now read too. Still
    # out of scope, deliberately: IUSE-aware wildcard expansion,
    # USE_EXPAND_HIDDEN/_IMPLICIT, and package.use's own USE_EXPAND-prefix
    # shorthand (a separate, not-yet-ported follow-up). Mirrors
    # portage-profile/src/lib.rs's resolve_config exactly.
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
        prefixed = " ".join(prefixed_tokens)
        _apply_incremental(prefixed, use_flags)
        use_tokens.append(prefixed)

    # USE_EXPAND_UNPREFIXED: real config.py's own companion to
    # USE_EXPAND -- the exact same mechanism, except the value is folded
    # into use_flags directly, with no "lowercase(name)_" prefix at all
    # -- real Gentoo's own profile sets
    # USE_EXPAND_UNPREFIXED="ARCH", so ARCH="amd64" contributes the bare
    # "amd64" flag, not "arch_amd64" (this is literally how
    # amd64/x86/arm64 etc. exist as real USE flags at all). Mirrors
    # portage-profile/src/lib.rs's resolve_config exactly.
    for var in use_expand_unprefixed:
        value = scalars.get(var)
        if value is None:
            continue
        _apply_incremental(value, use_flags)
        use_tokens.append(value)

    # use.mask/use.force: every profile level's own file (in chain
    # order), stacked with the same "-atom" removal semantics
    # package.mask uses (see _stack_mask_lines) -- mirrors
    # portage-profile/src/lib.rs's resolve_config exactly. Deliberately
    # NOT folded into use_flags here (an earlier version of this pilot
    # did, which was wrong): real regenerate() applies self.useforce/
    # self.usemask (which setcpv() sets to the *per-package*
    # getUseForce(pkg)/getUseMask(pkg) -- global use.force/use.mask
    # combined with the atom-scoped package.use.force/.mask this pilot
    # already applies per-candidate) as the literal *last* step of its
    # own incremental USE walk, strictly *after* the "pkg" (package.use)
    # tier -- see effective_use_flags's own doc comment for where
    # use_force/use_mask actually get applied now, alongside the atom-
    # scoped package_use_force/package_use_mask it already positions
    # correctly (force-add first, then force-remove, so a flag in both
    # ends up masked, not forced).
    usemask_sources = [_read_config_lines(os.path.join(level, "use.mask")) for level in chain]
    useforce_sources = [_read_config_lines(os.path.join(level, "use.force")) for level in chain]
    use_force = set(_stack_mask_lines(useforce_sources))
    use_mask = set(_stack_mask_lines(usemask_sources))

    # PORTAGE_ARCHLIST: same chain, same stacking semantics as
    # use.mask/use.force just above -- mirrors portage-profile/src/
    # lib.rs's own "archlist" doc comment for the full grounding.
    archlist_sources = [_read_config_lines(os.path.join(level, "arch.list")) for level in chain]
    archlist = set(_stack_mask_lines(archlist_sources))

    main_repo_mask_lines = _read_config_lines(
        os.path.join(main_repo_location, "profiles", "package.mask")
    )
    mask_sources = [main_repo_mask_lines]
    unmask_sources = [
        _read_config_lines(os.path.join(main_repo_location, "profiles", "package.unmask"))
    ]
    for repo_name, repo_location in overlay_repos:
        # Real masters: a repo with no explicit "masters =" implicitly
        # masters the main repo alone (config.py's own
        # "repo.masters = (self.mainRepo(),)" default) -- an overlay's
        # own package.mask is stacked *on top of* its master's own (main
        # repo's) package.mask before the usual "::reponame" scoping.
        # package.unmask deliberately does NOT get the same treatment --
        # confirmed by reading MaskManager.py's own two loops side by
        # side: only the package.mask loop iterates masters at all.
        overlay_mask_lines = _read_config_lines(
            os.path.join(repo_location, "profiles", "package.mask")
        )
        mastered_mask_lines = _stack_mask_lines([main_repo_mask_lines, overlay_mask_lines])
        mask_sources.append(_scope_repo_mask_lines(mastered_mask_lines, repo_name))
        unmask_sources.append(
            _scope_repo_mask_lines(
                _read_config_lines(os.path.join(repo_location, "profiles", "package.unmask")),
                repo_name,
            )
        )
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

    # A bare atom (empty token list, preserved by the parser above) gets
    # real accept_keywords_defaults's own implicit meaning: "~" plus
    # every plain (non-"~"/"-"-prefixed) token in the final global
    # ACCEPT_KEYWORDS -- computed once here, against accept_keywords as
    # already fully resolved by this point, exactly matching what real
    # portage computes it from at both of its own two call sites
    # (KeywordsManager.__init__'s own global_accept_keywords parameter,
    # getPKeywords's own pgroups -- both already-resolved global
    # ACCEPT_KEYWORDS by the time either runs). Sorted only for
    # deterministic output; downstream consumption folds these into a
    # set, so order was never semantically significant.
    accept_keywords_defaults = sorted(
        "~" + keyword for keyword in accept_keywords if keyword[:1] not in "~-"
    )
    package_accept_keywords = [
        (atom, keywords if keywords else accept_keywords_defaults)
        for atom, keywords in _parse_package_accept_keywords_lines(accept_keywords_lines)
    ]

    return {
        "use_flags": use_flags,
        "use_tokens": use_tokens,
        "accept_keywords": accept_keywords,
        "package_mask": _stack_mask_lines(mask_sources),
        "package_unmask": _stack_mask_lines(unmask_sources),
        "package_accept_keywords": package_accept_keywords,
        "package_use": (
            _parse_package_use_lines(repo_and_profile_use_lines)
            + _parse_package_use_lines(user_use_lines, use_expand_shorthand=True)
        ),
        "system_packages": system_packages,
        "use_force": use_force,
        "use_mask": use_mask,
        "archlist": archlist,
        "package_use_force": _parse_package_use_lines(use_force_lines),
        "package_use_mask": _parse_package_use_lines(use_mask_lines),
        "use_expand": use_expand,
        "use_expand_unprefixed": use_expand_unprefixed,
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
        # PKGDIR (--usepkg/--usepkgonly's own binary-package directory,
        # real bintree.py's own "pkgdir"): ordinary make.conf scalar,
        # real default /var/cache/binpkgs (cnf/make.globals). Mirrors
        # portage-profile/src/lib.rs's Config::pkgdir exactly.
        "pkgdir": scalars.get("PKGDIR", "/var/cache/binpkgs"),
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


def _resolve_disjunctions(nodes, uselist, alternative_satisfiable):
    """Walks `nodes` (real use_reduce(flat=False, uselist=uselist)'s own
    nested-list shape), picking the first alternative of every "||"
    group whose own flattened atoms `alternative_satisfiable` accepts --
    see _use_reduce_flat_disjunctive's own docstring for the full
    grounding. Real use_reduce(flat=False) already fully resolves every
    USE conditional against `uselist` before this ever sees the tree
    (an inactive flag?'s own group vanishes entirely, an active one
    splices its own contents in with no marker left) -- so, unlike
    portage-repo/src/lib.rs's own DepNode-based walk (which has to
    handle "flag?" pairing itself, since its own tree mirrors real
    paren_reduce's pre-conditional-resolution shape instead), this only
    ever needs to handle two node shapes at all: a bare atom string, and
    a nested list (a plain bracketed group, or one member of a "||"
    alternatives list). Mirrors portage-repo/src/lib.rs's own
    resolve_disjunctions in observable behavior, not literal structure."""
    result = []
    i = 0
    while i < len(nodes):
        node = nodes[i]
        if node == "||":
            alternatives = nodes[i + 1]
            chosen = None
            for alt in alternatives:
                alt_nodes = alt if isinstance(alt, list) else [alt]
                try:
                    flat_atoms = use_reduce(paren_enclose(alt_nodes), flat=True, uselist=uselist)
                except InvalidDependString:
                    continue
                if alternative_satisfiable(flat_atoms):
                    chosen = _resolve_disjunctions(alt_nodes, uselist, alternative_satisfiable)
                    break
            if chosen is not None:
                result.extend(chosen)
            else:
                result.append("||")
                result.append(alternatives)
            i += 2
        elif isinstance(node, list):
            result.append(_resolve_disjunctions(node, uselist, alternative_satisfiable))
            i += 1
        else:
            result.append(node)
            i += 1
    return result


def _use_reduce_flat_disjunctive(depstr, uselist, alternative_satisfiable):
    """Real _add_pkg_dep_string's own "||" resolution, considerably
    simplified: picks the first alternative every one of whose own
    atoms `alternative_satisfiable` accepts, instead of flattening
    every alternative into the result the way plain
    use_reduce(flat=True) always has. An alternative that resolves to
    zero atoms at all (every token inside it gated by an inactive
    conditional) counts as trivially satisfiable -- `alternative_
    satisfiable` is expected to return True for an empty list, the
    same vacuous-truth real portage itself gives a no-cost alternative.

    Falls back to keeping the *whole* "||" group exactly as
    use_reduce(flat=True) would have flattened it (literal "||" marker,
    every alternative's own atoms, no selection at all) whenever *no*
    alternative is currently satisfiable -- so a dependency this pilot
    can't currently resolve is never silently dropped, preserving the
    exact "never silently wrong about whether a dependency exists"
    invariant resolve_pretend_graph's own docstring already established
    for the unconditional-flatten v1 this replaces. Real portage's own
    considerably richer preference order (installed packages first,
    backtracking on a later constraint failure, etc.) isn't ported --
    this pilot has no backtracking architecture at all -- just the
    single "first currently-resolvable alternative wins" rule. Mirrors
    portage-repo/src/lib.rs's use_reduce_flat_disjunctive exactly."""
    tree = use_reduce(depstr, flat=False, uselist=uselist)
    resolved = _resolve_disjunctions(tree, uselist, alternative_satisfiable)
    return use_reduce(paren_enclose(resolved), flat=True, uselist=uselist)


def _atom_currently_satisfiable(repos, atom_str, config):
    """Whether every atom in `atoms` currently has a satisfying
    candidate -- the probe _use_reduce_flat_disjunctive needs to pick a
    "||" group's own first currently-resolvable alternative. A blocker
    atom is always satisfiable here, vacuously -- it isn't a dependency
    to *resolve* at all, just a conflict to report (_enqueue_flat_deps
    handles that separately, unaffected by which "||" alternative was
    chosen), so it never disqualifies an otherwise-fine alternative.

    Deliberately the *early* half of resolve_pretend's own logic only
    (list_candidates -> filter is_visible -> match_from_list -> USE-dep
    post-filter) -- not a call to resolve_pretend itself, which also
    applies --update/--newuse/--exclude/reinstall refinements that only
    matter once an alternative has already been chosen and is actually
    being resolved. Mirrors portage-repo/src/lib.rs's
    atom_currently_satisfiable exactly."""
    atom = _parse_atom(atom_str)
    if atom is None:
        return False
    if atom.blocker:
        return True
    category, package = atom.cp.split("/", 1)
    candidates = list_candidates(repos, category, package)
    visible = [c for c in candidates if is_visible(c, category, package, config)]
    if not visible:
        return False

    candidate_strs = [
        f"{category}/{package}-{c['version']}:{c['slot']}/{c['sub_slot']}::{c['repo_name']}"
        for c in visible
    ]
    by_str = dict(zip(candidate_strs, visible))
    matched = [by_str[m] for m in match_from_list(atom_str, candidate_strs) if m in by_str]

    if atom.use:
        matched = [
            c
            for c in matched
            if _use_deps_satisfied(atom, *_candidate_iuse_and_use(c, category, package, config))
        ]
    return bool(matched)


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
    changed_slot=False,
    selective=False,
    is_top_level=False,
    usepkg=False,
    usepkgonly=False,
    binpkg_respect_use=False,
):
    """The single-atom v1 resolution decision: find the best visible
    candidate matching `atom_str` (any atom portage-dep's v1 grammar
    supports -- operator, slot, not just a bare category/package) across
    all of `repos` (the main repo and any overlays -- see find_repos),
    compare it against what's installed. Returns a tuple whose first
    element is the outcome tag: "new", "upgrade", "downgrade", "reinstall",
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

    `changed_deps` (--changed-deps) and `changed_slot` (--changed-slot)
    are each an independent, freely-combinable reinstall trigger
    alongside newuse/changed_use -- see _deps_changed's/_slot_changed's
    own docstrings for the real depgraph.py::_changed_deps/_changed_slot
    behavior they port. `with_bdeps` (--with-bdeps) only affects which
    dependency keys _deps_changed itself compares; see
    resolve_pretend_graph's own docstring for the full with_bdeps
    grounding.

    `selective`/`is_top_level`: a real, previously-undiscovered gap in
    the `update` handling above, found by comparing this pilot's own
    output against the real, installed system emerge on a real package
    (sys-apps/portage) and tracing real portage's own decision live.
    Real portage's own avoid_update shortcut (`not update`, ported as
    `update` here) is NOT sufficient on its own for a
    **directly-requested (top-level) atom**: real
    _wrapped_select_pkg_highest_available_imp's own per-candidate loop
    (lib/_emerge/depgraph.py) computes `want_reinstall = reinstall or
    empty or (found_available_arg and not selective)`, and `if
    want_reinstall and matched_packages: continue` -- for a "found via
    an atom on the command line" (found_available_arg, real
    _iter_atoms_for_pkg) candidate, this SKIPS ever re-adding the
    already-installed Package object as a further candidate at all
    whenever `selective` is absent, so the later `if avoid_update: ...
    return pkg` shortcut (lib/_emerge/depgraph.py line ~8447) finds
    nothing installed to return and falls through to picking the best
    *available* (ebuild) candidate instead -- even when its version is
    identical to what's already installed. The net real effect: a bare
    `emerge <atom>` with no other flags, on an atom named directly (not
    reached via a dependency string), always resolves against the best
    *available* version (searching for a newer one exactly as --update
    would), and even when nothing newer exists, still reports a bare
    reinstall (real "[ebuild R] cat/pkg-ver", no parenthetical reason at
    all) rather than treating the identical installed version as
    satisfying -- confirmed live: --noreplace/--selective (both of which
    set real myparams["selective"]) restore the "nothing to do" result.
    `selective` here mirrors real create_depgraph_params.py's own
    myparams["selective"] = True condition, computed from whichever of
    its own real trigger flags this pilot actually implements: update,
    newuse, changed_use (real portage's own --changed-use/-U rewrites to
    --reinstall=changed-use before create_depgraph_params ever runs,
    lib/_emerge/main.py, and --reinstall is itself constrained to that
    one literal choice in real portage -- so changed_use alone covers
    this pilot's whole share of that real condition, no separate
    --reinstall flag needed), changed_deps (any non-"n" value),
    changed_slot, plus the two flags whose entire real effect is exactly
    this (see run()'s own CLI parsing): --noreplace/-n and
    --selective[=y|n] ("n" explicitly cancels selective even if one of
    the other conditions set it, matching real create_depgraph_
    params.py's own `if myopts.get("--selective") == "n"`: pop
    unconditionally). Real --newrepo (forces reinstall specifically on
    an installed-vs-current repo mismatch, and separately contributes to
    selective) is a documented, narrower scope cut, deliberately not
    modeled: this pilot has no vdb REPOSITORY reader (confirmed absent
    during this same investigation -- the real vdb file is even
    lowercase "repository", unlike every other metadata key).

    `is_top_level` is this pilot's own existing "argument" equivalent --
    resolve_pretend_graph's own `depth == 0`, the identical equivalence
    already established for --with-test-deps's own `pkg.depth == 0 and
    self._is_argument(pkg)` gating. A dependency atom (is_top_level =
    False) is NEVER affected by selective at all -- real
    found_available_arg is only ever set for an argument-derived
    candidate in the first place, so a dependency atom's own
    already-installed, still-satisfying version keeps exactly its
    pre-existing already_installed treatment, unconditionally, matching
    real _want_installed_pkg's own `return not arg` fallback (empty arg
    for a non-argument package).

    Applied at both places this function can otherwise decide an
    installed version satisfies the atom "as is": the `not update`
    shortcut immediately below (skipped entirely -- not just its outcome
    adjusted -- whenever `is_top_level and not selective`, so version
    selection also falls through to the ordinary "best across
    everything visible" comparison below, exactly reproducing real
    portage's own "searches for a newer version even without --update"
    effect for this case) and the final "best visible candidate happens
    to already be installed" comparison further down (where, instead,
    the outcome is forced to "reinstall" -- with whatever changed_flags/
    deps_changed_flag/slot_changed_flag were independently computed, all
    three possibly still empty/false, exactly matching real portage's
    own bare, reasonless "[ebuild R]").
    Mirrors portage-repo/src/lib.rs's resolve_pretend exactly."""
    atom = _parse_atom(atom_str)
    if atom is None:
        raise ResolutionError(f'invalid atom "{atom_str}"')
    category, package = atom.cp.split("/", 1)

    # --usepkg/--usepkgonly (real depgraph.py's own candidate-pool
    # construction): --usepkgonly excludes ebuild candidates entirely;
    # either flag alone makes binary candidates (PKGDIR/Packages)
    # eligible alongside them. Mirrors portage-repo/src/lib.rs's
    # resolve_pretend exactly.
    candidates = [] if usepkgonly else list_candidates(repos, category, package)
    if usepkg or usepkgonly:
        candidates = candidates + list_binary_candidates(config["pkgdir"], category, package)
    visible = [c for c in candidates if is_visible(c, category, package, config)]
    if not visible:
        return ("no_visible_candidate",)

    # Reuses the real match_from_list rather than re-deriving
    # version/slot matching rules here, mirroring portage-repo's Rust
    # side exactly.
    candidate_strs = [
        f"{category}/{package}-{c['version']}:{c['slot']}/{c['sub_slot']}::{c['repo_name']}"
        for c in visible
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

    # --binpkg-respect-use (real default: "auto", effectively on, unless
    # --usepkgonly is set -- see run()'s own default-resolution logic).
    # For each matched *binary* candidate, computes what USE would
    # currently be selected (the same effective_use_flags machinery an
    # ebuild candidate's own display/dependency-walk already uses) and
    # compares it, over this candidate's own declared IUSE flags only,
    # against its own baked-in "binary_use" -- any mismatch rejects it.
    # Mirrors portage-repo/src/lib.rs's resolve_pretend exactly.
    if binpkg_respect_use:
        new_matched = []
        for c in matched:
            if c["binary_use"] is None:
                new_matched.append(c)
                continue
            candidate_str = (
                f"{category}/{package}-{c['version']}:{c['slot']}/{c['sub_slot']}"
                f"::{c['repo_name']}"
            )
            would_select = effective_use_flags(
                c["iuse"],
                config["use_tokens"],
                config["package_use"],
                config["package_use_force"],
                config["package_use_mask"],
                config["use_force"],
                config["use_mask"],
                config["use_stable_force"],
                config["use_stable_mask"],
                config["package_use_stable_force"],
                config["package_use_stable_mask"],
                c["keywords"],
                config["accept_keywords"],
                config["package_accept_keywords"],
                candidate_str,
                category,
                package,
            )
            flags = [tok.lstrip("+-") for tok in c["iuse"].split()]
            if all((flag in would_select) == (flag in c["binary_use"]) for flag in flags):
                new_matched.append(c)
        matched = new_matched

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
                f"{installed_best['slot']}/{installed_best['sub_slot']}::{installed_best['repo_name']}"
            )
            if any(
                _matches_config_entry(ex, installed_str, category, package) for ex in excluded
            ):
                return ("already_installed", installed_best["version"])

    if not update and (not is_top_level or selective):
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
            slot_changed_flag = changed_slot and _slot_changed(
                root, repos, category, package, installed_best["version"]
            )
            if changed_flags or deps_changed_flag or slot_changed_flag:
                return (
                    "reinstall",
                    installed_best["version"],
                    changed_flags,
                    deps_changed_flag,
                    slot_changed_flag,
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
                    f"{category}/{package}-{c['version']}:{c['slot']}/{c['sub_slot']}::{c['repo_name']}",
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
        slot_changed_flag = changed_slot and _slot_changed(
            root, repos, category, package, best["version"]
        )
        # is_top_level and not selective: real portage's own bare,
        # reasonless "[ebuild R]" -- see this function's own docstring's
        # selective/is_top_level paragraph. changed_flags/
        # deps_changed_flag/slot_changed_flag may all still be
        # empty/false here; that's the whole point of this case.
        if changed_flags or deps_changed_flag or slot_changed_flag or (is_top_level and not selective):
            return (
                "reinstall",
                best["version"],
                changed_flags,
                deps_changed_flag,
                slot_changed_flag,
            )
        return ("already_installed", best["version"])
    if installed:
        current = _max_version(installed)
        if vercmp(best["version"], current) < 0:
            return ("downgrade", current, best["version"])
        return ("upgrade", current, best["version"])
    return ("new", best["version"])


def resolve_blockers(root, pending, entries):
    """Matches each `pending` blocker's target category/package against
    both currently-installed candidates (installed_candidates, sub-slot
    included) and this graph's own resolved New/Upgrade set (entries,
    which may now hold more than one slot for the same category/package
    -- every one of them is a real candidate, not just the first),
    reusing the real match_from_list exactly as every other atom-vs-
    candidate check in this module does (it ignores an atom's blocker
    marker entirely -- verified empirically -- so a "!"/"!!"-prefixed
    atom string matches candidates by category/package/version/slot
    exactly like a normal one). A match against the owner package's own
    resolved version is dropped defensively (a package blocking itself
    is nonsensical, but cheap to guard against). Returns (owner_key,
    conflict_dict) pairs.

    `entries`' own contribution has no real sub-slot data at all -- its
    own "slot" field deliberately stays main-slot-only for now (a
    documented, narrower scope cut than installed_candidates's own repo/
    vdb-backed fix), so it defaults sub-slot to the main slot itself,
    the same fallback _split_slot already uses for a plain (no "/")
    SLOT value. Mirrors portage-repo/src/lib.rs's resolve_blockers
    exactly."""
    conflicts = []
    for pb in pending:
        target_key = (pb["target_category"], pb["target_package"])
        candidates = list(
            installed_candidates(root, pb["target_category"], pb["target_package"])
        )
        for category, package, outcome, _blockers, slot, _use_display, _required_by, _source in entries:
            if (category, package) != target_key:
                continue
            if outcome[0] == "new":
                version = outcome[1]
            elif outcome[0] in ("upgrade", "downgrade"):
                version = outcome[2]
            elif outcome[0] == "reinstall":
                version = outcome[1]
            else:
                continue
            if slot is None:
                continue
            if not any(v == version and s == slot for v, s, _ss in candidates):
                candidates.append((version, slot, slot))
        candidate_strs = [
            f"{pb['target_category']}/{pb['target_package']}-{v}:{s}/{ss}"
            for v, s, ss in candidates
        ]
        matched = match_from_list(pb["atom_str"], candidate_strs)
        by_str = dict(zip(candidate_strs, candidates))
        for m in matched:
            matched_version, _matched_slot, _matched_sub_slot = by_str[m]
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


def _enqueue_flat_deps(flat_deps, key, version, depth, queue, pending_blockers):
    """Queues every atom in `flat_deps` (a use_reduce(flat=True) result,
    with or without `subset`) onto `queue` at `depth + 1`, owned by
    `key`/`version`, splitting off a blocker atom into `pending_blockers`
    instead -- shared by resolve_pretend_graph's own normal-deps queueing
    and its --with-test-deps follow-up, so the two can't drift apart on
    blocker handling or depth/owner bookkeeping. Mirrors
    portage-repo/src/lib.rs's enqueue_flat_deps exactly."""
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
                    "owner_key": key,
                    "owner_version": version,
                }
            )
            continue
        queue.append((tok, depth + 1, key))


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
    changed_slot=False,
    with_test_deps=False,
    changed_deps_report=False,
    selective=False,
    autounmask_suggest_keywords=False,
    usepkg=False,
    usepkgonly=False,
    binpkg_respect_use=False,
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
    going, using whichever version was resolved first.

    `selective` (see resolve_pretend's own docstring for the full real
    selective/is_top_level grounding) is threaded uniformly to every
    atom this BFS resolves, but its own effect only ever reaches
    resolve_pretend for a top-level one: is_top_level
    (resolve_pretend's own parameter) is this BFS's own pre-existing
    `depth == 0`, passed at the one call site below -- the same
    equivalence --with-test-deps already established between real
    "argument" and this pilot's own `depth == 0`.
    Mirrors portage-repo/src/lib.rs's resolve_pretend_graph exactly."""
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
    # REQUIRED_USE (see the check further below, in the main BFS loop):
    # real depgraph.py's own _add_pkg sets
    # _dynamic_config._required_use_unsatisfied = True and returns 0 on
    # a violation -- it does NOT abort the whole graph walk, unlike a
    # top-level atom's own NoVisibleCandidate. Every violation
    # encountered anywhere in the walk is collected here and the BFS
    # keeps going; the whole call only fails at the very end, once every
    # reachable candidate has had a chance to resolve (or fail) on its
    # own terms -- matching real portage's own _unsatisfied_deps_for_
    # display list (checked once, at the very end of the real resolve)
    # rather than this pilot's own previous "abort on the first hit"
    # shortcut. Mirrors portage-repo/src/lib.rs's own
    # required_use_violations exactly.
    required_use_violations = []
    slot_conflicts = []
    # --changed-deps-report: real _changed_deps_pkgs is a dict keyed by
    # the installed Package object, so a repeat visit to the same
    # installed category/package/version (e.g. via both a bare
    # "dev-libs/foo" and an explicit "dev-libs/foo:0" atom text, or a
    # diamond dependency) naturally collapses to one entry -- mirrored
    # here with an explicit dedup set, keyed the same way, preserving
    # first-encountered order (real dict iteration order) rather than
    # sorting.
    changed_deps_report_seen = set()
    changed_deps_report_entries = []
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
            changed_slot,
            selective,
            depth == 0,
            usepkg,
            usepkgonly,
            binpkg_respect_use,
        )

        # --changed-deps-report: real portage stays "completely silent"
        # whenever --changed-deps itself is also given (its own
        # collected _changed_deps_pkgs dict is discarded unread by
        # _changed_deps_report's own early return in that case) -- so,
        # rather than collecting anything now and discarding it at print
        # time, this simply never bothers computing deps_changed at all
        # when changed_deps is true, an equivalent, simpler
        # no-op-preserving shortcut. Only already_installed/reinstall
        # outcomes name a version that's genuinely installed right now
        # (the only case _deps_changed -- a vdb-vs-current-ebuild
        # comparison for one specific version -- is meaningful for); a
        # reinstall here can only be for newuse/changed_use/changed_slot
        # (never for changed_deps itself, since that's false in this
        # branch), so this still fires independently of those other
        # reasons, matching real portage's own freely-combinable
        # reinstall triggers.
        if changed_deps_report and not changed_deps:
            installed_version = None
            if outcome[0] in ("already_installed", "reinstall"):
                installed_version = outcome[1]
            if installed_version is not None:
                dedup_key = (category, package, installed_version)
                if dedup_key not in changed_deps_report_seen:
                    changed_deps_report_seen.add(dedup_key)
                    if _deps_changed(
                        root, repos, category, package, installed_version, with_bdeps
                    ):
                        repo_candidates = [
                            c
                            for c in list_candidates(repos, category, package)
                            if c["version"] == installed_version
                        ]
                        if repo_candidates:
                            resolved = max(repo_candidates, key=lambda c: c["repo_priority"])
                            changed_deps_report_entries.append(
                                {
                                    "category": category,
                                    "package": package,
                                    "version": installed_version,
                                    "repo_name": resolved["repo_name"],
                                }
                            )

        # A top-level atom (as opposed to a dependency reached while
        # recursing) with no visible candidate aborts the whole call --
        # matching real portage's own depgraph.py behavior for an
        # unsatisfiable target, not the "report and keep going" treatment
        # a dependency's own NoVisibleCandidate gets a few lines down.
        if current_atom_str in top_level and outcome[0] == "no_visible_candidate":
            message = f'there are no ebuilds to satisfy "{current_atom_str}".'
            # --autounmask's own keyword-suggestion sub-feature (see
            # this function's own docstring for the full on/off
            # default-resolution logic): only even attempted when
            # enabled, and only ever finds something to suggest when a
            # real candidate exists that's masked by KEYWORDS alone
            # (see _keyword_masked_only's own docstring) -- a candidate
            # masked by package.mask/license/etc. too gets no
            # suggestion here, matching real portage's own "only
            # suggest a change that would actually fix it" spirit.
            if autounmask_suggest_keywords:
                keyword_masked = [
                    c
                    for c in list_candidates(repos, category, package)
                    if _keyword_masked_only(c, category, package, config)
                    and _suggested_keyword(c) is not None
                ]
                if keyword_masked:
                    candidate = _best_candidate(keyword_masked)
                    keyword = _suggested_keyword(candidate)
                    message += (
                        f'\nnote: {category}/{package}-{candidate["version"]} exists but is '
                        f'masked by KEYWORDS; --autounmask-keep-keywords=n suggests adding '
                        f'"{category}/{package} {keyword}" to package.accept_keywords'
                    )
            raise ResolutionError(message)

        if outcome[0] == "new":
            version = outcome[1]
        elif outcome[0] in ("upgrade", "downgrade"):
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
            entries.append((category, package, outcome, [], None, [], [], "ebuild"))
            continue

        # The resolved version may have come from any of `repos`, or
        # from PKGDIR (--usepkg/--usepkgonly), so re-derive which one it
        # actually lives in -- reusing list_candidates/
        # list_binary_candidates rather than threading a repo location
        # back out of resolve_pretend's outcome tuple, since more than
        # one source could in principle carry the identical version. The
        # ordinary repo_priority tie-break already does the right thing
        # with no special-casing: a binary candidate's own repo_priority
        # (list_binary_candidates) is deliberately float("-inf"), lower
        # than any real repo, so an identical-version ebuild naturally
        # wins the tie. Mirrors portage-repo/src/lib.rs exactly.
        repo_candidates = [] if usepkgonly else list_candidates(repos, category, package)
        if usepkg or usepkgonly:
            repo_candidates = repo_candidates + list_binary_candidates(
                config["pkgdir"], category, package
            )
        repo_candidates = [c for c in repo_candidates if c["version"] == version]
        if not repo_candidates:
            continue
        resolved = max(repo_candidates, key=lambda c: c["repo_priority"])
        slot = resolved["slot"]
        sub_slot = resolved["sub_slot"]
        repo_location = resolved["repo_location"]
        repo_name = resolved["repo_name"]
        candidate_source = resolved["source"]

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
        entries.append((category, package, outcome, [], slot, [], [], candidate_source))

        pf = f"{package}-{version}"
        if candidate_source == "binary":
            metadata = read_binary_metadata(config["pkgdir"], category, package, version)
            if metadata is None:
                continue
        else:
            try:
                metadata = read_md5_cache(repo_location, category, pf)
            except OSError:
                continue
        candidate_str = f"{category}/{package}-{version}:{slot}/{sub_slot}::{repo_name}"
        use_flags = effective_use_flags(
            metadata.get("IUSE", ""),
            config["use_tokens"],
            config["package_use"],
            config["package_use_force"],
            config["package_use_mask"],
            config["use_force"],
            config["use_mask"],
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
        # visibility at all). A violation eventually fails the whole run
        # regardless of whether this candidate was reached as a
        # top-level atom or a dependency deep in the graph -- but NOT
        # immediately: real depgraph.py's own _add_pkg (~line 3600) sets
        # _dynamic_config._required_use_unsatisfied = True and returns 0
        # on a violation, which does NOT stop the rest of the graph walk
        # (unlike a top-level atom's own NoVisibleCandidate, which
        # genuinely does abort immediately). Every violation anywhere in
        # the walk is collected into required_use_violations and the BFS
        # keeps going -- see that variable's own doc comment, near the
        # top of this function, for the full grounding and where the
        # collected violations actually get turned into this call's own
        # ResolutionError. A genuinely *invalid* REQUIRED_USE (the
        # "except InvalidDependString" branch below) is different: real
        # check_required_use itself raises for that case, outside the
        # explicit "if not required_use_is_sat:" branch the delayed
        # collection above lives in -- so this pilot keeps that one
        # immediately fatal, same as before. Calls the real
        # portage.dep.check_required_use directly (pinned to eapi="8",
        # same reasoning as required_use_harness.py's own docstring) --
        # mirrors portage-repo/src/lib.rs's own ported algorithm
        # (portage_required_use::check_required_use) exactly, verified
        # to agree via the shared required-use-harness contract suite.
        required_use = metadata.get("REQUIRED_USE", "").strip()
        if required_use:
            # Real check_required_use validates a referenced flag against
            # pkg.iuse.is_valid_flag, not a package's own literal IUSE
            # alone -- real config.py's own _get_implicit_iuse() folds
            # PORTAGE_ARCHLIST (profiles/arch.list), use.mask ∪
            # use.force, and literal "build"/"bootstrap" into every
            # package's effective IUSE regardless of what that package's
            # own IUSE declares. Mirrors portage-repo/src/lib.rs's own
            # resolve_pretend_graph exactly -- see portage_profile::
            # Config::archlist's own doc comment for the full grounding.
            iuse_set = {tok.lstrip("+-") for tok in metadata.get("IUSE", "").split()}
            iuse_set |= config["archlist"]
            iuse_set |= config["use_mask"]
            iuse_set |= config["use_force"]
            iuse_set |= {"build", "bootstrap"}
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
                required_use_violations.append(
                    f'REQUIRED_USE not satisfied for {category}/{package}-{version}: '
                    f'"{normalized}"'
                )
                continue

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
            entries[entry_idx] = (category, package, outcome, [], slot, display, [], candidate_source)

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
            flat_deps = _use_reduce_flat_disjunctive(
                depstr,
                use_flags,
                lambda atoms: all(
                    _atom_currently_satisfiable(repos, a, config) for a in atoms
                ),
            )
        except InvalidDependString:
            continue
        _enqueue_flat_deps(flat_deps, key, version, depth, queue, pending_blockers)

        # --with-test-deps: additive on top of the normal deps just
        # queued above, never a replacement for them -- see this
        # function's own docstring for the full gating (depth == 0,
        # "test" a valid, not-already-enabled, not-masked IUSE flag).
        if with_test_deps and depth == 0 and "test" not in use_flags:
            iuse_flags = {
                tok.lstrip("+-") for tok in metadata.get("IUSE", "").split()
            }
            test_masked = "test" in config["use_mask"] or "test" in _specificity_ordered_flags(
                config["package_use_mask"], candidate_str, category, package
            )
            if "test" in iuse_flags and not test_masked:
                try:
                    test_deps = use_reduce(
                        depstr, flat=True, uselist=use_flags | {"test"}, subset={"test"}
                    )
                except InvalidDependString:
                    test_deps = []
                _enqueue_flat_deps(test_deps, key, version, depth, queue, pending_blockers)

    # Merge required_by_map into entries in a single post-pass, mirroring
    # portage-repo/src/lib.rs's own identical final loop (run before
    # resolve_blockers below, same order) -- entries are tuples
    # (immutable), so this rebuilds each one rather than mutating in
    # place.
    entries = [
        (category, package, outcome, blockers, slot, use_display, sorted(required_by_map.get((category, package), ())), source)
        for category, package, outcome, blockers, slot, use_display, _required_by, source in entries
    ]

    # setdefault (not a dict comprehension) so the *first* entry for a
    # given owner wins when the same category/package appears more than
    # once (multiple slots) -- mirrors portage-repo/src/lib.rs's
    # `entries.iter_mut().find(...)`, which also attaches to the first
    # match.
    blockers_by_owner = {}
    for category, package, _o, blockers, _slot, _use_display, _required_by, _source in entries:
        blockers_by_owner.setdefault((category, package), blockers)
    for owner_key, conflict in resolve_blockers(root, pending_blockers, entries):
        blockers_by_owner[owner_key].append(conflict)

    if required_use_violations:
        raise ResolutionError("\n".join(required_use_violations))

    return {
        "entries": entries,
        "slot_conflicts": slot_conflicts,
        "changed_deps_report": changed_deps_report_entries,
    }


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
    sub_slot = resolved["sub_slot"]
    repo_location = resolved["repo_location"]
    repo_name = resolved["repo_name"]

    pf = f"{package}-{version}"
    try:
        metadata = read_md5_cache(repo_location, category, pf)
    except OSError:
        return
    candidate_str = f"{category}/{package}-{version}:{slot}/{sub_slot}::{repo_name}"
    use_flags = effective_use_flags(
        metadata.get("IUSE", ""),
        config["use_tokens"],
        config["package_use"],
        config["package_use_force"],
        config["package_use_mask"],
        config["use_force"],
        config["use_mask"],
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
        flat_deps = _use_reduce_flat_disjunctive(
            depstr,
            use_flags,
            lambda atoms: all(_atom_currently_satisfiable(repos, a, config) for a in atoms),
        )
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
# --with-bdeps-auto, --changed-deps, --changed-deps-report, --changed-slot,
# --with-test-deps, --noreplace/-n, --selective, and --help/-h are
# actually implemented (see run()
# below); every table
# here exists purely for recognition, not behavior.
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
    ("--sync-submodule", None),
    ("--sysroot", None),
    ("--use-ebuild-visibility", None),
    ("--useoldpkg-atoms", None),
    ("--usepkg", "-k"),
    ("--usepkgonly", "-K"),
    ("--usepkg-exclude-live", None),
    ("--verbose-missing-ebuilds", None),
    ("--verbose-slot-rebuilds", None),
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
    elif tag in ("upgrade", "downgrade"):
        fields.append(f'"version":{_json_string(outcome[2])}')
        fields.append(f'"from_version":{_json_string(outcome[1])}')
    elif tag == "reinstall":
        fields.append(f'"version":{_json_string(outcome[1])}')
        changed_use = ",".join(_json_string(f) for f in outcome[2])
        fields.append(f'"changed_use":[{changed_use}]')
        fields.append(f'"changed_deps":{_json_bool(outcome[3])}')
        fields.append(f'"changed_slot":{_json_bool(outcome[4])}')
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


def _changed_deps_report_entry_to_json(c):
    return (
        f'{{"category":{_json_string(c["category"])},"package":{_json_string(c["package"])},'
        f'"version":{_json_string(c["version"])},"repo_name":{_json_string(c["repo_name"])}}}'
    )


def _print_json(entries, slot_conflicts, changed_deps_report, top_level_pkgs, verbose):
    """The whole --json output: {"entries": [...], "slot_conflicts": [...],
    "changed_deps_report": [...]}, one line, no pretty-printing (a
    pilot-specific convenience format, not a stable schema -- see run()'s
    own --json handling). Mirrors pretend.rs's own print_json exactly."""
    entries_json = ",".join(
        _entry_to_json(category, package, outcome, blockers, slot, use_display, required_by, top_level_pkgs, verbose)
        for category, package, outcome, blockers, slot, use_display, required_by, source in entries
    )
    conflicts_json = ",".join(_slot_conflict_to_json(c) for c in slot_conflicts)
    changed_deps_report_json = ",".join(
        _changed_deps_report_entry_to_json(c) for c in changed_deps_report
    )
    print(
        f'{{"entries":[{entries_json}],"slot_conflicts":[{conflicts_json}],'
        f'"changed_deps_report":[{changed_deps_report_json}]}}'
    )


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
            "--deselect/-W, --with-bdeps, --with-bdeps-auto, --changed-deps, "
            "--changed-deps-report, --changed-slot, --with-test-deps, "
            "--noreplace/-n, --selective, and --help/-h are implemented so "
            "far; see PROMPT.md)",
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
        '       --with-bdeps-auto y|n  changes the *default* --with-bdeps value (only when --with-bdeps itself isn\'t given) -- n makes it default to n instead of the real "auto" (y here)'
    )
    print(
        "       --changed-deps[=y|n]  reinstall an already-installed package whose own vdb-recorded dependencies differ from the current ebuild's"
    )
    print(
        "       --changed-deps-report[=y|n]  report (without reinstalling) an already-installed package whose own vdb-recorded dependencies differ from the current ebuild's; silent if --changed-deps is also given"
    )
    print(
        "       --changed-slot[=y|n]  reinstall an already-installed package whose own vdb-recorded SLOT differs from the current ebuild's"
    )
    print(
        "       --with-test-deps[=y|n]  also pull in a top-level atom's own test?-gated dependencies, if it has a \"test\" USE flag not already enabled"
    )
    print(
        "   -n, --noreplace  a directly-named, already-installed, still-satisfying atom is left as-is (real portage's own default without this needs --update/--newuse/--changed-use/--changed-deps/--changed-slot/--selective to get the same result)"
    )
    print(
        "       --selective[=y|n]  identical to --noreplace; \"n\" explicitly cancels it even if another flag above would otherwise set it"
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

    Only plain atom lines are read, via a leading "@" check -- this is
    real, not a simplification: real WorldSelectedPackagesSet's own
    ItemFileLoader validates each line with a plain isvalidatom
    (lib/portage/env/validators.py's own ValidAtomValidator, no "@"
    bypass), so a "@"-prefixed line in *this* file specifically really
    would just fail validation and be dropped in real portage too. A
    nested "@some-set" reference lives in a genuinely separate file --
    see _read_world_sets's own docstring for the other half of real
    @world's union (WorldSelectedSet in lib/portage/_sets/files.py).
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


def _read_world_sets(root):
    """Reads <root>/var/lib/portage/world_sets (real portage's own
    WORLD_SETS_FILE -- lib/portage/const.py), a file genuinely SEPARATE
    from the world file above, listing every "@name" set reference the
    user has directly selected (e.g. via a prior "emerge --noreplace
    @some-set") -- real WorldSelectedSetsSet, whose own validator
    (lib/portage/_sets/files.py) just checks each line starts with "@".
    Real @world is the union of WorldSelectedSetsSet (this) with
    WorldSelectedPackagesSet (_read_world_atoms above) -- see
    WorldSelectedSet.load's own "chain(self._pkgset, self._setset)". A
    missing file is not an error, same "absence is a real, valid state"
    precedent the world file itself already established. Returns each
    name with its own leading "@" stripped, ready for
    _resolve_custom_set. Mirrors pretend.rs's read_world_sets exactly."""
    path = os.path.join(root, "var", "lib", "portage", "world_sets")
    try:
        with open(path) as f:
            text = f.read()
    except FileNotFoundError:
        return []
    return [
        line[1:]
        for line in (raw.strip() for raw in text.splitlines())
        if line and not line.startswith("#") and line.startswith("@")
    ]


def _resolve_custom_set(config_root, name, seen):
    """Resolves one custom, file-based package set by `name` (no leading
    "@"), real portage's own default "usersets" source
    (lib/portage/_sets/__init__.py's own _create_default_config: class =
    StaticFileSet, directory = <config_root>/etc/portage/sets, one file
    per set, the file's own path relative to that directory becoming
    the set's name) -- reads <config_root>/etc/portage/sets/<name>, same
    line format as the world file itself (one atom per line,
    "#"-comment/blank-line handling identical), *except* a line starting
    with "@" is itself another nested set reference here, resolved
    recursively -- real StaticFileSet's own validator (unlike
    WorldSelectedPackagesSet's stricter one) explicitly accepts a
    "@"-prefixed line too, and real SetConfig.getSetAtoms walks every
    such non-atom entry, recursing into any that start with "@"
    (lib/portage/_sets/__init__.py). `seen` is that same recursion's own
    "ignorelist" -- a name already being expanded on the current path
    contributes nothing further (silently, not an error) rather than
    looping forever; a *fresh* `seen` set is used for each top-level
    name in _read_world_sets's own list, matching real
    getSetAtoms(setname, ignorelist=None)'s own per-top-level-call
    default.

    A `name` with no matching file raises ResolutionError (real
    PackageSetNotFound, eventually surfaced and fatal at every real call
    site in lib/_emerge/actions.py/depgraph.py) -- deliberately NOT the
    same "absence is valid" tolerance _read_world_atoms/_read_world_sets
    give their own *files*: those are optional, implicitly-checked-for
    state (a fresh ROOT may simply never have either), but a name
    explicitly listed in world_sets (or referenced by another set)
    pointing at nothing is a real configuration error, not an absence to
    tolerate. Mirrors pretend.rs's resolve_custom_set exactly."""
    if name in seen:
        return []
    seen.add(name)
    path = os.path.join(config_root, "etc", "portage", "sets", name)
    try:
        with open(path) as f:
            text = f.read()
    except OSError:
        raise ResolutionError(f"set {name!r} not found")
    atoms = []
    for line in (raw.strip() for raw in text.splitlines()):
        if not line or line.startswith("#"):
            continue
        if line.startswith("@"):
            atoms.extend(_resolve_custom_set(config_root, line[1:], seen))
        else:
            atoms.append(line)
    return atoms


def _run_deselect(targets, root):
    """Ports real action_deselect (lib/_emerge/actions.py, lines
    1740-1835) exactly: needs no repo/config resolution at all, only the
    world file and the vdb.

    A bare package name (no "/") is expanded via real portage's own
    "null category" mechanism -- scan the world file for a same-named
    atom and substitute in its category -- added to the candidate set
    *unconditionally*, no installed check at all: real action_deselect
    adds the substituted atom to expanded_atoms before ever touching the
    vardb. Real action_deselect does separately call vardb.match(atom)
    for this same original (still-null-category) atom, but that call can
    never match a real vardb entry (no package is ever catalogued under
    category "null"), so it's dead code for this branch and correctly
    contributes nothing here.

    An *explicit*-category target (already has a "/") is likewise added
    directly, with no installed check at all -- confirmed by reading
    real portage's own call chain feeding action_deselect's own atoms
    parameter: action_uninstall's own dep_expand(x, mydb=vardb, ...)
    (lib/portage/dbapi/dep_expand.py) returns an explicit-category atom
    completely unchanged, "if mydep.category != 'virtual': return
    mydep", before it ever reaches cpv_expand (the vardb-dependent part,
    only reached for a bare name); action_deselect itself then seeds
    expanded_atoms = set(atoms) with that same atom, unconditionally. So
    "--deselect cat/pkg" (or a bare "pkg" resolvable via the world file)
    genuinely discards a matching world entry even if never installed --
    this pilot's own earlier doc comment (and test) claimed installation
    was always required, an incorrect generalization: real portage's own
    vardb-derived narrowing (vardb.match) is a *separate, additional*
    contribution on top of the unconditional substitution/literal-target
    candidate, for BOTH the bare-name and explicit-category cases -- not
    a gate on it. For an explicit-category target specifically, that
    separate vardb contribution (installed_candidates, via
    match_from_list) still runs and adds a further bare
    category/package:slot candidate (real Atom(f"{pkg.cp}:{pkg.slot}"),
    no version/operator at all) for whatever version(s) are actually
    installed; for a bare name it's correctly omitted, per the dead-code
    reasoning above.

    Every candidate atom collected this way is compared against every
    world-file entry via the real Atom.intersects() method directly
    (the same "why re-derive it" reasoning as _matches_config_entry
    above -- this is the oracle, so it uses real portage's own method,
    unlike pretend.rs's own hand-ported portage_dep::atom_intersects)
    plus real action_deselect's own separate repo check ("not
    (arg_atom.repo and not atom.repo)"). A "@"-prefixed world entry is
    never matched this way, consistent with _read_world_atoms's own
    pre-existing cut for @world itself.

    A "@name" target: real action_deselect's own combined world_set
    (WorldSelectedSet) iterates BOTH the world file's own plain atoms AND
    the world_sets file's own literal "@name" reference *strings* --
    confirmed by reading WorldSelectedSet.load's own
    "self._setAtoms(chain(self._pkgset, self._setset))": a "@name"
    string fails real Atom(...) parsing and lands in _nonatoms, so it's
    carried through *unexpanded*, never resolved into its own member
    atoms at all. action_deselect's own matching loop confirms this: a
    "@"-prefixed CLI target can only ever discard a "@"-prefixed
    world_set entry via *exact string equality* -- there is no
    installed-candidate matching, no member-atom expansion, for either
    side. So despite _resolve_custom_set's own real, working nested-set
    expansion (built for -- and still only used by -- @world's own
    dependency-resolution walk, a genuinely different real mechanism),
    it has no role here at all: this pilot's own equivalent is a plain
    membership check against _read_world_sets, nothing more. Each
    discarded entry is reported against its own real source file
    ("world" for a plain atom, "world_sets" for a "@name" reference),
    sorted together into one combined list, not two separate blocks.
    Mirrors pretend.rs's run_deselect exactly."""
    world_atoms = _read_world_atoms(root)
    world_sets = _read_world_sets(root)

    expanded = []
    set_targets = set()
    for target in targets:
        if target.startswith("@"):
            set_targets.add(target[1:])
            continue
        if "/" in target:
            atom = _parse_atom(target)
            if atom is None:
                print(f"emerge: invalid atom {target!r}", file=sys.stderr)
                return 1
            expanded.append(atom)
            category, package = atom.cp.split("/", 1)
            for version, slot, _sub_slot in installed_candidates(root, category, package):
                candidate_str = f"{category}/{package}-{version}:{slot}"
                if match_from_list(target, [candidate_str]):
                    vardb_atom = _parse_atom(f"{category}/{package}:{slot}")
                    if vardb_atom is not None:
                        expanded.append(vardb_atom)
        else:
            for w in world_atoms:
                a = _parse_atom(w)
                if a is not None and a.cp.split("/", 1)[1] == target:
                    category = a.cp.split("/", 1)[0]
                    substituted = _parse_atom(f"{category}/{target}")
                    if substituted is not None:
                        expanded.append(substituted)

    discard = []
    for world_atom_str in world_atoms:
        w = _parse_atom(world_atom_str)
        if w is None:
            continue
        for arg_atom in expanded:
            if arg_atom.intersects(w) and not (arg_atom.repo and not w.repo):
                discard.append((world_atom_str, "world"))
                break
    for name in world_sets:
        if name in set_targets:
            discard.append((f"@{name}", "world_sets"))

    if not discard:
        print('>>> No matching atoms found in "world" favorites file...')
    else:
        for entry, filename in sorted(discard):
            print(f'>>> Would remove {entry} from "{filename}" favorites file...')
    return 0


def _reinstall_reason(changed_flags, deps_changed, slot_changed):
    """The "(reinstall for ...)" note's own reason text, real portage
    treating --newuse/--changed-use, --changed-deps, and --changed-slot
    as independent, freely-combinable triggers. Pilot-invented wording,
    same as the pre-existing "changed USE: ..." text -- real portage's
    own default --pretend output shows no such itemized reason at all.
    Returns None when all three are empty/False -- real portage's own
    bare, reasonless "[ebuild R]" (see resolve_pretend's own selective/
    is_top_level docstring paragraph): unlike every other reinstall,
    this one genuinely has no tracked reason to report at all, so the
    caller omits the whole "(reinstall for ...)" parenthetical rather
    than printing an empty one. Mirrors pretend.rs's own
    reinstall_reason exactly."""
    reasons = []
    if changed_flags:
        reasons.append(f"changed USE: {', '.join(changed_flags)}")
    if deps_changed:
        reasons.append("changed dependencies")
    if slot_changed:
        reasons.append("changed slot")
    if not reasons:
        return None
    return "; ".join(reasons)


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
    with_bdeps_given = False
    with_bdeps_auto = True
    changed_deps = False
    changed_slot = False
    with_test_deps = False
    changed_deps_report = False
    # --autounmask/--autounmask-keep-keywords: None means "not explicitly
    # given" -- see the on/off default-resolution logic just below where
    # these are actually consumed, mirroring pretend.rs exactly.
    autounmask = None
    autounmask_keep_keywords = None
    usepkg = False
    usepkgonly = False
    binpkg_respect_use = None
    noreplace = False
    # None until an explicit --selective/--selective=y/--selective=n is
    # given, so "n" can override whatever update/newuse/changed_use/
    # changed_deps/changed_slot/noreplace computed -- matching real
    # create_depgraph_params.py's own unconditional `if myopts.get(
    # "--selective") == "n": myparams.pop("selective", None)`, checked
    # after every other trigger. See selective's own computation just
    # before the resolve_pretend_graph call below.
    selective_flag = None

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
        elif arg in ("--noreplace", "-n"):
            # Real "--noreplace"/"-n": a plain boolean, no value at all
            # (real main.py's own boolean-options list) -- unlike
            # "--selective" below, which has the same name/meaning but a
            # real optional y_or_n value. Its entire real effect is
            # setting `selective` -- see resolve_pretend's own docstring.
            noreplace = True
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
                with_bdeps_given = True
                i += 2
            elif value == "n":
                with_bdeps = False
                with_bdeps_given = True
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
                with_bdeps_given = True
                i += 1
            elif value == "n":
                with_bdeps = False
                with_bdeps_given = True
                i += 1
            else:
                print(
                    f'emerge: option "--with-bdeps": invalid choice: "{value}" '
                    '(choose from "y", "n")',
                    file=sys.stderr,
                )
                return 2
        elif arg == "--with-bdeps-auto":
            # Real "--with-bdeps-auto": the identical required,
            # closed-choice ("y"/"n") shape --with-bdeps itself has.
            if i + 1 >= len(args):
                print(
                    'emerge: option "--with-bdeps-auto" requires an argument', file=sys.stderr
                )
                return 2
            value = args[i + 1]
            if value == "y":
                with_bdeps_auto = True
                i += 2
            elif value == "n":
                with_bdeps_auto = False
                i += 2
            else:
                print(
                    f'emerge: option "--with-bdeps-auto": invalid choice: "{value}" '
                    '(choose from "y", "n")',
                    file=sys.stderr,
                )
                return 2
        elif arg.startswith("--with-bdeps-auto="):
            value = arg[len("--with-bdeps-auto=") :]
            if value == "y":
                with_bdeps_auto = True
                i += 1
            elif value == "n":
                with_bdeps_auto = False
                i += 1
            else:
                print(
                    f'emerge: option "--with-bdeps-auto": invalid choice: "{value}" '
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
        elif arg == "--changed-deps-report":
            # Real "--changed-deps-report": y_or_n (default_arg_opts),
            # the identical optional-value shape "--changed-deps"
            # already has -- no short alias (real main.py declares
            # none). Unlike --changed-deps, this never changes what
            # gets reinstalled -- see resolve_pretend_graph's own
            # docstring.
            nxt = args[i + 1] if i + 1 < len(args) else None
            if nxt == "y":
                changed_deps_report = True
                i += 2
            elif nxt == "n":
                changed_deps_report = False
                i += 2
            else:
                changed_deps_report = True
                i += 1
        elif arg == "--changed-deps-report=y":
            changed_deps_report = True
            i += 1
        elif arg == "--changed-deps-report=n":
            changed_deps_report = False
            i += 1
        elif arg == "--selective":
            # Real "--selective": y_or_n (default_arg_opts), the same
            # optional-value shape "--changed-deps" already has -- no
            # short alias for this exact spelling (real main.py declares
            # none; "-n" is "--noreplace" above, real portage's own
            # separate, bare-boolean spelling of the identical meaning).
            # "n" here explicitly CANCELS selective even if some other
            # flag already set it -- see resolve_pretend's own docstring
            # and this override's own application just before the
            # resolve_pretend_graph call below.
            nxt = args[i + 1] if i + 1 < len(args) else None
            if nxt == "y":
                selective_flag = True
                i += 2
            elif nxt == "n":
                selective_flag = False
                i += 2
            else:
                selective_flag = True
                i += 1
        elif arg == "--selective=y":
            selective_flag = True
            i += 1
        elif arg == "--selective=n":
            selective_flag = False
            i += 1
        elif arg == "--changed-slot":
            # Real "--changed-slot": y_or_n (default_arg_opts), the
            # identical optional-value shape "--changed-deps" already
            # has -- no short alias (real main.py declares none).
            nxt = args[i + 1] if i + 1 < len(args) else None
            if nxt == "y":
                changed_slot = True
                i += 2
            elif nxt == "n":
                changed_slot = False
                i += 2
            else:
                changed_slot = True
                i += 1
        elif arg == "--changed-slot=y":
            changed_slot = True
            i += 1
        elif arg == "--changed-slot=n":
            changed_slot = False
            i += 1
        elif arg == "--with-test-deps":
            # Real "--with-test-deps": y_or_n (default_arg_opts), the
            # identical optional-value shape "--changed-deps"/
            # "--changed-slot" already have -- no short alias (real
            # main.py declares none).
            nxt = args[i + 1] if i + 1 < len(args) else None
            if nxt == "y":
                with_test_deps = True
                i += 2
            elif nxt == "n":
                with_test_deps = False
                i += 2
            else:
                with_test_deps = True
                i += 1
        elif arg == "--with-test-deps=y":
            with_test_deps = True
            i += 1
        elif arg == "--with-test-deps=n":
            with_test_deps = False
            i += 1
        elif arg == "--autounmask":
            # Real "--autounmask": choices=true_y_or_n ("True", "y",
            # "n") -- a bare flag means true, same optional-value shape
            # "--changed-slot"/"--with-test-deps" already have.
            nxt = args[i + 1] if i + 1 < len(args) else None
            if nxt == "y":
                autounmask = True
                i += 2
            elif nxt == "n":
                autounmask = False
                i += 2
            else:
                autounmask = True
                i += 1
        elif arg == "--autounmask=y":
            autounmask = True
            i += 1
        elif arg == "--autounmask=n":
            autounmask = False
            i += 1
        elif arg == "--autounmask-keep-keywords":
            # Real "--autounmask-keep-keywords": plain y_or_n, a
            # REQUIRED value -- no bare/optional form real "--autounmask"
            # itself has, the same required shape "--with-bdeps" has.
            if i + 1 >= len(args):
                print(
                    'emerge: option "--autounmask-keep-keywords" requires an argument',
                    file=sys.stderr,
                )
                return 2
            value = args[i + 1]
            if value == "y":
                autounmask_keep_keywords = True
                i += 2
            elif value == "n":
                autounmask_keep_keywords = False
                i += 2
            else:
                print(
                    f'emerge: option "--autounmask-keep-keywords": invalid choice: "{value}" '
                    '(choose from "y", "n")',
                    file=sys.stderr,
                )
                return 2
        elif arg.startswith("--autounmask-keep-keywords="):
            value = arg[len("--autounmask-keep-keywords=") :]
            if value == "y":
                autounmask_keep_keywords = True
                i += 1
            elif value == "n":
                autounmask_keep_keywords = False
                i += 1
            else:
                print(
                    f'emerge: option "--autounmask-keep-keywords": invalid choice: "{value}" '
                    '(choose from "y", "n")',
                    file=sys.stderr,
                )
                return 2
        elif arg == "--usepkg" or arg == "-k":
            nxt = args[i + 1] if i + 1 < len(args) else None
            if nxt == "y":
                usepkg = True
                i += 2
            elif nxt == "n":
                usepkg = False
                i += 2
            else:
                usepkg = True
                i += 1
        elif arg == "--usepkg=y":
            usepkg = True
            i += 1
        elif arg == "--usepkg=n":
            usepkg = False
            i += 1
        elif arg == "--usepkgonly" or arg == "-K":
            nxt = args[i + 1] if i + 1 < len(args) else None
            if nxt == "y":
                usepkgonly = True
                i += 2
            elif nxt == "n":
                usepkgonly = False
                i += 2
            else:
                usepkgonly = True
                i += 1
        elif arg == "--usepkgonly=y":
            usepkgonly = True
            i += 1
        elif arg == "--usepkgonly=n":
            usepkgonly = False
            i += 1
        elif arg == "--binpkg-respect-use":
            nxt = args[i + 1] if i + 1 < len(args) else None
            if nxt == "y":
                binpkg_respect_use = True
                i += 2
            elif nxt == "n":
                binpkg_respect_use = False
                i += 2
            else:
                binpkg_respect_use = True
                i += 1
        elif arg == "--binpkg-respect-use=y":
            binpkg_respect_use = True
            i += 1
        elif arg == "--binpkg-respect-use=n":
            binpkg_respect_use = False
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
                elif c == "n":
                    noreplace = True
                elif c == "D":
                    deep = True
                elif c == "k":
                    usepkg = True
                elif c == "K":
                    usepkgonly = True
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
        all_repos = find_repos(_config_root())
        main_repo = next(r for r in all_repos if r["is_main"])
        # Every non-main repo's own (name, location) -- resolve_config's
        # own package.mask/.unmask reading needs each overlay's own name
        # to scope its repo-level entries via "::name" (see its own
        # docstring); ascending-priority order, same as find_repos' own
        # order, which only matters if two overlays' own entries could
        # otherwise interfere, and the "::name" scoping already rules
        # that out regardless. The same list, plus the main repo's own
        # name below, also lets resolve_config follow a profile's own
        # cross-repo "parent" entries (reponame:path syntax).
        overlay_repos = [(r["name"], r["location"]) for r in all_repos if not r["is_main"]]
        config = resolve_config(
            _config_root(), main_repo["location"], overlay_repos, main_repo["name"]
        )
    except ResolutionError as e:
        print(f"emerge: {e}", file=sys.stderr)
        return 1

    # "@world"/"@system" each expand to their own real atom list, in
    # place, at whichever position they appear -- see _read_world_atoms's
    # own docstring for the world file's own scope, _read_world_sets's
    # for the world_sets file's own nested-@set half of real @world's
    # union, and resolve_config's own docstring for @system's. Only
    # these two literal tokens trigger expansion -- any other
    # "@"-prefixed token falls through to the ordinary atom-parsing path
    # below and gets a clear "invalid atom" error, not a silent no-op.
    try:
        expanded_atoms = []
        for atom_arg in atom_args:
            if atom_arg == "@world":
                expanded_atoms.extend(_read_world_atoms(_root()))
                for set_name in _read_world_sets(_root()):
                    expanded_atoms.extend(
                        _resolve_custom_set(_config_root(), set_name, set())
                    )
            elif atom_arg == "@system":
                expanded_atoms.extend(config["system_packages"])
            else:
                expanded_atoms.append(atom_arg)
    except ResolutionError as e:
        print(f"emerge: {e}", file=sys.stderr)
        return 1
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

    # Real create_depgraph_params.py's own precedence: an explicit
    # --with-bdeps always wins; only when it's absent does
    # --with-bdeps-auto=n override the real default ("auto", this
    # pilot's own pre-existing with_bdeps=True) down to "n" instead.
    if not with_bdeps_given:
        with_bdeps = with_bdeps_auto

    # Real create_depgraph_params.py's own `selective` condition,
    # computed from whichever of its real trigger flags this pilot
    # implements -- see resolve_pretend's own docstring for the full
    # grounding, including why --changed-use alone covers this pilot's
    # whole share of real --reinstall's own contribution. An explicit
    # --selective=n unconditionally cancels it regardless of what the
    # other flags computed, matching real create_depgraph_params.py's
    # own unconditional `if myopts.get("--selective") == "n": pop`,
    # checked last, after every other trigger.
    if selective_flag is None:
        selective = update or newuse or changed_use or changed_deps or changed_slot or noreplace
    else:
        selective = selective_flag

    # --autounmask/--autounmask-keep-keywords: real create_depgraph_
    # params.py's own default-resolution logic, simplified for this
    # pilot's own v1 scope (only the keyword-suggestion sub-feature is
    # implemented at all -- --autounmask-use/-license/-masks aren't
    # read here, matching every real fixture/user who also never
    # touches them getting the exact same outcome this simplification
    # produces). Real logic: autounmask itself defaults to enabled
    # (only --autounmask=n turns the whole feature off). autounmask_
    # keep_keywords (real: "suppress keyword suggestions") defaults to
    # suppressed (True) when --autounmask itself was NOT explicitly
    # given at all, but defaults to *not* suppressed (False, i.e.
    # keyword suggestions ARE generated) once --autounmask itself WAS
    # explicitly given (any value) -- real portage's own "explicitly
    # asking for autounmask implies wanting its keyword suggestions
    # too, but the ambient always-on default doesn't" asymmetry, ported
    # exactly. Mirrors pretend.rs exactly.
    autounmask_enabled = autounmask is not False
    if autounmask_keep_keywords is not None:
        autounmask_suggest_keywords = autounmask_enabled and not autounmask_keep_keywords
    else:
        autounmask_suggest_keywords = autounmask_enabled and autounmask is not None

    # --binpkg-respect-use: real default is "auto" (effectively on)
    # whenever --usepkgonly is NOT given, left off (unset/falsy) when it
    # IS -- create_depgraph_params.py:47-55. An explicit
    # --binpkg-respect-use=y/=n always wins outright either way.
    resolved_binpkg_respect_use = (
        binpkg_respect_use if binpkg_respect_use is not None else not usepkgonly
    )

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
            changed_slot,
            with_test_deps,
            changed_deps_report,
            selective,
            autounmask_suggest_keywords,
            usepkg,
            usepkgonly,
            resolved_binpkg_respect_use,
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
        _print_json(
            entries,
            result["slot_conflicts"],
            result["changed_deps_report"],
            top_level_pkgs,
            verbose,
        )
        return 0

    for category, package, outcome, blockers, _slot, use_display, _required_by, source in entries:
        tag = outcome[0]
        # --onlydeps (man/emerge.1: "Only merge (or pretend to merge) the
        # dependencies of the packages specified, not the packages
        # themselves"): a directly-requested (top-level) atom's own line
        # is suppressed -- whatever its outcome -- while its dependencies
        # (reached the same as always, since resolve_pretend_graph's own
        # recursion is entirely unaffected by this flag) print normally.
        onlydeps_suppressed = onlydeps and (category, package) in top_level_pkgs
        # Real --pretend's own bracket word: literally pkg.type_name
        # ("ebuild"/"binary") -- a binary merge prints "[binary", never
        # "[ebuild", regardless of outcome. Mirrors pretend.rs exactly.
        bracket = "binary" if source == "binary" else "ebuild"
        if tag == "new":
            if not onlydeps_suppressed:
                print(f"[{bracket}  N] {category}/{package}-{outcome[1]}{use_suffix(use_display)}")
            print_blockers(category, package, outcome[1], blockers)
        elif tag == "upgrade":
            if not onlydeps_suppressed:
                print(
                    f"[{bracket}  U] {category}/{package}-{outcome[2]} (upgrade from {outcome[1]})"
                    f"{use_suffix(use_display)}"
                )
            print_blockers(category, package, outcome[2], blockers)
        elif tag == "downgrade":
            if not onlydeps_suppressed:
                print(
                    f"[{bracket}  D] {category}/{package}-{outcome[2]} (downgrade from {outcome[1]})"
                    f"{use_suffix(use_display)}"
                )
            print_blockers(category, package, outcome[2], blockers)
        elif tag == "reinstall":
            changed_flags = outcome[2]
            deps_changed_flag = outcome[3]
            slot_changed_flag = outcome[4]
            if not onlydeps_suppressed:
                reason = _reinstall_reason(changed_flags, deps_changed_flag, slot_changed_flag)
                if reason is None:
                    print(f"[{bracket}  r] {category}/{package}-{outcome[1]}{use_suffix(use_display)}")
                else:
                    print(
                        f"[{bracket}  r] {category}/{package}-{outcome[1]} "
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

    # --changed-deps-report: real _changed_deps_report's own WARN block,
    # ported verbatim (real portage colorizes it when the terminal
    # supports it; this pilot, like every other message it prints, stays
    # plain text). Already empty unless changed_deps_report was given
    # AND changed_deps was NOT (see resolve_pretend_graph's own
    # docstring for that gating), so no extra condition needed here
    # beyond "is there anything to report at all".
    if result["changed_deps_report"]:
        root = _root()
        print(file=sys.stderr)
        print("!!! Detected ebuild dependency change(s) without revision bump:", file=sys.stderr)
        print(file=sys.stderr)
        for c in result["changed_deps_report"]:
            if root == "/":
                print(f"    {c['category']}/{c['package']}-{c['version']}::{c['repo_name']}", file=sys.stderr)
            else:
                print(
                    f"    {c['category']}/{c['package']}-{c['version']}::{c['repo_name']} for {root}",
                    file=sys.stderr,
                )
        print(file=sys.stderr)
        print("NOTE: Refer to the following page for more information about dependency", file=sys.stderr)
        print("      change(s) without revision bump:", file=sys.stderr)
        print(file=sys.stderr)
        print("          https://wiki.gentoo.org/wiki/Project:Portage/Changed_dependencies", file=sys.stderr)
        print(file=sys.stderr)
        print("      In order to suppress reports about dependency changes, add", file=sys.stderr)
        print("      --changed-deps-report=n to the EMERGE_DEFAULT_OPTS variable in", file=sys.stderr)
        print("      '/etc/portage/make.conf'.", file=sys.stderr)
        print(file=sys.stderr)
        print("HINT: In order to avoid problems involving changed dependencies, use the", file=sys.stderr)
        print("      --changed-deps option to automatically trigger rebuilds when changed", file=sys.stderr)
        print("      dependencies are detected. Refer to the emerge man page for more", file=sys.stderr)
        print("      information about this option.", file=sys.stderr)

    return 0


if __name__ == "__main__":
    sys.exit(run(sys.argv[1:]))
