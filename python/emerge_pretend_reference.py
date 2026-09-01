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
the `repo`/`pkginternal`/`defaults`/`conf`/`pkg` USE_ORDER layers are
modeled (`env`/`features`/`env.d` are not), `masters` (layout.conf repo
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

Usage mirrors the real emerge CLI (and the Rust portuale's `emerge`
applet) directly:
    emerge_pretend_reference.py --pretend <category/package>

Config/target roots come from the real PORTAGE_CONFIGROOT/ROOT environment
variables, defaulting to "/" -- see lib/portage/const.py.
"""

import configparser
import functools
import os
import re
import sys
from collections import deque

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "..", "lib"))

from portage.dep import Atom, check_required_use, match_from_list, paren_enclose, use_reduce
from portage.dep._slot_operator import strip_slots
from portage.dep.libc import strip_libc_deps
from portage.exception import InvalidAtom, InvalidDependString
from portage.versions import ververify, vercmp


class ResolutionError(Exception):
    pass


def _config_root():
    return os.environ.get("PORTAGE_CONFIGROOT") or "/"


def _root():
    return os.environ.get("ROOT") or "/"


def _running_root():
    """--root-deps's own real running-root default: real ESYSROOT
    resolves to the real build machine's own "/" whenever SYSROOT is left
    unset -- see the Rust side's own running_root_satisfies_atom doc
    comment for the full grounding. PORTAGE_RUNNING_ROOT itself is NOT a
    real portage environment variable (real portage has no way to
    override this at all) -- a pilot-specific override purely so a test
    can point this at a fixture's own fake vdb tree instead of the real
    host, matching PORTAGE_CONFIGROOT/ROOT's own existing precedent.
    """
    return os.environ.get("PORTAGE_RUNNING_ROOT") or "/"


def _parse_layout_conf(repo_location):
    """Parses a repo's own metadata/layout.conf (real parse_layout_conf,
    lib/portage/repository/config.py:1516) -- a section-less key = value
    file. Empty dict when absent. This pilot reads exactly three keys:
    masters, repo-name, profile-formats. Mirrors portage-repo/src/lib.rs's
    parse_layout_conf exactly."""
    path = os.path.join(repo_location, "metadata", "layout.conf")
    out = {}
    try:
        with open(path) as f:
            text = f.read()
    except OSError:
        return out
    for raw in text.splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if "=" in line:
            key, _, value = line.partition("=")
            out[key.strip()] = value.strip()
    return out


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
        repos_conf_masters = parser.get(name, "masters", fallback=None)
        # Real name resolution: profiles/repo_name file first, else the
        # section name (real _read_repo_name, config.py:670-688).
        try:
            with open(os.path.join(location, "profiles", "repo_name")) as f:
                repo_name_file = f.readline().strip()
        except OSError:
            repo_name_file = ""
        repos.append(
            {
                "name": repo_name_file or name,
                "location": location,
                "priority": priority,
                "is_main": name == main_repo,
                "_section_name": name,
                # None = repos.conf key absent (fall through to layout.conf
                # tier); a list (possibly empty) = explicit.
                "_repos_conf_masters": (
                    None if repos_conf_masters is None else repos_conf_masters.split()
                ),
                "_repos_conf_aliases": parser.get(name, "aliases", fallback="").split(),
            }
        )

    # Real layout.conf (lib/portage/repository/config.py): repo-name
    # overrides the name (config.py:500-505); aliases are prepended
    # before the repos.conf ones (config.py:492-499); profile-formats
    # feeds the colon-parent gate; layout.conf masters is the middle
    # tier. Mirrors portage-repo/src/lib.rs.
    for repo in repos:
        layout = _parse_layout_conf(repo["location"])
        repo["profile_formats"] = layout.get("profile-formats", "").split()
        repo["aliases"] = (
            layout.get("aliases", "").split() + repo["_repos_conf_aliases"]
        )
        new_name = layout.get("repo-name", "").strip()
        if new_name:
            repo["name"] = new_name
        repo["_layout_masters"] = (
            None if "masters" not in layout else layout["masters"].split()
        )

    # Real config.py:1121-1136: a repo whose resolved name differs from
    # its repos.conf [section] name is dropped with an error -- unless
    # the section name is one of its aliases. Ported faithfully, drop
    # included (not a soft warning).
    kept = []
    for repo in repos:
        if repo["name"] != repo["_section_name"] and repo["_section_name"] not in repo["aliases"]:
            print(
                f"!!! Section '{repo['_section_name']}' in repos.conf has name "
                f"different from repository name '{repo['name']}' set inside repository",
                file=sys.stderr,
            )
            continue
        kept.append(repo)
    repos = kept

    if not any(r["name"] == main_repo for r in repos):
        raise ResolutionError(f'no location for repo "{main_repo}" in repos.conf')

    # "masters" resolution -- real three-tier (config.py:237-245/484-490):
    # repos.conf masters wins outright; else layout.conf masters (an empty
    # one is a real "no masters"); else the implicit default (main repo
    # alone for every non-main repo; the main repo can never be its own
    # master). Unknown master names are silently dropped at every tier.
    location_by_name = {r["name"]: r["location"] for r in repos}
    main_repo_location = location_by_name.get(main_repo)

    def _resolve(names):
        return [location_by_name[n] for n in names if n in location_by_name]

    for repo in repos:
        if repo["_repos_conf_masters"] is not None:
            repo["masters"] = _resolve(repo["_repos_conf_masters"])
        elif repo["_layout_masters"] is not None:
            repo["masters"] = _resolve(repo["_layout_masters"])
        elif repo["name"] == main_repo:
            repo["masters"] = []
        else:
            repo["masters"] = [main_repo_location] if main_repo_location else []
        for k in ("_repos_conf_masters", "_layout_masters", "_section_name", "_repos_conf_aliases"):
            del repo[k]

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


def _scope_repo_package_use_lines(lines, repo_name):
    """Same real "add ::repo to every atom without one" rule as
    _scope_repo_mask_lines, applied to the package.use/.mask/.force/
    .stable.mask/.stable.force line shape instead ("<atom> <flag>
    <flag> ...", see _parse_package_use_lines): only the leading atom
    token gets scoped, the flag tokens after it are passed through
    untouched. Unlike package.mask/.unmask, none of these files has
    -atom whole-entry removal syntax (real portage only ever extends
    these, and a leading "-" inside the flag list masks/forces a single
    flag off, not an atom) -- so there's no leading-"-" case to
    preserve here, unlike _scope_repo_mask_lines. Mirrors
    portage-profile/src/lib.rs's scope_repo_package_use_lines exactly."""
    result = []
    for line in lines:
        parts = line.split(None, 1)
        atom = parts[0] if parts else line
        rest = parts[1] if len(parts) > 1 else ""
        scoped_atom = atom if "::" in atom else f"{atom}::{repo_name}"
        result.append(f"{scoped_atom} {rest}" if rest else scoped_atom)
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
    more than one source can contribute an unmask entry.

    Ports real stack_lists's own ignore_repo=True behavior (the flag
    real MaskManager.__init__ always passes for its own final
    [repo_pkgmasklines, profile_pkgmasklines, user_pkgmasklines]
    combination -- confirmed by reading it directly): "let -cat/pkg
    remove cat/pkg::repo" -- an unscoped removal token (no "::" of its
    own, which is all a profile-level or user-level -atom can ever be,
    since only repo-level entries ever get ::repo-scoped at all, see
    _scope_repo_mask_lines) strips any "::repo" suffix off every
    existing entry before comparing, so it cancels a repo-scoped atom
    from *any* repo, not just an identically-unscoped one -- without
    this, a profile's -dev-libs/foo could never again cancel the main
    repo's own (now ::reponame-scoped) dev-libs/foo mask entry. A
    removal token that's already ::repo-scoped itself (rare -- only
    possible if a user writes one by hand) keeps exact-match semantics
    instead, matching real stack_lists's own "::" not in token guard
    exactly. Mirrors portage-profile/src/lib.rs's stack_mask_lines
    exactly."""
    result = []
    for lines in sources:
        for line in lines:
            if line.startswith("-"):
                removed = line[1:]
                if "::" in removed:
                    result = [x for x in result if x != removed]
                else:
                    result = [x for x in result if x.split("::", 1)[0] != removed]
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


_GPKG_COMPRESSIONS = {
    ".gz": ["gzip", "-dc"],
    ".bz2": ["bzip2", "-dc"],
    ".lz4": ["lz4", "-dc"],
    ".lz": ["lzip", "-dc"],
    ".lzo": ["lzop", "-dc"],
    ".xz": ["xz", "-T0", "-dc"],
    ".zst": ["zstd", "-dc", "--long=31"],
}


def _read_gpkg_metadata(path):
    """Real gpkg.get_metadata() / unpack_metadata(want=None), narrowed
    exactly like portuale/src/binpkg.rs::read_gpkg_metadata (NO
    Manifest/.sig verification -- so it also reads this pilot's own
    hand-built fixture gpkgs, which real portage.gpkg would reject).
    Hand-rolled rather than `from portage.gpkg import gpkg` for that
    reason. Mirrors the Rust reader."""
    import io
    import subprocess
    import tarfile

    with tarfile.open(path, "r") as container:
        members = container.getmembers()
        if "gpkg-1" not in (os.path.basename(m.name) for m in members):
            raise ValueError(f"{path}: not a gpkg container (no gpkg-1 marker)")
        member = None
        comp = None
        for m in members:
            base = os.path.basename(m.name)
            if base == "metadata.tar":
                member = m
                break
            for ext, argv in _GPKG_COMPRESSIONS.items():
                if base == "metadata.tar" + ext:
                    member, comp = m, argv
                    break
            if member is not None:
                break
        if member is None:
            raise ValueError(f"{path}: no metadata.tar member")
        raw = container.extractfile(member).read()

    if comp is not None:
        raw = subprocess.run(comp, input=raw, capture_output=True, check=True).stdout

    out = {}
    with tarfile.open(fileobj=io.BytesIO(raw), mode="r:") as md:
        for m in md.getmembers():
            if not m.isfile():
                continue
            key = m.name.split("/", 1)[-1] if "/" in m.name else m.name
            try:
                out[key] = md.extractfile(m).read().decode("utf-8").strip()
            except UnicodeDecodeError:
                continue
    return out


def _scan_pkgdir(pkgdir):
    """Real bintree._populate_local, narrowed -- see
    portuale/src/binpkg.rs::scan_pkgdir. Walks <pkgdir>/<cat>/<pf>.{tbz2,
    gpkg.tar} (one level deep) and synthesizes one Packages-style entry
    per file from its own embedded metadata. CPV from the path, SIZE from
    the file, REPO from the embedded `repository`. Bare .xpak
    (multi-instance) skipped; a parse failure aborts the scan (surfaced
    by run()). Mirrors the Rust scan."""
    from portage.xpak import tbz2

    out = []
    try:
        categories = sorted(os.listdir(pkgdir))
    except OSError:
        return out
    for category in categories:
        cat_path = os.path.join(pkgdir, category)
        if not os.path.isdir(cat_path):
            continue
        for name in sorted(os.listdir(cat_path)):
            path = os.path.join(cat_path, name)
            if name.endswith(".gpkg.tar"):
                pf = name[: -len(".gpkg.tar")]
                meta = _read_gpkg_metadata(path)
            elif name.endswith(".tbz2"):
                pf = name[: -len(".tbz2")]
                meta = {
                    (k.decode("utf-8", "replace") if isinstance(k, bytes) else k): (
                        v.decode("utf-8", "replace") if isinstance(v, bytes) else v
                    ).strip()
                    for k, v in tbz2(path).get_data().items()
                }
            else:
                continue
            meta["CPV"] = f"{category}/{pf}"
            meta.setdefault("CATEGORY", category)
            meta.setdefault("PF", pf)
            if "repository" in meta:
                meta.setdefault("REPO", meta.pop("repository"))
            try:
                meta["SIZE"] = str(os.path.getsize(path))
            except OSError:
                pass
            meta["PATH"] = f"{category}/{name}"
            out.append(meta)
    out.sort(key=lambda e: e.get("CPV") or "")
    return out


def _local_binpkg_index(config):
    """The local $PKGDIR binary index for this run -- the CLI layer's own
    $PKGDIR directory scan (config["scanned_binpkgs"], set when
    <pkgdir>/Packages was absent) when it did one, otherwise the parsed
    <pkgdir>/Packages file. Mirrors portage-repo/src/lib.rs's
    local_binpkg_index."""
    scanned = config.get("scanned_binpkgs")
    if scanned is not None:
        return scanned
    return _read_packages_index(config["pkgdir"])


def list_binary_candidates(index, category, package):
    """Binary candidates from a parsed binary index (`remote` False).
    Mirrors portage-repo/src/lib.rs's list_binary_candidates."""
    return _binary_candidates_from_index(index, category, package, False)


def _binary_candidates_from_index(index, category, package, remote):
    """Shared body of list_binary_candidates (local, remote=False) and
    _list_remote_binary_candidates (remote=True). `index` is a list of
    parsed Packages-style entry dicts. repo_name comes from the entry's
    own REPO field (real Packages records it per package -- so a
    --getbinpkg binary shows ::gentoo at -pv), falling back to
    portage.versions._unknown_repo ("__unknown__"). Mirrors
    portage-repo/src/lib.rs's binary_candidates_from_index."""
    candidates = []
    for entry in index:
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
                "repo_name": entry.get("REPO") or "__unknown__",
                "license": entry.get("LICENSE", ""),
                "iuse": entry.get("IUSE", ""),
                "properties": entry.get("PROPERTIES", ""),
                "restrict": entry.get("RESTRICT", ""),
                "source": "binary",
                "binary_use": set(entry.get("USE", "").split()),
                "remote": remote,
                "size": _int_or_none(entry.get("SIZE")),
            }
        )
    return candidates


def _int_or_none(s):
    try:
        return int(s)
    except (TypeError, ValueError):
        return None


def _binrepo_packages_dir(sync_uri, root):
    """The on-disk directory holding a binrepo's Packages index (real
    bintree._populate_remote's pkgindex_file, bintree.py:1496-1504).
    Mirrors portage-profile/src/lib.rs's BinRepo::packages_dir."""
    if sync_uri.startswith("file://"):
        return sync_uri[len("file://") :]
    for scheme in ("https://", "http://", "ssh://"):
        if sync_uri.startswith(scheme):
            rest = sync_uri[len(scheme) :]
            hostport, _, path = rest.partition("/")
            host = hostport.split(":", 1)[0]
            return os.path.join(root, "var/cache/edb/binhost", host, path.strip("/"))
    return sync_uri


def _list_remote_binary_candidates(binrepos, root, local_index, category, package):
    """--getbinpkg/-g: binary candidates from every binrepo's own on-disk
    Packages index. A remote build of a cpv+version the local index
    (`local_index`) also carries is dropped (real bintree.isremote).
    Mirrors portage-repo/src/lib.rs's list_remote_binary_candidates."""
    if not binrepos:
        return []
    seen = {
        c["version"]
        for c in _binary_candidates_from_index(local_index, category, package, False)
    }
    out = []
    for binrepo in binrepos:
        pkgdir = _binrepo_packages_dir(binrepo["sync_uri"], root)
        for cand in _binary_candidates_from_index(
            _read_packages_index(pkgdir), category, package, True
        ):
            if cand["version"] not in seen:
                seen.add(cand["version"])
                out.append(cand)
    return out


def _read_binary_metadata_any(config, root, local_index, category, package, version):
    """read_binary_metadata extended to --getbinpkg: the local index
    first, then each binrepo's cached Packages. Mirrors
    portage-repo/src/lib.rs's read_binary_metadata_any."""
    m = read_binary_metadata(local_index, category, package, version)
    if m is not None:
        return m
    for binrepo in config.get("binrepos", []):
        m = read_binary_metadata(
            _read_packages_index(_binrepo_packages_dir(binrepo["sync_uri"], root)),
            category,
            package,
            version,
        )
        if m is not None:
            return m
    return None


def _filter_usepkg_exclude_include(binary_candidates, category, package, usepkg_exclude, usepkg_include):
    """--usepkg-exclude/--usepkg-include (real main.py: "a space
    separated list of package names or slot atoms", same "plain atom or
    *-wildcard" two-tier matcher _matches_config_entry already backs
    --exclude/.mask/.unmask with). Ports real depgraph.py's own per-
    candidate binary-eligibility check: in_usepkg_exclude =
    have_usepkg_exclude and usepkg_exclude.findAtomForPackage(pkg, ...);
    in_usepkg_include = not have_usepkg_include or usepkg_include.
    findAtomForPackage(pkg, ...); if in_usepkg_exclude or not
    in_usepkg_include: break -- the candidate is dropped from the binary
    pool entirely. Applied only to binary candidates, never ebuilds.
    Mirrors portage-repo/src/lib.rs's filter_usepkg_exclude_include
    exactly."""
    if not usepkg_exclude and not usepkg_include:
        return binary_candidates
    result = []
    for c in binary_candidates:
        candidate_str = (
            f"{category}/{package}-{c['version']}:{c['slot']}/{c['sub_slot']}::{c['repo_name']}"
        )
        is_excluded = any(
            _matches_config_entry(ex, candidate_str, category, package) for ex in usepkg_exclude
        )
        is_included = not usepkg_include or any(
            _matches_config_entry(inc, candidate_str, category, package) for inc in usepkg_include
        )
        if not is_excluded and is_included:
            result.append(c)
    return result


def read_binary_metadata(index, category, package, version):
    """Finds category/package-version's own entry in a parsed binary
    index -- the binary-candidate counterpart to read_md5_cache, giving
    DEPEND/RDEPEND/etc once a binary candidate has actually been chosen.
    None if not found. Mirrors portage-repo/src/lib.rs's
    read_binary_metadata exactly."""
    want = f"{category}/{package}-{version}"
    for entry in index:
        if entry.get("CPV") == want:
            return entry
    return None


def _match_atom_str(atom_str, cpv):
    """Whether `atom_str` (a plain dependency/set atom -- `*` prefix
    stripped for `@system` lines) matches the cpv `cat/pkg-ver`. Real
    Atom + match_from_list, same as everywhere else. Used only for the
    colour renderer's world/system classification."""
    try:
        atom = Atom(atom_str.lstrip("*"), allow_wildcard=True)
    except InvalidAtom:
        return False
    return bool(match_from_list(atom, [cpv]))


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


def _license_struct_masked_names(struct, acceptable):
    """The list form of `_license_struct_has_masked` -- real
    `LicenseManager._getMaskedLicenses`'s own return value: every license
    name not in `acceptable`. For a `||` group, `[]` the moment one
    alternative is fully clean; otherwise every masked name across every
    alternative. Order-preserving; the caller sorts + dedups. Mirrors
    portage-repo/src/lib.rs's tree_masked_license_names."""
    if not struct:
        return []
    if struct[0] == "||":
        ret = []
        for element in struct[1:]:
            if isinstance(element, list):
                if element:
                    tmp = _license_struct_masked_names(element, acceptable)
                    if not tmp:
                        return []
                    ret.extend(tmp)
            elif element in acceptable:
                return []
            else:
                ret.append(element)
        return ret
    ret = []
    for element in struct:
        if isinstance(element, list):
            if element:
                ret.extend(_license_struct_masked_names(element, acceptable))
        elif element not in acceptable:
            ret.append(element)
    return ret


def _missing_licenses(candidate, category, package, candidate_str, config):
    """The exact license names `candidate`'s own LICENSE needs accepting
    -- real LicenseManager.getMissingLicenses in its list form. Sorted +
    deduped ("' '.join(sorted(missing_licenses))"). Mirrors
    portage-repo/src/lib.rs's missing_licenses."""
    license_str = candidate.get("license", "")
    if not license_str.strip():
        return []
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
        struct = use_reduce(license_str, uselist=list(use_flags), opconvert=True)
    except InvalidDependString:
        return []
    acceptable = _resolve_acceptable_tokens(accept_tokens, all_mentioned)
    return sorted(set(_license_struct_masked_names(struct, acceptable)))


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
        config["conf_use_tokens"],
        config["package_use_repo"],
        config["package_use"],
        config["package_use_user"],
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


def _evaluated_metadata_tokens(value_str, candidate, category, package, candidate_str, config):
    """A candidate's own PROPERTIES (or RESTRICT) tokens after real
    USE-conditional evaluation against this candidate's own effective USE
    -- real _PackageMetadataWrapper.__getitem__'s own use_reduce(...)
    pass over a _use_conditional_keys value ("local_config and '?' in
    v"), which is exactly what pkg.properties/pkg.restrict then .split().
    Used for the display-only `interactive` bracket-column check. An
    unparsable value yields an empty set. Mirrors portage-repo/src/
    lib.rs's evaluated_metadata_tokens exactly."""
    if not value_str.strip():
        return set()
    use_flags = _use_flags_if_conditional(
        value_str, candidate, category, package, candidate_str, config
    )
    try:
        return {t for t in use_reduce(value_str, uselist=list(use_flags), flat=True) if t != "||"}
    except InvalidDependString:
        return set()


def _flatten_src_uri(src_uri, use_flags):
    """Flattens a SRC_URI string into the ordered list of local filenames
    it names -- the "arrow" rename target, or the URI's own basename (PMS
    3.1.6). Recursive-descent, mirroring portage-fetch::flatten_src_uri /
    parse_list exactly (a small bespoke parser, not real use_reduce, the
    same "two independent implementations" approach the rest of this
    pilot uses). Raises ValueError on a grammar it can't parse."""
    tokens = src_uri.split()
    pos = 0

    def parse_list():
        nonlocal pos
        out = []
        while pos < len(tokens) and tokens[pos] != ")":
            tok = tokens[pos]
            if tok.endswith("?"):
                pos += 1
                if pos >= len(tokens) or tokens[pos] != "(":
                    raise ValueError(f'SRC_URI: expected "(" after {tok!r}')
                pos += 1
                flag = tok[:-1]
                negated = flag.startswith("!")
                if negated:
                    flag = flag[1:]
                inner = parse_list()
                if pos >= len(tokens) or tokens[pos] != ")":
                    raise ValueError(f"SRC_URI: unterminated {tok!r} group")
                pos += 1
                on = flag in use_flags
                if (not on) if negated else on:
                    out.extend(inner)
            elif tok in ("(", ")"):
                raise ValueError(f"SRC_URI: unexpected {tok!r}")
            else:
                pos += 1
                if pos < len(tokens) and tokens[pos] == "->":
                    pos += 1
                    if pos >= len(tokens):
                        raise ValueError('SRC_URI: missing filename after "->"')
                    out.append(tokens[pos])
                    pos += 1
                else:
                    out.append(tok.rsplit("/", 1)[-1])
        return out

    result = parse_list()
    if pos != len(tokens):
        raise ValueError(f"SRC_URI: unexpected token {tokens[pos]!r}")
    return result


def _manifest_dist_sizes(manifest_path):
    """Every `DIST <name> <size> ...` line of a repo Manifest, as
    {name: size}. A missing Manifest is an empty dict (same tolerance the
    Rust parse_manifest gives). Mirrors portage-fetch::parse_manifest,
    narrowed to the size field this pilot's f/F column needs."""
    out = {}
    try:
        with open(manifest_path) as fh:
            text = fh.read()
    except OSError:
        return out
    for line in text.splitlines():
        parts = line.split()
        if len(parts) >= 3 and parts[0] == "DIST":
            try:
                out[parts[1]] = int(parts[2])
            except ValueError:
                pass
    return out


def _fetch_restrict_files_all_present(
    src_uri, use_flags, repo_location, category, package, distdir
):
    """Real output.py:636's `not getfetchsizes(cpv, useflags=...,
    only_restricted=True)`: whether every distfile SRC_URI names
    (flattened against effective USE) is already in `distdir` at the
    size its repo Manifest records. Unparsable SRC_URI / missing
    Manifest entry -> not satisfied (the loud F). Empty SRC_URI ->
    trivially satisfied. Mirrors portage-repo/src/lib.rs's
    fetch_restrict_files_all_present."""
    try:
        files = _flatten_src_uri(src_uri, use_flags)
    except ValueError:
        return False
    if not files:
        return True
    sizes = _manifest_dist_sizes(
        os.path.join(repo_location, category, package, "Manifest")
    )
    for name in files:
        if name not in sizes:
            return False
        try:
            if os.path.getsize(os.path.join(distdir, name)) != sizes[name]:
                return False
        except OSError:
            return False
    return True


def _fetch_bytes_to_download(src_uri, use_flags, repo_location, category, package, distdir):
    """Real output.py:300-332's _calc_size -> counters.totalsize:
    (filename, size) for each SRC_URI distfile whose on-disk size in
    `distdir` isn't already its Manifest size. (filename, _) carried so
    the caller can dedup a shared distfile across the graph (real
    myfetchlist). Unparsable SRC_URI / incomplete Manifest -> [] (real
    getfetchsizes returns None, _calc_size adds nothing). Mirrors
    portage-repo/src/lib.rs's fetch_bytes_to_download."""
    try:
        files = _flatten_src_uri(src_uri, use_flags)
    except ValueError:
        return []
    if not files:
        return []
    sizes = _manifest_dist_sizes(os.path.join(repo_location, category, package, "Manifest"))
    out = []
    for name in files:
        if name not in sizes:
            return []
        try:
            on_disk = os.path.getsize(os.path.join(distdir, name))
        except OSError:
            on_disk = None
        if on_disk != sizes[name]:
            out.append((name, sizes[name]))
    return out


def _localized_size(num_bytes):
    """Real portage.localization.localized_size: math.ceil(num_bytes /
    1024) KiB ("always round up, so small files don't end up as
    '0 KiB'"). This pilot drops real portage's LC_NUMERIC thousands
    grouping of the KiB count -- only observable above 999 KiB, and a
    locale-dependent separator would break the contract suite. Always
    KiB. Mirrors pretend.rs's localized_size."""
    return f"{-(-num_bytes // 1024)} KiB"


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


def _mask_masked_only(candidate, category, package, config):
    """--autounmask-keep-masks=n's own v1 slice, the package.mask analogue
    of _keyword_masked_only: true iff candidate matches a package.mask
    entry (and no package.unmask) but every *other* is_visible check
    (KEYWORDS, LICENSE, PROPERTIES, RESTRICT) passes. Mirrors
    portage-repo/src/lib.rs's mask_masked_only. v1 cut: real's
    `# <filename>:` + masking-comment lines (no source provenance)."""
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
    if not masked:
        return False
    if not _keywords_accepted(
        candidate["keywords"],
        candidate_str,
        category,
        package,
        config["accept_keywords"],
        config["package_accept_keywords"],
    ):
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
    return _metadata_key_accepted(
        candidate.get("restrict", ""),
        candidate,
        category,
        package,
        candidate_str,
        config,
        config["accept_restrict"],
        config["package_accept_restrict"],
    )


def _suggested_mask_candidate(repos, category, package, config):
    """The best --autounmask-keep-masks=n candidate: the highest-versioned
    candidate masked by package.mask alone, as its version string. Mirrors
    portage-repo/src/lib.rs's suggested_mask_candidate."""
    masked = [
        c
        for c in list_candidates(repos, category, package)
        if _mask_masked_only(c, category, package, config)
    ]
    if not masked:
        return None
    return _best_candidate(masked)["version"]


def _license_masked_only(candidate, category, package, config):
    """--autounmask-license's own v1 slice, the LICENSE analogue of
    _keyword_masked_only: true iff candidate would be is_visible except
    for its own LICENSE (package.mask, KEYWORDS, PROPERTIES, RESTRICT all
    pass, only _license_accepted fails). Mirrors portage-repo/src/lib.rs's
    license_masked_only."""
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
    if not _keywords_accepted(
        candidate["keywords"],
        candidate_str,
        category,
        package,
        config["accept_keywords"],
        config["package_accept_keywords"],
    ):
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
    return not _license_accepted(candidate, category, package, candidate_str, config)


def _suggested_license_candidate(repos, category, package, config):
    """The --autounmask-license analogue of _suggested_keyword_candidate:
    among the candidates masked by LICENSE alone, the highest-versioned
    one, paired with its space-joined sorted missing-license names. None
    if none is license-masked-only. Mirrors portage-repo/src/lib.rs's
    suggested_license_candidate."""
    candidates = list_candidates(repos, category, package)
    masked = [c for c in candidates if _license_masked_only(c, category, package, config)]
    if not masked:
        return None
    c = _best_candidate(masked)
    cs = (
        f"{category}/{package}-{c['version']}:{c['slot']}/{c['sub_slot']}::{c['repo_name']}"
    )
    return (
        c["version"],
        " ".join(_missing_licenses(c, category, package, cs, config)),
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


def _suggested_keyword_candidate(repos, category, package, config):
    """The best --autounmask keyword suggestion for category/package, if
    any: among every candidate masked by KEYWORDS alone
    (_keyword_masked_only), the highest-versioned one (_best_candidate),
    paired with its own _suggested_keyword. None if category/package
    isn't listable at all, or no candidate is masked by KEYWORDS alone.
    Mirrors portage-repo/src/lib.rs's suggested_keyword_candidate --
    shared by both call sites that need this exact "what would I suggest
    here" computation: a top-level atom's own fatal NoVisibleCandidate
    (which raises ResolutionError with it) and a *dependency's* own
    NoVisibleCandidate (which attaches it to that entry tuple instead --
    see resolve_pretend_graph's own docstring)."""
    candidates = list_candidates(repos, category, package)
    keyword_masked = [
        c
        for c in candidates
        if _keyword_masked_only(c, category, package, config) and _suggested_keyword(c) is not None
    ]
    if not keyword_masked:
        return None
    candidate = _best_candidate(keyword_masked)
    return candidate["version"], _suggested_keyword(candidate)


def _use_masked_only(candidate, category, package, atom, config):
    """Real --autounmask-use's own v1 slice: true iff candidate would be
    is_visible (package.mask/license/properties/restrict/KEYWORDS all
    pass -- unlike _keyword_masked_only, which explicitly *skips* the
    keywords check, this one requires it) but atom's own use-deps don't
    match its current IUSE/effective-USE state (_use_deps_satisfied).
    KEYWORDS and USE-deps are two genuinely independent reasons a
    candidate can be rejected; a candidate masked by KEYWORDS too gets no
    USE suggestion here, matching real portage's own "only suggest a
    change that would actually fix it" spirit _keyword_masked_only's own
    docstring already established. Mirrors portage-repo/src/lib.rs's
    use_masked_only exactly."""
    if not is_visible(candidate, category, package, config):
        return False
    iuse, use_flags = _candidate_iuse_and_use(candidate, category, package, config)
    return not _use_deps_satisfied(atom, _valid_iuse(iuse, config), use_flags)


def _flag_is_settable(candidate, category, package, flag, desired, config):
    """Whether `flag` can actually be forced to `desired` via a
    package.use entry for `candidate` -- real pkg.use.mask/pkg.use.force
    (global use.mask/use.force folded in) always override package.use
    regardless of what it says, so a masked/forced flag can never really
    be "fixed" this way. Rather than re-deriving use.mask/.force/
    package.use.mask/.force matching logic separately, this recomputes
    effective_use_flags with a synthetic, exact-version package.use entry
    appended and checks whether the result actually reflects `desired` --
    if mask/force override it, the synthetic entry's own effect is
    silently discarded the same way a real one would be. Mirrors
    portage-repo/src/lib.rs's flag_is_settable exactly, including its own
    "a plain =category/package-version atom, not the fully-qualified
    slot/repo-suffixed candidate_str" fix (match_from_list expects an
    atom pattern on the left, not a candidate string used as both)."""
    try:
        metadata = read_md5_cache(
            candidate["repo_location"], category, f"{package}-{candidate['version']}"
        )
    except OSError:
        return False
    iuse = metadata.get("IUSE", "")
    candidate_str = (
        f"{category}/{package}-{candidate['version']}:{candidate['slot']}/{candidate['sub_slot']}::"
        f"{candidate['repo_name']}"
    )
    synthetic_token = flag if desired else f"-{flag}"
    synthetic_atom = f"={category}/{package}-{candidate['version']}"
    # The synthetic entry stands in for a hypothetical *user* package.use
    # line, so it joins the "pkg" layer (strongest).
    package_use_user = [
        *config["package_use_user"],
        (synthetic_atom, [synthetic_token]),
    ]
    use_flags = effective_use_flags(
        iuse,
        config["use_tokens"],
        config["conf_use_tokens"],
        config["package_use_repo"],
        config["package_use"],
        package_use_user,
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
    return (flag in use_flags) == desired


def _suggested_use_flip(candidate, category, package, atom, config):
    """The best --autounmask-use flag-flip suggestion for `candidate`
    against `atom`'s own use-deps (only the two unconditional forms,
    real atom.use.enabled/.disabled, are ever consulted -- the four
    conditional forms are a wholly different, unimplemented-here
    mechanism, see _use_deps_satisfied's own docstring). None when
    nothing needs to change, when a needed flag isn't even in the
    candidate's own IUSE at all (real "flag not in IUSE" unfixability --
    no package.use entry could address it), or when any needed change is
    blocked by _flag_is_settable -- a partially-fixable atom (some flags
    adjustable, one masked/forced) suggests nothing at all rather than a
    change that wouldn't actually resolve the mismatch. Mirrors
    portage-repo/src/lib.rs's suggested_use_flip exactly."""
    iuse, use_flags = _candidate_iuse_and_use(candidate, category, package, config)
    use = atom.use
    wanted = [(flag, True) for flag in use.enabled] + [(flag, False) for flag in use.disabled]
    changes = []
    for flag, desired in wanted:
        if flag not in iuse:
            return None
        if (flag in use_flags) != desired:
            changes.append((flag, desired))
    if not changes:
        return None
    if any(
        not _flag_is_settable(candidate, category, package, flag, desired, config)
        for flag, desired in changes
    ):
        return None
    changes.sort()
    return changes


def _suggested_use_candidate(repos, category, package, atom, config):
    """The best --autounmask-use suggestion for category/package against
    atom, if any: among every candidate masked by a plain USE-dep
    mismatch alone (_use_masked_only) that also has a real
    _suggested_use_flip, the highest-versioned one (repo priority
    breaking a tie, same as _best_candidate). None when atom has no
    use-deps at all, category/package isn't listable at all, or no
    candidate qualifies. Mirrors portage-repo/src/lib.rs's
    suggested_use_candidate exactly, shared by the same two call sites as
    _suggested_keyword_candidate."""
    if atom.use is None:
        return None
    candidates = list_candidates(repos, category, package)
    qualifying = []
    for c in candidates:
        if not _use_masked_only(c, category, package, atom, config):
            continue
        flip = _suggested_use_flip(c, category, package, atom, config)
        if flip is not None:
            qualifying.append((c, flip))
    if not qualifying:
        return None
    best_candidate, best_flip = qualifying[0]
    for c, flip in qualifying[1:]:
        cmp = vercmp(c["version"], best_candidate["version"]) or 0
        if cmp > 0 or (cmp == 0 and c["repo_priority"] > best_candidate["repo_priority"]):
            best_candidate, best_flip = c, flip
    return best_candidate["version"], best_flip


def _implicit_iuse_set(iuse, config):
    """Real config.py's own _get_implicit_iuse(): a package's own
    declared IUSE (default markers stripped) folded together with
    PORTAGE_ARCHLIST (profiles/arch.list), use.mask ∪ use.force, and the
    literal "build"/"bootstrap" flags -- real pkg.iuse.is_valid_flag's
    own full domain, not a package's own literal IUSE alone. Mirrors
    portage-repo/src/lib.rs's implicit_iuse_set exactly."""
    iuse_set = {tok.lstrip("+-") for tok in iuse.split()}
    iuse_set |= config["archlist"]
    iuse_set |= config["use_mask"]
    iuse_set |= config["use_force"]
    iuse_set |= {"build", "bootstrap"}
    # Real EAPI 5+ check_required_use is called with pkg.iuse.is_valid_flag
    # = explicit ∪ IUSE_EFFECTIVE, so a REQUIRED_USE referencing an
    # elibc_*/kernel_*/... implicit flag (USE_EXPAND_IMPLICIT) is valid
    # the same way one referencing x86 (archlist) is. _valid_iuse (for
    # _use_deps_satisfied) is the narrower declared ∪ iuse_effective
    # subset of this.
    iuse_set |= config.get("iuse_effective", set())
    return iuse_set


def _parent_use_state(repos, entries, owner, config):
    """The requesting parent's own current resolved candidate, implicit
    IUSE, and effective USE -- looked up via its own already-resolved
    entry in `entries`. The parent is always already present there by
    the time any of its own dependencies are dequeued (BFS processes a
    package's own entry before ever enqueueing its dependencies). None
    when the parent isn't found or has no version to look up by
    (already_installed/no_visible_candidate -- moot anyway, since the
    already_installed recursion path never conditional-evaluates deps at
    all). Mirrors portage-repo/src/lib.rs's parent_use_state exactly."""
    category, package = owner
    parent_entry = next(
        (e for e in entries if e[0] == category and e[1] == package), None
    )
    if parent_entry is None:
        return None
    outcome = parent_entry[2]
    tag = outcome[0]
    if tag == "new":
        version = outcome[1]
    elif tag in ("upgrade", "downgrade"):
        version = outcome[2]
    elif tag == "reinstall":
        version = outcome[1]
    else:
        return None
    candidates = list_candidates(repos, category, package)
    matching = [c for c in candidates if c["version"] == version]
    if not matching:
        return None
    resolved = max(matching, key=lambda c: c["repo_priority"])
    _iuse, use_flags = _candidate_iuse_and_use(resolved, category, package, config)
    try:
        metadata = read_md5_cache(resolved["repo_location"], category, f"{package}-{version}")
    except OSError:
        return None
    full_iuse = _implicit_iuse_set(metadata.get("IUSE", ""), config)
    return (resolved, full_iuse, use_flags, metadata.get("REQUIRED_USE"))


def _conditional_flags(unevaluated_atom_str):
    """Which of `unevaluated_atom_str`'s own use-deps are conditional on
    the *requesting parent's* own USE (opt?/!opt?/opt=/!opt= -- real
    Atom.use.conditional's own .enabled/.disabled/.equal/.not_equal
    frozensets), deduplicated and sorted. Empty when the atom has no
    conditional use-deps at all. Mirrors portage-repo/src/lib.rs's
    conditional_flags exactly."""
    atom = _parse_atom(unevaluated_atom_str)
    if atom is None or atom.use is None or atom.use.conditional is None:
        return []
    c = atom.use.conditional
    flags = set(c.equal) | set(c.not_equal) | set(c.enabled) | set(c.disabled)
    return sorted(flags)


def _suggested_parent_use_candidate(repos, entries, unevaluated_atom, owner, config):
    """Real --autounmask-use's own second, architecturally distinct
    mechanism (real _show_unsatisfied_dep, lib/_emerge/depgraph.py:
    6756-6846): unlike _suggested_use_candidate (which flips the
    *candidate's* own flag), this one flips the *requesting parent's* own
    flag, for the case where a dependency atom's use-dep was originally
    conditional on the parent's own USE state -- _enqueue_flat_deps
    already evaluated it away into a concrete form (or dropped it) before
    this atom was ever queued, using the parent's own *current* USE;
    this asks "if the parent's own involved flag(s) were toggled together
    instead, would the re-evaluated atom now actually resolve?"

    Deliberately narrower than real Atom.violated_conditionals (~150
    lines of per-token-operator partitioning this pilot doesn't
    reproduce): instead of determining exactly *which* conditional
    use-deps were violated, this toggles *every* flag the unevaluated
    atom's own conditional use-deps reference, together, in one
    hypothetical -- matching real portage's own target_use (which also
    flips every involved_flags member at once) for the common case, but
    diverging from it for more exotic mixed cases (concrete *and*
    conditional use-deps on the same atom, or independent conditional
    flags where only a subset actually needs flipping). Confirmed with
    the user before implementing.

    Gated on: every involved flag must be real, valid IUSE on the parent;
    none may be package.use.mask/.force'd on the parent
    (_flag_is_settable, reused as-is); the re-evaluated atom must
    actually become satisfiable (_atom_currently_satisfiable) against the
    hypothetical flip; and the flip must not newly violate the parent's
    own REQUIRED_USE (mirrors real _show_unsatisfied_dep's own
    "collect_use_changes and not required_use_warning" gate). Returns
    (parent_category, parent_package, parent_version, [(flag,
    desired_state)]), attached to the *dependency's* own entry
    (parent_use_suggestion) rather than the parent's own entry, unlike
    real portage's own missing_use_reasons.append((myparent, ...)) -- a
    pragmatic simplification, same as the Rust side. Mirrors
    portage-repo/src/lib.rs's suggested_parent_use_candidate exactly."""
    involved_flags = _conditional_flags(unevaluated_atom)
    if not involved_flags:
        return None
    parent_state = _parent_use_state(repos, entries, owner, config)
    if parent_state is None:
        return None
    parent_candidate, parent_iuse, parent_use, parent_required_use = parent_state
    if any(f not in parent_iuse for f in involved_flags):
        return None

    category, package = owner
    target_use = [(f, f not in parent_use) for f in involved_flags]
    if any(
        not _flag_is_settable(parent_candidate, category, package, flag, desired, config)
        for flag, desired in target_use
    ):
        return None

    hypothetical_use = set(parent_use)
    for flag, desired in target_use:
        if desired:
            hypothetical_use.add(flag)
        else:
            hypothetical_use.discard(flag)

    dep_atom = _parse_atom(unevaluated_atom)
    if dep_atom is None:
        return None
    re_evaluated = str(dep_atom.evaluate_conditionals(hypothetical_use))
    if not _atom_currently_satisfiable(repos, re_evaluated, config):
        return None

    if parent_required_use and parent_required_use.strip():
        try:
            old_sat = bool(
                check_required_use(
                    parent_required_use,
                    parent_use,
                    lambda flag: flag in parent_iuse,
                    eapi="8",
                )
            )
            new_sat = bool(
                check_required_use(
                    parent_required_use,
                    hypothetical_use,
                    lambda flag: flag in parent_iuse,
                    eapi="8",
                )
            )
        except InvalidDependString:
            old_sat = new_sat = False
        if old_sat and not new_sat:
            return None

    target_use.sort()
    return (category, package, parent_candidate["version"], target_use)


def _visibility_provenance(candidate, category, package, config):
    """--json's own "state-change trace" (this pilot's own feature, not
    a port of any real emerge output): which config entries, if any,
    were actually load-bearing for an already-is_visible candidate to
    end up visible. Mirrors portage-repo/src/lib.rs's
    visibility_provenance/VisibilityProvenance exactly -- see their own
    docstrings for the full rationale, including why this duplicates a
    small, stable chunk of is_visible's own body (same precedent
    _keyword_masked_only above already set) rather than threading a
    reason out of is_visible's own hot filtering loop. Returns a dict
    with "mask_entry"/"unmask_entry"/"keyword_entry", each None or the
    specific config entry string responsible. Only meaningful to call on
    a candidate already known is_visible."""
    candidate_str = (
        f"{category}/{package}-{candidate['version']}:{candidate['slot']}/{candidate['sub_slot']}"
        f"::{candidate['repo_name']}"
    )

    mask_entry = next(
        (m for m in config["package_mask"] if _matches_config_entry(m, candidate_str, category, package)),
        None,
    )
    unmask_entry = None
    if mask_entry is not None:
        unmask_entry = next(
            (
                u
                for u in config["package_unmask"]
                if _matches_config_entry(u, candidate_str, category, package)
            ),
            None,
        )
    keyword_entry = _keyword_provenance(
        candidate["keywords"],
        candidate_str,
        category,
        package,
        config["accept_keywords"],
        config["package_accept_keywords"],
    )
    return {
        "mask_entry": mask_entry,
        "unmask_entry": unmask_entry,
        "keyword_entry": keyword_entry,
    }


def _keyword_mask_marker(candidate, category, package, config, mask_entry):
    """Real output.py:gen_mask_str + Package.get_keyword_mask/isHardMasked,
    for the -v one-character bracket-mask column: "#" for a candidate
    hard-masked somewhere but pulled in anyway (isHardMasked, wins first),
    None for one accepted by the global ACCEPT_KEYWORDS alone (no marker),
    "~" for one visible only via a ~<our-arch> testing keyword
    (get_keyword_mask "unstable"), "*" for one visible only via ** or a
    different arch ("missing"). The ~-vs-* split is read straight off the
    candidate's own KEYWORDS rather than reconstructing getRawMissing-
    Keywords -- sufficient for every realistic single-arch case. Mirrors
    portage-repo/src/lib.rs's keyword_mask_marker exactly."""
    if mask_entry is not None:
        return "#"
    candidate_str = (
        f"{category}/{package}-{candidate['version']}:{candidate['slot']}/{candidate['sub_slot']}"
        f"::{candidate['repo_name']}"
    )
    if _keywords_accepted(
        candidate["keywords"], candidate_str, category, package, config["accept_keywords"], []
    ):
        return None
    testing_for_our_arch = any(
        k.startswith("~") and k[1:] in config["accept_keywords"]
        for k in candidate["keywords"]
    )
    return "~" if testing_for_our_arch else "*"


def _keyword_provenance(
    keywords, candidate_str, category, package, accept_keywords, package_accept_keywords
):
    """The specific package.accept_keywords entry (if any) responsible
    for keywords being accepted -- mirrors portage-repo/src/lib.rs's
    keyword_provenance exactly. None if the plain global accept_keywords
    set alone already accepts it (checked via _keywords_accepted with no
    package entries at all); otherwise walks package_accept_keywords in
    the same least-to-most-specific order _specificity_ordered_flags
    itself applies them in, accumulating onto a copy of the global set,
    and reports the first entry whose own addition flips
    _keywords_accepted from false to true."""
    if _keywords_accepted(keywords, candidate_str, category, package, accept_keywords, []):
        return None
    matching = [
        (entry, tokens)
        for entry, tokens in package_accept_keywords
        if _matches_config_entry(entry, candidate_str, category, package)
    ]
    matching.sort(key=lambda et: _atom_specificity(et[0]))
    seed = set(accept_keywords)
    for entry, tokens in matching:
        _apply_incremental(" ".join(tokens), seed)
        if _keywords_accepted(keywords, candidate_str, category, package, seed, []):
            return entry
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
    conf_use_tokens,
    package_use_repo,
    package_use,
    package_use_user,
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
    """The USE flags in effect for one specific package -- one continuous
    incremental walk over the real USE_ORDER layers this pilot models,
    low priority to high (real config.py::regenerate() over the reversed
    uvlist; USE_ORDER default
    "env:pkg:conf:defaults:pkginternal:features:repo:env.d"). The "Config
    depth" slice (SCOPE_BACKLOG Part 2.C) split the earlier flat model
    ("IUSE seed, then use_tokens, then one flat package.use list") so
    each package.use source sits at its own real position:

      1. repo -- every configured repo's own profiles/package.use
         (package_use_repo), applied *before* the IUSE defaults. Weakest
         layer modeled. (Real portage also folds repo make.defaults USE
         in here; not modeled.)
      2. pkginternal -- iuse's own +flag/-flag default markers (see the
         paragraph below for the grounding).
      3. defaults -- every profile level's own make.defaults USE
         (use_tokens, chain order), then every profile level's own
         package.use (package_use, as one group).
      4. conf -- make.conf USE, then the USE_EXPAND folded values
         (conf_use_tokens).
      5. pkg -- the user-level /etc/portage/package.use
         (package_use_user). Strongest layer before the final
         use.force/use.mask step.

    Every layer is replayed via _apply_incremental directly -- not a
    pre-flattened set unioned on top (see the `iuse` paragraph below).
    env/features/env.d are documented cuts. Applied per package,
    mirroring portage-repo/src/lib.rs's effective_use_flags exactly.

    After the walk:
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
    pkginternal (position 5 of 8) is applied after repo but well *before*
    defaults (profile), conf (make.conf), and pkg (package.use) -- real
    portage's own actual precedence has every one of those three able to
    override an IUSE default; only env/env.d (real per-invocation/
    stacked-profile-env overrides, positions 8 and 1) sit even lower/
    higher than this pilot models at all. Applied here at that same
    relative position (step 2 of the walk above), with the repo
    package.use applied first (step 1) and every later layer replayed
    directly on top via _apply_incremental -- NOT a plain set union of
    the already-flattened use_flags. An earlier version of this pilot did
    union a flattened base here, which meant base could only ever *add* a
    flag, never explicitly cancel an IUSE +default the way real
    defaults/conf genuinely can (real regenerate() runs one continuous
    incremental walk across the whole reversed uvlist -- repo then
    pkginternal then defaults then conf then pkg -- so a -flag token in
    defaults/conf really does cancel an earlier pkginternal +flag,
    exactly like any other incremental variable). Replaying the ordered
    raw tokens instead of the flattened set closes that gap:
    resolve_config exposes both use_flags (the flattened result, still
    used elsewhere for e.g. --newuse comparisons) and the per-layer token
    lists / package.use keys that produced it. The dominant real-world
    case -- an ebuild author sets a sensible IUSE default, and nothing
    else ever mentions the flag at all -- was already correct either
    way; this closes the narrower case where a profile, make.conf, or a
    wrongly-layered repo/user package.use genuinely does mention it.

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
    use_flags = set()

    def _apply_matching(entries):
        for entry, tokens in entries:
            if _matches_config_entry(entry, candidate_str, category, package):
                _apply_incremental(" ".join(tokens), use_flags)

    # repo (real configdict["repo"]): before the IUSE defaults.
    _apply_matching(package_use_repo)

    # pkginternal: only a token with an explicit "+"/"-" marker
    # contributes anything at all.
    iuse_defaults = " ".join(
        tok for tok in iuse.split() if tok.startswith("+") or tok.startswith("-")
    )
    _apply_incremental(iuse_defaults, use_flags)

    # defaults (real configdict["defaults"]): profile make.defaults USE,
    # then profile package.use (as one group -- see resolve_config).
    for token in use_tokens:
        _apply_incremental(token, use_flags)
    _apply_matching(package_use)

    # conf (real configdict["conf"]): make.conf USE, then the USE_EXPAND
    # folded values.
    for token in conf_use_tokens:
        _apply_incremental(token, use_flags)

    # pkg (real configdict["pkg"]): user-level /etc/portage/package.use --
    # strongest before the final use.force/use.mask step below.
    _apply_matching(package_use_user)

    # _* wildcard USE_EXPAND expansion (real config.py setcpv ~2242):
    # once package.use has been applied, a "k_*" flag still in the set
    # (from USE="linguas_*" / LINGUAS="*" folding / package.use "LINGUAS: *"
    # shorthand) means "enable every k_<x> flag declared in THIS
    # candidate's own IUSE" -- the per-package expansion the IUSE-blind
    # global config layer can't do. Masked k_<x> are dropped again by the
    # use.mask steps below (real portage's own "x not in usemask" guard).
    # Not guarded on k being a USE_EXPAND var name -- a "_*" token in
    # this pilot's USE set only ever comes from USE_EXPAND folding or
    # package.use's USE_EXPAND shorthand. Mirrors portage-repo/src/lib.rs's
    # effective_use_flags exactly.
    iuse_names = [tok.lstrip("+-") for tok in iuse.split()]
    wildcard_prefixes = [f[:-1] for f in use_flags if f.endswith("_*")]
    for pfx in wildcard_prefixes:
        for name in iuse_names:
            if name.startswith(pfx):
                use_flags.add(name)

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
    # The "k_*" pseudo-flags themselves are not real USE flags -- real
    # portage strips every "_*"-suffixed token from PORTAGE_USE
    # (config.py ~2260) once they've done their expansion job above.
    use_flags = {f for f in use_flags if not f.endswith("_*")}
    return use_flags


def _forced_or_masked_flags(iuse, keywords, candidate_str, category, package, config):
    """Real _display_use's self.forced_flags = pkg.use.force | pkg.use.mask
    (fed to map_to_use_expand(..., forced_flags=True)), restricted to
    `iuse`'s own declared flags: the IUSE flags `emerge -pv` wraps in
    ( ... ) because they're force-enabled or mask-disabled. Built from
    the exact same use.force/use.mask + package.use.force/.mask (+ the
    stable variants when stable) layering effective_use_flags applies.
    Mirrors portage-repo/src/lib.rs's forced_or_masked_flags exactly."""
    iuse_names = {tok.lstrip("+-") for tok in iuse.split()}
    result = set(config["use_force"]) | set(config["use_mask"])
    result |= _specificity_ordered_flags(
        config["package_use_force"], candidate_str, category, package
    )
    result |= _specificity_ordered_flags(
        config["package_use_mask"], candidate_str, category, package
    )
    if _is_stable(
        keywords,
        candidate_str,
        category,
        package,
        config["accept_keywords"],
        config["package_accept_keywords"],
    ):
        result |= set(config["use_stable_force"]) | set(config["use_stable_mask"])
        result |= _specificity_ordered_flags(
            config["package_use_stable_force"], candidate_str, category, package
        )
        result |= _specificity_ordered_flags(
            config["package_use_stable_mask"], candidate_str, category, package
        )
    return result & iuse_names


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
        config["conf_use_tokens"],
        config["package_use_repo"],
        config["package_use"],
        config["package_use_user"],
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


def _alnum_sort_key(s):
    """Real output_helpers.py::_alnum_sort_key: split on runs of digits
    and compare the digit runs as numbers, not lexically -- so
    `python3_9` sorts before `python3_12`. Used for the `emerge -pv`
    USE="..." flag list (real `_create_use_string`'s own
    `any_iuse.sort(key=_alnum_sort_key)`). Mirrors portage-repo/src/
    lib.rs's alnum_sort_key."""
    parts = re.split(r"(\d+)", s)
    # parts alternates non-digit / digit / non-digit / ...; digit parts
    # (odd indices) become ints. A tuple of (str, int, str, int, ...)
    # compares element-wise; type at each position is consistent between
    # two well-formed keys.
    return tuple(int(p) if i % 2 else p for i, p in enumerate(parts))


def _use_flag_sort_key(tok):
    """The bare flag name inside a rendered USE= token, for the
    --alphabetical re-sort: strip a leading '(' / '-' and any trailing
    ')' / '*' / '%'. Mirrors pretend.rs's use_flag_sort_key."""
    return tok.lstrip("(-").rstrip(")*%")


def _build_use_expand_display(
    use_display,
    use_expand,
    use_expand_hidden,
    installed=None,
    forced=None,
    all_flags=True,
    reinst_flags=None,
):
    """Real output.py:_display_use + map_to_use_expand +
    output_helpers.py:_create_use_string, for `emerge --pretend -v`'s USE
    line. Splits `use_display` (already-bare-name-sorted (flag, enabled)
    pairs) into the plain USE group plus one group per `use_expand`
    variable whose lowercase(name)_ prefixes the flag (prefix stripped,
    real map_to_use_expand's val[len(exp)+1:]); drops `use_expand_hidden`
    groups (real remove_hidden). Returns [(VAR_NAME, "flag -flag"), ...],
    USE first then the USE_EXPAND vars sorted; an empty group produces no
    entry at all (real _create_use_string's `if ret:` guard). Within each
    group the enabled flags render first, then the disabled ones, each in
    bare-name order -- real _create_use_string's `" ".join(enabled +
    disabled)`. `emerge --alphabetical` collapses the two back into one
    interleaved bare-name-sorted list; that is applied at render time
    (use_suffix), not here.

    `installed`, when given, is (old_use, old_iuse) -- the installed
    version's own recorded USE/IUSE (bare names, old_use already
    intersected with old_iuse). Real _DisplayConfig sets verbosity=3
    whenever --verbose is given, so all_flags is true for `emerge -pv`
    and the diff shows every flag: enabled+new-IUSE -> flag%*,
    enabled+newly-on -> flag*, enabled+unchanged -> flag;
    disabled+new-IUSE -> -flag%, disabled+was-on -> -flag*,
    disabled+unchanged -> -flag; a flag dropped from IUSE by the new
    ebuild -> (-flag%) / (-flag%*). At plain `emerge -p` (all_flags
    False, the default verbosity 2) real _create_use_string leaves an
    *unchanged* flag -- and any removed-from-IUSE flag -- as flag_str =
    None, so it's omitted: only the changed flags render for a
    Reinstall/Upgrade, the full list for a New (is_new renders
    everything regardless). `reinst_flags` (real `reinst_flags_map`, the
    Reinstall's own `_reinstall_for_flags` trigger set) force-shows a
    flag even at plain -p: the one case it changes is a flag the new
    ebuild dropped from IUSE that still triggered a --newuse/--changed-use
    reinstall -- it now appears in the `(-flag%)` removed list at -p.

    `forced` (full flag names -- real self.forced_flags = pkg.use.force |
    pkg.use.mask) is any flag the user can't control: its rendered token
    is wrapped in ( ... ), and the trailing "%" on a -flag% is skipped
    (a masked flag brand-new to IUSE renders "(-flag)", not "(-flag%)").
    Mirrors portage-repo/src/lib.rs's build_use_expand_display exactly."""
    expand_vars = sorted(use_expand)
    hidden = {v.upper() for v in use_expand_hidden}
    old_use, old_iuse = installed if installed is not None else (None, None)
    forced = forced or set()
    reinst_flags = reinst_flags or set()

    # state: "enabled" / "disabled" / "removed" (rendered in that order).
    def render_flag(bare, full, state):
        is_forced = full in forced
        reinst = full in reinst_flags
        if state == "removed":
            if not all_flags and not reinst:
                return None
            in_old_use = installed is not None and full in old_use
            return f"(-{bare}%{'*' if in_old_use else ''})"
        enabled = state == "enabled"
        if installed is None:
            core = f"{'' if enabled else '-'}{bare}"
        else:
            in_old_iuse = full in old_iuse
            in_old_use = full in old_use
            if enabled:
                if not in_old_iuse:
                    core = f"{bare}%*"
                elif not in_old_use:
                    core = f"{bare}*"
                elif all_flags or reinst:
                    core = bare
                else:
                    return None
            elif not in_old_iuse:
                core = f"-{bare}" if is_forced else f"-{bare}%"
            elif in_old_use:
                core = f"-{bare}*"
            elif all_flags or reinst:
                core = f"-{bare}"
            else:
                return None
        return f"({core})" if is_forced else core

    # Entries keep the FULL flag name; the prefix is stripped at render.
    groups = [("", [])] + [(v.upper(), []) for v in expand_vars]
    by_name = {name: flags for name, flags in groups}

    def route(flag, state):
        for var in expand_vars:
            if flag.startswith(var.lower() + "_"):
                by_name[var.upper()].append((flag, state))
                return
        by_name[""].append((flag, state))

    for flag, enabled in use_display:
        route(flag, "enabled" if enabled else "disabled")
    if installed is not None and (all_flags or reinst_flags):
        cur = {f for f, _ in use_display}
        for flag in sorted(
            (f for f in old_iuse if f not in cur), key=_alnum_sort_key
        ):
            route(flag, "removed")

    rank = {"enabled": 0, "disabled": 1, "removed": 2}
    out = []
    for name, flags in groups:
        if name and name in hidden:
            continue
        prefix = name.lower() + "_"
        rendered_pairs = []
        for full, state in flags:
            bare = full if not name else (full[len(prefix):] if full.startswith(prefix) else full)
            tok = render_flag(bare, full, state)
            if tok is not None:
                rendered_pairs.append((rank[state], tok))
        # Real _create_use_string: `enabled + disabled + removed` --
        # stable, so the incoming bare-name order is kept within a rank.
        rendered_pairs.sort(key=lambda p: p[0])
        rendered = [tok for _r, tok in rendered_pairs]
        if not rendered:
            continue
        out.append(("USE" if not name else name, " ".join(rendered)))
    return out


def _deps_changed(root, repos, category, package, version, with_bdeps):
    """--changed-deps: whether `version`'s own vdb-recorded dependency
    strings differ from the repo's own *current* ebuild for that exact
    version -- real depgraph.py's own _changed_deps
    (lib/_emerge/depgraph.py:3168), ported essentially verbatim: for each
    dep key, real use_reduce(token_class=Atom) (i.e. flat=False, the
    structured nested-list form) against the installed package's own
    recorded USE (real portage's own uselist=pkg.use.enabled, used for
    *both* sides so a pure USE change is never what this detects), then
    real strip_slots and real strip_libc_deps, then built_deps !=
    unbuilt_deps compared element-wise (one struct per key). Which keys
    are compared respects with_bdeps exactly like _enqueue_dependencies's
    own dep-key list does.

    Because the comparison is a Python list `!=`, it is order-sensitive
    everywhere -- a ||-group reorder AND a plain "a b" -> "b a" reorder
    both register as changed (matching real portage), while a
    redundant-bracket difference ("a b" vs "( a b )") does not
    (use_reduce collapses those).

    A vdb-side dependency string that fails to parse counts as "changed"
    unconditionally, matching real portage's own "except
    InvalidDependString: changed = True"; a repo-side one that fails to
    parse instead reports "unchanged" (False), the same tolerant
    "can't tell, don't crash" fallback _enqueue_dependencies already
    uses (real portage assumes the repo side is always well-formed).
    Mirrors portage-repo/src/lib.rs's deps_changed exactly."""
    dep_keys = (
        ("DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND")
        if with_bdeps
        else ("RDEPEND", "PDEPEND", "IDEPEND")
    )

    installed_use = _read_vdb_flag_set(root, category, package, version, "USE")

    repo_candidates = [c for c in list_candidates(repos, category, package) if c["version"] == version]
    if not repo_candidates:
        return False
    resolved = max(repo_candidates, key=lambda c: c["repo_priority"])
    try:
        metadata = read_md5_cache(resolved["repo_location"], category, f"{package}-{version}")
    except OSError:
        return False

    libc_deps = {Atom(cp) for cp in _libc_provider_cps(root)}

    def reduced(depstr):
        ds = use_reduce(depstr, uselist=installed_use, token_class=Atom)
        strip_slots(ds)
        strip_libc_deps(ds, libc_deps)
        return ds

    # vdb side first (real built_deps loop, whose "except
    # InvalidDependString: changed = True" makes an unparsable vdb
    # dependency string an unconditional "changed").
    built_deps = []
    for k in dep_keys:
        try:
            built_deps.append(reduced(_read_vdb_string(root, category, package, version, k)))
        except InvalidDependString:
            return True

    # repo side (real unbuilt_deps loop) -- the repo's own current ebuild
    # metadata is assumed well-formed, so an unparsable one here stays the
    # tolerant "can't tell, don't crash" False.
    unbuilt_deps = []
    for k in dep_keys:
        try:
            unbuilt_deps.append(reduced(metadata.get(k, "")))
        except InvalidDependString:
            return False

    return built_deps != unbuilt_deps


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


def _rebuilt_binary_changed(root, index, category, package, version, rebuilt_binaries_timestamp):
    """--rebuilt-binaries: real depgraph.py's own reinstall trigger
    (lines ~8394-8429, confirmed by reading it) comparing a binary
    candidate's own BUILD_TIME against the already-installed package's
    own recorded BUILD_TIME -- "replace installed packages with binary
    packages that have been rebuilt" (real main.py's own help text), the
    common real-world case being a same-version binary rebuilt against
    updated dependencies (a toolchain/ABI bump), not a version change at
    all. Real code's own "skip the check if a newer *source* (unbuilt)
    candidate exists" branch has no equivalent here: this function is
    only ever called once the caller has already established `version`
    is both the best *visible* candidate and what's already installed,
    so nothing newer (built or unbuilt) can exist by construction.
    rebuilt_binaries_timestamp mirrors real --rebuilt-binaries-timestamp:
    when given, only a *newer* (built_timestamp > installed_timestamp)
    binary at or above that cutoff triggers a reinstall ("use
    --rebuilt-binaries-timestamp 0 if you want only newer binaries
    pulled in", real code comment); when absent, any *different*
    BUILD_TIME triggers one either direction ("don't care ... this is
    for closely tracking a binhost", same comment) -- real portage's own
    asymmetry, not a simplification here. A missing/unparseable
    BUILD_TIME on either side never triggers a reinstall, matching real
    code's own "if built_timestamp and ..." guard (bug #306659: a
    missing local/remote BUILD_TIME must never cause a spurious
    reinstall). Mirrors portage-repo/src/lib.rs's rebuilt_binary_changed
    exactly."""
    binary_metadata = read_binary_metadata(index, category, package, version)
    if binary_metadata is None:
        return False
    try:
        built_timestamp = int(binary_metadata.get("BUILD_TIME", "").strip())
    except ValueError:
        return False
    try:
        installed_timestamp = int(_read_vdb_string(root, category, package, version, "BUILD_TIME").strip())
    except ValueError:
        return False
    if rebuilt_binaries_timestamp is not None:
        return built_timestamp > installed_timestamp and built_timestamp >= rebuilt_binaries_timestamp
    return built_timestamp != installed_timestamp


def _new_repo_changed(root, category, package, version, current_repo_name):
    """--newrepo: whether version's own vdb-recorded "repository" file
    differs from current_repo_name (the repo the caller has already
    established currently provides this exact version -- the resolved
    candidate's own "repo_name" at each of this function's own two call
    sites, not re-derived here the way _slot_changed's own re-lookup
    works, since the caller already has it in hand). Real depgraph.py:
    "--newrepo" in myopts and myeb.repo != pkg.repo /
    pkg.repo != inst_pkg.repo -- a straight repo-name comparison, no
    md5-cache re-read needed at all, unlike _slot_changed. A vdb entry
    with no "repository" file at all (real portage predates this
    tracking, or a hand-installed/synthetic entry) is treated as real
    portage.versions._unknown_repo ("__unknown__") exactly -- not
    "unchanged" the way _slot_changed/_deps_changed's own missing-data
    fallbacks work, since real portage's own comparison has no such
    tolerant fallback at all: an unrecorded repo is a real, distinct
    value ("__unknown__"), and it either equals current_repo_name or it
    doesn't, the same as any other string. Mirrors portage-repo/src/
    lib.rs's new_repo_changed exactly."""
    vdb_repo = _read_vdb_string(root, category, package, version, "repository").strip()
    if not vdb_repo:
        vdb_repo = "__unknown__"
    return vdb_repo != current_repo_name


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


def _valid_iuse(declared, config):
    """A candidate's own is_valid_flag domain: its declared IUSE unioned
    with the profile's real EAPI 5+ IUSE_EFFECTIVE (config["iuse_effective"]
    -- USE_EXPAND_IMPLICIT-derived elibc_*/kernel_*/... and IUSE_IMPLICIT
    flags). Matches real pkg.iuse.is_valid_flag, so foo[elibc_glibc]
    matches a foo that never lists elibc_glibc. Used only for a USE-dep's
    own .required/(+)/(-) check (_use_deps_satisfied) -- deliberately NOT
    for --newuse's IUSE-presence diff (which must stay strictly
    declared-IUSE). Mirrors portage-repo/src/lib.rs's valid_iuse exactly."""
    effective = config.get("iuse_effective")
    if not effective:
        return declared
    return declared | effective


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
        config["conf_use_tokens"],
        config["package_use_repo"],
        config["package_use"],
        config["package_use_user"],
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


def _installed_pkg_repo(root, category, package, version):
    """The repo an installed cat/pkg-version was merged from -- its vdb
    `repository` file's first line, or `"__unknown__"` (real
    portage.versions._unknown_repo) when absent/empty. Mirrors
    portage-repo/src/lib.rs's installed_pkg_repo."""
    repo = _read_vdb_string(root, category, package, version, "repository").strip()
    return repo or "__unknown__"


def _installed_refs(root, category, package):
    """Every installed version of cat/pkg as a dict {version, slot,
    sub_slot, repo}. Mirrors portage-repo/src/lib.rs's installed_refs."""
    return [
        {
            "version": v,
            "slot": s,
            "sub_slot": ss,
            "repo": _installed_pkg_repo(root, category, package, v),
        }
        for (v, s, ss) in installed_candidates(root, category, package)
    ]


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


def _running_root_satisfies_atom(atom_str, running_root):
    """--root-deps's own real ESYSROOT-vs-ROOT distinction, narrowed to an
    "is it already there" existence check -- see _root_deps_satisfied_atoms's
    own doc comment for the full real grounding and why a fuller,
    recursive second-root graph isn't attempted. Whether atom_str is
    satisfied by anything installed under running_root's own real vdb --
    installed_candidates, keyed directly off the atom's own parsed
    category/package (no wildcard-atom support needed: this pilot's own
    atom grammar never has an atom without an explicit category/package),
    matched via match_from_list the same way every other real
    installed-package match in this pilot works. Deliberately generic on
    running_root: this function has no idea whether it's being pointed at
    a real host "/" or a fixture's own fake vdb tree -- only the real CLI
    boundary (--root-deps's own default resolution) ever points this at
    real "/". USE-deps on the atom aren't checked against the running
    root's own recorded USE (the same simplification blocker-atom
    matching elsewhere in this pilot already makes) -- a documented v1
    scope cut. Mirrors portage-repo/src/lib.rs's own
    running_root_satisfies_atom."""
    try:
        atom = Atom(atom_str)
    except InvalidAtom:
        return False
    category, package = atom.cp.split("/", 1)
    candidates = installed_candidates(running_root, category, package)
    candidate_strs = [
        f"{atom.cp}-{version}:{slot}/{sub_slot}" for version, slot, sub_slot in candidates
    ]
    matched = match_from_list(atom_str, candidate_strs)
    return bool(matched)


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
    text,
    scalars,
    use_flags,
    use_tokens,
    accept_keywords,
    use_expand,
    use_expand_unprefixed,
    use_expand_implicit,
    iuse_implicit,
    use_expand_hidden,
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
        elif key == "USE_EXPAND_IMPLICIT":
            _apply_incremental(value, use_expand_implicit)
        elif key == "IUSE_IMPLICIT":
            _apply_incremental(value, iuse_implicit)
        elif key == "USE_EXPAND_HIDDEN":
            _apply_incremental(value, use_expand_hidden)
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


def _expand_parent_colon(
    parent, current_repo, repos, repo_aliases, parents_file, parent_colon_repos
):
    """Expands a profile "parent" file line's real cross-repo ":path"/
    "reponame:path" syntax (LocationsManager._expand_parent_colon): a
    ":" with nothing before it means "this same repo" (current_repo),
    anything else before the ":" is another repo's own name, looked up
    in repos then repo_aliases (real repositories.get_location_for_name
    is keyed on aliases too). Both forms expand to
    "<repo_location>/profiles/<rest>". A line with no ":" at all is
    returned unchanged. Real portage only allows this syntax when the
    current profile node's own repo declares profile-formats = portage-2
    in layout.conf (_allow_parent_colon gate, _config/LocationsManager.
    py:207/259). Mirrors portage-profile/src/lib.rs's expand_parent_colon
    exactly. (An atom's own "::alias" is a different thing and is NOT
    resolved -- real match_from_list does a straight pkg.repo ==
    atom.repo name comparison.)"""
    colon = parent.find(":")
    if colon == -1:
        return parent
    if current_repo is not None and current_repo[0] not in parent_colon_repos:
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
        repo_loc = next(
            (loc for name, loc in list(repos) + list(repo_aliases) if name == repo_name),
            None,
        )
        if repo_loc is None:
            raise ResolutionError(
                f'parent "{parent}" not found: {parents_file} '
                f'(no repo named "{repo_name}")'
            )
        rest = parent[colon + 1 :]
    return os.path.join(repo_loc, "profiles", rest)


def _visit_profile(directory, repos, repo_aliases, parent_colon_repos, visited, chain):
    canon = os.path.realpath(directory)
    if not os.path.isdir(canon):
        raise ResolutionError(f"resolving profile {directory}: not a directory")
    if canon in visited:
        return
    visited.add(canon)
    current_repo = _repo_containing(canon, repos)
    parents_file = os.path.join(canon, "parent")
    for parent in _read_parent_lines(canon):
        expanded = _expand_parent_colon(
            parent, current_repo, repos, repo_aliases, parents_file, parent_colon_repos
        )
        _visit_profile(
            os.path.join(canon, expanded),
            repos,
            repo_aliases,
            parent_colon_repos,
            visited,
            chain,
        )
    chain.append(canon)


def _resolve_profile_chain(leaf, repos, repo_aliases, parent_colon_repos):
    visited = set()
    chain = []
    _visit_profile(leaf, repos, repo_aliases, parent_colon_repos, visited, chain)
    return chain


def _process_make_conf_file(
    path,
    config_root,
    scalars,
    use_flags,
    conf_use_tokens,
    accept_keywords,
    use_expand,
    use_expand_unprefixed,
    use_expand_implicit,
    iuse_implicit,
    use_expand_hidden,
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
                conf_use_tokens,
                accept_keywords,
                use_expand,
                use_expand_unprefixed,
                use_expand_implicit,
                iuse_implicit,
                use_expand_hidden,
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
            conf_use_tokens.append(value)
        elif key == "ACCEPT_KEYWORDS":
            _apply_incremental(value, accept_keywords)
        elif key == "USE_EXPAND":
            _apply_incremental(value, use_expand)
        elif key == "USE_EXPAND_UNPREFIXED":
            _apply_incremental(value, use_expand_unprefixed)
        elif key == "USE_EXPAND_IMPLICIT":
            _apply_incremental(value, use_expand_implicit)
        elif key == "IUSE_IMPLICIT":
            _apply_incremental(value, iuse_implicit)
        elif key == "USE_EXPAND_HIDDEN":
            _apply_incremental(value, use_expand_hidden)
        scalars[key] = value


def resolve_config(
    config_root,
    main_repo_location,
    overlay_repos=(),
    repo_aliases=(),
    main_repo_name="",
    repo_masters=None,
):
    """Computes real USE/ACCEPT_KEYWORDS/package.mask/.unmask/
    .accept_keywords: the profile chain rooted at
    <config_root>/etc/portage/make.profile (if it exists), then
    <config_root>/etc/portage/make.conf (if it exists) as the final,
    highest-priority USE/ACCEPT_KEYWORDS layer, then package.*. Own
    implementation (not a wrapper around real config.py), mirroring
    portage-profile/src/lib.rs's resolve_config exactly -- see that
    crate's doc comment for the full algorithm and its documented scope
    cuts. Returns a dict with keys "use_flags", "use_tokens",
    "conf_use_tokens", "accept_keywords",
    "package_mask", "package_unmask", "package_accept_keywords",
    "package_use_repo", "package_use", "package_use_user",
    "system_packages", "package_provided", "use_force",
    "use_mask", "package_use_force", "package_use_mask", "use_expand",
    "use_expand_unprefixed", "use_expand_implicit", "iuse_implicit",
    "iuse_effective", "use_stable_force",
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
    multi-master chain, stays unimplemented. license_groups IS read from
    every repo's own profiles/ dir (<repo>/profiles/license_groups, main
    then each overlay) -- real LicenseManager._read_license_groups over
    LocationsManager.profile_locations (LocationsManager.py:432), which
    is [main_repo/profiles] + [overlay/profiles ...], never the
    per-profile-chain levels. An overlay's own profiles/ PROFILE
    DIRECTORY joining the active chain is still only reached via a chain
    parent file's reponame:path syntax (_expand_parent_colon).

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
    conf_use_tokens = []
    accept_keywords = set()
    use_expand = set()
    use_expand_unprefixed = set()
    use_expand_implicit = set()
    iuse_implicit = set()
    use_expand_hidden = set()
    scalars = {}

    all_repos = [(main_repo_name, main_repo_location)] + list(overlay_repos)

    # Real layout.conf profile-formats = portage-2 gate on a profile
    # "parent" line's own reponame:path/:path cross-repo syntax
    # (_config/LocationsManager.py:47/259): the set of repo names that
    # declare it. Read here directly (via _parse_layout_conf) rather than
    # threading a param through every resolve_config call site. Mirrors
    # portage-profile/src/lib.rs's resolve_config exactly.
    parent_colon_repos = {
        name
        for name, loc in all_repos
        if "portage-2" in _parse_layout_conf(loc).get("profile-formats", "").split()
    }

    make_profile = os.path.join(config_root, "etc", "portage", "make.profile")
    chain = (
        _resolve_profile_chain(make_profile, all_repos, repo_aliases, parent_colon_repos)
        if os.path.exists(make_profile)
        else []
    )
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
            use_expand_implicit,
            iuse_implicit,
            use_expand_hidden,
        )

    make_conf = os.path.join(config_root, "etc", "portage", "make.conf")
    if os.path.isfile(make_conf):
        _process_make_conf_file(
            make_conf,
            config_root,
            scalars,
            use_flags,
            conf_use_tokens,
            accept_keywords,
            use_expand,
            use_expand_unprefixed,
            use_expand_implicit,
            iuse_implicit,
            use_expand_hidden,
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
    # applied in the loop right below this one) IS now read too, as are
    # USE_EXPAND_IMPLICIT/IUSE_IMPLICIT/USE_EXPAND_VALUES_* (iuse_effective,
    # computed after the unprefixed loop). IUSE-aware _* wildcard
    # expansion (linguas_*) is done in effective_use_flags (needs a
    # candidate's own IUSE); package.use's USE_EXPAND-prefix shorthand is
    # read too. Still out of scope: USE_EXPAND_HIDDEN (display-only for
    # EAPI 5+). Mirrors portage-profile/src/lib.rs's resolve_config
    # exactly.
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
        conf_use_tokens.append(prefixed)

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
        conf_use_tokens.append(value)

    # Real EAPI 5+ IUSE_EFFECTIVE (config.py::_calc_iuse_effective) --
    # iuse_implicit, plus every USE_EXPAND_VALUES_<v> value for each
    # USE_EXPAND_UNPREFIXED var v that's also in USE_EXPAND_IMPLICIT
    # (unprefixed), plus lowercase(v)_<value> for each USE_EXPAND var v
    # that's also in USE_EXPAND_IMPLICIT. The extra domain real
    # pkg.iuse.is_valid_flag grants a candidate on top of its declared
    # IUSE, so foo[elibc_glibc] matches a foo that never lists it.
    # USE_EXPAND_HIDDEN is NOT part of this (display-only for EAPI 5+).
    # Mirrors portage-profile/src/lib.rs's resolve_config exactly.
    iuse_effective = set(iuse_implicit)
    for var in use_expand_unprefixed:
        if var not in use_expand_implicit:
            continue
        iuse_effective.update(scalars.get(f"USE_EXPAND_VALUES_{var}", "").split())
    for var in use_expand_implicit:
        if var not in use_expand:
            continue
        prefix = var.lower()
        for v in scalars.get(f"USE_EXPAND_VALUES_{var}", "").split():
            iuse_effective.add(f"{prefix}_{v}")

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

    # Real append_repo scopes EVERY repo's own repo-level package.mask/
    # .unmask, including the main repo's own -- not just an overlay's,
    # confirmed by reading MaskManager.py's own repo_pkgmasklines/
    # repo_pkgunmasklines loop ("for repo in repositories.
    # repos_with_profiles()", unconditional). Previously left the main
    # repo's own entries unscoped, a genuine, documented gap: an
    # identically-named package.mask atom from main would incorrectly
    # also mask a same-named-but-different overlay package that no mask
    # file ever actually mentions. Not the same as the profile-chain's
    # own package.mask/.unmask below (chain loop) or the user-level one
    # further down -- real MaskManager.py's own profile_pkgmasklines/
    # user_pkgmasklines never get append_repo'd at all, only the
    # repo-level ones do, so those two stay exactly as unscoped as they
    # already were.
    main_repo_mask_lines = _read_config_lines(
        os.path.join(main_repo_location, "profiles", "package.mask")
    )
    main_repo_unmask_lines = _read_config_lines(
        os.path.join(main_repo_location, "profiles", "package.unmask")
    )
    mask_sources = [_scope_repo_mask_lines(main_repo_mask_lines, main_repo_name)]
    unmask_sources = [_scope_repo_mask_lines(main_repo_unmask_lines, main_repo_name)]
    _repo_masters = repo_masters or {}
    for repo_name, repo_location in overlay_repos:
        # Real masters (repo_masters, resolved by the caller from real
        # repos.conf's own "masters = name1 name2 ..." key -- find_repos'
        # own docstring has the full real grounding, config.py:
        # 1229-1260): an overlay's own package.mask is stacked *on top
        # of* every one of its declared masters' own package.mask, in
        # declared order, before the usual "::reponame" scoping. No
        # entry for repo_name in repo_masters at all falls back to the
        # same "main repo alone" default this pilot always used before
        # "masters =" parsing existed. package.unmask deliberately does
        # NOT get the same treatment -- confirmed by reading
        # MaskManager.py's own two loops side by side: only the
        # package.mask loop iterates masters at all. Simplified from
        # real MaskManager.py's own per-master stack_lists (stacks each
        # master separately against the repo's own lines, then
        # concatenates every one of those per-master results) to one
        # flat _stack_mask_lines call over every master's lines followed
        # by the repo's own -- same simplification as
        # portage-repo/src/lib.rs's own resolve_config, see its own doc
        # comment for the full reasoning.
        masters = _repo_masters.get(repo_name) or [main_repo_location]
        mastered_lines_stack = [
            _read_config_lines(os.path.join(master_location, "profiles", "package.mask"))
            for master_location in masters
        ]
        mastered_lines_stack.append(
            _read_config_lines(os.path.join(repo_location, "profiles", "package.mask"))
        )
        mastered_mask_lines = _stack_mask_lines(mastered_lines_stack)
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

    # package.use: three real sources, each into its own config key at
    # its own real USE_ORDER position (the "Config depth" slice,
    # SCOPE_BACKLOG Part 2.C) -- see effective_use_flags for the
    # per-package walk. Mirrors portage-profile/src/lib.rs's
    # resolve_config exactly:
    #   - repo-level (every configured repo's profiles/package.use;
    #     overlays ::repo-scoped via _scope_repo_package_use_lines, main
    #     unscoped since it implicitly masters every overlay) ->
    #     package_use_repo, real configdict["repo"]. Applied *before* the
    #     ebuild's own IUSE +/- defaults -- the weakest layer modeled.
    #   - every profile level's own package.use (chain order) ->
    #     package_use, real configdict["defaults"]. Applied after the
    #     profile make.defaults USE tokens, before make.conf.
    #   - user-level /etc/portage/package.use -> package_use_user, real
    #     configdict["pkg"]. Applied after make.conf; the strongest layer
    #     before the final use.force/use.mask step. This is the only
    #     source that gets the USE_EXPAND-prefix shorthand (real
    #     user-only extended_syntax) -- see _parse_package_use_lines.
    # Still simplified: repo make.defaults USE (real _repo_make_defaults)
    # is not modeled, and profile package.use is applied as one group
    # after all profile make.defaults rather than interleaved per level.
    repo_use_lines = _read_config_lines(
        os.path.join(main_repo_location, "profiles", "package.use")
    )
    for repo_name, repo_location in overlay_repos:
        overlay_use_lines = _read_config_lines(
            os.path.join(repo_location, "profiles", "package.use")
        )
        repo_use_lines.extend(
            _scope_repo_package_use_lines(overlay_use_lines, repo_name)
        )
    profile_use_lines = []
    for level in chain:
        profile_use_lines.extend(
            _read_config_lines(os.path.join(level, "package.use"))
        )
    user_use_lines = _read_config_lines(
        os.path.join(config_root, "etc", "portage", "package.use")
    )

    # package.use.mask/package.use.force: repo-level (every repo, not
    # just main -- same ::repo-scoping the package.use bullet above now
    # applies here too) plus every profile level's own file (in chain
    # order) -- NO user-level source at all, unlike package.use:
    # confirmed by reading UseManager.__init__'s own file/variable table
    # (the "user config" section lists only "package.use -> _pusedict",
    # nothing for mask/force). Unlike package.mask/.unmask, real
    # UseManager.py never merges an overlay's own file with its master's
    # own at load time (no stack_lists-equivalent combination here at
    # all); the masters chain is only consulted later, per-package, in
    # getUseMask/getUseForce (repos = masters + [pkg.repo], each repo's
    # own already-independent dict appended in that order) -- so no
    # _stack_mask_lines-style merge is needed here either, just the same
    # scope-then-append package.use above already does. Flat-
    # concatenated the same way use_lines already is; which entry
    # actually wins when more than one matches the same candidate is
    # decided later, at application time -- see effective_use_flags's
    # own docstring (atom-specificity ordering). Mirrors
    # portage-profile/src/lib.rs's resolve_config exactly.
    use_force_lines = _read_config_lines(
        os.path.join(main_repo_location, "profiles", "package.use.force")
    )
    use_mask_lines = _read_config_lines(
        os.path.join(main_repo_location, "profiles", "package.use.mask")
    )
    for repo_name, repo_location in overlay_repos:
        overlay_force = _read_config_lines(
            os.path.join(repo_location, "profiles", "package.use.force")
        )
        use_force_lines.extend(_scope_repo_package_use_lines(overlay_force, repo_name))
        overlay_mask = _read_config_lines(
            os.path.join(repo_location, "profiles", "package.use.mask")
        )
        use_mask_lines.extend(_scope_repo_package_use_lines(overlay_mask, repo_name))
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

    # package.use.stable.mask/package.use.stable.force: repo-level
    # (every repo, ::repo-scoped, same no-masters-merge treatment
    # package.use.mask/.force just above now gets) plus every profile
    # level's own file (in chain order) -- NO user-level source at all,
    # mirroring package.use.force/.mask's own confirmed sourcing
    # exactly. No shorthand either, same reasoning.
    use_stable_force_lines = _read_config_lines(
        os.path.join(main_repo_location, "profiles", "package.use.stable.force")
    )
    use_stable_mask_lines = _read_config_lines(
        os.path.join(main_repo_location, "profiles", "package.use.stable.mask")
    )
    for repo_name, repo_location in overlay_repos:
        overlay_stable_force = _read_config_lines(
            os.path.join(repo_location, "profiles", "package.use.stable.force")
        )
        use_stable_force_lines.extend(
            _scope_repo_package_use_lines(overlay_stable_force, repo_name)
        )
        overlay_stable_mask = _read_config_lines(
            os.path.join(repo_location, "profiles", "package.use.stable.mask")
        )
        use_stable_mask_lines.extend(
            _scope_repo_package_use_lines(overlay_stable_mask, repo_name)
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

    # package.provided (real config.py:970-1027's pprovideddict),
    # flattened: every profile level's own file (chain order) + the
    # user-level /etc/portage/profile/package.provided, stacked with the
    # same stack_lists(incremental=1) `-atom` removal. See
    # portage-profile/src/lib.rs's Config::package_provided -- including
    # why the flat list (not a cp-keyed dict) is equivalent, and why the
    # real EAPI 7+ gate isn't ported.
    pprovided_sources = [
        _read_config_lines(os.path.join(level, "package.provided")) for level in chain
    ]
    pprovided_sources.append(
        _read_config_lines(
            os.path.join(config_root, "etc", "portage", "profile", "package.provided")
        )
    )
    package_provided = _stack_mask_lines(pprovided_sources)

    # license_groups: real LicenseManager._read_license_groups
    # (LicenseManager.py:47) over LocationsManager.profile_locations
    # (LocationsManager.py:432) -- the `profiles/` directory of the MAIN
    # REPO and each overlay, NOT the individual profile-chain levels
    # (real gentoo puts license_groups at <repo>/profiles/license_groups,
    # never in a profiles/<foo>/ profile dir -- verified live). Then the
    # user-level /etc/portage/license_groups. "extend, don't
    # stack/replace" -- see _parse_license_groups_lines. Mirrors
    # portage-profile/src/lib.rs's resolve_config exactly.
    license_groups = {}
    for _name, location in all_repos:
        for name, members in _parse_license_groups_lines(
            _read_config_lines(os.path.join(location, "profiles", "license_groups"))
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
        "conf_use_tokens": conf_use_tokens,
        "accept_keywords": accept_keywords,
        "package_mask": _stack_mask_lines(mask_sources),
        "package_unmask": _stack_mask_lines(unmask_sources),
        "package_accept_keywords": package_accept_keywords,
        "package_use_repo": _parse_package_use_lines(repo_use_lines),
        "package_use": _parse_package_use_lines(profile_use_lines),
        "package_use_user": _parse_package_use_lines(
            user_use_lines, use_expand_shorthand=True
        ),
        "system_packages": system_packages,
        "package_provided": package_provided,
        "use_force": use_force,
        "use_mask": use_mask,
        "archlist": archlist,
        "package_use_force": _parse_package_use_lines(use_force_lines),
        "package_use_mask": _parse_package_use_lines(use_mask_lines),
        "use_expand": use_expand,
        "use_expand_unprefixed": use_expand_unprefixed,
        "use_expand_implicit": use_expand_implicit,
        "use_expand_hidden": use_expand_hidden,
        "iuse_implicit": iuse_implicit,
        "iuse_effective": iuse_effective,
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
        # binrepos.conf + PORTAGE_BINHOST (--getbinpkg/--getbinpkgonly),
        # real lib/portage/binrepo/config.py. Mirrors
        # portage-profile/src/lib.rs's Config::binrepos.
        "binrepos": _parse_binrepos(
            _read_text_opt(os.path.join(config_root, "etc/portage/binrepos.conf")),
            scalars.get("PORTAGE_BINHOST", ""),
        ),
        # Every non-USE/ACCEPT_KEYWORDS variable's final scalar value --
        # the `scalars` map, exposed for `emerge --info`. Mirrors
        # portage-profile/src/lib.rs's Config::other_vars.
        "other_vars": dict(scalars),
    }


def _read_text_opt(path):
    try:
        with open(path) as f:
            return f.read()
    except OSError:
        return ""


def _parse_binrepos(binrepos_conf, portage_binhost):
    """Real BinRepoConfigLoader (binrepo/config.py:97-172), narrowed:
    [section] / key=value INI (only sync-uri, priority), then one
    implicit BinRepo per whitespace-separated PORTAGE_BINHOST URI not
    already a section's sync-uri (real "Convert PORTAGE_BINHOST entries
    into implicit binrepos.conf ones", reversed, incrementing priority).
    Each URI _normalize_uri'd. Sorted by (priority, name). Mirrors
    portage-profile/src/lib.rs's parse_binrepos -- see its docstring for
    the narrowings (implicit-name uses host/path not md5, no [DEFAULT]
    interpolation / exclude-include / fetchcommand / location fallback)."""
    repos = []
    seen_uris = set()
    section = None
    sync_uri = None
    priority = 0

    def flush():
        nonlocal section, sync_uri, priority
        if section is not None and sync_uri is not None:
            uri = sync_uri.rstrip("/")
            seen_uris.add(uri)
            repos.append({"name": section, "sync_uri": uri, "priority": priority})
        section, sync_uri, priority = None, None, 0

    for line in binrepos_conf.splitlines():
        line = line.strip()
        if not line or line.startswith("#") or line.startswith(";"):
            continue
        if line.startswith("[") and line.endswith("]"):
            flush()
            section = line[1:-1].strip()
            continue
        if "=" in line:
            k, _, v = line.partition("=")
            k, v = k.strip(), v.strip()
            if k == "sync-uri":
                sync_uri = v
            elif k == "priority":
                try:
                    priority = int(v)
                except ValueError:
                    priority = 0
    flush()
    repos = [r for r in repos if r["name"] != "DEFAULT"]

    current_priority = 0
    for uri in reversed(portage_binhost.split()):
        uri = uri.rstrip("/")
        if uri not in seen_uris:
            seen_uris.add(uri)
            current_priority += 1
            name = uri.split("://", 1)[1] if "://" in uri else uri
            repos.append(
                {"name": name, "sync_uri": uri, "priority": current_priority}
            )

    repos.sort(key=lambda r: (r["priority"], r["name"]))
    return repos


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
        matched = [c for c in matched if _candidate_use_deps_satisfied(atom, c, category, package, config)]
    return bool(matched)


def _candidate_use_deps_satisfied(atom, c, category, package, config):
    """`atom`'s own USE-deps checked against candidate `c` -- its
    is_valid_flag domain (_valid_iuse) and effective USE. Small shared
    helper for the two tree-candidate USE-dep filters below."""
    iuse, use_flags = _candidate_iuse_and_use(c, category, package, config)
    return _use_deps_satisfied(atom, _valid_iuse(iuse, config), use_flags)


def _root_deps_satisfied_atoms(
    metadata, use_flags, repos, config, running_root, dep_keys=("DEPEND", "BDEPEND")
):
    """--root-deps's own DEPEND/BDEPEND-vs-ESYSROOT distinction, factored
    out so both real dep-walk sites in this file (the main New/Upgrade/
    Reinstall flatten and _enqueue_dependencies's own
    AlreadyInstalled-recursion path) share one implementation rather than
    drifting apart. Reads metadata's own dep_keys (real ("DEPEND",
    "BDEPEND", "IDEPEND") at both ordinary dep-walk sites -- DEPEND/BDEPEND
    are the classic ESYSROOT build deps, and IDEPEND always targets the
    running root for every package, not just recursed build entries --
    depgraph.py:4247-4252), flattens
    them the exact same way (use_flags/repos/config) the caller already
    flattened its own combined dep string with, *except* for one
    deliberate branch-selection difference: the disjunctive ("||")
    closure passed to _use_reduce_flat_disjunctive here accepts a branch
    when every atom in it is either ordinarily satisfiable
    (_atom_currently_satisfiable, tree-visibility) *or* running-root-
    satisfied (_running_root_satisfies_atom) -- so a DEPEND/BDEPEND "||"
    group with no branch visible in the fixture tree at all still
    flattens correctly here as long as some branch is already installed
    on the running root, matching real portage's own effective behavior.
    Returns only the tokens satisfied by running_root's own real vdb
    (_running_root_satisfies_atom) -- callers drop these from their own
    already-flattened flat_deps before queueing (real "no separate graph
    node needed for an already-satisfied dep"). Degrades to an empty set
    on any flatten failure -- never a false negative that could silently
    drop a dep this pilot actually needed to walk. Mirrors
    portage-repo/src/lib.rs's root_deps_satisfied_atoms exactly."""
    build_depstr = " ".join(metadata[k] for k in dep_keys if metadata.get(k))
    try:
        build_flat = _use_reduce_flat_disjunctive(
            build_depstr,
            use_flags,
            lambda atoms: all(
                _atom_currently_satisfiable(repos, a, config)
                or _running_root_satisfies_atom(a, running_root)
                for a in atoms
            ),
        )
    except InvalidDependString:
        return set()
    return {
        tok
        for tok in build_flat
        if tok != "||" and _running_root_satisfies_atom(tok, running_root)
    }


def _unsatisfied_root_deps_atoms(
    metadata, use_flags, repos, config, running_root, dep_keys=("DEPEND", "BDEPEND")
):
    """The complement of _root_deps_satisfied_atoms: real DEPEND/BDEPEND
    atoms (or, recursing into a package that is *itself* being built
    against the running root, RDEPEND + IDEPEND too -- see dep_keys and
    _resolve_root_deps_build_entries's own docstring) that flatten out of
    metadata but are *not* already satisfied by running_root's own vdb --
    the set real portage would need to recursively resolve (and
    potentially build) against the running root itself, rather than the
    target ROOT. dep_keys is ("DEPEND", "BDEPEND", "IDEPEND") at the two
    ordinary dep-walk sites (DEPEND/BDEPEND-vs-ESYSROOT, plus IDEPEND
    which always targets the running root for every package), and
    ("DEPEND", "BDEPEND", "RDEPEND", "IDEPEND") when
    recursing into an already-targets_running_root entry (real
    _add_pkg_deps's deps tuple: a package whose own pkg.root is the
    running root has its RDEPEND resolved there too, and IDEPEND always
    targets the running root regardless -- depgraph.py:4247-4252). A
    blocker atom is never a real build target, so
    it's excluded here the same way _enqueue_flat_deps/
    _enqueue_dependencies already exclude one from their own ordinary
    queueing. Computed as its own separate flatten (duplicating
    _root_deps_satisfied_atoms's own work) rather than refactoring that
    already-shipped function to return both halves at once. Mirrors
    portage-repo/src/lib.rs's unsatisfied_root_deps_atoms exactly."""
    build_depstr = " ".join(metadata[k] for k in dep_keys if metadata.get(k))
    try:
        build_flat = _use_reduce_flat_disjunctive(
            build_depstr,
            use_flags,
            lambda atoms: all(
                _atom_currently_satisfiable(repos, a, config)
                or _running_root_satisfies_atom(a, running_root)
                for a in atoms
            ),
        )
    except InvalidDependString:
        return []
    result = []
    for tok in build_flat:
        if tok == "||":
            continue
        dep_atom = _parse_atom(tok)
        if dep_atom is not None and dep_atom.blocker:
            continue
        if _running_root_satisfies_atom(tok, running_root):
            continue
        result.append(tok)
    return result


def _resolved_version_meta_and_use(repos, category, package, version, config):
    """Metadata (md5-cache) and effective USE flags for category/package's
    own `version`, resolved against `repos` -- the highest-repo_priority
    candidate providing that exact version (the same re-lookup
    _slot_changed/_deps_changed already use). None if the version is no
    longer in the tree or its metadata is unreadable. Used by
    _resolve_root_deps_build_entries to walk a freshly-pulled running-root
    build entry's own dependency strings with that package's *own*
    effective USE. Mirrors portage-repo/src/lib.rs's
    resolved_version_meta_and_use exactly."""
    candidates = [c for c in list_candidates(repos, category, package) if c["version"] == version]
    if not candidates:
        return None
    resolved = max(candidates, key=lambda c: c["repo_priority"])
    try:
        metadata = read_md5_cache(
            resolved["repo_location"], category, f"{package}-{version}"
        )
    except OSError:
        return None
    _iuse, use_flags = _candidate_iuse_and_use(resolved, category, package, config)
    return (metadata, use_flags)


def _slot_conflict_meta(repos, category, package, version):
    """(sub_slot, repo_name, slot) of category/package's own `version` --
    the highest-repo_priority candidate carrying that exact version. All
    empty strings if the version is no longer in any repo. Slice 4's
    slot-collision block. Mirrors portage-repo/src/lib.rs's
    slot_conflict_meta exactly."""
    cands = [c for c in list_candidates(repos, category, package) if c["version"] == version]
    if not cands:
        return ("", "", "")
    c = max(cands, key=lambda c: c["repo_priority"])
    return (c["sub_slot"], c["repo_name"], c["slot"])


def _build_slot_conflict(
    repos, category, package, slot, existing_version, current_atom, current_version, slot_pullers
):
    """Assembles a slot_conflicts entry (real slot_collision_handler's
    (pkg, parent_atoms) per slot_atom): instance A is `existing_version`
    (already in the graph), instance B is `current_version`. Each
    slot_pullers entry for cat/pkg is filed under whichever instance its
    atom matches (under A when it matches both). Mirrors
    portage-repo/src/lib.rs's build_slot_conflict exactly."""
    a_sub, a_repo, _ = _slot_conflict_meta(repos, category, package, existing_version)
    b_sub, b_repo, _ = _slot_conflict_meta(repos, category, package, current_version)
    a_match = f"{category}/{package}-{existing_version}:{slot}"
    b_match = f"{category}/{package}-{current_version}:{slot}"

    def _puller_cpv(pc, pp, pv):
        if not pc:
            return ""
        psub, prepo, pslot = _slot_conflict_meta(repos, pc, pp, pv)
        return f"{pc}/{pp}-{pv}:{pslot}/{psub}::{prepo}"

    parents_a = []
    parents_b = []
    for pc, pp, pv, atom in slot_pullers.get((category, package), []):
        hits_a = bool(match_from_list(atom, [a_match]))
        hits_b = bool(match_from_list(atom, [b_match]))
        entry = [_puller_cpv(pc, pp, pv), atom]
        if hits_a:
            if entry not in parents_a:
                parents_a.append(entry)
        elif hits_b and entry not in parents_b:
            parents_b.append(entry)
    return {
        "category": category,
        "package": package,
        "slot": slot,
        "resolved_version": existing_version,
        "conflicting_atom": current_atom,
        "instances": [
            {
                "version": existing_version,
                "sub_slot": a_sub,
                "repo_name": a_repo,
                "parents": parents_a,
            },
            {
                "version": current_version,
                "sub_slot": b_sub,
                "repo_name": b_repo,
                "parents": parents_b,
            },
        ],
    }


def _resolve_root_deps_build_entries(repos, running_root, atom_str, config, owner, seen):
    """Real "recursively pull in and build new packages against the
    running root" (--root-deps, depgraph.py:4207-4271). Resolves atom_str
    against running_root the same way any dependency atom is resolved
    (reusing resolve_pretend wholesale, is_top_level=False/selective=True,
    usepkg/usepkgonly both False), then walks the resolved package's *own*
    DEPEND + BDEPEND + RDEPEND + IDEPEND against the running root too,
    recursively -- real portage resolves all four of those against the
    running root when pkg.root is the running root (IDEPEND always,
    regardless -- depgraph.py:4247-4252). PDEPEND stays a target-ROOT
    concern and is not walked here.

    `seen` (the shared root_deps_build_seen set) is both the cross-package
    dedup key *and* the cycle guard: a (category, package) is added
    *before* its own deps are walked, so mutual BDEPENDs terminate
    cleanly. One required_by edge is lost wherever a cycle is cut (a
    documented, bounded imprecision).

    Per outcome: new/upgrade/downgrade/reinstall -> a real entry
    (targets_running_root=True, required_by naming the *immediate*
    requester) plus the recursion; no_visible_candidate -> a real
    no_visible_candidate entry too (so an unbuildable, not-installed build
    dep is surfaced by the renderer's own "!!! no visible ebuild" note as
    it is without --root-deps), no recursion; already_installed ->
    nothing, no recursion. Mirrors portage-repo/src/lib.rs's
    resolve_root_deps_build_entries exactly."""
    atom = _parse_atom(atom_str)
    if atom is None:
        return []
    category, package = atom.cp.split("/", 1)
    key = (category, package)
    if key in seen:
        return []
    seen.add(key)
    outcome = resolve_pretend(
        repos,
        running_root,
        atom_str,
        config,
        with_bdeps=True,
        selective=True,
        is_top_level=False,
    )

    if outcome[0] in ("new", "reinstall"):
        recurse_version = outcome[1]
    elif outcome[0] in ("upgrade", "downgrade"):
        recurse_version = outcome[2]
    elif outcome[0] == "no_visible_candidate":
        recurse_version = None
    else:  # already_installed
        return []

    # usepkg/usepkgonly are both False in the resolve_pretend call above,
    # so outcome can only ever have come from an ebuild candidate.
    result = [
        (
            category,
            package,
            outcome,
            [],
            None,
            [],
            [owner],
            "ebuild",
            {"mask_entry": None, "unmask_entry": None, "keyword_entry": None},
            None,
            None,
            None,
            True,
        )
    ]

    if recurse_version is not None:
        meta_and_use = _resolved_version_meta_and_use(
            repos, category, package, recurse_version, config
        )
        if meta_and_use is not None:
            metadata, use_flags = meta_and_use
            for dep_atom in _unsatisfied_root_deps_atoms(
                metadata,
                use_flags,
                repos,
                config,
                running_root,
                dep_keys=("DEPEND", "BDEPEND", "RDEPEND", "IDEPEND"),
            ):
                result.extend(
                    _resolve_root_deps_build_entries(
                        repos, running_root, dep_atom, config, key, seen
                    )
                )

    return result


def _dependency_avoid_update_candidate(root, atom, atom_str, category, package, candidates, installed, config):
    """Real `_select_pkg_highest_available_imp`'s own early avoid_update
    return for a DEPENDENCY atom (`lib/_emerge/depgraph.py` ~8440: "if
    inst_pkg is not None and parent is not None and not self.
    _want_update_pkg(parent, inst_pkg): return inst_pkg") -- the highest
    installed version of category/package that matches atom_str
    (version/slot/repo, via the FULL candidates list, deliberately NOT
    is_visible-filtered) and, if the atom carries a USE-dep
    (pkg[flag]), satisfies it against that version's own real, installed
    vdb USE/IUSE -- NOT the current tree's, matching real
    _iter_match_pkgs's own vardb-sourced USE-dep check for an already-
    installed package. The valid-flag domain for that check follows real
    dbapi._iuse_implicit_cnstr for a built package (recorded IUSE |
    profile IUSE_EFFECTIVE | the package's own recorded USE, bug 640318 --
    see the inline comment). None when no installed version qualifies.

    `installed` is the vdb's own (version, slot) pairs: a repo candidate
    counts only when that exact (version, slot) pair is installed -- real
    vardb.match(atom) returns installed cpvs that match the atom's slot
    too, so cat/pkg-1.0 installed only at slot 0 never satisfies a
    cat/pkg:2 dependency even when the repo offers cat/pkg-1.0 in slot 2.
    Called
    from two places in resolve_pretend below: once before visibility/
    USE-dep filtering against the tree even begins (so a dependency
    reached only via a keyword-masked-but-installed version never
    spuriously hits no_visible_candidate in the first place), and once
    more from the ordinary "not update" shortcut further down (the only
    place this can still matter once --exclude is in play). Mirrors
    portage-repo/src/lib.rs's dependency_avoid_update_candidate
    exactly."""
    all_candidate_strs = [
        f"{category}/{package}-{c['version']}:{c['slot']}/{c['sub_slot']}::{c['repo_name']}"
        for c in candidates
    ]
    all_by_str = dict(zip(all_candidate_strs, candidates))
    all_matched = [
        all_by_str[m] for m in match_from_list(atom_str, all_candidate_strs) if m in all_by_str
    ]
    installed_matched = [
        c for c in all_matched if (c["version"], c["slot"]) in installed
    ]
    if atom.use:

        def _built_use_dep_ok(c):
            vdb_iuse = _read_vdb_flag_set(root, category, package, c["version"], "IUSE")
            vdb_use = _read_vdb_flag_set(root, category, package, c["version"], "USE")
            # Real dbapi._iuse_implicit_cnstr for a *built* package on an
            # EAPI 5+ (iuse_effective): valid-flag domain = recorded IUSE
            # | profile IUSE_EFFECTIVE (_valid_iuse) | the package's own
            # recorded USE (_iuse_implicit_built's `flag in use` clause,
            # bug 640318 -- a built package's USE is authoritative,
            # independent of the profile's current IUSE_IMPLICIT / an
            # ebuild that has since dropped a flag from IUSE). Real
            # _match_use recomputes this rather than reading a vdb
            # IUSE_EFFECTIVE file. Mirrors pretend.rs.
            valid = _valid_iuse(vdb_iuse, config) | vdb_use
            return _use_deps_satisfied(atom, valid, vdb_use)

        installed_matched = [c for c in installed_matched if _built_use_dep_ok(c)]
    if not installed_matched:
        return None
    return _best_candidate(installed_matched)


def _already_installed_or_reinstall(
    root,
    repos,
    config,
    category,
    package,
    installed_best,
    newuse,
    changed_use,
    changed_deps,
    with_bdeps,
    changed_slot,
    usepkg,
    usepkgonly,
    rebuilt_binaries,
    rebuilt_binaries_timestamp,
    newrepo,
    empty=False,
):
    """Shared by both of resolve_pretend's own "not update" shortcut call
    sites: once an installed version has been chosen to keep, decides
    between already_installed and reinstall exactly the same way.
    `empty` (--emptytree) forces a bare "reinstall" instead of
    "already_installed". Mirrors portage-repo/src/lib.rs's
    already_installed_or_reinstall exactly."""
    changed_flags = (
        _reinstall_flags_for_use_change(root, category, package, installed_best, config, newuse)
        if newuse or changed_use
        else None
    ) or []
    deps_changed_flag = changed_deps and _deps_changed(
        root, repos, category, package, installed_best["version"], with_bdeps
    )
    slot_changed_flag = changed_slot and _slot_changed(
        root, repos, category, package, installed_best["version"]
    )
    rebuilt_binary_flag = (usepkg or usepkgonly) and rebuilt_binaries and _rebuilt_binary_changed(
        root, _local_binpkg_index(config), category, package, installed_best["version"], rebuilt_binaries_timestamp
    )
    new_repo_flag = newrepo and _new_repo_changed(
        root, category, package, installed_best["version"], installed_best["repo_name"]
    )
    if (
        empty
        or changed_flags
        or deps_changed_flag
        or slot_changed_flag
        or rebuilt_binary_flag
        or new_repo_flag
    ):
        return (
            "reinstall",
            installed_best["version"],
            changed_flags,
            deps_changed_flag,
            slot_changed_flag,
            rebuilt_binary_flag,
            new_repo_flag,
            # slot_operator_rebuild -- a separate post-resolution pass
            # (_slot_operator_rebuild_entries in run()), never a property
            # of the resolved candidate itself.
            False,
        )
    return ("already_installed", installed_best["version"])


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
    usepkg_exclude=(),
    usepkg_include=(),
    rebuilt_binaries=False,
    rebuilt_binaries_timestamp=None,
    newrepo=False,
    empty=False,
    getbinpkg=False,
    autounmask_keywords=False,
    autounmask_use=False,
    autounmask_license=False,
    autounmask_masks=False,
    extra_constraints=(),
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
    `empty` (--emptytree/-e, real create_depgraph_params.py:176-179):
    clears `selective` locally and turns every "already installed at the
    resolved version" result into a bare "reinstall" -- the caller
    (_resolve_pretend_graph) also forces `deep` on, so the whole deep
    tree is re-merged. Mirrors portage-repo/src/lib.rs's resolve_pretend
    exactly."""
    # Real create_depgraph_params.py:179: --emptytree does
    # myparams.pop("selective", None).
    selective = selective and not empty
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
        local_index = _local_binpkg_index(config)
        binary_candidates = list_binary_candidates(local_index, category, package)
        if getbinpkg:
            binary_candidates = binary_candidates + _list_remote_binary_candidates(
                config.get("binrepos", []), root, local_index, category, package
            )
        candidates = candidates + _filter_usepkg_exclude_include(
            binary_candidates, category, package, usepkg_exclude, usepkg_include
        )

    # Real avoid_update's own EARLY return for a dependency atom (see
    # _dependency_avoid_update_candidate's own docstring for the full
    # citation) genuinely happens before real portage ever tries to
    # find a "best available" candidate at all -- so it's checked here
    # too, before this pilot's own visibility/USE-dep-against-the-tree
    # filtering below gets a chance to (wrongly) bail out with
    # no_visible_candidate for an atom whose installed version already
    # satisfies it. Confirmed live: sys-fs/fuse's own real
    # sys-libs/liburing:=[abi_x86_64(-)?,...] dependency needs exactly
    # this -- the tree's only *visible* liburing candidate doesn't even
    # have the right USE profile to satisfy the atom (nothing enables
    # it there), while the real, installed version does (its own real
    # vdb USE). --exclude deliberately keeps this pilot's own
    # pre-existing, narrower behavior instead (see the later,
    # is_top_level-aware "not update" shortcut's own comment) --
    # skipped here so that block still gets a chance to run. Mirrors
    # portage-repo/src/lib.rs's resolve_pretend exactly.
    if not update and not is_top_level and not excluded:
        installed = {
            (v, s) for v, s, _sub in installed_candidates(root, category, package)
        }
        installed_best = _dependency_avoid_update_candidate(
            root, atom, atom_str, category, package, candidates, installed, config
        )
        if installed_best is not None:
            return _already_installed_or_reinstall(
                root,
                repos,
                config,
                category,
                package,
                installed_best,
                newuse,
                changed_use,
                changed_deps,
                with_bdeps,
                changed_slot,
                usepkg,
                usepkgonly,
                rebuilt_binaries,
                rebuilt_binaries_timestamp,
                newrepo,
                empty,
            )

    visible = [c for c in candidates if is_visible(c, category, package, config)]
    if not visible and autounmask_keywords:
        # Real --autounmask: a candidate masked by KEYWORDS alone becomes
        # visible via the implicit `=cpv ~arch` change (see portage-repo's
        # resolve_pretend `autounmask_keywords` param). Everything else
        # (package.mask/license/properties/restrict) still has to pass.
        visible = [
            c
            for c in candidates
            if _keyword_masked_only(c, category, package, config)
        ]
    if not visible and autounmask_license:
        # Real --autounmask-license: a candidate masked by LICENSE alone
        # becomes visible via the implicit package.license accept. Order
        # among the three *_masked_only fallbacks is irrelevant (each
        # requires the other reasons to pass).
        visible = [
            c
            for c in candidates
            if _license_masked_only(c, category, package, config)
        ]
    if not visible and autounmask_masks:
        # Real --autounmask-keep-masks=n: a candidate masked by
        # package.mask alone becomes visible via the implicit
        # package.unmask entry.
        visible = [
            c
            for c in candidates
            if _mask_masked_only(c, category, package, config)
        ]
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

    # Slot-conflict reconciliation (real `_process_slot_conflicts` /
    # `_select_pkg`'s "all the atoms pulling this slot", not just the
    # first): when `resolve_pretend_graph` re-resolves a package that hit
    # a slot conflict, it passes every other parent atom that targeted the
    # same `cat/pkg:slot` here -- the winning candidate must satisfy all of
    # them at once. An entry beginning with "!" is a *negative* constraint
    # (real backtracking's runtime_pkg_mask): the winning candidate must
    # NOT match the atom after the "!". An empty list (the default at every
    # ordinary call site) is a strict no-op. Mirrors portage-repo/src/lib.rs
    # exactly.
    if extra_constraints:

        def _con_ok(c, con):
            candidate_str = [
                f"{category}/{package}-{c['version']}:{c['slot']}"
                f"/{c['sub_slot']}::{c['repo_name']}"
            ]
            if con.startswith("!"):
                return not match_from_list(con[1:], candidate_str)
            return bool(match_from_list(con, candidate_str))

        matched = [
            c for c in matched if all(_con_ok(c, con) for con in extra_constraints)
        ]

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
            if _candidate_use_deps_satisfied(atom, c, category, package, config)
            or (
                autounmask_use
                and _suggested_use_flip(c, category, package, atom, config) is not None
            )
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
                config["conf_use_tokens"],
                config["package_use_repo"],
                config["package_use"],
                config["package_use_user"],
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

    # installed_pairs carries each installed version's own main slot, so
    # "is this candidate already installed" is answered the way real
    # output.py::_get_installed_best does -- against
    # vardb.match(pkg.slot_atom) (the resolved candidate's own main
    # slot), not merely "this version exists in some slot". Without the
    # slot filter, `emerge -p cat/foo:1` with only foo:0 installed
    # mis-classifies a new-slot install as an upgrade/downgrade (real
    # portage: "[ebuild NS]"). Mirrors portage-repo/src/lib.rs's
    # resolve_pretend exactly.
    installed_pairs = installed_candidates(root, category, package)
    installed = {(v, s) for v, s, _sub in installed_pairs}

    def _candidate_is_installed(c):
        # In-slot only, at that version, sub-slot ignored (real
        # pkg.slot_atom).
        return any(
            version == c["version"] and slot == c["slot"]
            for version, slot, _sub_slot in installed_pairs
        )

    # --exclude/-X: an installed version matching an exclude atom is
    # left exactly as-is, unconditionally, before --update/--newuse/
    # --changed-use ever get a say -- see this function's own docstring.
    if excluded:
        installed_matched = [c for c in matched if _candidate_is_installed(c)]
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

    # --update/-u: see this function's own docstring. Skipped entirely
    # for a top-level atom without selective, so version selection
    # falls through to the ordinary best-visible-candidate comparison
    # below too.
    #
    # For a DEPENDENCY atom, the early, pre-visibility-filtering
    # shortcut above (_dependency_avoid_update_candidate) already
    # handles the common case (see its own docstring for the full real-
    # portage citation). It deliberately skips when --exclude is
    # active, though, to preserve this pilot's own pre-existing
    # --exclude-vs-matched interaction exactly -- so this block still
    # needs its own not-is_top_level branch, reusing the same broader
    # (not is_visible-filtered) lookup, for that one remaining
    # combination. Mirrors portage-repo/src/lib.rs's resolve_pretend
    # exactly.
    if not update and (not is_top_level or selective):
        if not is_top_level:
            installed_best = _dependency_avoid_update_candidate(
                root, atom, atom_str, category, package, candidates, installed, config
            )
        else:
            installed_matched = [c for c in matched if _candidate_is_installed(c)]
            installed_best = _best_candidate(installed_matched) if installed_matched else None
        if installed_best is not None:
            return _already_installed_or_reinstall(
                root,
                repos,
                config,
                category,
                package,
                installed_best,
                newuse,
                changed_use,
                changed_deps,
                with_bdeps,
                changed_slot,
                usepkg,
                usepkgonly,
                rebuilt_binaries,
                rebuilt_binaries_timestamp,
                newrepo,
                empty,
            )

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

    if _candidate_is_installed(best):
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
        rebuilt_binary_flag = (usepkg or usepkgonly) and rebuilt_binaries and _rebuilt_binary_changed(
            root, config["pkgdir"], category, package, best["version"], rebuilt_binaries_timestamp
        )
        new_repo_flag = newrepo and _new_repo_changed(
            root, category, package, best["version"], best["repo_name"]
        )
        # is_top_level and not selective: real portage's own bare,
        # reasonless "[ebuild R]" -- see this function's own docstring's
        # selective/is_top_level paragraph. changed_flags/
        # deps_changed_flag/slot_changed_flag/rebuilt_binary_flag/
        # new_repo_flag may all still be empty/false here; that's the
        # whole point of this case.
        if (
            empty
            or changed_flags
            or deps_changed_flag
            or slot_changed_flag
            or rebuilt_binary_flag
            or new_repo_flag
            or (is_top_level and not selective)
        ):
            return (
                "reinstall",
                best["version"],
                changed_flags,
                deps_changed_flag,
                slot_changed_flag,
                rebuilt_binary_flag,
                new_repo_flag,
                False,
            )
        return ("already_installed", best["version"])
    # Upgrade/downgrade/new is decided against only what's installed in
    # best's own main slot (real _get_installed_best's myinslotlist =
    # vardb.match(pkg.slot_atom)). An installed version in a different
    # slot never makes this a downgrade/upgrade -- it's a "new" into a
    # fresh slot (the renderer's "[ebuild NS]", see the graph builder's
    # provenance["new_slot"]).
    installed_in_slot = [
        version for version, slot, _sub_slot in installed_pairs if slot == best["slot"]
    ]
    if installed_in_slot:
        current = _max_version(installed_in_slot)
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
        for (
            category,
            package,
            outcome,
            _blockers,
            slot,
            _use_display,
            _required_by,
            _source,
            _provenance,
            _keyword_suggestion,
            _use_suggestion,
            _parent_use_suggestion,
            _targets_running_root,
        ) in entries:
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


def _enqueue_flat_deps(flat_deps, key, version, depth, parent_use, queue, pending_blockers):
    """Queues every atom in `flat_deps` (a use_reduce(flat=True) result,
    with or without `subset`) onto `queue` at `depth + 1`, owned by
    `key`/`version`, splitting off a blocker atom into `pending_blockers`
    instead -- shared by resolve_pretend_graph's own normal-deps queueing
    and its --with-test-deps follow-up, so the two can't drift apart on
    blocker handling or depth/owner bookkeeping.

    `parent_use` (the owning package's own already-computed effective
    USE -- the exact same set passed as use_reduce's own `uselist`
    argument for this same dependency string) evaluates each token's own
    PMS 8.3.4 conditional use-deps (flag?/!flag?/flag=/!flag=) before
    it's ever queued or classified as a blocker, via the real Atom
    class's own evaluate_conditionals (this pilot's Python side uses the
    real portage.dep.Atom directly, so this is genuinely the same
    mechanism real use_reduce's own per-token integration point uses
    -- lib/portage/dep/__init__.py:1045-1046, confirmed by reading it
    -- not a reimplementation the way the Rust side needed). Applied
    uniformly to every token, blockers included (a blocker atom can
    syntactically carry use-deps too, e.g. "!foo/bar[baz=]").
    evaluate_conditionals is a safe no-op for an atom with no
    conditional use-deps at all (real Atom.evaluate_conditionals's own
    "if not (self.use and self.use.conditional): return self" guard).
    Mirrors portage-repo/src/lib.rs's enqueue_flat_deps exactly."""
    for tok in flat_deps:
        if tok == "||":
            continue
        unevaluated = None
        dep_atom = _parse_atom(tok)
        if dep_atom is not None:
            original_atom = dep_atom
            dep_atom = dep_atom.evaluate_conditionals(set(parent_use))
            tok = str(dep_atom)
            # Real Atom.evaluate_conditionals is a no-op (returns self)
            # when there's no conditional use-dep at all -- only a
            # genuine rewrite constructs a new Atom with its own
            # unevaluated_atom pointing back at the original. This is
            # exactly real _show_unsatisfied_dep's own
            # "atom.unevaluated_atom" -- --autounmask-use's own opt?/
            # REQUIRED_USE-conditional suggestion mechanism (see
            # _suggested_parent_use_candidate's own docstring) needs it
            # to recover the original conditional form after evaluation
            # has already replaced it in the queued atom text itself.
            if dep_atom is not original_atom:
                unevaluated = str(dep_atom.unevaluated_atom)
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
        queue.append((tok, depth + 1, key, unevaluated))


def _autounmask_dep_chain(owner, current_atom, top_level, entries):
    """Real _get_dep_chain_as_comment (depgraph.py:6457), narrowed to
    this pilot's one-level-parent tracking: the `#required by ...` lines
    for an autounmask change on `owner`'s dependency (or a top-level
    `current_atom`). A top-level atom yields a single `required by <atom>
    (argument)`; a dependency yields `required by <parent cpv>::<repo>`
    and, when that parent is itself a command-line argument, a trailing
    `required by <parent atom> (argument)`. Mirrors
    portage-repo/src/lib.rs's autounmask_dep_chain. The list holds the
    text after `# ` (the renderer adds the prefix)."""
    if owner is None:
        return [f"required by {current_atom} (argument)"]
    oc, op = owner
    chain = []
    parent = next(
        (e for e in entries if e[0] == oc and e[1] == op),
        None,
    )
    if parent is not None:
        po = parent[2]
        pv = None
        if po[0] in ("new", "reinstall"):
            pv = po[1]
        elif po[0] in ("upgrade", "downgrade"):
            pv = po[2]
        if pv is not None:
            prepo = parent[8].get("repo_name") if isinstance(parent[8], dict) else None
            if prepo:
                chain.append(f"required by {oc}/{op}-{pv}::{prepo}")
            else:
                chain.append(f"required by {oc}/{op}-{pv}")
    arg = next(
        (
            t
            for t in top_level
            if (_parse_atom(t) is not None and _parse_atom(t).cp == f"{oc}/{op}")
        ),
        None,
    )
    if arg is not None:
        chain.append(f"required by {arg} (argument)")
    if not chain:
        chain.append(f"required by {oc}/{op}")
    return chain


def _check_if_latest_atom_form(
    resolved, all_candidates, category, package, config, check_visibility
):
    """Real _display_autounmask's check_if_latest(pkg,
    check_visibility=...) (depgraph.py:10649), for an autounmask change's
    left-hand atom: `>=<cpv>` / `>=<cpv>:<slot>` / `=<cpv>`. Real portage
    uses `>=` for USE AND license changes (unlike keywords' always-`=`,
    bug #536392). check_visibility=True for USE (a use-masked-only
    candidate is still is_visible), False for license (every higher build
    counts). Mirrors portage-repo/src/lib.rs's check_if_latest_atom_form."""
    cpv = f"{category}/{package}-{resolved['version']}"

    def higher(same_slot):
        return any(
            (not same_slot or c["slot"] == resolved["slot"])
            and vercmp(c["version"], resolved["version"]) > 0
            and (not check_visibility or is_visible(c, category, package, config))
            for c in all_candidates
        )

    if not higher(False):
        return f">={cpv}"
    if not higher(True):
        return f">={cpv}:{resolved['slot']}"
    return f"={cpv}"


def _autounmask_use_atom_form(resolved, all_candidates, category, package, config):
    """check_visibility=True form (USE changes) -- see
    _check_if_latest_atom_form."""
    return _check_if_latest_atom_form(
        resolved, all_candidates, category, package, config, True
    )


def _slot_operator_rebuild_entries(root, repos, entries):
    """Real depgraph's _slot_operator_trigger_reinstalls +
    _slot_operator_replace_installed (the
    @__auto_slot_operator_replace_installed__ set), single-pass v1: an
    installed package whose vdb *DEPEND carries a built slot-operator
    atom (cat/pkg:S/SS=) whose bound S/SS no longer matches how this run
    leaves cat/pkg in that same slot is scheduled for a reinstall.
    Returns (new_entries, abi_rebuilds), the latter being the sorted,
    deduped (provider-cpv, consumer-cpv) pairs real _compute_abi_rebuild_
    info records for _show_abi_rebuild_info. Mirrors portage-repo/src/
    lib.rs's slot_operator_rebuild_entries exactly (v1 cuts and all)."""
    new_slot = {}
    in_graph = set()
    for entry in entries:
        category, package, outcome = entry[0], entry[1], entry[2]
        tag = outcome[0]
        if tag not in ("already_installed", "no_visible_candidate"):
            in_graph.add((category, package))
        if tag in ("upgrade", "downgrade", "reinstall"):
            version = outcome[2] if tag in ("upgrade", "downgrade") else outcome[1]
            matching = [
                c for c in list_candidates(repos, category, package) if c["version"] == version
            ]
            if not matching:
                continue
            resolved = max(matching, key=lambda c: c["repo_priority"])
            new_slot[(category, package)] = (version, resolved["slot"], resolved["sub_slot"])
    if not new_slot:
        return [], []

    out = []
    abi_rebuilds = []
    for category, package, version, _slot in _all_installed_packages(root):
        if (category, package) in in_graph:
            continue
        consumer_cpv = f"{category}/{package}-{version}"
        providers = set()
        for key in ("RDEPEND", "PDEPEND", "DEPEND", "BDEPEND", "IDEPEND"):
            for tok in _read_vdb_string(root, category, package, version, key).split():
                try:
                    atom = Atom(tok, allow_repo=True)
                except Exception:
                    continue
                if atom.slot_operator != "=" or atom.slot is None or atom.sub_slot is None:
                    continue
                ns = new_slot.get(tuple(atom.cp.split("/", 1)))
                if ns is not None and atom.slot == ns[1] and atom.sub_slot != ns[2]:
                    providers.add(f"{atom.cp}-{ns[0]}")
        if not providers:
            continue
        for provider_cpv in providers:
            abi_rebuilds.append((provider_cpv, consumer_cpv))
        slot, sub_slot = _read_vdb_slot(root, category, package, version)
        repo = _read_vdb_string(root, category, package, version, "repository").strip()
        outcome = ("reinstall", version, [], False, False, False, False, True)
        out.append(
            (
                category,
                package,
                outcome,
                [],
                slot,
                [],
                [],
                "ebuild",
                {"mask_entry": None, "unmask_entry": None, "keyword_entry": None},
                None,
                None,
                None,
                False,
            )
        )
    out.sort(key=lambda e: (e[0], e[1]))
    abi_rebuilds = sorted(set(abi_rebuilds))
    return out, abi_rebuilds


def _topological_merge_order(entries):
    """Put `entries` in real portage's dependency-first *merge* order.

    Real portage's `mylist` is a genuine topological merge schedule (its
    Scheduler runs every install after the installs it depends on). This
    reference has no scheduler, but each entry carries required_by (the
    (category, package) of every entry that pulled it in) -- the reverse
    of a dependency edge -- which is enough to order the list.

    A stable topological sort: an entry is emitted only once every other
    entry it requires (within this set) has already been emitted; among
    the entries that are all currently emittable, the one with the
    earliest original (BFS-discovery, i.e. argv) position goes first. Two
    packages with no dependency relationship keep their discovery order;
    a dependency always precedes the packages that pull it in.

    A genuine dependency cycle (real portage's Scheduler breaks these
    with priority heuristics this reference doesn't reproduce) is left in
    discovery order: when no unplaced entry has all its in-set
    dependencies placed, the earliest still-unplaced one is emitted
    anyway. Mirrors portage-repo/src/lib.rs's topological_merge_order
    exactly.
    """
    n = len(entries)
    if n < 2:
        return entries
    # (category, package) -> every entry index with that cp (a multi-slot
    # package has one entry per resolved slot).
    cp_indices = {}
    for i, e in enumerate(entries):
        cp_indices.setdefault((e[0], e[1]), []).append(i)
    # requires[i] = the entries i depends on, i.e. every j whose
    # required_by (tuple index 6) names i's own cp. j precedes i.
    requires = [set() for _ in range(n)]
    for j, e in enumerate(entries):
        for owner in e[6]:
            for i in cp_indices.get(tuple(owner), ()):
                if i != j:
                    requires[i].add(j)
    placed = [False] * n
    order = []
    while len(order) < n:
        nxt = next(
            (
                i
                for i in range(n)
                if not placed[i] and all(placed[d] for d in requires[i])
            ),
            None,
        )
        if nxt is None:
            nxt = next(i for i in range(n) if not placed[i])
        placed[nxt] = True
        order.append(nxt)
    return [entries[i] for i in order]


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
    autounmask_suggest_use=False,
    autounmask_suggest_license=False,
    autounmask_suggest_masks=False,
    usepkg=False,
    usepkgonly=False,
    binpkg_respect_use=False,
    usepkg_exclude=(),
    usepkg_include=(),
    rebuilt_binaries=False,
    rebuilt_binaries_timestamp=None,
    newrepo=False,
    buildpkgonly=False,
    root_deps_running_root=None,
    distdir="/var/cache/distfiles",
    empty=False,
    getbinpkg=False,
    ignore_built_slot_operator_deps=False,
    backtrack_max=10,
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

    New-slot installs ("[ebuild NS]", real output.py::
    _get_installed_best's own new_slot): resolve_pretend's own "is this
    candidate already installed" checks are slot-aware -- matching is
    filtered to the resolved candidate's own main slot (pkg.slot_atom,
    sub-slot ignored), so requesting a slot the package isn't installed
    in resolves as "new" (with provenance["new_slot"] set when another
    slot IS installed), never as a bogus upgrade/downgrade off an
    unrelated slot's version. _dependency_avoid_update_candidate (the
    not-update shortcut for a dependency atom, real avoid_update) is
    slot-aware too now -- it matches an installed (version, slot) pair,
    not merely a version present in some slot.

    `empty` (--emptytree/-e): forces `deep` on (real
    create_depgraph_params.py:178) and is threaded into every
    resolve_pretend call so an already-installed atom -- top level or a
    dependency reached by the now-mandatory deep walk -- resolves to a
    bare "reinstall". Net effect matches real `emerge -e`: the whole
    deep dependency tree is re-merged.

    Mirrors portage-repo/src/lib.rs's resolve_pretend_graph exactly."""
    repos = find_repos(config_root)
    top_level = set(atoms)
    # Real create_depgraph_params.py:178: --emptytree sets
    # myparams["deep"] = True.
    if empty:
        deep = True

    # Backtracking (real `_emerge/resolver/backtracking.py`): run the
    # whole graph walk, and if it hits a *solvable* slot conflict (real
    # `_process_slot_conflicts` -- one version can satisfy every parent
    # atom that landed on the slot), record the extra constraints and
    # re-run the entire walk from scratch. `entries` and every other
    # per-pass accumulator is rebuilt each iteration; only
    # `slot_constraints` (keyed by `cat/pkg`, the union of every atom
    # text that targeted a conflicted package) and the counter survive.
    # The retry ceiling is `backtrack_max` (real `--backtrack=COUNT`,
    # default 10, `0` disables). Mirrors portage-repo/src/lib.rs's
    # resolve_pretend_graph exactly.
    slot_constraints = {}
    backtrack_iteration = 0
    # Backtracking slice 3 (unsolvable conflict -> runtime_pkg_mask): a
    # small state machine across passes. "none" = ordinary pass; "trying" =
    # the pass just ran with a trial set of "!=cpv" masks and its result
    # must be judged (kept if every conflict cleared with no new
    # no_visible_candidate, else reverted); "reverting" = one final clean
    # pass after a rejected trial. `mask_trial_spent` stops a second trial
    # once the first has been judged. Mirrors portage-repo/src/lib.rs.
    mask_phase = "none"
    mask_trial_spent = False
    mask_negatives = []
    pre_trial_nvc = 0

    def _graph_pass():
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
        # (category, package) -> already added a targets_running_root entry
        # for it (see _resolve_root_deps_build_entries's own docstring).
        # Deliberately separate from resolved_slots/other_outcomes above --
        # those two dedup ROOT-targeted resolutions, and a package genuinely
        # can need building into both ROOT (as an ordinary RDEPEND) and the
        # running root (as some other package's own BDEPEND) at once.
        root_deps_build_seen = set()

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
        # A top-level atom has no "unevaluated" form distinct from itself (no
        # parent to ever flip a flag on).
        queue = deque((a, 0, None, None) for a in atoms)
        pending_blockers = []
        # Top-level atoms matched by package.provided -- see
        # portage-repo/src/lib.rs's GraphResult::pprovided_atoms.
        package_provided = config["package_provided"]
        pprovided_atoms = []
        # Real --autounmask changes applied during this walk -- see
        # portage-repo/src/lib.rs's GraphResult::autounmask_keyword_changes /
        # autounmask_use_changes.
        autounmask_keyword_changes = []
        autounmask_use_changes = []
        autounmask_license_changes = []
        autounmask_mask_changes = []
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
        # Backtracking: every atom text (bare or constrained) that targeted
        # a given `cat/pkg` this pass. On a solvable slot conflict the whole
        # set for the conflicted package becomes its `slot_constraints`
        # entry for the next attempt. Mirrors portage-repo/src/lib.rs.
        slot_want = {}
        # Backtracking (slice 3): for each `cat/pkg` targeted by a
        # dependency string this pass, the (cat, pkg, version) of every
        # package that pulled it in. The unsolvable-slot-conflict handler
        # masks a puller version that has a lower alternative so the retry
        # falls back to it (real runtime_pkg_mask propagation).
        slot_pullers = {}

        while queue:
            current_atom_str, depth, owner, unevaluated_atom = queue.popleft()
            atom = _parse_atom(current_atom_str)
            if atom is None:
                continue
            if atom.blocker:
                continue
            # package.provided (real dep_check.py:1052 for a dependency,
            # depgraph.py:5497-5615 for a top-level target): an atom matched
            # by a listed CPV is treated as already installed. A dependency
            # is silently dropped (no entry, no required_by edge); a
            # top-level atom is recorded for the WARNING block. Mirrors
            # portage-repo/src/lib.rs's resolve_pretend_graph.
            if package_provided and match_from_list(
                Atom(current_atom_str, allow_wildcard=True), package_provided
            ):
                if depth == 0 and current_atom_str not in pprovided_atoms:
                    pprovided_atoms.append(current_atom_str)
                continue
            category, package = atom.cp.split("/", 1)
            key = (category, package)
            if owner is not None:
                required_by_map.setdefault(key, set()).add(owner)
            if current_atom_str in visited_atoms:
                continue
            visited_atoms.add(current_atom_str)
            # Backtracking: record this atom as one of the constraints
            # pulling `cat/pkg` (real `_select_pkg_highest_available` sees
            # the whole atom set for a package, not just the first).
            slot_want.setdefault(key, []).append(current_atom_str)
            # Backtracking slice 4: a top-level atom targeting this cat/pkg
            # is an "Argument" puller for the slot-collision block.
            if owner is None:
                slot_pullers.setdefault(key, []).append(("", "", "", current_atom_str))

            # Backtracking: if an earlier attempt hit a *solvable* slot
            # conflict on this `cat/pkg`, every parent atom that targeted it
            # is now enforced together, so this attempt picks the one
            # version that satisfies all of them and the conflict
            # disappears.
            extra_constraints = slot_constraints.get(key, ())

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
                usepkg_exclude,
                usepkg_include,
                rebuilt_binaries,
                rebuilt_binaries_timestamp,
                newrepo,
                empty,
                getbinpkg,
                autounmask_suggest_keywords,
                autounmask_suggest_use,
                autounmask_suggest_license,
                autounmask_suggest_masks,
                extra_constraints,
            )

            # Real --autounmask-use PART B *resolution*
            # (_apply_parent_use_changes -> _show_unsatisfied_dep(
            # collect_use_changes=True), depgraph.py:5820/6768): a dependency's
            # use-dep was originally conditional on the *requesting parent's*
            # own USE (opt?/opt= forms) and no candidate satisfies the
            # evaluated form -- because the child's own flag is use.mask'd, so
            # a child-side package.use flip (_suggested_use_flip) is
            # impossible. Real portage flips the *parent's* conditional flag
            # instead (_suggested_parent_use_candidate), re-resolves, and
            # prints it in the same "necessary to proceed" USE block. This
            # reference applies the one change and re-resolves only the freed
            # dependency (real portage re-resolves the whole graph -- see the
            # cut note); --autounmask-use=n suppresses it via the shared gate.
            # Mirrors portage-repo/src/lib.rs's own inline block.
            if (
                outcome[0] == "no_visible_candidate"
                and autounmask_suggest_use
                and depth > 0
                and owner is not None
                and unevaluated_atom is not None
            ):
                _pfc = _suggested_parent_use_candidate(
                    repos, entries, unevaluated_atom, owner, config
                )
                _pstate = (
                    _parent_use_state(repos, entries, owner, config)
                    if _pfc is not None
                    else None
                )
                if _pfc is not None and _pstate is not None:
                    _pc, _pp, _pv, _target_use = _pfc
                    _parent_cand, _piuse, _parent_use, _pru = _pstate
                    _new_parent_use = set(_parent_use)
                    for _flag, _want in _target_use:
                        (_new_parent_use.add if _want else _new_parent_use.discard)(_flag)
                    _re_atom = str(
                        _parse_atom(unevaluated_atom).evaluate_conditionals(_new_parent_use)
                    )
                    _re_parsed = _parse_atom(_re_atom)
                    if _re_parsed is not None:
                        _re_outcome = resolve_pretend(
                            repos,
                            root,
                            _re_atom,
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
                            usepkg_exclude,
                            usepkg_include,
                            rebuilt_binaries,
                            rebuilt_binaries_timestamp,
                            newrepo,
                            empty,
                            getbinpkg,
                            autounmask_suggest_keywords,
                            autounmask_suggest_use,
                            autounmask_suggest_license,
                            autounmask_suggest_masks,
                        )
                        if _re_outcome[0] != "no_visible_candidate":
                            outcome = _re_outcome
                            current_atom_str = _re_atom
                            atom = _re_parsed
                            _token = " ".join(
                                f if e else f"-{f}" for f, e in _target_use
                            )
                            autounmask_use_changes.append(
                                {
                                    "atom": _autounmask_use_atom_form(
                                        _parent_cand,
                                        list_candidates(repos, _pc, _pp),
                                        _pc,
                                        _pp,
                                        config,
                                    ),
                                    "token": _token,
                                    "dep_chain": _autounmask_dep_chain(
                                        (_pc, _pp), "", top_level, entries
                                    ),
                                }
                            )
                            _pflip_display = sorted(
                                (
                                    (t.lstrip("+-"), t.lstrip("+-") in _new_parent_use)
                                    for t in _parent_cand.get("iuse", "").split()
                                ),
                                key=lambda p: _alnum_sort_key(p[0]),
                            )
                            for _i, _e in enumerate(entries):
                                if _e[0] == _pc and _e[1] == _pp:
                                    entries[_i] = _e[:5] + (_pflip_display,) + _e[6:]
                                    break

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
                    suggestion = _suggested_keyword_candidate(repos, category, package, config)
                    if suggestion is not None:
                        version, keyword = suggestion
                        message += (
                            f"\nnote: {category}/{package}-{version} exists but is "
                            f"masked by KEYWORDS; --autounmask-keep-keywords=n suggests adding "
                            f'"{category}/{package} {keyword}" to package.accept_keywords'
                        )
                # --autounmask-use's own suggestion sub-feature -- same
                # gating/"only suggest a fix that would actually work"
                # spirit as the keyword one just above. Message format
                # mirrors real package.use suggestion syntax
                # (=category/package-version flag -flag).
                if autounmask_suggest_use:
                    use_suggestion = _suggested_use_candidate(repos, category, package, atom, config)
                    if use_suggestion is not None:
                        version, flip = use_suggestion
                        adjustments = " ".join(
                            flag if enabled else f"-{flag}" for flag, enabled in flip
                        )
                        message += (
                            f"\nnote: {category}/{package}-{version} exists but its USE flags "
                            f"don't satisfy this atom; --autounmask-use suggests adding "
                            f'"={category}/{package}-{version} {adjustments}" to package.use'
                        )
                # --autounmask-license's own suggestion sub-feature.
                if autounmask_suggest_license:
                    lic_suggestion = _suggested_license_candidate(
                        repos, category, package, config
                    )
                    if lic_suggestion is not None:
                        version, licenses = lic_suggestion
                        message += (
                            f"\nnote: {category}/{package}-{version} exists but its LICENSE "
                            f"is not accepted; --autounmask-license suggests adding "
                            f'"={category}/{package}-{version} {licenses}" to package.license'
                        )
                # --autounmask-keep-masks=n's own suggestion sub-feature.
                if autounmask_suggest_masks:
                    mask_version = _suggested_mask_candidate(repos, category, package, config)
                    if mask_version is not None:
                        message += (
                            f"\nnote: {category}/{package}-{mask_version} exists but is "
                            f"package.mask'd; --autounmask-keep-masks=n suggests adding "
                            f'"={category}/{package}-{mask_version}" to package.unmask'
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
                        root_deps_running_root,
                        entries,
                        root_deps_build_seen,
                    )
                # --autounmask's own keyword-suggestion sub-feature, extended
                # here to a *dependency's* own NoVisibleCandidate -- see
                # portage-repo/src/lib.rs's GraphEntry::keyword_suggestion own
                # doc comment.
                keyword_suggestion = None
                if outcome[0] == "no_visible_candidate" and autounmask_suggest_keywords:
                    keyword_suggestion = _suggested_keyword_candidate(repos, category, package, config)
                # --autounmask-use's own suggestion sub-feature -- see
                # portage-repo/src/lib.rs's GraphEntry::use_suggestion own
                # doc comment. `atom.use` is the dependency atom's own
                # use-dep spec (already conditional-evaluated by
                # _enqueue_flat_deps before this atom was ever queued, so
                # only plain flag/-flag forms survive to be checked here).
                use_suggestion = None
                if outcome[0] == "no_visible_candidate" and autounmask_suggest_use:
                    use_suggestion = _suggested_use_candidate(repos, category, package, atom, config)
                # --autounmask-use's own second, architecturally distinct
                # suggestion sub-feature -- see
                # _suggested_parent_use_candidate's own docstring. Only ever
                # attempted when this atom actually had a conditional
                # use-dep evaluated away (unevaluated_atom is not None) and
                # has a real parent to flip a flag on (owner is always set
                # here: a top-level atom's own no_visible_candidate already
                # aborted the whole call via the fatal check above, so any
                # no_visible_candidate reaching this point is necessarily a
                # dependency's own, which always has an owner).
                parent_use_suggestion = None
                if (
                    outcome[0] == "no_visible_candidate"
                    and autounmask_suggest_use
                    and owner is not None
                    and unevaluated_atom is not None
                ):
                    parent_use_suggestion = _suggested_parent_use_candidate(
                        repos, entries, unevaluated_atom, owner, config
                    )
                entries.append(
                    (
                        category,
                        package,
                        outcome,
                        [],
                        None,
                        [],
                        [],
                        "ebuild",
                        {"mask_entry": None, "unmask_entry": None, "keyword_entry": None},
                        keyword_suggestion,
                        use_suggestion,
                        parent_use_suggestion,
                        False,
                    )
                )
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
                local_index = _local_binpkg_index(config)
                binary_candidates = list_binary_candidates(local_index, category, package)
                if getbinpkg:
                    binary_candidates = binary_candidates + _list_remote_binary_candidates(
                        config.get("binrepos", []), root, local_index, category, package
                    )
                repo_candidates = repo_candidates + _filter_usepkg_exclude_include(
                    binary_candidates, category, package, usepkg_exclude, usepkg_include
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
                        _build_slot_conflict(
                            repos,
                            category,
                            package,
                            slot,
                            existing_version,
                            current_atom_str,
                            version,
                            slot_pullers,
                        )
                    )
                continue
            entry_idx = len(entries)
            resolved_slots[slot_key] = entry_idx
            provenance = _visibility_provenance(resolved, category, package, config)
            # Real output.py:gen_mask_str's -v bracket-mask column -- stashed
            # on the provenance dict (not serialized by _entry_to_json, which
            # picks out specific keys) rather than growing the entry tuple.
            # Mirrors portage-repo/src/lib.rs's GraphEntry::keyword_mask.
            provenance["keyword_mask"] = _keyword_mask_marker(
                resolved, category, package, config, provenance["mask_entry"]
            )
            # Real --autounmask keyword resolution: this candidate resolved
            # only because resolve_pretend was told to accept a KEYWORDS-alone
            # mask (_keyword_masked_only). Record the implicit `=<cpv> <kw>`
            # change (real depgraph.py::_display_autounmask's
            # unstable_keyword_msg + _get_dep_chain_as_comment;
            # autounmask_unrestricted_atoms defaults to "n", so always the
            # exact-version `=` form). Mirrors portage-repo/src/lib.rs.
            if autounmask_suggest_keywords and _keyword_masked_only(
                resolved, category, package, config
            ):
                _kw = _suggested_keyword(resolved)
                if _kw is not None:
                    autounmask_keyword_changes.append(
                        {
                            "atom": f"={category}/{package}-{version}",
                            "token": _kw,
                            "dep_chain": _autounmask_dep_chain(
                                owner, current_atom_str, top_level, entries
                            ),
                        }
                    )
            # Real --autounmask-license: this candidate resolved only because
            # resolve_pretend was told to accept a LICENSE-alone mask
            # (_license_masked_only). Record the missing licenses (real
            # _display_autounmask's license_msg; check_if_latest(pkg) without
            # check_visibility -> the `>=` / `>=…:slot` / `=` form).
            if autounmask_suggest_license and _license_masked_only(
                resolved, category, package, config
            ):
                _cs = f"{category}/{package}-{version}:{slot}/{sub_slot}::{repo_name}"
                _missing = _missing_licenses(resolved, category, package, _cs, config)
                if _missing:
                    _all = list_candidates(repos, category, package)
                    autounmask_license_changes.append(
                        {
                            "atom": _check_if_latest_atom_form(
                                resolved, _all, category, package, config, False
                            ),
                            "token": " ".join(_missing),
                            "dep_chain": _autounmask_dep_chain(
                                owner, current_atom_str, top_level, entries
                            ),
                        }
                    )
            # Real --autounmask-keep-masks=n: this candidate resolved only
            # because resolve_pretend was told to accept a package.mask-alone
            # mask (_mask_masked_only). Record `=<cpv>` (real p_mask_change_msg;
            # no token, always the exact-version form).
            if autounmask_suggest_masks and _mask_masked_only(
                resolved, category, package, config
            ):
                autounmask_mask_changes.append(
                    {
                        "atom": f"={category}/{package}-{version}",
                        "token": "",
                        "dep_chain": _autounmask_dep_chain(
                            owner, current_atom_str, top_level, entries
                        ),
                    }
                )
            # Real output.py::_get_installed_best's own new_slot flag (the
            # "S" bracket column, PkgAttrDisplay.new_slot): a "new" entry
            # whose category/package is installed in some *other* slot (the
            # in-slot new/upgrade decision already happened inside
            # resolve_pretend, so "new" here means nothing is installed in
            # *this* slot). Stashed on provenance like keyword_mask above.
            # Mirrors portage-repo/src/lib.rs's GraphEntry::new_slot.
            provenance["new_slot"] = outcome[0] == "new" and bool(
                installed_candidates(root, category, package)
            )
            # Real output.py:833: `if "interactive" in pkg.properties and
            # pkg.operation == "merge"`. pkg.properties is PROPERTIES after
            # real USE-conditional evaluation; every graph entry reaching
            # this point is a merge (new/upgrade/downgrade/reinstall -- the
            # only outcomes resolved_slots ever indexes), so no separate
            # operation check is needed. Stashed on provenance like
            # keyword_mask/new_slot above. Mirrors portage-repo/src/lib.rs's
            # GraphEntry::interactive.
            _candidate_str = f"{category}/{package}-{version}:{slot}/{sub_slot}::{repo_name}"
            provenance["interactive"] = "interactive" in _evaluated_metadata_tokens(
                resolved.get("properties", ""), resolved, category, package, _candidate_str, config
            )
            # Real output.py:633: `not pkg.built and "fetch" in pkg.restrict`
            # (ebuild candidates only). fetch_restrict_satisfied is filled in
            # below, once metadata (SRC_URI) + use_flags are read. Stashed on
            # provenance (shared dict, mutated after append) like interactive.
            # Mirrors portage-repo/src/lib.rs's GraphEntry::fetch_restrict.
            provenance["fetch_restrict"] = candidate_source != "binary" and (
                "fetch"
                in _evaluated_metadata_tokens(
                    resolved.get("restrict", ""), resolved, category, package, _candidate_str, config
                )
            )
            provenance["fetch_restrict_satisfied"] = False
            # Real output.py:648: attr_display.remote_binary = pkg.remote (the
            # `g` bracket column). Mirrors portage-repo/src/lib.rs's
            # GraphEntry::remote_binary.
            provenance["remote_binary"] = bool(resolved.get("remote"))
            # Real _append_slot / _append_repository / convert_myoldbest
            # inputs (verbosity 3 -- emerge -pv), stashed on provenance like
            # new_slot/interactive above. Mirrors portage-repo/src/lib.rs's
            # GraphEntry::sub_slot / repo_name / oldbest.
            provenance["sub_slot"] = sub_slot
            provenance["repo_name"] = repo_name
            if outcome[0] in ("upgrade", "downgrade"):
                _ob = [r for r in _installed_refs(root, category, package) if r["slot"] == slot]
            elif outcome[0] == "new" and provenance["new_slot"]:
                _ob = _installed_refs(root, category, package)
            else:
                _ob = []
            _ob.sort(key=functools.cmp_to_key(lambda a, b: vercmp(a["version"], b["version"]) or 0))
            provenance["oldbest"] = _ob
            entries.append(
                (
                    category,
                    package,
                    outcome,
                    [],
                    slot,
                    [],
                    [],
                    candidate_source,
                    provenance,
                    None,
                    None,
                    None,
                    False,
                )
            )

            pf = f"{package}-{version}"
            if candidate_source == "binary":
                metadata = _read_binary_metadata_any(
                    config, root, _local_binpkg_index(config), category, package, version
                )
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
                config["conf_use_tokens"],
                config["package_use_repo"],
                config["package_use"],
                config["package_use_user"],
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

            # Real --autounmask-use USE resolution: resolve_pretend kept this
            # candidate despite an atom use-dep mismatch because a
            # package.use flip fixes it (_suggested_use_flip). Apply the flip
            # to use_flags here -- once, at the graph layer -- so the -pv
            # USE="..." line, the REQUIRED_USE check and the dependency walk
            # all see the adjusted state (real _pkg_use_enabled) -- and
            # record the change (real _display_autounmask's use_changes_msg +
            # _get_dep_chain_as_comment(pkg, unsatisfied_dependency=True)).
            # Mirrors portage-repo/src/lib.rs.
            if autounmask_suggest_use and atom.use:
                _iuse_declared = {
                    tok.lstrip("+-") for tok in metadata.get("IUSE", "").split()
                }
                if not _use_deps_satisfied(
                    atom, _valid_iuse(_iuse_declared, config), use_flags
                ):
                    _flip = _suggested_use_flip(resolved, category, package, atom, config)
                    if _flip is not None:
                        for _flag, _enabled in _flip:
                            if _enabled:
                                use_flags.add(_flag)
                            else:
                                use_flags.discard(_flag)
                        _token = " ".join(
                            _f if _e else f"-{_f}" for _f, _e in _flip
                        )
                        autounmask_use_changes.append(
                            {
                                "atom": _autounmask_use_atom_form(
                                    resolved,
                                    list_candidates(repos, category, package),
                                    category,
                                    package,
                                    config,
                                ),
                                "token": _token,
                                "dep_chain": _autounmask_dep_chain(
                                    owner, current_atom_str, top_level, entries
                                ),
                            }
                        )

            # Real output.py:636's `not getfetchsizes(only_restricted=True)`.
            if provenance["fetch_restrict"]:
                provenance["fetch_restrict_satisfied"] = _fetch_restrict_files_all_present(
                    metadata.get("SRC_URI", ""),
                    use_flags,
                    repo_location,
                    category,
                    package,
                    distdir,
                )
            # Real output.py:300-332's _calc_size -> counters.totalsize (the
            # -v Total: line's "Size of downloads"), for every merge-bound
            # ebuild entry. Stashed on provenance like the other bracket
            # data. Mirrors portage-repo/src/lib.rs's GraphEntry::download_files.
            if candidate_source != "binary":
                provenance["download_files"] = _fetch_bytes_to_download(
                    metadata.get("SRC_URI", ""),
                    use_flags,
                    repo_location,
                    category,
                    package,
                    distdir,
                )
            elif provenance["remote_binary"]:
                # A --getbinpkg remote binary: real bindbapi.getfetchsizes ->
                # {<cpv>: SIZE} from the binhost Packages index. Feeds both
                # the -v per-line " N KiB" suffix and Size of downloads:.
                _size = _int_or_none(metadata.get("SIZE"))
                if _size is not None:
                    provenance["download_files"] = [
                        (f"{category}/{package}-{version}", _size)
                    ]

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
                iuse_set = _implicit_iuse_set(metadata.get("IUSE", ""), config)
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
                    (
                        (flag.lstrip("+-"), flag.lstrip("+-") in use_flags)
                        for flag in metadata["IUSE"].split()
                    ),
                    key=lambda p: _alnum_sort_key(p[0]),
                )
                entries[entry_idx] = (
                    category,
                    package,
                    outcome,
                    [],
                    slot,
                    display,
                    [],
                    candidate_source,
                    entries[entry_idx][8],
                    entries[entry_idx][9],
                    entries[entry_idx][10],
                    entries[entry_idx][11],
                    entries[entry_idx][12],
                )

            # --nodeps: skip this package's own DEPEND/RDEPEND/etc entirely --
            # see this function's own docstring.
            if nodeps:
                continue

            depstr = " ".join(
                metadata[k]
                for k in ("DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND")
                if metadata.get(k)
            )
            # --root-deps branch-selection feed-in (see
            # _root_deps_satisfied_atoms's own docstring): a "||" group with
            # no branch tree-visible still needs a branch selected here too,
            # not just in _root_deps_satisfied_atoms's own separate
            # re-flatten -- otherwise the *other*, genuinely unsatisfiable
            # branch would remain in flat_deps and get queued as an ordinary
            # (and wrongly reported) dependency. Real --root-deps only ever
            # applies to DEPEND/BDEPEND -- this closure can't tell which of
            # the five merged dep keys a given atom came from (this pilot's
            # own single-unified-graph architecture merges them into one
            # combined string before flattening at all), so an
            # RDEPEND/PDEPEND/IDEPEND "||" group gets this same permissive
            # check too -- harmless in practice, mirrors
            # portage-repo/src/lib.rs's identical fix exactly.
            try:
                flat_deps = _use_reduce_flat_disjunctive(
                    depstr,
                    use_flags,
                    lambda atoms: all(
                        _atom_currently_satisfiable(repos, a, config)
                        or (
                            root_deps_running_root is not None
                            and _running_root_satisfies_atom(a, root_deps_running_root)
                        )
                        for a in atoms
                    ),
                )
            except InvalidDependString:
                continue
            # --root-deps: real ESYSROOT-vs-ROOT distinction (see
            # _root_deps_satisfied_atoms's own doc comment for the full
            # grounding) -- a strict no-op when root_deps_running_root is
            # None, matching every pre-existing call site/test.
            root_deps_satisfied = (
                _root_deps_satisfied_atoms(
                    metadata,
                    use_flags,
                    repos,
                    config,
                    root_deps_running_root,
                    dep_keys=("DEPEND", "BDEPEND", "IDEPEND"),
                )
                if root_deps_running_root is not None
                else set()
            )
            # Real "recursively pull in and build new packages against the
            # running root" -- the other half of the same real DEPEND/BDEPEND
            # set root_deps_satisfied above already covers -- every atom in
            # it isn't satisfied by the running root either, so it must not
            # fall through into the ordinary flat_deps queue below and get
            # wrongly resolved against ROOT instead (real DEPEND/BDEPEND
            # never targets ROOT/ESYSROOT at all under this pilot's own
            # established --root-deps simplification). Each one instead gets
            # resolved against the running root directly, added as its own
            # targets_running_root entry, and recursed into. Kept as a list
            # (not a set) for deterministic entry order. Mirrors
            # portage-repo/src/lib.rs's identical step exactly.
            root_deps_unsatisfied = (
                _unsatisfied_root_deps_atoms(
                    metadata,
                    use_flags,
                    repos,
                    config,
                    root_deps_running_root,
                    dep_keys=("DEPEND", "BDEPEND", "IDEPEND"),
                )
                if root_deps_running_root is not None
                else []
            )
            flat_deps = [
                tok for tok in flat_deps if tok not in root_deps_satisfied and tok not in root_deps_unsatisfied
            ]
            if root_deps_running_root is not None:
                for atom_str in root_deps_unsatisfied:
                    entries.extend(
                        _resolve_root_deps_build_entries(
                            repos, root_deps_running_root, atom_str, config, key, root_deps_build_seen
                        )
                    )
            # Backtracking (slices 3/4): record this package as a puller of
            # every non-blocker cat/pkg its dependency string names,
            # keeping the atom text for slice 4's `pulled in by` lines.
            for tok in flat_deps:
                dep_atom = _parse_atom(tok)
                if dep_atom is not None and not dep_atom.blocker:
                    dc, dp = dep_atom.cp.split("/", 1)
                    slot_pullers.setdefault((dc, dp), []).append((key[0], key[1], version, tok))
            _enqueue_flat_deps(flat_deps, key, version, depth, use_flags, queue, pending_blockers)

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
                    _enqueue_flat_deps(
                        test_deps, key, version, depth, use_flags | {"test"}, queue, pending_blockers
                    )

        # Merge required_by_map into entries in a single post-pass, mirroring
        # portage-repo/src/lib.rs's own identical final loop (run before
        # resolve_blockers below, same order) -- entries are tuples
        # (immutable), so this rebuilds each one rather than mutating in
        # place. Only entries the map actually has a key for get their
        # required_by replaced -- matching the Rust side's own `if let
        # Some(owners) = required_by_map.get(...)` guard: an entry added
        # outside the normal flat-deps queue (a --root-deps running-root
        # build entry, whose own [owner] was set at construction by
        # _resolve_root_deps_build_entries) keeps that value instead of being
        # wiped to []. Both sides use a non-destructive lookup, not a pop/
        # remove: more than one entry can share a (category, package) -- one
        # per resolved slot -- and every one was pulled in by the same
        # owner(s), so every one needs the same required_by (a destructive
        # lookup would hand the owners to the first slot's entry only).
        entries = [
            (
                category,
                package,
                outcome,
                blockers,
                slot,
                use_display,
                sorted(required_by_map[(category, package)])
                if (category, package) in required_by_map
                else _required_by,
                source,
                provenance,
                keyword_suggestion,
                use_suggestion,
                parent_use_suggestion,
                targets_running_root,
            )
            for category, package, outcome, blockers, slot, use_display, _required_by, source, provenance, keyword_suggestion, use_suggestion, parent_use_suggestion, targets_running_root in entries
        ]

        # setdefault (not a dict comprehension) so the *first* entry for a
        # given owner wins when the same category/package appears more than
        # once (multiple slots) -- mirrors portage-repo/src/lib.rs's
        # `entries.iter_mut().find(...)`, which also attaches to the first
        # match.
        blockers_by_owner = {}
        for (
            category,
            package,
            _o,
            blockers,
            _slot,
            _use_display,
            _required_by,
            _source,
            _provenance,
            _keyword_suggestion,
            _use_suggestion,
            _parent_use_suggestion,
            _targets_running_root,
        ) in entries:
            blockers_by_owner.setdefault((category, package), blockers)
        for owner_key, conflict in resolve_blockers(root, pending_blockers, entries):
            blockers_by_owner[owner_key].append(conflict)

        return (
            entries,
            slot_conflicts,
            required_use_violations,
            changed_deps_report_entries,
            pprovided_atoms,
            autounmask_keyword_changes,
            autounmask_use_changes,
            autounmask_license_changes,
            autounmask_mask_changes,
            slot_want,
            slot_pullers,
        )

    def _nvc_count(rows):
        return sum(1 for r in rows if r[2][0] == "no_visible_candidate")

    while True:
        (
            entries,
            slot_conflicts,
            required_use_violations,
            changed_deps_report_entries,
            pprovided_atoms,
            autounmask_keyword_changes,
            autounmask_use_changes,
            autounmask_license_changes,
            autounmask_mask_changes,
            slot_want,
            slot_pullers,
        ) = _graph_pass()

        if required_use_violations:
            raise ResolutionError("\n".join(required_use_violations))

        # Backtracking slice 3: judge a pending runtime_pkg_mask trial.
        if mask_phase == "trying":
            mask_phase = "none"
            if not slot_conflicts and _nvc_count(entries) <= pre_trial_nvc:
                # The trial cleared every conflict without making any
                # dependency unsatisfiable -- keep the masks.
                pass
            else:
                # Rejected: drop the trial masks and re-run one clean pass.
                for _k, _neg in mask_negatives:
                    if _k in slot_constraints and _neg in slot_constraints[_k]:
                        slot_constraints[_k].remove(_neg)
                mask_negatives = []
                mask_phase = "reverting"
                continue
        elif mask_phase == "reverting":
            mask_phase = "none"

        # Solvable slot conflict -> fold every atom that targeted the
        # conflicted `cat/pkg` into `slot_constraints` and re-run the
        # whole walk. Solvability is pre-checked against the raw
        # candidate list (real `_select_pkg_highest_available` over the
        # full atom set). Unsolvable conflicts fall through to the
        # runtime_pkg_mask trial below; anything still conflicting after
        # `backtrack_max` attempts is reported as before.
        if mask_phase == "none" and slot_conflicts and backtrack_iteration < backtrack_max:
            progressed = False
            for _sc in slot_conflicts:
                _pkg_key = (_sc["category"], _sc["package"])
                _wants = slot_want.get(_pkg_key, [])
                if len(_wants) < 2:
                    continue
                _cands = list_candidates(repos, _pkg_key[0], _pkg_key[1])
                _cand_strs = [
                    f"{_pkg_key[0]}/{_pkg_key[1]}-{_c['version']}:{_c['slot']}"
                    for _c in _cands
                ]
                _solvable = any(
                    all(match_from_list(_w, [_cs]) for _w in _wants)
                    for _cs in _cand_strs
                )
                if not _solvable:
                    continue
                _bucket = slot_constraints.setdefault(_pkg_key, [])
                for _w in _wants:
                    if _w not in _bucket:
                        _bucket.append(_w)
                        progressed = True
            if progressed:
                backtrack_iteration += 1
                continue

        # Backtracking slice 3 (real _slot_conflict_backtrack ->
        # runtime_pkg_mask): a slot conflict no single version can solve.
        # Hide the currently-resolved version of the conflicted package,
        # plus every puller-parent version that has a lower alternative,
        # then re-run once and judge the result above.
        if (
            mask_phase == "none"
            and not mask_trial_spent
            and slot_conflicts
            and backtrack_iteration < backtrack_max
        ):
            negatives = []
            for _sc in slot_conflicts:
                _cp = (_sc["category"], _sc["package"])
                negatives.append(
                    (_cp, f'!={_sc["category"]}/{_sc["package"]}-{_sc["resolved_version"]}')
                )
                _seen = set()
                for _pc, _pp, _pv, _atom in slot_pullers.get(_cp, []):
                    if not _pc or (_pc, _pp, _pv) in _seen:
                        continue
                    _seen.add((_pc, _pp, _pv))
                    _lower = any(
                        (vercmp(_c["version"], _pv) or 0) < 0
                        for _c in list_candidates(repos, _pc, _pp)
                    )
                    if _lower:
                        negatives.append(((_pc, _pp), f"!={_pc}/{_pp}-{_pv}"))
            added = False
            for _k, _neg in negatives:
                _bucket = slot_constraints.setdefault(_k, [])
                if _neg not in _bucket:
                    _bucket.append(_neg)
                    added = True
            if added:
                mask_negatives = negatives
                pre_trial_nvc = _nvc_count(entries)
                mask_phase = "trying"
                mask_trial_spent = True
                backtrack_iteration += 1
                continue
        break

    if required_use_violations:
        raise ResolutionError("\n".join(required_use_violations))

    # Real depgraph's slot-operator auto-rebuild -- see
    # portage-repo/src/lib.rs's slot_operator_rebuild_entries.
    # --ignore-built-slot-operator-deps (real main.py:470) skips the scan
    # entirely (real portage strips the built := parts so it finds
    # nothing; same net effect).
    if ignore_built_slot_operator_deps:
        slot_op_rebuilds, abi_rebuilds = [], []
    else:
        slot_op_rebuilds, abi_rebuilds = _slot_operator_rebuild_entries(root, repos, entries)
    entries.extend(slot_op_rebuilds)

    # Real portage's `mylist` is dependency-first (its Scheduler installs
    # a package only after everything it depends on); this BFS builds
    # `entries` the other way (a package's entry is appended before its
    # dependencies are ever queued). Re-sort into merge order now that
    # every required_by edge is known. Mirrors portage-repo/src/lib.rs's
    # topological_merge_order exactly.
    entries = _topological_merge_order(entries)

    # Real depgraph.py:5706-5717 -- see the Rust side's own
    # GraphResult::buildpkgonly_deps_unsatisfied doc comment.
    buildpkgonly_deps_unsatisfied = False
    if buildpkgonly:
        needs_action = {
            (category, package)
            for (category, package, outcome, *_rest) in entries
            if outcome[0] not in ("already_installed", "no_visible_candidate")
        }
        buildpkgonly_deps_unsatisfied = any(
            (category, package) in needs_action
            and any(owner in needs_action for owner in required_by)
            for (category, package, _o, _b, _s, _u, required_by, *_rest) in entries
        )

    return {
        "entries": entries,
        "slot_conflicts": slot_conflicts,
        "changed_deps_report": changed_deps_report_entries,
        "buildpkgonly_deps_unsatisfied": buildpkgonly_deps_unsatisfied,
        "pprovided_atoms": pprovided_atoms,
        "autounmask_keyword_changes": autounmask_keyword_changes,
        "autounmask_use_changes": autounmask_use_changes,
        "autounmask_license_changes": autounmask_license_changes,
        "autounmask_mask_changes": autounmask_mask_changes,
        "abi_rebuilds": abi_rebuilds,
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
    root_deps_running_root=None,
    entries=None,
    root_deps_build_seen=None,
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
    for an already-built package.

    `root_deps_running_root` (real --root-deps, see
    _running_root_satisfies_atom's own doc comment for the full real
    ESYSROOT-vs-ROOT grounding, and _root_deps_satisfied_atoms's own doc
    comment for the shared implementation and its documented scope cut):
    None for every pre-existing call site/test, and a strict no-op when
    None. When given, any already-flattened plain atom in flat_deps that
    _root_deps_satisfied_atoms reports as running-root-satisfied is
    dropped from the queue entirely (real portage's own "no separate
    graph node needed for an already-satisfied dep"). Mirrors
    portage-repo/src/lib.rs's enqueue_dependencies exactly."""
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
        config["conf_use_tokens"],
        config["package_use_repo"],
        config["package_use"],
        config["package_use_user"],
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
    # --root-deps branch-selection feed-in -- see the main
    # New/Upgrade/Reinstall loop's own identical fix for the full
    # grounding (this is _enqueue_dependencies's own
    # --deep/AlreadyInstalled-recursion counterpart to it).
    try:
        flat_deps = _use_reduce_flat_disjunctive(
            depstr,
            use_flags,
            lambda atoms: all(
                _atom_currently_satisfiable(repos, a, config)
                or (
                    root_deps_running_root is not None
                    and _running_root_satisfies_atom(a, root_deps_running_root)
                )
                for a in atoms
            ),
        )
    except InvalidDependString:
        return

    root_deps_satisfied = (
        _root_deps_satisfied_atoms(
            metadata,
            use_flags,
            repos,
            config,
            root_deps_running_root,
            dep_keys=("DEPEND", "BDEPEND", "IDEPEND"),
        )
        if root_deps_running_root is not None and with_bdeps
        else set()
    )

    # Real "recursively pull in and build new packages against the
    # running root" -- the --deep/AlreadyInstalled-recursion counterpart
    # to the main New/Upgrade/Reinstall loop's own identical step (see
    # _resolve_root_deps_build_entries's own docstring). Gated on
    # with_bdeps the same way root_deps_satisfied just above already is.
    # Kept as a list (not a set) for deterministic entry order.
    root_deps_unsatisfied = (
        _unsatisfied_root_deps_atoms(
            metadata,
            use_flags,
            repos,
            config,
            root_deps_running_root,
            dep_keys=("DEPEND", "BDEPEND", "IDEPEND"),
        )
        if root_deps_running_root is not None and with_bdeps
        else []
    )
    if (
        root_deps_running_root is not None
        and entries is not None
        and root_deps_build_seen is not None
    ):
        for atom_str in root_deps_unsatisfied:
            entries.extend(
                _resolve_root_deps_build_entries(
                    repos, root_deps_running_root, atom_str, config, owner_key, root_deps_build_seen
                )
            )

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
        if tok in root_deps_satisfied:
            # Real "no separate graph node needed for an
            # already-satisfied dep": ESYSROOT (here, the real running
            # root) already has it.
            continue
        if tok in root_deps_unsatisfied:
            # Real DEPEND/BDEPEND never targets ROOT/ESYSROOT at all
            # under this pilot's own established --root-deps
            # simplification -- already handled above instead.
            continue
        # This path never calls evaluate_conditionals at all (a real,
        # pre-existing gap unrelated to --autounmask-use: an
        # AlreadyInstalled package's own further-dependency walk under
        # --deep doesn't evaluate conditional use-deps against its own
        # USE either) -- so there's never an "unevaluated" form to
        # preserve here.
        queue.append((tok, child_depth, owner_key, None))


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
# --newuse/-N, --changed-use/-U, --nodeps/-O, --onlydeps/-o, --oneshot/-1,
# --update/-u, --deep/-D, --exclude/-X, --deselect/-W, --with-bdeps,
# --with-bdeps-auto, --changed-deps, --changed-deps-report, --changed-slot,
# --with-test-deps, --noreplace/-n, --selective, and --help/-h are
# actually implemented (see run()
# below); every table
# here exists purely for recognition, not behavior.
# Mirrors
# PORTING/rust/portuale/src/emerge_options.rs's own copy of these same
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


def _entry_to_json(category, package, merge_order, outcome, blockers, slot, use_display, required_by, source, provenance, keyword_suggestion, use_suggestion, parent_use_suggestion, targets_running_root, top_level_pkgs, verbose, running_root):
    """One JSON object per entry -- a structured mirror of the plain-text
    "[ebuild ...]"/"[binary ...]"/"already installed"/blocker lines in
    run(), plus two fields no plain-text line carries at all: "requested"
    (was this exact category/package one of the atoms given directly, as
    opposed to reached only via a dependency string) and "required_by"
    (which package(s), if any, pulled it in that way). "source" mirrors
    the plain-text loop's own "bracket" variable in run()
    ("binary"/"ebuild", real RootConfig.py's own pkg_tree_map-driven
    type_name) -- until the binary-package slice (--usepkg/--usepkgonly)
    this was always "ebuild" unconditionally; it no longer is.
    Deliberately NOT affected by --onlydeps's own suppression (a
    display-only concern for the plain-text loop in run()): --json
    always dumps the whole resolved graph, letting a consumer filter on
    "requested" itself if they want the --onlydeps view. "provenance"
    (alongside "source", so also absent for "no_visible_candidate")
    mirrors this pilot's own state-change trace -- which package.mask/
    .unmask/package.accept_keywords entries, if any, were actually
    load-bearing for this candidate to be visible at all -- always
    present (each of its three sub-fields null rather than omitted when
    not applicable), no verbose gate, unlike use_flags above; see
    _visibility_provenance's own docstring. "keyword_suggestion" is
    provenance's own mirror image -- present (as {"version", "keyword"}
    or null) only for "no_visible_candidate" entries, since that's the
    one outcome with nothing visible to trace provenance for and
    something to suggest instead; see _suggested_keyword_candidate's own
    docstring. Mirrors pretend.rs's own entry_to_json exactly, field for
    field, in the same order."""
    requested = (category, package) in top_level_pkgs
    fields = [
        f'"category":{_json_string(category)}',
        f'"package":{_json_string(package)}',
        # 0-based position in real portage's dependency-first merge order
        # (_topological_merge_order). The entries array is already in this
        # order -- the field is here so a consumer that re-sorts or
        # filters the array keeps the schedule.
        f'"merge_order":{merge_order}',
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
        fields.append(f'"rebuilt_binary":{_json_bool(outcome[5])}')
        fields.append(f'"new_repo":{_json_bool(outcome[6])}')
        fields.append(f'"slot_operator_rebuild":{_json_bool(outcome[7])}')
    # Real output.py's own "S" bracket column, exposed unconditionally
    # (like every other --json field): true for a "new" into a slot the
    # package isn't installed in while another slot of it is
    # (provenance["new_slot"]). Mirrors pretend.rs's entry_to_json.
    if tag == "new":
        new_slot_val = provenance.get("new_slot", False) if isinstance(provenance, dict) else False
        fields.append(f'"new_slot":{_json_bool(new_slot_val)}')
    # Real output.py:833's own "I" bracket column, exposed
    # unconditionally: true for a merge-bound entry whose evaluated
    # PROPERTIES contains "interactive" (provenance["interactive"]).
    # Mirrors pretend.rs's entry_to_json.
    if tag in ("new", "upgrade", "downgrade", "reinstall"):
        pv = provenance if isinstance(provenance, dict) else {}
        fields.append(f'"interactive":{_json_bool(pv.get("interactive", False))}')
        # Real output.py:633's own f/F fetch-restrict column.
        fields.append(f'"fetch_restrict":{_json_bool(pv.get("fetch_restrict", False))}')
        fields.append(
            f'"fetch_restrict_satisfied":{_json_bool(pv.get("fetch_restrict_satisfied", False))}'
        )
    fields.append(f'"slot":{_json_string(slot) if slot is not None else "null"}')
    if tag != "no_visible_candidate":
        fields.append(f'"source":{_json_string(source)}')

        def _opt_str(v):
            return _json_string(v) if v is not None else "null"

        fields.append(
            f'"provenance":{{"mask_entry":{_opt_str(provenance["mask_entry"])},'
            f'"unmask_entry":{_opt_str(provenance["unmask_entry"])},'
            f'"keyword_entry":{_opt_str(provenance["keyword_entry"])}}}'
        )
    else:
        if keyword_suggestion is not None:
            version, keyword = keyword_suggestion
            fields.append(
                f'"keyword_suggestion":{{"version":{_json_string(version)},'
                f'"keyword":{_json_string(keyword)}}}'
            )
        else:
            fields.append('"keyword_suggestion":null')
        if use_suggestion is not None:
            version, flip = use_suggestion
            flags_json = ",".join(
                f'{{"flag":{_json_string(flag)},"enabled":{_json_bool(enabled)}}}'
                for flag, enabled in flip
            )
            fields.append(
                f'"use_suggestion":{{"version":{_json_string(version)},"flags":[{flags_json}]}}'
            )
        else:
            fields.append('"use_suggestion":null')
        if parent_use_suggestion is not None:
            parent_category, parent_package, parent_version, flip = parent_use_suggestion
            flags_json = ",".join(
                f'{{"flag":{_json_string(flag)},"enabled":{_json_bool(enabled)}}}'
                for flag, enabled in flip
            )
            fields.append(
                f'"parent_use_suggestion":{{"category":{_json_string(parent_category)},'
                f'"package":{_json_string(parent_package)},'
                f'"version":{_json_string(parent_version)},"flags":[{flags_json}]}}'
            )
        else:
            fields.append('"parent_use_suggestion":null')
    fields.append(f'"requested":{_json_bool(requested)}')
    required_by_json = ",".join(
        f'{{"category":{_json_string(c)},"package":{_json_string(p)}}}' for c, p in required_by
    )
    fields.append(f'"required_by":[{required_by_json}]')
    # --root-deps's own running-root build entries (see root_suffix in
    # run() and pretend.rs's own entry_to_json): the same "to <root>"
    # distinction the plain-text output carries, as an explicit field --
    # the running-root path string for such an entry, null for every
    # ordinary ROOT-targeted one. null (rather than absent) universally,
    # same shape as "slot" above.
    if targets_running_root and running_root is not None:
        fields.append(f'"builds_against_running_root":{_json_string(str(running_root))}')
    else:
        fields.append('"builds_against_running_root":null')
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
    instances = ",".join(
        (
            f'{{"version":{_json_string(inst["version"])},'
            f'"sub_slot":{_json_string(inst["sub_slot"])},'
            f'"repo_name":{_json_string(inst["repo_name"])},"parents":['
            + ",".join(
                f'{{"parent":{_json_string(p[0])},"atom":{_json_string(p[1])}}}'
                for p in inst["parents"]
            )
            + "]}"
        )
        for inst in c["instances"]
    )
    return (
        f'{{"category":{_json_string(c["category"])},"package":{_json_string(c["package"])},'
        f'"slot":{_json_string(c["slot"])},"resolved_version":{_json_string(c["resolved_version"])},'
        f'"conflicting_atom":{_json_string(c["conflicting_atom"])},"instances":[{instances}]}}'
    )


def _changed_deps_report_entry_to_json(c):
    return (
        f'{{"category":{_json_string(c["category"])},"package":{_json_string(c["package"])},'
        f'"version":{_json_string(c["version"])},"repo_name":{_json_string(c["repo_name"])}}}'
    )


def _autounmask_change_to_json(change):
    chain = ",".join(_json_string(line) for line in change["dep_chain"])
    return (
        f'{{"atom":{_json_string(change["atom"])},'
        f'"token":{_json_string(change["token"])},'
        f'"dep_chain":[{chain}]}}'
    )


def _print_json(
    entries,
    slot_conflicts,
    changed_deps_report,
    autounmask_keyword_changes,
    autounmask_use_changes,
    autounmask_license_changes,
    autounmask_mask_changes,
    abi_rebuilds,
    top_level_pkgs,
    verbose,
    running_root=None,
):
    """The whole --json output: {"entries": [...], "slot_conflicts": [...],
    "changed_deps_report": [...], "autounmask_keyword_changes": [...]}, one
    line, no pretty-printing (a pilot-specific convenience format, not a
    stable schema -- see run()'s own --json handling). Mirrors
    pretend.rs's own print_json exactly."""
    entries_json = ",".join(
        _entry_to_json(
            category, package, merge_order, outcome, blockers, slot, use_display, required_by, source, provenance, keyword_suggestion, use_suggestion, parent_use_suggestion, targets_running_root, top_level_pkgs, verbose, running_root
        )
        for merge_order, (category, package, outcome, blockers, slot, use_display, required_by, source, provenance, keyword_suggestion, use_suggestion, parent_use_suggestion, targets_running_root) in enumerate(entries)
    )
    conflicts_json = ",".join(_slot_conflict_to_json(c) for c in slot_conflicts)
    changed_deps_report_json = ",".join(
        _changed_deps_report_entry_to_json(c) for c in changed_deps_report
    )
    autounmask_kw_json = ",".join(
        _autounmask_change_to_json(c) for c in autounmask_keyword_changes
    )
    autounmask_use_json = ",".join(
        _autounmask_change_to_json(c) for c in autounmask_use_changes
    )
    autounmask_license_json = ",".join(
        _autounmask_change_to_json(c) for c in autounmask_license_changes
    )
    autounmask_mask_json = ",".join(
        _autounmask_change_to_json(c) for c in autounmask_mask_changes
    )
    abi_rebuilds_json = ",".join(
        f'{{"provider":{_json_string(child)},"consumer":{_json_string(parent)}}}'
        for child, parent in abi_rebuilds
    )
    print(
        f'{{"entries":[{entries_json}],"slot_conflicts":[{conflicts_json}],'
        f'"changed_deps_report":[{changed_deps_report_json}],'
        f'"autounmask_keyword_changes":[{autounmask_kw_json}],'
        f'"autounmask_use_changes":[{autounmask_use_json}],'
        f'"autounmask_license_changes":[{autounmask_license_json}],'
        f'"autounmask_mask_changes":[{autounmask_mask_json}],'
        f'"abi_rebuilds":[{abi_rebuilds_json}]}}'
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
            "--deselect/-W, --unmerge/-C, --depclean/-c, --prune/-P, --config, --with-bdeps, --with-bdeps-auto, --changed-deps, "
            "--changed-deps-report, --changed-slot, --verbose-slot-rebuilds, --ignore-built-slot-operator-deps, --buildpkg/-b, --buildpkg-exclude, --with-test-deps, "
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
        '   -W, --deselect  a standalone action: remove matching ATOMS from the world / world_sets favorites files (--pretend previews)'
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


def _expand_selected(config_root, root):
    """Real @selected (WorldSelectedSet -- cnf/sets/portage.conf): the
    world file's own package atoms unioned with every nested set named in
    world_sets. This pilot's @world expands to exactly this too -- real
    @world = @profile @selected @system and the @profile/@system union is
    a pre-existing documented simplification. Mirrors pretend.rs's
    expand_selected."""
    atoms = list(_read_world_atoms(root))
    for name in _read_world_sets(root):
        atoms.extend(_resolve_custom_set(config_root, name, set()))
    return atoms


def _installed_set_atoms(root):
    """Real @installed (EverythingSet.load, _sets/dbapi.py): a
    "cat/pkg:slot" atom for every package under <root>/var/db/pkg --
    always slot-qualified, even for a lone installed slot (bug #338959).
    Deduplicated + sorted for deterministic output (real portage's own
    _setAtoms is an unordered set). Mirrors pretend.rs's
    installed_set_atoms."""
    atoms = {
        f"{cat}/{name}:{slot}"
        for cat, name, _version, slot in _all_installed_packages(root)
    }
    return sorted(atoms)


def _collect_installed_sets(config_root, root):
    """Real _unmerge_display's own `installed_sets` -- every custom set
    directly/indirectly selected via world_sets, paired with its DIRECT
    atoms only (the "still listed" warning names the set that directly
    contains the package). BFS over the @-references, cycle-guarded. A
    referenced-but-missing set is dropped silently (real portage eerrors
    "Unknown set"). Mirrors pretend.rs's collect_installed_sets."""
    out = []
    seen = set()
    queue = list(_read_world_sets(root))
    while queue:
        name = queue.pop()
        if name in seen:
            continue
        seen.add(name)
        path = os.path.join(config_root, "etc", "portage", "sets", name)
        try:
            with open(path) as f:
                text = f.read()
        except OSError:
            continue
        direct = []
        for line in (raw.strip() for raw in text.splitlines()):
            if not line or line.startswith("#"):
                continue
            if line.startswith("@"):
                queue.append(line[1:])
            else:
                direct.append(line)
        out.append((name, direct))
    return out


def _still_listed_parents(root, installed_sets, cat, pkg, version):
    """Real unmerge.py:355-447's "still listed in the following package
    sets" check for one selected category/package-version: the names of
    the user-editable sets (installed_sets) that still directly list a
    matching atom, MINUS any set whose matching atom is also satisfied by
    an installed newer version of the same cp in a different slot (real
    unmerge.py:421-441's higher_slot: pkg.slot_atom != inst_pkg.slot_atom
    after the descending-order `pkg >= inst_pkg` break). Shared by the
    -pC and -pP paths, matching real portage's single _unmerge_display.
    Mirrors pretend.rs's still_listed_parents."""
    installed = installed_candidates(root, cat, pkg)
    selected_slot = next(
        (s for (v, s, _ss) in installed if v == version), None
    )

    def covered_by_higher_slot(atom_str):
        return any(
            (vercmp(v, version) or 0) > 0
            and s != selected_slot
            and match_from_list(atom_str, [f"{cat}/{pkg}-{v}:{s}/{ss}"])
            for (v, s, ss) in installed
        )

    candidate = f"{cat}/{pkg}-{version}"
    parents = []
    for set_name, atoms in installed_sets:
        if any(
            (parsed := _parse_atom(a)) is not None
            and parsed.cp.split("/", 1) == [cat, pkg]
            and match_from_list(a, [candidate])
            and not covered_by_higher_slot(a)
            for a in atoms
        ):
            parents.append(set_name)
    return parents


def _defined_set_names(config_root):
    """Every defined package-set name -- see _run_list_sets. Built-ins
    from cnf/sets/portage.conf section headers (skipping the multiset
    [usersets] generator), plus user set files. Mirrors pretend.rs's
    defined_set_names."""
    names = []
    conf = os.path.join(
        os.path.dirname(__file__), "..", "..", "cnf", "sets", "portage.conf"
    )
    try:
        with open(conf) as f:
            text = f.read()
    except OSError:
        text = ""
    section = None
    multiset = False
    for line in text.splitlines():
        line = line.strip()
        if line.startswith("[") and line.endswith("]"):
            if section is not None and not multiset:
                names.append(section)
            section = line[1:-1]
            multiset = False
        elif "".join(line.split()).lower() == "multiset=true":
            multiset = True
    if section is not None and not multiset:
        names.append(section)
    sets_dir = os.path.join(config_root, "etc", "portage", "sets")
    try:
        for name in os.listdir(sets_dir):
            if os.path.isfile(os.path.join(sets_dir, name)):
                names.append(name)
    except OSError:
        pass
    return names


def _run_list_sets(config_root):
    """Real `emerge --list-sets` (_emerge/actions.py:3839): every defined
    package-set name, sorted, one per line. Mirrors pretend.rs's
    run_list_sets."""
    for name in sorted(set(_defined_set_names(config_root))):
        print(name)
    return 0


def _all_cp(repos):
    """Every category/package with an ebuild in any of `repos` -- real
    portdbapi.cp_all(). Mirrors portage-repo's all_cp."""
    _skip = {
        "eclass",
        "profiles",
        "metadata",
        "licenses",
        "scripts",
        "distfiles",
        ".git",
    }
    out = set()
    for repo in repos:
        try:
            cats = os.listdir(repo["location"])
        except OSError:
            continue
        for category in cats:
            if category in _skip:
                continue
            cat_dir = os.path.join(repo["location"], category)
            if not os.path.isdir(cat_dir):
                continue
            try:
                pkgs = os.listdir(cat_dir)
            except OSError:
                continue
            for package in pkgs:
                pkg_dir = os.path.join(cat_dir, package)
                if not os.path.isdir(pkg_dir):
                    continue
                try:
                    entries = os.listdir(pkg_dir)
                except OSError:
                    continue
                if any(
                    e.endswith(".ebuild")
                    and _strip_version_prefix(e[: -len(".ebuild")], package) is not None
                    for e in entries
                ):
                    out.add(f"{category}/{package}")
    return sorted(out)


def _search_best_candidate(cands):
    best = None
    for c in cands:
        if best is None:
            best = c
            continue
        cmp = vercmp(c["version"], best["version"]) or 0
        if cmp > 0 or (cmp == 0 and c["repo_priority"] > best["repo_priority"]):
            best = c
    return best


def _search_candidate_visible(c):
    return any(
        k in ("amd64", "~amd64", "*", "~*", "**") for k in c["keywords"]
    )


def _run_search(terms, config_root, root, searchdesc, verbose, color):
    """Real `emerge --search`/`-s` (action_search -> search.py): a
    case-insensitive substring search of every category/package in the
    configured repos (the package-name half only, unless the key
    contains "/"), plus defined set names; `-S`/`--searchdesc` also
    matches DESCRIPTION. Output shape is real search.output(). Mirrors
    pretend.rs's run_search, including its v1 cuts (no fuzzy/regex/index
    matching, no --usepkg results, no Size of files)."""
    if not terms:
        print("emerge: no search terms provided.")
        return 0

    repos = find_repos(config_root)
    all_cp = _all_cp(repos)
    set_names = _defined_set_names(config_root)

    for term in terms:
        key = term.lstrip("%")
        match_category = "/" in key
        needle = key.lower()

        hits = []
        for cp in all_cp:
            hay = cp.lower() if match_category else cp.split("/")[-1].lower()
            name_match = needle in hay
            cat, _, pkg = cp.partition("/")
            cands = list_candidates(repos, cat, pkg)
            best = _search_best_candidate(cands)
            desc_match = False
            if not name_match and searchdesc and best is not None:
                try:
                    meta = read_md5_cache(
                        best["repo_location"], cat, f"{pkg}-{best['version']}"
                    )
                    desc_match = needle in meta.get("DESCRIPTION", "").lower()
                except OSError:
                    pass
            if name_match or desc_match:
                masked = best is None or not _search_candidate_visible(best)
                hits.append((cp, masked))

        set_hits = sorted(s for s in set_names if needle in s.lower())

        star = color.c("GOOD", "*")
        sys.stdout.write("Searching...\n\n")
        sys.stdout.write(
            "\b\b  \n[ Results for search key : %s ]\n" % color.c("bold", key)
        )
        total = len(hits) + len(set_hits)
        for name in set_hits:
            print(f"{star}  {color.c('bold', name)}")
        for cp, masked in hits:
            cat, _, pkg = cp.partition("/")
            cands = list_candidates(repos, cat, pkg)
            best = _search_best_candidate(cands)
            if masked:
                print(
                    f"{star}  {color.c('bold', cp)} {color.c('BAD', '[ Masked ]')}"
                )
            else:
                print(f"{star}  {color.c('bold', cp)}")
            if verbose and best is not None:
                try:
                    meta = read_md5_cache(
                        best["repo_location"], cat, f"{pkg}-{best['version']}"
                    )
                except OSError:
                    meta = {}

                def _g(k):
                    return color.c("darkgreen", k)

                installed = installed_versions(root, cat, pkg)
                inst = " ".join(installed) if installed else "[ Not Installed ]"
                print(f"      {_g('Latest version available:')} {best['version']}")
                print(f"      {_g('Latest version installed:')} {inst}")
                print(f"      {_g('Homepage:')}      {meta.get('HOMEPAGE', '')}")
                print(f"      {_g('Description:')}   {meta.get('DESCRIPTION', '')}")
                print(f"      {_g('License:')}       {meta.get('LICENSE', '')}\n")
        print(f"[ Applications found : {color.c('bold', str(total))} ]\n")
    return 0


def _news_item_valid(text):
    for line in text.splitlines():
        if line.startswith("News-Item-Format:"):
            v = line[len("News-Item-Format:") :].strip()
            if v.startswith(("1.", "2.")) or v in ("1", "2"):
                return True
    return False


def _news_item_relevant(text, root):
    installed_atoms = [
        line[len("Display-If-Installed:") :].strip()
        for line in text.splitlines()
        if line.startswith("Display-If-Installed:")
    ]
    if not installed_atoms:
        return True
    for atom in installed_atoms:
        if "/" in atom:
            cat, _, pkg = atom.partition("/")
            if installed_versions(root, cat, pkg):
                return True
    return False


def _run_check_news(repos, root, quiet, color):
    """Real `emerge --check-news` (actions.py:3844 -> portage.news
    count_unread_news / display_news_notifications). Mirrors pretend.rs's
    run_check_news, including its v1 cuts (no .unread/.skip persistence;
    only bare cat/pkg Display-If-Installed atoms)."""
    per_repo = []
    any_unread = False
    for repo in repos:
        news_dir = os.path.join(repo["location"], "metadata", "news")
        try:
            ids = sorted(
                d
                for d in os.listdir(news_dir)
                if os.path.isdir(os.path.join(news_dir, d))
            )
        except OSError:
            per_repo.append((repo["name"], 0))
            continue
        read_file = os.path.join(
            root, "var", "lib", "gentoo", "news", f"news-{repo['name']}.read"
        )
        try:
            with open(read_file) as f:
                read = {ln.strip() for ln in f if ln.strip()}
        except OSError:
            read = set()
        count = 0
        for itemid in ids:
            if itemid in read:
                continue
            path = os.path.join(news_dir, itemid, f"{itemid}.en.txt")
            try:
                with open(path, encoding="utf-8", errors="replace") as f:
                    text = f.read()
            except OSError:
                continue
            if not _news_item_valid(text):
                continue
            if _news_item_relevant(text, root):
                count += 1
        per_repo.append((repo["name"], count))
        if count > 0:
            any_unread = True

    if any_unread:
        first = True
        for name, count in per_repo:
            if count > 0:
                if first:
                    print()
                    first = False
                print(
                    "%s %d news items need reading for repository '%s'."
                    % (color.c("WARN", " * IMPORTANT:"), count, name)
                )
        print(
            "%s Use %s to view new items.\n"
            % (color.c("WARN", " *"), color.c("GOOD", "eselect news read"))
        )
    elif not quiet:
        print(f" {color.c('GOOD', '*')} No news items were found.")
    return 0


def _run_info(config, repos, root):
    """Real `emerge --info` (action_info), narrowed to its deterministic
    config/repository block. Mirrors pretend.rs's run_info -- see its
    docstring for the large host-state cut (Portage version header,
    uname/mem, version probes, info_pkgs, timestamps)."""
    print("Repositories:\n")
    name_of = {r["location"]: r["name"] for r in repos}
    for repo in repos:
        print(repo["name"])
        print(f"    location: {repo['location']}")
        if repo["masters"]:
            ms = [name_of[m] for m in repo["masters"] if m in name_of]
            if ms:
                print(f"    masters: {' '.join(ms)}")
        print(f"    priority: {repo['priority']}")
        if repo.get("aliases"):
            print(f"    aliases: {' '.join(repo['aliases'])}")
        print()

    binrepos = config.get("binrepos", [])
    if binrepos:
        print("Binary Repositories:\n")
        for br in reversed(binrepos):
            if not br["name"]:
                continue
            print(br["name"])
            print(f"    sync-uri: {br['sync_uri']}")
            print(f"    priority: {br['priority']}")
            print()

    sets = sorted(f"@{s}" for s in _read_world_sets(root))
    if sets:
        print(f"Installed sets: {', '.join(sets)}")

    use_expand = sorted(config["use_expand"])
    unset = []
    for k in [
        "ACCEPT_KEYWORDS",
        "ACCEPT_LICENSE",
        "CFLAGS",
        "CHOST",
        "CONFIG_PROTECT",
        "CONFIG_PROTECT_MASK",
        "CXXFLAGS",
        "DISTDIR",
        "EMERGE_DEFAULT_OPTS",
        "ENV_UNSET",
        "FEATURES",
        "GENTOO_MIRRORS",
        "PKGDIR",
        "PORTAGE_BINHOST",
        "PORTAGE_BUNZIP2_COMMAND",
        "PORTAGE_BZIP2_COMMAND",
        "PORTAGE_TMPDIR",
        "USE",
    ]:
        if k == "ACCEPT_KEYWORDS":
            v = " ".join(sorted(config["accept_keywords"])) or None
        elif k == "ACCEPT_LICENSE":
            v = " ".join(config["accept_license"]) or None
        elif k == "USE":
            prefixes = tuple(f"{ve.lower()}_" for ve in use_expand)
            flags = sorted(
                f for f in config["use_flags"] if not f.startswith(prefixes)
            )
            v = " ".join(flags)
        else:
            v = config["other_vars"].get(k)

        if v is None:
            unset.append(k)
        elif k == "PORTAGE_BZIP2_COMMAND" and v == "bzip2":
            pass
        elif k == "USE":
            line = f'USE="{v}"'
            for ve in use_expand:
                val = config["other_vars"].get(ve)
                if val:
                    line += f' {ve}="{val}"'
            print(line)
        else:
            print(f'{k}="{v}"')
    if unset:
        print(f"Unset:  {', '.join(unset)}")
    print()
    print()
    return 0


def _run_deselect(targets, root, pretend):
    """Ports real action_deselect (lib/_emerge/actions.py, lines
    1740-1835) exactly: needs no repo/config resolution at all, only the
    world file and the vdb. Under --pretend the line verb is `Would
    remove`; without it real portage prints `Removing` and rewrites the
    world / world_sets files -- this reference has no execution machinery,
    so it prints `Removing` but does not write (the real write is
    Rust-only, covered in test_portuale.py, same as -C/--depclean/--prune).

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
        verb = "Would remove" if pretend else "Removing"
        for entry, filename in sorted(discard):
            print(f'>>> {verb} {entry} from "{filename}" favorites file...')
    return 0


def _split_pf(dirname):
    """Splits a vdb directory name (foo-bar-1.2.3-r1) into
    (package, version): the earliest '-'-split point whose right half
    real ververify() accepts as a version. Mirrors pretend.rs's
    split_pf."""
    parts = dirname.split("-")
    for i in range(1, len(parts)):
        candidate = "-".join(parts[i:])
        if ververify(candidate):
            return "-".join(parts[:i]), candidate
    return None


def _installed_cp_versions(root):
    """Every installed (category, package, version, slot) in the vdb --
    real vartree.dbapi.cpv_all(), used only for --unmerge/-C's own
    bare-name resolution. Mirrors pretend.rs's installed_cp_versions."""
    out = []
    vdb = os.path.join(root, "var", "db", "pkg")
    try:
        cats = sorted(os.listdir(vdb))
    except OSError:
        return out
    for category in cats:
        catdir = os.path.join(vdb, category)
        if not os.path.isdir(catdir):
            continue
        for dirname in sorted(os.listdir(catdir)):
            pkgdir = os.path.join(catdir, dirname)
            if not os.path.isdir(pkgdir):
                continue
            split = _split_pf(dirname)
            if split is None:
                continue
            name, version = split
            try:
                with open(os.path.join(pkgdir, "SLOT")) as fh:
                    slot = fh.read().strip().split("/", 1)[0] or "0"
            except OSError:
                slot = "0"
            out.append((category, name, version, slot))
    return out


def _print_unmerge_row(label, versions, color):
    """One '    selected: 1.0 ' / '   protected: none ' row of
    _unmerge_display's per-package block -- label right-justified into 14
    columns (real (mytype + ": ").rjust(14)), each version + trailing
    space, or the literal 'none ' when empty. Real: each `selected`
    version is colorize("UNMERGE_WARN", v+" ") (red), each
    `protected`/`omitted` version colorize("GOOD", v+" ") (green).
    Mirrors pretend.rs's print_unmerge_row."""
    padded = f"{label}: ".rjust(14)
    if not versions:
        print(f"{padded}none ")
    else:
        key = "UNMERGE_WARN" if label == "selected" else "GOOD"
        print(padded + "".join(color.c(key, f"{v} ") for v in versions))


def _resolve_vdb_path_arg(arg, root):
    """Real unmerge.py:137-182's own installed-ebuild-path handling: a
    --unmerge/-C argument that starts with '.' or '/', or ends with
    '.ebuild', is a path into the vdb, not an atom. Returns None if not
    path-shaped, '=cat/pkg-ver' for a valid vdb entry (echoed to stdout,
    like real portage), or raises _CleanupArgsExit after printing the
    diagnostic. Mirrors pretend.rs's resolve_vdb_path_arg -- see its
    docstring (path resolved with realpath; real portage's stray
    print(sp_absx)/print(absx) debug lines omitted)."""
    if not (arg.startswith((".", "/")) or arg.endswith(".ebuild")):
        return None
    if not os.path.exists(arg):
        print(f"\n!!! The path '{arg}' doesn't exist.\n")
        raise _CleanupArgsExit(1)
    absx = os.path.realpath(arg)
    if absx.rsplit("/", 1)[-1].endswith(".ebuild"):
        absx = absx.rsplit("/", 1)[0]
    if not os.path.exists(os.path.join(absx, "CONTENTS")):
        print(f"!!! Not a valid db dir: {absx}")
        raise _CleanupArgsExit(1)
    vdb = os.path.realpath(os.path.join(str(root), "var/db/pkg"))
    if not (absx == vdb or absx.startswith(vdb + "/")):
        print(f"\n!!! {arg} is not inside {vdb}; aborting.\n")
        raise _CleanupArgsExit(1)
    rel = absx[len(vdb) + 1 :]
    if "/" not in rel:
        print(f"\n!!! {arg} cannot be inside {vdb}; aborting.\n")
        raise _CleanupArgsExit(1)
    atom = "=" + rel
    print(atom)
    return atom


def _run_unmerge_pretend(targets, root, config_root, config, preserve_order=False, color=None, action="unmerge"):
    """emerge --pretend --unmerge / -pC <atoms>: real
    _emerge/unmerge.py::_unmerge_display for unmerge_action == "unmerge",
    narrowed to a preview. Mirrors pretend.rs's run_unmerge_pretend --
    see its docstring for the algorithm and the documented cuts
    (set-protection / system-profile warnings, --prune/--depclean,
    the Python-interpreter self-skip).

    preserve_order mirrors real _unmerge_display's `ordered` flag
    (unmerge.py:459): when True the per-package blocks follow `targets`
    order and are not regrouped/re-sorted by cat/pn -- only --depclean's
    topologically-sorted cleanlist sets it."""
    if not targets:
        print(f"emerge: no package atoms given to --{action}", file=sys.stderr)
        return 1

    expanded = []
    # Real root_config.setconfig.active -- the @set targets, excluded
    # from the "still listed in package sets" check below.
    active_sets = {t[1:] for t in targets if t.startswith("@")}
    for target in targets:
        if target in ("@world", "@selected"):
            try:
                expanded.extend(_expand_selected(config_root, root))
            except ResolutionError as e:
                print(f"emerge: {e}", file=sys.stderr)
                return 1
        elif target == "@system":
            expanded.extend(config["system_packages"])
        elif target == "@installed":
            expanded.extend(_installed_set_atoms(root))
        elif target.startswith("@"):
            try:
                seen = set()
                expanded.extend(_resolve_custom_set(config_root, target[1:], seen))
            except ResolutionError as e:
                print(f"emerge: {e}", file=sys.stderr)
                return 1
        else:
            try:
                atom = _resolve_vdb_path_arg(target, root)
            except _CleanupArgsExit as e:
                return e.code
            expanded.append(atom if atom is not None else target)

    print(color.c("darkgreen", ">>> These are the packages that would be unmerged:"))

    portage_self = ("sys-apps", "portage")
    per_cp = {}  # (cat, pkg) -> [selected, protected]
    order = []
    all_selected = set()  # (cat, pkg, version)

    for atom_str in expanded:
        if "/" not in atom_str:
            found = [
                (c, p, v, s)
                for (c, p, v, s) in _installed_cp_versions(root)
                if p == atom_str
            ]
            cats = {c for (c, _, _, _) in found}
            if len(cats) > 1:
                print(
                    f'\n!!! The short package name "{atom_str}" is ambiguous. Please specify',
                    file=sys.stderr,
                )
                print(
                    "!!! one of the following fully-qualified package names instead:\n",
                    file=sys.stderr,
                )
                for n in sorted(f"    {c}/{atom_str}" for c in cats):
                    print(n)
                return 1
            matches = found
        else:
            atom = _parse_atom(atom_str)
            if atom is None:
                print(f"emerge: invalid atom {atom_str!r}", file=sys.stderr)
                return 1
            cat, pkg = atom.cp.split("/", 1)
            matches = []
            for version, slot, sub_slot in installed_candidates(root, cat, pkg):
                cs = f"{cat}/{pkg}-{version}:{slot}/{sub_slot}"
                if match_from_list(atom_str, [cs]):
                    matches.append((cat, pkg, version, slot))

        if not matches:
            print(f"\n--- Couldn't find '{atom_str}' to {action}.")
            continue

        for cat, pkg, version, _slot in matches:
            cp = (cat, pkg)
            if cp not in per_cp:
                per_cp[cp] = [[], []]
                order.append(cp)
            key = (cat, pkg, version)
            if key not in all_selected:
                all_selected.add(key)
                per_cp[cp][0].append(version)

    if not all_selected:
        print(f"\n>>> No packages selected for removal by {action}")
        return 1

    if portage_self in per_cp and per_cp[portage_self][0]:
        for v in per_cp[portage_self][0]:
            print(
                f"!!! Not unmerging package sys-apps/portage-{v} since there is no "
                f"valid reason for Portage to {action} itself.",
                file=sys.stderr,
            )
            all_selected.discard((portage_self[0], portage_self[1], v))
            per_cp[portage_self][1].append(v)
        per_cp[portage_self][0] = []

    if not all_selected:
        print(f"\n>>> No packages selected for removal by {action}")
        return 1

    # Real syslist = root_config.sets["system"].getAtoms() -> the
    # @system cps, for the "is part of your system profile" warning.
    syslist = set()
    for a in config["system_packages"]:
        parsed = _parse_atom(a)
        if parsed is not None:
            syslist.add(tuple(parsed.cp.split("/", 1)))

    # Real _unmerge_display's "still listed in the following package
    # sets" warning: a selected package a user-editable set (reached via
    # world_sets) still lists would be re-pulled on the next @world
    # update -- unless an installed newer version of the same cp in a
    # different slot also matches the set atom (real unmerge.py:421-441's
    # higher_slot). Mirrors pretend.rs's still_listed_parents.
    try:
        installed_sets = [
            (name, atoms)
            for (name, atoms) in _collect_installed_sets(config_root, root)
            if name not in active_sets
        ]
    except ResolutionError as e:
        print(f"emerge: {e}", file=sys.stderr)
        return 1
    for (cat, pkg, version) in sorted(all_selected):
        parents = _still_listed_parents(root, installed_sets, cat, pkg, version)
        if parents:
            parents.sort()
            print(color.c("WARN", f"Package {cat}/{pkg}-{version} is going to be unmerged,"))
            print(color.c("WARN", "but still listed in the following package sets:"))
            print(f"    {', '.join(parents)}\n")

    import functools

    vkey = functools.cmp_to_key(lambda a, b: (vercmp(a, b) or (a > b) - (a < b)))
    all_selected_display = []
    for cp in (order if preserve_order else sorted(order)):
        selected, protected = per_cp[cp]
        if not selected:
            continue
        selected.sort(key=vkey)
        protected.sort(key=vkey)
        omitted = sorted(
            (
                v
                for (v, _slot, _sub) in installed_candidates(root, cp[0], cp[1])
                if v not in selected and v not in protected
            ),
            key=vkey,
        )
        # Real _unmerge_display: `if not (protected or omitted) and cp in
        # syslist` -- a cp fully removed and a @system member. To stderr.
        if not protected and not omitted and cp in syslist:
            print(
                color.c(
                    "BAD",
                    f"\n\n!!! '{cp[0]}/{cp[1]}' is part of your system profile.",
                ),
                file=sys.stderr,
            )
            print(
                color.c("WARN", "!!! Unmerging it may be damaging to your system.\n"),
                file=sys.stderr,
            )
        print(f"\n {cp[0]}/{cp[1]}")
        _print_unmerge_row("selected", selected, color)
        _print_unmerge_row("protected", protected, color)
        _print_unmerge_row("omitted", omitted, color)
        all_selected_display.extend(f"={cp[0]}/{cp[1]}-{v}" for v in selected)

    all_selected_display.sort()
    print(f"\nAll selected packages: {' '.join(all_selected_display)}")
    sel = color.c("UNMERGE_WARN", "'Selected'")
    prot = color.c("GOOD", "'Protected'")
    omit = color.c("GOOD", "'omitted'")
    print(f"\n>>> {sel} packages are slated for removal.")
    print(f">>> {prot} and {omit} packages will not be removed.")
    return 0


def _split_installed_dir(dirname):
    """Split a vdb dir name into (package, version): the earliest
    '-'-split whose right half real ververify() accepts. Mirrors
    portage-repo/src/lib.rs's split_installed_dir."""
    parts = dirname.split("-")
    for i in range(1, len(parts)):
        version = "-".join(parts[i:])
        if ververify(version):
            return "-".join(parts[:i]), version
    return None


def _all_installed_packages(root):
    """Every package under <root>/var/db/pkg -- real
    vartree.dbapi.cpv_all(), (category, package, version, slot) each.
    Mirrors portage-repo/src/lib.rs's all_installed_packages."""
    out = []
    vdb = os.path.join(root, "var", "db", "pkg")
    try:
        cats = sorted(os.listdir(vdb))
    except OSError:
        return out
    for category in cats:
        catdir = os.path.join(vdb, category)
        if not os.path.isdir(catdir):
            continue
        for dirname in sorted(os.listdir(catdir)):
            if not os.path.isdir(os.path.join(catdir, dirname)):
                continue
            split = _split_installed_dir(dirname)
            if split is None:
                continue
            name, version = split
            try:
                with open(os.path.join(catdir, dirname, "SLOT")) as fh:
                    slot = fh.read().strip().split("/", 1)[0] or "0"
            except OSError:
                slot = "0"
            out.append((category, name, version, slot))
    return out


def _render_show_parents(edges):
    """Real show_parents's per-package rendering (actions.py:1274-1291):
    group (parent, atom) edges by parent, render one "<parent> requires
    <atom>, <atom>" line each (atoms sorted by atom package-name
    descending), then sort the lines. Shared by _depclean_cleanlist and
    _prune_cleanlist. Mirrors portage-repo/src/lib.rs's
    render_show_parents."""

    def atom_pkg_name(a):
        parsed = _parse_atom(a)
        return parsed.cp.split("/", 1)[1] if parsed is not None else ""

    by_parent = {}
    for par, atom in edges:
        by_parent.setdefault(par, [])
        if atom not in by_parent[par]:
            by_parent[par].append(atom)
    return sorted(
        "{} requires {}".format(
            par, ", ".join(sorted(atoms, key=atom_pkg_name, reverse=True))
        )
        for par, atoms in by_parent.items()
    )


# --------------------------------------------------------------------------
# NEEDED.ELF.2 soname linkage -- the subset of pretend.rs's `needed_elf`
# module `--depclean-lib-check` needs (real portage.util._dyn_libs). A
# from-scratch port mirroring needed_elf.rs, NOT a wrapper around real
# LinkageMapELF (which needs a full vardbapi). See that module's own doc
# comments for the semantics and documented narrowings.
# --------------------------------------------------------------------------

_APPROX_MULTILIB_CATEGORY = {
    "386": "x86_32",
    "68K": "m68k_32",
    "AARCH64": "arm_64",
    "ALPHA": "alpha_64",
    "ARM": "arm_32",
    "IA_64": "ia64_64",
    "MIPS": "mips_o32",
    "PARISC": "hppa_64",
    "PPC": "ppc_32",
    "PPC64": "ppc_64",
    "S390": "s390_64",
    "SH": "sh_32",
    "SPARC": "sparc_32",
    "SPARC32PLUS": "sparc_32",
    "SPARCV9": "sparc_64",
    "X86_64": "x86_64",
}


def _needed_parse(line):
    """Real NeededEntry.parse -- arch;filename;soname;rpaths;needed, an
    optional 6th multilib_category, extra fields ignored. None for a
    malformed line (<5 fields)."""
    fields = line.split(";")
    if len(fields) < 5:
        return None
    multilib_category = fields[5] if len(fields) > 5 and fields[5] else None
    rpaths = "" if fields[3] == "  -  " else fields[3]
    return {
        "arch": fields[0],
        "filename": fields[1],
        "soname": fields[2],
        "runpaths": [s for s in rpaths.split(":") if s],
        "needed": [s for s in fields[4].split(",") if s],
        "multilib_category": multilib_category,
    }


def _read_all_needed_entries(root):
    """Real LinkageMap.rebuild()'s data-gathering loop: every installed
    package's own vdb NEEDED.ELF.2, parsed, in sorted vdb-listing order."""
    result = []
    pkg_root = os.path.join(root, "var/db/pkg")
    if not os.path.isdir(pkg_root):
        return result
    for category in sorted(os.listdir(pkg_root)):
        cat_path = os.path.join(pkg_root, category)
        if not os.path.isdir(cat_path):
            continue
        for pf in sorted(os.listdir(cat_path)):
            pf_path = os.path.join(cat_path, pf)
            if not os.path.isdir(pf_path):
                continue
            entries = []
            try:
                with open(os.path.join(pf_path, "NEEDED.ELF.2")) as fh:
                    text = fh.read()
            except OSError:
                text = ""
            for line in text.splitlines():
                parsed = _needed_parse(line)
                if parsed is not None:
                    entries.append(parsed)
            result.append((f"{category}/{pf}", entries))
    return result


def _normalize_path(path):
    """Real portage.util.normalize_path -- lexical, no filesystem access."""
    absolute = path.startswith("/")
    out = []
    for seg in path.split("/"):
        if seg in ("", "."):
            continue
        if seg == "..":
            if out and out[-1] != "..":
                out.pop()
            elif not absolute:
                out.append("..")
        else:
            out.append(seg)
    joined = "/".join(out)
    if absolute:
        return "/" + joined
    return joined if joined else "."


def _elf_dirname(path):
    """Real os.path.dirname: everything before the last '/', '/' for a
    top-level absolute path, '' if there is none."""
    i = path.rfind("/")
    if i < 0:
        return ""
    if i == 0:
        return "/"
    return path[:i]


def _expand_origin(rpath, origin):
    return rpath.replace("${ORIGIN}", origin).replace("$ORIGIN", origin)


def _obj_key(root, obj):
    """Real LinkageMap._ObjectKey: (dev, ino) when the object exists
    (follows symlinks), else the literal path string."""
    abs_path = os.path.join(root, obj.lstrip("/"))
    try:
        st = os.stat(abs_path)
    except OSError:
        return ("path", obj)
    return ("inode", st.st_dev, st.st_ino)


def _grab_lines(path):
    """Real grabfile, narrowed: whitespace-normalize, drop an inline
    '#'-token onward, skip empties; missing file -> []."""
    try:
        with open(path) as fh:
            text = fh.read()
    except OSError:
        return []
    out = []
    for line in text.splitlines():
        tokens = []
        for tok in line.split():
            if tok.startswith("#"):
                break
            tokens.append(tok)
        if tokens:
            out.append(" ".join(tokens))
    return out


def _getlibpaths(root, ld_library_path=None):
    """Real getlibpaths: LD_LIBRARY_PATH, /etc/ld.so.conf lines, then the
    /usr/lib + /lib defaults, each normalize_path'd. ld.so.conf.d include
    expansion is deliberately out (same cut as env_update)."""
    rval = list((ld_library_path or "").split(":"))
    rval += _grab_lines(os.path.join(root, "etc/ld.so.conf"))
    rval += ["/usr/lib", "/lib"]
    return [_normalize_path(s) for s in rval if s]


def _linkage_rebuild(root, owner_entries):
    """Real LinkageMap.rebuild()'s indexing logic: build the soname
    providers/consumers map. Returns (libs, obj_properties):
      libs: category -> soname -> {"providers": set(key), "consumers": set(key)}
      obj_properties: key -> {category, needed(set), runpaths, soname,
                              alt_paths(list), owner}
    """
    resolved_by_owner = []
    for owner, entries in owner_entries:
        resolved = []
        for entry in entries:
            category = entry["multilib_category"] or _APPROX_MULTILIB_CATEGORY.get(
                entry["arch"], entry["arch"]
            )
            filename = _normalize_path(entry["filename"])
            origin = _elf_dirname(filename)
            runpaths = [
                _normalize_path(_expand_origin(r, origin)) for r in entry["runpaths"]
            ]
            resolved.append(
                {
                    "owner": owner,
                    "category": category,
                    "filename": filename,
                    "soname": entry["soname"],
                    "runpaths": runpaths,
                    "needed": list(entry["needed"]),
                }
            )
        resolved_by_owner.append(resolved)

    # Real "implicit runpath" inference for same-owner bundled libs.
    for resolved in resolved_by_owner:
        providers = {
            (e["category"], e["soname"]): e["filename"]
            for e in resolved
            if e["soname"]
        }
        for entry in resolved:
            implicit = []
            for soname in entry["needed"]:
                provider_filename = providers.get((entry["category"], soname))
                if provider_filename is None:
                    continue
                provider_dir = _elf_dirname(provider_filename)
                if provider_dir not in entry["runpaths"]:
                    implicit.append(provider_dir)
            entry["runpaths"].extend(implicit)

    libs = {}
    obj_properties = {}
    for resolved in resolved_by_owner:
        for entry in resolved:
            key = _obj_key(root, entry["filename"])
            if key in obj_properties:
                obj_properties[key]["alt_paths"].append(entry["filename"])
                continue
            obj_properties[key] = {
                "category": entry["category"],
                "needed": set(entry["needed"]),
                "runpaths": entry["runpaths"],
                "soname": entry["soname"],
                "alt_paths": [entry["filename"]],
                "owner": entry["owner"],
            }
            arch_map = libs.setdefault(entry["category"], {})
            if entry["soname"]:
                arch_map.setdefault(
                    entry["soname"], {"providers": set(), "consumers": set()}
                )["providers"].add(key)
            for needed_soname in entry["needed"]:
                arch_map.setdefault(
                    needed_soname, {"providers": set(), "consumers": set()}
                )["consumers"].add(key)
    return libs, obj_properties


def _find_consumers(root, libs, obj_properties, defpath, obj, greedy):
    """Real LinkageMap.findConsumers, narrowed to the calling convention
    _lib_consumer_scan uses (obj is a path, no exclude_providers). Returns
    the set of consumer paths that would actually break if `obj` went
    away. Empty set for an object not in the map."""
    obj_key_val = _obj_key(root, obj)
    obj_props = obj_properties.get(obj_key_val)
    if obj_props is None:
        return set()

    # Real "shadowed by another version" check.
    soname = obj_props["soname"]
    if soname:
        soname_link = os.path.join(
            root, _elf_dirname(obj).lstrip("/"), soname
        )
        obj_path = os.path.join(root, obj.lstrip("/"))
        try:
            sl = os.stat(soname_link)
            op = os.stat(obj_path)
            if (op.st_dev, op.st_ino) != (sl.st_dev, sl.st_ino):
                return set()
        except OSError:
            pass

    category = obj_props["category"]
    soname_node = libs.get(category, {}).get(soname)
    defpath_keys = {_obj_key(root, p) for p in defpath}
    satisfied = set()

    if soname_node is not None and not greedy:
        relevant_dir_keys = set()
        for provider_key in soname_node["providers"]:
            if not greedy and provider_key == obj_key_val:
                continue
            provider_props = obj_properties.get(provider_key)
            if provider_props is None:
                continue
            for p in provider_props["alt_paths"]:
                relevant_dir_keys.add(_obj_key(root, _elf_dirname(p)))
        if relevant_dir_keys:
            for consumer_key in soname_node["consumers"]:
                consumer_props = obj_properties.get(consumer_key)
                if consumer_props is None:
                    continue
                path_keys = set(defpath_keys)
                path_keys |= {_obj_key(root, p) for p in consumer_props["runpaths"]}
                if relevant_dir_keys & path_keys:
                    satisfied.add(consumer_key)

    result = set()
    if soname_node is not None:
        objs_dir_key = _obj_key(root, _elf_dirname(obj))
        for consumer_key in soname_node["consumers"]:
            if consumer_key in satisfied:
                continue
            consumer_props = obj_properties.get(consumer_key)
            if consumer_props is None:
                continue
            path_keys = set(defpath_keys)
            path_keys |= {_obj_key(root, p) for p in consumer_props["runpaths"]}
            if objs_dir_key in path_keys:
                result.update(consumer_props["alt_paths"])
    return result


def _lib_consumer_scan(root, cleanlist):
    """Real _calc_depclean's --depclean-lib-check scan (actions.py:
    1381-1546), narrowed to its pure computation. Mirrors pretend.rs's
    lib_consumer_scan -- see its doc comment for the documented
    narrowings. Returns [(provider (c,p,v), [(consumer_cpv, [sonames])])].
    """
    libs, obj_properties = _linkage_rebuild(root, _read_all_needed_entries(root))
    defpath = _getlibpaths(root)
    clean_cpvs = {f"{c}/{p}-{v}" for (c, p, v) in cleanlist}

    out = []
    for (c, p, v) in cleanlist:
        pkg_cpv = f"{c}/{p}-{v}"
        provided = sorted(
            (props["alt_paths"][0], props["soname"])
            for props in obj_properties.values()
            if props["owner"] == pkg_cpv and props["soname"]
        )
        per_consumer = {}
        for lib_path, soname in provided:
            consumers = _find_consumers(
                root, libs, obj_properties, defpath, lib_path, False
            )
            for consumer_path in consumers:
                ckey = _obj_key(root, consumer_path)
                cprops = obj_properties.get(ckey)
                if cprops is None:
                    continue
                c_owner = cprops["owner"]
                if not c_owner or c_owner == pkg_cpv:
                    continue
                if c_owner in clean_cpvs:
                    continue
                per_consumer.setdefault(c_owner, set()).add(soname)
        if per_consumer:
            out.append(
                (
                    (c, p, v),
                    [
                        (consumer, sorted(sonames))
                        for consumer, sonames in sorted(per_consumer.items())
                    ],
                )
            )
    out.sort(key=lambda e: f"{e[0][0]}/{e[0][1]}-{e[0][2]}")
    return out


def _apply_depclean_lib_check(root, result, lib_check, color, recompute):
    """Real _calc_depclean's --depclean-lib-check phase (actions.py:
    1356-1590). Mirrors pretend.rs's apply_depclean_lib_check. `result`
    and the return value are the (cleanlist, required_count, ordered,
    kept_parents) tuple; `recompute(providers)` is a second cleanlist
    pass seeding the protected providers as roots."""
    cleanlist = result[0]
    if not lib_check or not cleanlist:
        return result
    print(">>> Checking for lib consumers...", file=sys.stderr)
    protections = _lib_consumer_scan(root, cleanlist)
    if not protections:
        return result
    print(">>> Assigning files to packages...", file=sys.stderr)

    star = color.c("BAD", " * ")
    for line in (
        "In order to avoid breakage of link level dependencies, one or more",
        "packages will not be removed. This can be solved by rebuilding the",
        "packages that pulled them in.",
    ):
        print(f"{star}{line}", file=sys.stderr)
    for (c, p, v), consumers in protections:
        print(star, file=sys.stderr)
        print(f"{star}  {c}/{p}-{v} pulled in by:", file=sys.stderr)
        for consumer, sonames in consumers:
            print(f"{star}    {consumer} needs {', '.join(sonames)}", file=sys.stderr)
    print(star, file=sys.stderr)

    print(">>> Adding lib providers to graph...", file=sys.stderr)
    return recompute([prov for (prov, _c) in protections])


def _unresolved_runtime_deps(root, kept, installed, libc_cps):
    """Real _calc_depclean's unresolved_deps() check (actions.py:1137-1245):
    a *kept* installed package's hard runtime dep (RDEPEND/PDEPEND --
    real dep.priority > UnmergeDepPriority.SOFT; DEPEND/BDEPEND are
    buildtime = SOFT and never trip the halt) that no installed package
    satisfies. Mirrors portage-repo's unresolved_runtime_deps -- see its
    docstring for the narrowings (|| groups skipped, libc-provider atoms
    skipped, the unevaluated-atom readability case not reproduced)."""
    cand = [(c, p, f"{c}/{p}-{v}:{s}") for (c, p, v, s) in installed]

    def matches_any(atom_str, atom):
        cat, pn = atom.cp.split("/", 1)
        for (c, p, cs) in cand:
            if c == cat and p == pn and match_from_list(atom_str, [cs]):
                return True
        return False

    def hard_atoms(struct):
        i = 0
        while i < len(struct):
            item = struct[i]
            if item == "||":
                i += 2  # skip '||' and its alternatives group
                continue
            if isinstance(item, list):
                yield from hard_atoms(item)
            else:
                yield item
            i += 1

    out = []
    for (c, p, v, s) in kept:
        use_flags = _read_vdb_flag_set(root, c, p, v, "USE")
        parent_cpv = f"{c}/{p}-{v}"
        for dep_key in ("RDEPEND", "PDEPEND"):
            depstr = _read_vdb_string(root, c, p, v, dep_key)
            if not depstr.strip():
                continue
            try:
                struct = use_reduce(depstr, uselist=list(use_flags))
            except InvalidDependString:
                continue
            for atom_str in hard_atoms(struct):
                if atom_str.startswith("!"):
                    continue
                atom = _parse_atom(atom_str)
                if atom is None:
                    continue
                if atom.cp in libc_cps:
                    continue
                if not matches_any(atom_str, atom):
                    edge = (atom_str, parent_cpv)
                    if edge not in out:
                        out.append(edge)
    out.sort()
    return out


def _depclean_unresolved_halt(unresolved, is_prune, color):
    """Real _calc_depclean's unresolved_deps() halt (actions.py:1177-1248):
    the bad(" * ")-prefixed `Dependencies could not be completely
    resolved ...` block (logging.ERROR -> stderr) + exit 1 without
    removing anything. Mirrors pretend.rs's depclean_unresolved_halt.
    Returns 1 when it halted, None to carry on."""
    if not unresolved:
        return None
    star = color.c("BAD", " * ")
    print(f"{star}Dependencies could not be completely resolved due to", file=sys.stderr)
    print(
        f"{star}the following required packages not being installed:", file=sys.stderr
    )
    for atom, parent in unresolved:
        print(star, file=sys.stderr)
        print(f"{star}  {atom} pulled in by:", file=sys.stderr)
        print(f"{star}    {parent}", file=sys.stderr)
    print(star, file=sys.stderr)
    # Real textwrap.wrap(..., 65) -- pinned, it never changes.
    for line in (
        "Have you forgotten to do a complete update prior to depclean? The",
        "most comprehensive command for this purpose is as follows:",
        "",
    ):
        print(f"{star}{line}", file=sys.stderr)
    print(
        f"{star}  "
        + color.c("GOOD", "emerge --update --newuse --deep --with-bdeps=y @world"),
        file=sys.stderr,
    )
    for line in (
        "",
        "Note that the --with-bdeps=y option is not required in many",
        "situations. Refer to the emerge manual page (run `man emerge`)",
        "for more information about --with-bdeps.",
        "",
        "Also, note that it may be necessary to manually uninstall",
        "packages that no longer exist in the repository, since it may not",
        "be possible to satisfy their dependencies.",
    ):
        print(f"{star}{line}", file=sys.stderr)
    if is_prune:
        print(star, file=sys.stderr)
        print(
            f"{star}If you would like to ignore dependencies then use "
            + color.c("GOOD", "--nodeps")
            + ".",
            file=sys.stderr,
        )
    return 1


def _depclean_cleanlist(
    root, world_seeds, system_atoms, args, deselect=True, lib_protected_providers=()
):
    """Real emerge --depclean's removal list (_calc_depclean +
    create_cleanlist). No `args`: roots = installed pkgs @world ∪ @system
    match; cleanlist = every installed pkg none reach. With `args`: real
    _complete_graph drops the world "selected" plain atoms (deselect
    default) and makes every non-`args` installed pkg a protected root,
    so roots = @system ∪ {non-arg installed}, cleanlist = the
    args-matched pkgs nothing reaches. `world_seeds` is (atom, set_label)
    pairs (label used only for the --verbose reverse-dep display).
    Returns (cleanlist, required_count, ordered, kept_parents) --
    kept_parents mirrors real create_cleanlist's `elif "--verbose":
    show_parents(pkg)`: [(pkg tuple, [rendered line, ...])] for every
    kept pkg, cpv-sorted. Mirrors portage-repo/src/lib.rs's
    depclean_cleanlist -- see its docstring for the documented
    narrowings.

    The build-time keys DEPEND/BDEPEND are followed as well: real
    _calc_depclean builds its graph via the full depgraph in "remove"
    mode, where create_depgraph_params(myopts, "remove") sets
    bdeps="auto" and depgraph.py:4208-4213 only drops DEPEND/BDEPEND from
    a removal walk when --with-bdeps=n is passed explicitly. So a package
    that is only a build-time dep of a kept package is itself kept."""
    installed = _all_installed_packages(root)

    def matches_atom(atom_str):
        parsed = _parse_atom(atom_str)
        if parsed is None:
            return []
        cat, pkg = parsed.cp.split("/", 1)
        found = []
        for (c, p, v, s) in installed:
            if c != cat or p != pkg:
                continue
            if match_from_list(atom_str, [f"{c}/{p}-{v}:{s}"]):
                found.append((c, p, v, s))
        return found

    def matched_by_args(pkg):
        c, p, v, s = pkg
        for a in args:
            parsed = _parse_atom(a)
            if (
                parsed is not None
                and tuple(parsed.cp.split("/", 1)) == (c, p)
                and match_from_list(a, [f"{c}/{p}-{v}:{s}"])
            ):
                return True
        return False

    reachable = set()
    queue = []
    # Real _dynamic_config._parent_atoms: child key -> [(parent desc,
    # atom)]; parent desc is a cpv or an @set label.
    parent_atoms = {}

    def add_edge(child_key, parent_desc, atom):
        parent_atoms.setdefault(child_key, []).append((parent_desc, atom))

    def seed(c, p, v):
        if (c, p, v) not in reachable:
            reachable.add((c, p, v))
            queue.append((c, p, v))

    seed_pairs = [(a, "@system") for a in system_atoms]
    # `args` mode drops the world "selected" seeds (real _complete_graph
    # empties selected_set) -- unless --deselect=n keeps them.
    if not args or not deselect:
        seed_pairs += [(a, label) for (a, label) in world_seeds]
    for atom_str, label in seed_pairs:
        for (c, p, v, _s) in matches_atom(atom_str):
            add_edge((c, p, v), label, atom_str)
            seed(c, p, v)
    if args:
        for pkg in installed:
            if not matched_by_args(pkg):
                seed(pkg[0], pkg[1], pkg[2])

    # --depclean-lib-check feedback: a provider the caller's NEEDED.ELF.2
    # scan found is still needed at link level becomes a root, so it and
    # its own dependency closure drop out of the cleanlist (real
    # _calc_depclean's resolver._add_pkg + _complete_graph). No parent
    # edge for the provider itself -- matches portage-repo.
    for prov in lib_protected_providers:
        seed(prov[0], prov[1], prov[2])

    while queue:
        c, p, v = queue.pop()
        parent_cpv = f"{c}/{p}-{v}"
        use_flags = _read_vdb_flag_set(root, c, p, v, "USE")
        for dep_key in ("RDEPEND", "PDEPEND", "DEPEND", "BDEPEND"):
            depstr = _read_vdb_string(root, c, p, v, dep_key)
            if not depstr.strip():
                continue
            atoms = _flat_dep_atoms(depstr, use_flags)
            if atoms is None:
                continue
            for atom_str in atoms:
                for (dc, dp, dv, _ds) in matches_atom(atom_str):
                    add_edge((dc, dp, dv), parent_cpv, atom_str)
                    if (dc, dp, dv) not in reachable:
                        reachable.add((dc, dp, dv))
                        queue.append((dc, dp, dv))

    import functools

    vkey = functools.cmp_to_key(lambda a, b: (vercmp(a, b) or (a > b) - (a < b)))
    cleanlist = sorted(
        (
            (c, p, v)
            for pkg in installed
            for (c, p, v) in [(pkg[0], pkg[1], pkg[2])]
            if (c, p, v) not in reachable and (not args or matched_by_args(pkg))
        ),
        key=lambda t: (t[0], t[1], vkey(t[2])),
    )

    kept = sorted(
        (
            (c, p, v)
            for pkg in installed
            for (c, p, v) in [(pkg[0], pkg[1], pkg[2])]
            if (c, p, v) in reachable and (not args or matched_by_args(pkg))
        ),
        key=lambda t: (t[0], t[1], vkey(t[2])),
    )
    kept_parents = []
    for k in kept:
        lines = _render_show_parents(parent_atoms.get(k) or [])
        if lines:
            kept_parents.append((k, lines))

    # Real unresolved_deps() -- over every kept installed package.
    all_kept = [(c, p, v, s) for (c, p, v, s) in installed if (c, p, v) in reachable]
    unresolved = _unresolved_runtime_deps(
        root, all_kept, installed, _libc_provider_cps(root)
    )

    slot_of = {(c, p, v): s for (c, p, v, s) in installed}
    ordered, cleanlist = _topological_removal_order(root, cleanlist, slot_of)
    return cleanlist, len(reachable), ordered, kept_parents, unresolved


def _topological_removal_order(root, cleanlist, slot_of):
    """Real _calc_depclean's own unmerge-order pass (actions.py:1591-1731):
    build a digraph over the cleanlist (edge depender -> dep when one
    member satisfies another's DEPEND/RDEPEND/BDEPEND/PDEPEND/IDEPEND,
    flattened against the depender's vdb USE), then topologically sort so
    each package is unmerged before the ones it depends on. Returns
    (ordered, cleanlist); ordered is False (and the input order kept) only
    when the digraph has no edges.

    Each edge carries its key's UnmergeDepPriority (higher = harder):
    IDEPEND 0, RDEPEND -2 (-> -1 when the atom is slot_operator_built,
    real runtime_slot_op, bug 916135), PDEPEND -3, DEPEND/BDEPEND -4;
    highest wins for a repeated (i, j). True roots (nothing left depends
    on them) are emitted all at once, cpv-descending; a genuine cycle
    falls back to real portage's ignore_priority_range scan
    ([-4, -3, -2, -1, 0]) and pops just ONE node (cpv-max) whose every
    remaining incoming edge is <= ignore_priority. Mirrors
    portage-repo/src/lib.rs's topological_removal_order."""
    n = len(cleanlist)
    if n < 2:
        return False, cleanlist
    cand = [f"{c}/{p}-{v}:{slot_of[(c, p, v)]}" for (c, p, v) in cleanlist]
    deps = [set() for _ in range(n)]
    edge_prio = {}
    for i, (c, p, v) in enumerate(cleanlist):
        use_flags = _read_vdb_flag_set(root, c, p, v, "USE")
        for dep_key in ("DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"):
            depstr = _read_vdb_string(root, c, p, v, dep_key)
            if not depstr.strip():
                continue
            atoms = _flat_dep_atoms(depstr, use_flags)
            if atoms is None:
                continue
            base_prio = {"IDEPEND": 0, "RDEPEND": -2, "PDEPEND": -3}.get(dep_key, -4)
            for atom_str in atoms:
                parsed = _parse_atom(atom_str)
                if parsed is None:
                    continue
                acat, apkg = parsed.cp.split("/", 1)
                slot_op_built = (
                    parsed.slot_operator == "=" and parsed.sub_slot is not None
                )
                prio = -1 if (dep_key == "RDEPEND" and slot_op_built) else base_prio
                for j, (jc, jp, _jv) in enumerate(cleanlist):
                    if i == j or jc != acat or jp != apkg:
                        continue
                    if match_from_list(atom_str, [cand[j]]):
                        deps[i].add(j)
                        edge_prio[(i, j)] = max(edge_prio.get((i, j), prio), prio)
    if not any(deps):
        return False, cleanlist

    import functools

    vk = functools.cmp_to_key(lambda a, b: (vercmp(a, b) or (a > b) - (a < b)))

    def cpv_key(idx):
        c, p, v = cleanlist[idx]
        return (c, p, vk(v))

    rev = [[] for _ in range(n)]
    indeg = [0] * n
    for i, d in enumerate(deps):
        for j in d:
            indeg[j] += 1
            rev[j].append(i)
    done = [False] * n
    result = []

    def remove(k):
        done[k] = True
        for j in deps[k]:
            if not done[j]:
                indeg[j] -= 1
        result.append(cleanlist[k])

    while len(result) < n:
        ready = [k for k in range(n) if not done[k] and indeg[k] == 0]
        if ready:
            ready.sort(key=cpv_key, reverse=True)
            for k in ready:
                remove(k)
            continue
        # Genuine cycle -- real ignore_priority_range scan. Pop ONE.
        for ignore in (-4, -3, -2, -1, 0):
            pool = [
                k
                for k in range(n)
                if not done[k]
                and all(edge_prio[(i, k)] <= ignore for i in rev[k] if not done[i])
            ]
            if not pool:
                continue
            pool.sort(key=cpv_key, reverse=True)
            remove(pool[0])
            break
    return True, result


def _resolve_cleanup_args(targets, root, action):
    """Real action_depclean's own argument handling (actions.py:848-863),
    shared by --depclean and --prune: resolve bare names against the vdb
    (ambiguous -> '!!! ... ambiguous' + Exception(1)), then check each
    atom -- one matching nothing prints '--- Couldn't find ...' (stderr),
    and if none match, print '>>> No packages selected for removal by
    <action>' + raise _CleanupArgsExit(1). Empty targets -> []. Mirrors
    pretend.rs's resolve_cleanup_args."""
    args = []
    scan = _installed_cp_versions(root)
    for t in targets:
        if "/" in t:
            args.append(t)
            continue
        cats = sorted({c for (c, p, _v, _s) in scan if p == t})
        if len(cats) == 0:
            args.append(t)
        elif len(cats) == 1:
            args.append(f"{cats[0]}/{t}")
        else:
            print(
                f'\n!!! The short package name "{t}" is ambiguous. Please specify',
                file=sys.stderr,
            )
            print(
                "!!! one of the following fully-qualified package names instead:\n",
                file=sys.stderr,
            )
            for n in sorted(f"    {c}/{t}" for c in cats):
                print(n)
            raise _CleanupArgsExit(1)

    if args:
        any_matched = False
        for a in args:
            parsed = _parse_atom(a)
            matched = parsed is not None and any(
                match_from_list(a, [f"{parsed.cp.split('/', 1)[0]}/{parsed.cp.split('/', 1)[1]}-{v}:{s}/{sub}"])
                for (v, s, sub) in installed_candidates(
                    root, *parsed.cp.split("/", 1)
                )
            )
            if matched:
                any_matched = True
            else:
                print(
                    f"--- Couldn't find '{a.replace('null/', '')}' to {action}.",
                    file=sys.stderr,
                )
        if not any_matched:
            print(f">>> No packages selected for removal by {action}")
            raise _CleanupArgsExit(1)
    return args


class _CleanupArgsExit(Exception):
    def __init__(self, code):
        super().__init__(code)
        self.code = code


def _run_prune_pretend(
    targets, root, config_root, config, color, verbose=False, lib_check=True
):
    """emerge --pretend --prune / -pP (real action_depclean with
    action="prune"). Unlike --depclean, real action_depclean returns
    right after the unmerge() preview (actions.py:888): no ' * ' advisory
    block, no stats block. The empty-cleanlist message gains a '>>> To
    ignore dependencies, use --nodeps' line. Mirrors pretend.rs's
    run_prune_pretend. See _prune_cleanlist for the removal-set
    semantics."""
    try:
        args = _resolve_cleanup_args(targets, root, "prune")
    except _CleanupArgsExit as e:
        return e.code

    result = _prune_cleanlist(root, args)
    # Real _calc_depclean's unresolved_deps() safety halt -- serves
    # action in ("depclean", "prune"), so it applies here too (with the
    # prune-only `use --nodeps` trailer).
    halt = _depclean_unresolved_halt(result[4], True, color)
    if halt is not None:
        return halt
    # Real _calc_depclean serves action in ("depclean", "prune"), so
    # --depclean-lib-check applies to --prune too.
    result = _apply_depclean_lib_check(
        root,
        result,
        lib_check,
        color,
        lambda providers: _prune_cleanlist(root, args, providers),
    )
    cleanlist, _required_count, ordered, kept_parents, _unresolved = result

    # Real create_cleanlist's prune branch prints show_parents(pkg) inline
    # while building the removal list -- before the removal-order line /
    # empty message.
    if verbose:
        for (c, p, v), lines in kept_parents:
            print(f"  {c}/{p}-{v} pulled in by:")
            for line in lines:
                print(f"    {line}")
            print()

    if not cleanlist:
        print(">>> No packages selected for removal by prune")
        if not verbose:
            print(">>> To see reverse dependencies, use --verbose")
        print(">>> To ignore dependencies, use --nodeps")
        return 0

    print(">>> Calculating removal order...")
    cpv_atoms = [f"={c}/{p}-{v}" for (c, p, v) in cleanlist]
    return _run_unmerge_pretend(
        cpv_atoms, root, config_root, config, preserve_order=ordered, color=color
    )


def _prune_nodeps_selection(root, args):
    """Real emerge --prune --nodeps's own selection (unmerge.py:245-272):
    NO dependency check -- for every cp with >1 matched version, protect
    the highest (best) version, select every other. Returns a cp-sorted
    list of (cat, pkg, best_version, [other_versions asc]). Mirrors
    portage-repo/src/lib.rs's prune_nodeps_selection -- see its docstring
    for the args-vs-no-args split and the COUNTER-tiebreak narrowing."""
    import functools

    vk = functools.cmp_to_key(lambda a, b: (vercmp(a, b) or (a > b) - (a < b)))
    installed = _all_installed_packages(root)  # (c, p, v, s)
    if args:
        matched = [
            (c, p, v, s)
            for (c, p, v, s) in installed
            if any(
                (parsed := _parse_atom(a)) is not None
                and tuple(parsed.cp.split("/", 1)) == (c, p)
                and match_from_list(a, [f"{c}/{p}-{v}:{s}"])
                for a in args
            )
        ]
    else:
        matched = installed

    by_cp = {}
    for (c, p, v, _s) in matched:
        by_cp.setdefault((c, p), [])
        if v not in by_cp[(c, p)]:
            by_cp[(c, p)].append(v)

    out = []
    for (c, p), versions in by_cp.items():
        versions = sorted(versions, key=vk)
        if len(versions) < 2:
            continue
        out.append((c, p, versions[-1], versions[:-1]))
    out.sort(key=lambda t: (t[0], t[1]))
    return out


def _clean_selection(root, args):
    """Real emerge --clean's own selection (unmerge.py:274-293): like
    --prune --nodeps but PER SLOT. Mirrors portage-repo's clean_selection."""
    import functools

    vk = functools.cmp_to_key(lambda a, b: (vercmp(a, b) or (a > b) - (a < b)))
    installed = _all_installed_packages(root)
    if args:
        matched = [
            (c, p, v, s)
            for (c, p, v, s) in installed
            if any(
                (parsed := _parse_atom(a)) is not None
                and tuple(parsed.cp.split("/", 1)) == (c, p)
                and match_from_list(a, [f"{c}/{p}-{v}:{s}"])
                for a in args
            )
        ]
    else:
        matched = installed

    by_cp_slot = {}
    for (c, p, v, s) in matched:
        by_cp_slot.setdefault((c, p, s), [])
        if v not in by_cp_slot[(c, p, s)]:
            by_cp_slot[(c, p, s)].append(v)

    by_cp = {}
    for (c, p, _s), versions in by_cp_slot.items():
        versions = sorted(versions, key=vk)
        if len(versions) < 2:
            continue
        best, others = versions[-1], versions[:-1]
        cur = by_cp.get((c, p))
        if cur is None:
            by_cp[(c, p)] = [best, list(others)]
        else:
            cur[1].extend(others)
            if (vercmp(best, cur[0]) or 0) > 0:
                cur[0] = best

    out = [
        (c, p, best, sorted(set(others), key=vk))
        for (c, p), (best, others) in by_cp.items()
    ]
    out.sort(key=lambda t: (t[0], t[1]))
    return out


def _run_prune_nodeps_pretend(targets, root, config_root, color):
    return _run_prune_nodeps_or_clean(targets, root, config_root, color, False)


def _run_clean_pretend(targets, root, config_root, color):
    return _run_prune_nodeps_or_clean(targets, root, config_root, color, True)


def _run_prune_nodeps_or_clean(targets, root, config_root, color, is_clean):
    """emerge --pretend --prune --nodeps / emerge --pretend --clean
    (actions.py:2684-2697). --clean uses a per-slot selection, has no
    sys-apps/portage self-skip, and names 'clean' in the empty message.
    Mirrors pretend.rs's run_prune_nodeps_or_clean."""
    action = "clean" if is_clean else "prune"
    try:
        args = _resolve_cleanup_args(targets, root, action)
    except _CleanupArgsExit as e:
        return e.code

    selection = (
        _clean_selection(root, args) if is_clean else _prune_nodeps_selection(root, args)
    )

    print(color.c("darkgreen", ">>> These are the packages that would be unmerged:"))

    # sys-apps/portage self-skip (realistically dead code; NOT for --clean).
    if not is_clean:
        fixed = []
        for (c, p, best, others) in selection:
            if (c, p) == ("sys-apps", "portage"):
                for v in others:
                    print(
                        f"!!! Not unmerging package sys-apps/portage-{v} since there is no "
                        "valid reason for Portage to prune itself.",
                        file=sys.stderr,
                    )
                others = []
            fixed.append((c, p, best, others))
        selection = fixed

    total_selected = sum(len(others) for (_c, _p, _b, others) in selection)
    if total_selected == 0:
        if not args:
            print("\n>>> No outdated packages were found on your system.")
        else:
            print(f"\n>>> No packages selected for removal by {action}")
        return 1

    try:
        installed_sets = _collect_installed_sets(config_root, root)
    except ResolutionError as e:
        print(f"emerge: {e}", file=sys.stderr)
        return 1
    selected_flat = sorted(
        (c, p, v)
        for (c, p, _b, others) in selection
        for v in others
    )
    for (c, p, v) in selected_flat:
        parents = _still_listed_parents(root, installed_sets, c, p, v)
        if parents:
            parents.sort()
            print(color.c("WARN", f"Package {c}/{p}-{v} is going to be unmerged,"))
            print(color.c("WARN", "but still listed in the following package sets:"))
            print(f"    {', '.join(parents)}\n")

    all_selected_display = []
    for (c, p, best, others) in selection:
        if not others:
            continue
        print(f"\n {c}/{p}")
        _print_unmerge_row("selected", others, color)
        _print_unmerge_row("protected", [best], color)
        _print_unmerge_row("omitted", [], color)
        all_selected_display.extend(f"={c}/{p}-{v}" for v in others)

    all_selected_display.sort()
    print(f"\nAll selected packages: {' '.join(all_selected_display)}")
    sel = color.c("UNMERGE_WARN", "'Selected'")
    prot = color.c("GOOD", "'Protected'")
    omit = color.c("GOOD", "'omitted'")
    print(f"\n>>> {sel} packages are slated for removal.")
    print(f">>> {prot} and {omit} packages will not be removed.")
    return 0


def _prune_cleanlist(root, args, lib_protected_providers=()):
    """Real emerge --prune's removal list (_calc_depclean with
    action="prune" -- actions.py:1059-1110 + create_cleanlist's prune
    branch). Removes superseded installed versions: for every cp with >1
    version installed, the non-highest ones, kept only if something needs
    that exact old version. With no args, args_set auto-fills with every
    multi-version cp. Returns (cleanlist, required_count, ordered,
    kept_parents) -- kept_parents is real create_cleanlist's prune-branch
    `elif "--verbose": show_parents(pkg)` for every args_set-matched kept
    version. Mirrors portage-repo/src/lib.rs's prune_cleanlist -- see its
    docstring for the seed/candidate split and the deliberate cuts."""
    import functools

    installed = _all_installed_packages(root)  # (c, p, v, s)

    vk = functools.cmp_to_key(lambda a, b: (vercmp(a, b) or (a > b) - (a < b)))
    highest = {}
    for (c, p, v, _s) in installed:
        cur = highest.get((c, p))
        if cur is None or (vercmp(v, cur) or 0) > 0:
            highest[(c, p)] = v

    def is_highest(pkg):
        c, p, v, _s = pkg
        return highest.get((c, p)) == v

    counts = {}
    for (c, p, _v, _s) in installed:
        counts[(c, p)] = counts.get((c, p), 0) + 1
    multi_version = {cp for cp, n in counts.items() if n > 1}

    def matched_by_args(pkg):
        c, p, v, s = pkg
        if not args:
            return (c, p) in multi_version
        cs = f"{c}/{p}-{v}:{s}"
        for a in args:
            parsed = _parse_atom(a)
            if (
                parsed is not None
                and tuple(parsed.cp.split("/", 1)) == (c, p)
                and match_from_list(a, [cs])
            ):
                return True
        return False

    def is_candidate(pkg):
        return not is_highest(pkg) and matched_by_args(pkg)

    def matches_atom(atom_str):
        parsed = _parse_atom(atom_str)
        if parsed is None:
            return []
        cat, pkg = parsed.cp.split("/", 1)
        return [
            (c, p, v, s)
            for (c, p, v, s) in installed
            if c == cat and p == pkg and match_from_list(atom_str, [f"{c}/{p}-{v}:{s}"])
        ]

    reachable = set()
    queue = []
    # Real _parent_atoms -- only dep-walk edges (a Package parent); the
    # prune seeds' protected-set / bare-cp parents are filtered by
    # show_parents, so no seed edge is recorded.
    parent_atoms = {}
    for pkg in installed:
        if not is_candidate(pkg):
            key = (pkg[0], pkg[1], pkg[2])
            if key not in reachable:
                reachable.add(key)
                queue.append(key)
    # --depclean-lib-check feedback -- see _depclean_cleanlist.
    for prov in lib_protected_providers:
        key = (prov[0], prov[1], prov[2])
        if key not in reachable:
            reachable.add(key)
            queue.append(key)
    while queue:
        c, p, v = queue.pop()
        parent_cpv = f"{c}/{p}-{v}"
        use_flags = _read_vdb_flag_set(root, c, p, v, "USE")
        for dep_key in ("RDEPEND", "PDEPEND", "DEPEND", "BDEPEND"):
            depstr = _read_vdb_string(root, c, p, v, dep_key)
            if not depstr.strip():
                continue
            atoms = _flat_dep_atoms(depstr, use_flags)
            if atoms is None:
                continue
            for atom_str in atoms:
                for (dc, dp, dv, _ds) in matches_atom(atom_str):
                    parent_atoms.setdefault((dc, dp, dv), []).append((parent_cpv, atom_str))
                    if (dc, dp, dv) not in reachable:
                        reachable.add((dc, dp, dv))
                        queue.append((dc, dp, dv))

    cleanlist = sorted(
        (
            (c, p, v)
            for (c, p, v, s) in installed
            if is_candidate((c, p, v, s)) and (c, p, v) not in reachable
        ),
        key=lambda t: (t[0], t[1], vk(t[2])),
    )

    # Real create_cleanlist's prune branch: `elif "--verbose":
    # show_parents(pkg)` for every args_set-matched *kept* version with a
    # non-protected-set parent edge, cpv-sorted.
    kept = sorted(
        (
            (c, p, v)
            for (c, p, v, s) in installed
            if (c, p, v) in reachable and matched_by_args((c, p, v, s))
        ),
        key=lambda t: (t[0], t[1], vk(t[2])),
    )
    kept_parents = []
    for k in kept:
        lines = _render_show_parents(parent_atoms.get(k) or [])
        if lines:
            kept_parents.append((k, lines))

    all_kept = [(c, p, v, s) for (c, p, v, s) in installed if (c, p, v) in reachable]
    unresolved = _unresolved_runtime_deps(
        root, all_kept, installed, _libc_provider_cps(root)
    )

    slot_of = {(c, p, v): s for (c, p, v, s) in installed}
    ordered, cleanlist = _topological_removal_order(root, cleanlist, slot_of)
    return cleanlist, len(reachable), ordered, kept_parents, unresolved


def _run_depclean_pretend(
    targets, root, config_root, config, color, verbose=False, lib_check=True, deselect=True
):
    """emerge --pretend --depclean / -pc (real action_depclean +
    _calc_depclean). `deselect` is real action_depclean's
    `myopts.get("--deselect") != "n"` (default True) -- `-pc <atoms>
    --deselect=n` keeps the world set as a protection root. Mirrors
    pretend.rs's run_depclean_pretend."""
    try:
        args = _resolve_cleanup_args(targets, root, "depclean")
    except _CleanupArgsExit as e:
        return e.code

    if not args:
        # Real action_depclean: each line is colorize("WARN", " * ")
        # (yellow) + text, each backtick-wrapped command good("`…`")
        # (green). None = real's leading bare writemsg_stdout("\n").
        star = color.c("WARN", " * ")

        def _green_ticks(text):
            out, rest = "", text
            while "`" in rest:
                open_ = rest.index("`")
                out += rest[:open_]
                after = rest[open_ + 1 :]
                if "`" in after:
                    close = after.index("`")
                    out += color.c("GOOD", "`" + after[:close] + "`")
                    rest = after[close + 1 :]
                else:
                    out += "`"
                    rest = after
            return out + rest

        libcheck_off_paragraph = (
            (
                "Depclean may break link level dependencies. Thus, it is",
                "recommended to use a tool such as `revdep-rebuild` (from",
                "app-portage/gentoolkit) in order to detect such breakage.",
                "",
            )
            if not lib_check
            else ()
        )
        for text in (
            (None,)
            + libcheck_off_paragraph
            + (
            "Always study the list of packages to be cleaned for any obvious",
            "mistakes. Packages that are part of the world set will always",
            "be kept.  They can be manually added to this set with",
            "`emerge --noreplace <atom>`.  Packages that are listed in",
            "package.provided (see portage(5)) will be removed by",
            "depclean, even if they are part of the world set.",
            "",
            "As a safety measure, depclean will not remove any packages",
            "unless *all* required dependencies have been resolved.  As a",
            "consequence of this, it often becomes necessary to run ",
            "`emerge --update --newuse --deep @world` prior to depclean.",
            )
        ):
            if text is None:
                print()
            else:
                print(f"{star}{_green_ticks(text)}")

    world_seeds = []
    try:
        world_seeds.extend((a, "@selected") for a in _read_world_atoms(root))
        for name in _read_world_sets(root):
            seen = set()
            world_seeds.extend(
                (a, f"@{name}") for a in _resolve_custom_set(config_root, name, seen)
            )
    except ResolutionError as e:
        print(f"emerge: {e}", file=sys.stderr)
        return 1
    world_atom_count = len({a for (a, _l) in world_seeds})

    result = _depclean_cleanlist(
        root, world_seeds, config["system_packages"], args, deselect=deselect
    )
    # Real _calc_depclean's unresolved_deps() safety halt (actions.py:1247)
    # -- checked before the lib scan.
    halt = _depclean_unresolved_halt(result[4], False, color)
    if halt is not None:
        return halt
    # Real _calc_depclean's --depclean-lib-check phase: a cleanlist
    # package still needed at link level by a survivor is kept (and its
    # own deps with it, via a second _depclean_cleanlist pass).
    result = _apply_depclean_lib_check(
        root,
        result,
        lib_check,
        color,
        lambda providers: _depclean_cleanlist(
            root,
            world_seeds,
            config["system_packages"],
            args,
            deselect=deselect,
            lib_protected_providers=providers,
        ),
    )
    cleanlist, required_count, ordered, kept_parents, _unresolved = result
    installed_total = len(_all_installed_packages(root))

    # Real create_cleanlist's `elif "--verbose": show_parents(pkg)` --
    # after the ` * ` advisory, before the removal-order / empty message.
    if verbose:
        for (c, p, v), lines in kept_parents:
            print(f"  {c}/{p}-{v} pulled in by:")
            for line in lines:
                print(f"    {line}")
            print()

    def stats():
        print(f"Packages installed:   {installed_total}")
        print(f"Packages in world:    {world_atom_count}")
        print(f"Packages in system:   {len(config['system_packages'])}")
        print(f"Required packages:    {required_count}")
        print(f"Number to remove:     {len(cleanlist)}")

    if not cleanlist:
        print(">>> No packages selected for removal by depclean")
        if not verbose:
            print(">>> To see reverse dependencies, use --verbose")
        stats()
        return 0

    print(">>> Calculating removal order...")
    cpv_atoms = [f"={c}/{p}-{v}" for (c, p, v) in cleanlist]
    rc = _run_unmerge_pretend(
        cpv_atoms, root, config_root, config, preserve_order=ordered, color=color
    )
    stats()
    return rc


def _attr_display_field(
    interactive,
    new,
    force_reinstall,
    new_slot,
    replace,
    fetch_restrict,
    fetch_restrict_satisfied,
    remote_binary,
    new_version,
    downgrade,
    mask,
    color,
):
    """Real PkgAttrDisplay.__str__ (_emerge/resolver/output_helpers.py):
    the fixed-width status field rendered inside the "[ebuild ...]"
    bracket, exactly "[{pkg.type_name} {attr_display}]". One column per
    attribute, a literal space where the attribute is absent, in this
    exact order:

      0. I  -- interactive
      1. N  -- new; r instead when force_reinstall (this pilot has no
               --emptytree/arg.force_reinstall concept, so always N or
               space here -- a plain reinstall shows R at col 2)
      2. S  -- new_slot; R instead when replace (the cpv is already
               installed -- every Reinstall outcome)
      3. f/F/g -- fetch-restrict satisfied / unsatisfied / remote binary
               (g out of scope, needs --getbinpkg)
      4. U  -- new_version (an in-slot version change -- Upgrade/Downgrade)
      5. D  -- downgrade
      6. the mask column -- the #/~/* char from gen_mask_str or a space.
         Real set_pkg_info fills it in only `if self.include_mask_str()`
         (verbosity > 1), and real default `emerge -p` verbosity is 2
         (_DisplayConfig.__init__: `--quiet and 1 or --verbose and 3 or
         2`) -- so the column is present at plain -p and -pv, absent only
         under --quiet (verbosity 1), which this pilot doesn't model.
         Always rendered.

    Each present letter is ANSI-coloured per real PkgAttrDisplay.__str__
    (green("N"), yellow("R"), turquoise("U"), blue("D"),
    colorize("WARN", "I"), the #/*/~ mask via BAD/WARN) when colour is on;
    a space is never coloured, so the field stays 7 visible columns
    either way. Mirrors pretend.rs's attr_display_field exactly."""

    def col(key, ch):
        return color.c(key, ch)

    f = []
    f.append(col("WARN", "I") if interactive else " ")
    f.append(
        col("red", "r") if force_reinstall else col("green", "N") if new else " "
    )
    f.append(col("yellow", "R") if replace else col("green", "S") if new_slot else " ")
    f.append(
        col("green", "f")
        if fetch_restrict_satisfied
        else col("red", "F")
        if fetch_restrict
        else col("fuchsia", "g")
        if remote_binary
        else " "
    )
    f.append(col("turquoise", "U") if new_version else " ")
    f.append(col("blue", "D") if downgrade else " ")
    # Real __str__ appends self.mask only `if self.mask is not None`, and
    # set_pkg_info sets it only `if self.include_mask_str()` (verbosity >
    # 1) -- true at real portage's default `emerge -p` verbosity of 2, so
    # the column is always present here (this pilot has no --quiet).
    # Real gen_mask_str: #/* -> BAD (red), ~ -> WARN (yellow), no mark ->
    # a space.
    if mask in ("#", "*"):
        f.append(col("BAD", mask))
    elif mask == "~":
        f.append(col("WARN", "~"))
    else:
        f.append(" ")
    return "".join(f)


def _package_counters_summary(entries, top_level_pkgs, onlydeps, color):
    """Real _PackageCounters.__str__ (output_helpers.py), the trailing
    "Total: ..." summary line real output.py::print_verbose emits via
    writemsg_stdout(f"\\n{self.counters}\\n") -- gated, in real portage
    too, on verbosity == 3 (i.e. -v), never plain -p. Now includes
    ", Size of downloads: ..." (real _calc_size/counters.totalsize, via
    provenance["download_files"], deduped by filename like real
    myfetchlist) and the "\\nFetch Restriction: N package[s][ (M
    unsatisfied)]" line (from provenance["fetch_restrict"] /
    "fetch_restrict_satisfied"). The "Conflict:" line's own "(N
    unsatisfied)"/"(all satisfied)" suffix is still dropped -- this pilot
    resolves no blocker. A top-level package suppressed by --onlydeps
    isn't in real's merge list, so it isn't counted here either. Mirrors
    pretend.rs's package_counters_summary."""
    upgrades = downgrades = new = newslot = reinst = 0
    binary = interactive = blocks = 0
    restrict_fetch = restrict_fetch_satisfied = 0
    totalsize = 0
    fetched = set()
    for entry in entries:
        category, package, outcome = entry[0], entry[1], entry[2]
        source, provenance = entry[7], entry[8]
        blocks += len(entry[3])
        if onlydeps and (category, package) in top_level_pkgs:
            continue
        tag = outcome[0]
        merge_bound = True
        if tag == "new":
            if isinstance(provenance, dict) and provenance.get("new_slot"):
                newslot += 1
            else:
                new += 1
        elif tag == "upgrade":
            upgrades += 1
        elif tag == "downgrade":
            downgrades += 1
        elif tag == "reinstall":
            reinst += 1
        else:
            merge_bound = False
        if merge_bound:
            pv = provenance if isinstance(provenance, dict) else {}
            if source == "binary":
                binary += 1
            if pv.get("interactive"):
                interactive += 1
            if pv.get("fetch_restrict"):
                restrict_fetch += 1
            if pv.get("fetch_restrict_satisfied"):
                restrict_fetch_satisfied += 1
            # Real _calc_size: sum the bytes still to fetch, counting a
            # shared distfile once (real myfetchlist).
            for name, size in pv.get("download_files", []):
                if name not in fetched:
                    fetched.add(name)
                    totalsize += size

    total = upgrades + downgrades + newslot + new + reinst
    out = f"Total: {total} package" + ("s" if total != 1 else "")
    details = []
    if upgrades > 0:
        details.append(f"{upgrades} upgrade" + ("s" if upgrades > 1 else ""))
    if downgrades > 0:
        details.append(f"{downgrades} downgrade" + ("s" if downgrades > 1 else ""))
    if new > 0:
        details.append(f"{new} new")
    if newslot > 0:
        details.append(f"{newslot} in new slot" + ("s" if newslot > 1 else ""))
    if reinst > 0:
        details.append(f"{reinst} reinstall" + ("s" if reinst > 1 else ""))
    if binary > 0:
        details.append(f"{binary} " + ("binaries" if binary > 1 else "binary"))
    if interactive > 0:
        details.append(f"{interactive} " + color.c("WARN", "interactive"))
    if total != 0:
        out += f" ({', '.join(details)})"
    # Real __str__: `f", Size of downloads: {localized_size(...)}"` --
    # appended to the Total: line unconditionally.
    out += f", Size of downloads: {_localized_size(totalsize)}"
    if restrict_fetch > 0:
        out += f"\nFetch Restriction: {restrict_fetch} package" + (
            "s" if restrict_fetch > 1 else ""
        )
        if restrict_fetch_satisfied < restrict_fetch:
            out += color.c(
                "BAD",
                f" ({restrict_fetch - restrict_fetch_satisfied} unsatisfied)",
            )
    if blocks > 0:
        out += f"\nConflict: {blocks} block" + ("s" if blocks > 1 else "")
    return out


def _columnwidth_from_env():
    """Real output_helpers.py's own columnwidth resolution
    (MergeListItem.__init__): 130 by default, overridden by a
    COLUMNWIDTH setting -- this pilot only ever reads it as a plain
    environment variable (real portage's own frozen_config.settings is
    env + make.conf + profile merged together; parsing COLUMNWIDTH out
    of make.conf too would need a new generic scalar-lookup path through
    the config dict, which nothing else in this pilot needs yet -- a
    deliberate v1 narrowing, same spirit as every other scope cut in
    this codebase). An unparsable value warns and falls back to the
    default, exactly like real portage's own except ValueError branch,
    rather than treating it as a hard error. Real portage's own warning
    has a first line echoing the raw exception text -- omitted here,
    same as every other parse-error message in this pilot (see
    --deep's own invalid-value handling): Rust's ParseIntError and
    Python's ValueError never stringify identically, so echoing either
    verbatim would make this the one message the two implementations
    could never agree on byte-for-byte. Mirrors pretend.rs's own
    columnwidth_from_env exactly."""
    value = os.environ.get("COLUMNWIDTH")
    if value is None:
        return 130
    try:
        return int(value)
    except ValueError:
        print(f'!!! Unable to parse COLUMNWIDTH="{value}"', file=sys.stderr)
        return 130


# --- ANSI colour (increment 2 of the -pv layout + colour buildout) ---
# Ports the slice of lib/portage/output.py the pretend renderer needs: the
# RGB-name -> ANSI-code table (output.py:30-92), colorize() (383-392), the
# _styles entries it reaches (126-154), nc_len() (249-251), and the
# actions.py:2816-2828 + util.no_color colour gate. No color.map /
# PORTAGE_COLORMAP parsing -- the real default map is hardcoded, same
# "optional config not modelled" cut as elsewhere. Mirrors
# portuale/src/color.rs exactly.
_COLOR_CODES = {
    "normal": "\x1b[0m",
    "reset": "\x1b[39;49;00m",
    "bold": "\x1b[01m",
    "black": "\x1b[30m",
    "darkgray": "\x1b[30;01m",
    "darkred": "\x1b[31m",
    "red": "\x1b[31;01m",
    "darkgreen": "\x1b[32m",
    "green": "\x1b[32;01m",
    "brown": "\x1b[33m",
    "yellow": "\x1b[33;01m",
    "darkblue": "\x1b[34m",
    "blue": "\x1b[34;01m",
    "purple": "\x1b[35m",
    "fuchsia": "\x1b[35;01m",
    "teal": "\x1b[36m",
    "turquoise": "\x1b[36;01m",
    "lightgray": "\x1b[37m",
    "white": "\x1b[37;01m",
}
_COLOR_STYLES = {
    "BAD": "red",
    "WARN": "yellow",
    "GOOD": "green",
    "NORMAL": "normal",
    "HILITE": "teal",
    "BRACKET": "blue",
    "PKG_MERGE": "darkgreen",
    "PKG_MERGE_SYSTEM": "darkgreen",
    "PKG_MERGE_WORLD": "green",
    "PKG_BINARY_MERGE": "purple",
    "PKG_BINARY_MERGE_SYSTEM": "purple",
    "PKG_BINARY_MERGE_WORLD": "fuchsia",
    "PKG_UNINSTALL": "red",
    "UNMERGE_WARN": "red",
    "INFORM": "darkgreen",
    "MERGE_LIST_PROGRESS": "yellow",
    "PKG_NOMERGE": "teal",
    "PKG_NOMERGE_SYSTEM": "teal",
    "PKG_NOMERGE_WORLD": "blue",
    "PKG_BLOCKER": "red",
    "PKG_BLOCKER_SATISFIED": "teal",
}
_ANSI_SGR = re.compile("\x1b[^m]+m")


def _nc_len(s):
    """Real output.py:249-251's nc_len -- visible length, ANSI SGR
    sequences removed first."""
    return len(_ANSI_SGR.sub("", s))


def _resolve_havecolor(color_opt):
    """Real actions.py:2816-2828 + util.no_color. color_opt: True/False for
    --color y|n, None when not given (fall through to
    NO_COLOR/NOCOLOR/isatty/TERM=dumb)."""
    no_color = bool(os.environ.get("NO_COLOR")) or os.environ.get(
        "NOCOLOR", "false"
    ).lower() in ("yes", "true")
    havecolor = not no_color
    if color_opt is not None:
        havecolor = color_opt
    elif os.environ.get("TERM") == "dumb" or not sys.stdout.isatty():
        havecolor = False
    return havecolor


_color_map_overrides_cache = None


def _color_map_overrides():
    """Real output.py::_parse_color_map (output.py:158-230): parse
    $PORTAGE_CONFIGROOT/etc/portage/color.map into {key: final escape
    sequence}. Mirrors color.rs's color_map_overrides -- see its
    docstring."""
    global _color_map_overrides_cache
    if _color_map_overrides_cache is not None:
        return _color_map_overrides_cache
    out = {}
    _color_map_overrides_cache = out
    config_root = os.environ.get("PORTAGE_CONFIGROOT", "/")
    path = os.path.join(config_root, "etc", "portage", "color.map")
    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            text = f.read()
    except OSError:
        return out
    ansi_re = re.compile(r"^[0-9;]*m$")

    def strip_quotes(t):
        if len(t) >= 2 and t[0] in "'\"" and t[0] == t[-1]:
            return t[1:-1]
        return t

    for lineno, raw in enumerate(text.splitlines()):
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        parts = line.split("=")
        if len(parts) != 2:
            sys.stderr.write(
                f"'{path}', line {lineno}: expected exactly one occurrence "
                "of '=' operator\n"
            )
            continue
        k = strip_quotes(parts[0].strip())
        v = strip_quotes(parts[1].strip())
        if k not in _COLOR_STYLES and k not in _COLOR_CODES:
            sys.stderr.write(f"'{path}', line {lineno}: Unknown variable: '{k}'\n")
            continue
        if ansi_re.match(v):
            out[k] = "\x1b[" + v
        else:
            seq = ""
            bad = False
            for name in v.split():
                if name not in _COLOR_CODES:
                    bad = True
                    break
                seq += _COLOR_CODES[name]
            if bad:
                sys.stderr.write(f"'{path}', line {lineno}: Undefined: '{v}'\n")
                continue
            out[k] = seq
    return out


def _resolved_code(name):
    ov = _color_map_overrides()
    if name in ov:
        return ov[name]
    if name in _COLOR_CODES:
        return _COLOR_CODES[name]
    s = _COLOR_STYLES.get(name, "")
    if not s:
        return ""
    return ov.get(s, _COLOR_CODES.get(s, ""))


class _Colorizer:
    """Real portage's module-global `havecolor` + `colorize()`, together.
    When disabled, every method returns its input unchanged."""

    def __init__(self, enabled):
        self.enabled = enabled

    def c(self, key, text):
        if not self.enabled:
            return text
        seq = _resolved_code(key)
        if not seq:
            return text
        return seq + text + _resolved_code("reset")

    def pkgprint(self, text, binary, system, world):
        """Real Display.pkgprint (output.py:265-292), merge-list case
        (always true for a bracket entry): system wins over world."""
        if binary:
            key = (
                "PKG_BINARY_MERGE_SYSTEM"
                if system
                else "PKG_BINARY_MERGE_WORLD"
                if world
                else "PKG_BINARY_MERGE"
            )
        else:
            key = (
                "PKG_MERGE_SYSTEM"
                if system
                else "PKG_MERGE_WORLD"
                if world
                else "PKG_MERGE"
            )
        return self.c(key, text)


def _colorize_use_token(tok, color):
    """Real _create_use_string's per-flag colour
    (output_helpers.py:262-334), re-derived from an already-rendered
    token's shape -- the marker suffix and sign fully determine it: a
    plain enabled `flag` is red, a plain disabled `-flag` is blue, a
    `%`/`%*` marker means yellow (newly in IUSE), a lone `*` means green
    (polarity flipped). Only the flag/-flag core is coloured -- the
    `*`/`%` markers and any `( )` wrap stay plain. Known imperfection (no
    fixture reaches it): a forced disabled flag newly in IUSE on an
    Upgrade renders `(-flag)` and is coloured blue here where real
    portage would yellow it. Mirrors pretend.rs's colorize_use_token."""
    if tok.startswith("(") and tok.endswith(")"):
        open_, inner, close = "(", tok[1:-1], ")"
    else:
        open_, inner, close = "", tok, ""
    if inner.endswith("%*"):
        core, markers = inner[:-2], "%*"
    elif inner.endswith("*"):
        core, markers = inner[:-1], "*"
    elif inner.endswith("%"):
        core, markers = inner[:-1], "%"
    else:
        core, markers = inner, ""
    if markers in ("%*", "%"):
        key = "yellow"
    elif markers == "*":
        key = "green"
    elif core.startswith("-"):
        key = "blue"
    else:
        key = "red"
    return f"{open_}{color.c(key, core)}{markers}{close}"


def _decorate_version(version, slot, sub_slot, repo, show_slot):
    """Real output.py::_append_slot + _append_repository (verbosity 3 --
    emerge -pv only): decorate a bare version with `:slot` (plus
    `/sub_slot` when it differs) and `::repo`. `show_slot` carries real
    _append_slot's own gate. Mirrors pretend.rs's decorate_version."""
    s = version
    if show_slot:
        s += ":" + slot
        if slot != sub_slot:
            s += "/" + sub_slot
    return s + "::" + repo


def _columns_line(
    bracket_word,
    field,
    indent,
    category,
    package,
    version,
    oldbest,
    columnwidth,
    color,
    binary,
    system,
    world,
):
    """One --columns line: real _set_root_columns's own layout algorithm
    (the pkg_info.merge == True branch only -- the "not merging" branch
    never applies to any outcome this pilot prints in brackets at all),
    color stripped for increment 1 (real's nc_len/plain len() distinction
    collapses to just len() until increment 2 adds ANSI color).
    bracket/field reproduce the exact same "[{bracket} {field}]" segment
    the non-columns format prints -- field is the full fixed-width
    attr_display_field -- only what comes after it differs:
    category/package (no version -- that's the whole point of --columns)
    padded out to columnwidth - 60 (newlp), then [version] right-padded
    to columnwidth - 30 (oldlp), then oldbest ("[from]" for an
    Upgrade/Downgrade, empty otherwise -- real pkg_info.oldbest_list,
    mirrored here via data this pilot already has). Padding is skipped
    once the line's already past the target width, exactly like real
    portage's own guard -- never truncates, just doesn't pad further.
    Mirrors pretend.rs's own columns_line exactly."""
    newlp = max(columnwidth - 60, 0)
    oldlp = max(columnwidth - 30, 0)
    cp = color.pkgprint(f"{category}/{package}", binary, system, world)
    bword = color.pkgprint(bracket_word, binary, system, world)
    line = f"[{bword} {field}] {indent}{cp}"
    if newlp > _nc_len(line):
        line += " " * (newlp - _nc_len(line))
    line += " " + color.c("green", f"[{version}]") + " "
    if oldlp > _nc_len(line):
        line += " " * (oldlp - _nc_len(line))
    if oldbest:
        line += color.c("blue", oldbest)
    return line


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
    # --oneshot/-1: don't colour a favorite as a would-be world member
    # (real _DisplayConfig.oneshot). This reference is --pretend-only, so
    # the world-file-write half of real --oneshot never applies here.
    oneshot = False
    # --tree/-t and --unordered-display: display-only, entirely
    # independent of resolution itself. See print_tree's own docstring
    # for the full pilot-specific design this needed.
    tree = False
    unordered_display = False
    # --columns: display-only, same "entirely independent of resolution"
    # shape as --tree above -- mutually exclusive with --tree, checked
    # once parsing finishes.
    columns = False
    # --alphabetical: display-only, real output_helpers.py conf.alphabetical.
    alphabetical = False
    # --color y|n (real argument_options, choices ("y","n")): None = not
    # given (fall through to NO_COLOR/NOCOLOR/isatty). See pretend.rs.
    color_opt = None
    # --depclean-lib-check y|n (real _DEPCLEAN_LIB_CHECK_DEFAULT = True).
    # Only consulted by --depclean/--prune. See pretend.rs.
    lib_check = True
    update = False
    deep = 0
    excluded = []
    usepkg_exclude = []
    usepkg_include = []
    json_output = False
    deselect = False
    # Real --deselect=n / --deselect n -- consulted by --depclean <atoms>
    # (real action_depclean's `deselect = myopts.get("--deselect") !=
    # "n"`). Never triggers the standalone deselect action.
    deselect_n = False
    unmerge = False
    depclean = False
    prune = False
    config_action = False
    # --list-sets / --search / --searchdesc: standalone read-only query
    # actions (see _run_list_sets / _run_search). Mirrors pretend.rs.
    list_sets = False
    search_action = False
    searchdesc = False
    check_news = False
    clean_action = False
    rage_clean = False
    info_action = False
    with_bdeps = True
    with_bdeps_given = False
    with_bdeps_auto = True
    changed_deps = False
    changed_slot = False
    # --newrepo: real main.py's own plain boolean "options" list, no
    # value at all (same shape as --changed-use/-U) -- unlike
    # --changed-slot/--rebuilt-binaries, which are real "true_y_or_n".
    newrepo = False
    # --emptytree/-e: real main.py plain-boolean "options" (short alias
    # `e`). Reinstalls the whole deep dependency tree. Mirrors pretend.rs.
    emptytree = False
    # --buildpkgonly/-B: same plain-boolean shape as --newrepo above.
    buildpkgonly = False
    # --root-deps: real main.py's own choices=("True", "rdeps"), plus a
    # bare form (no =value at all). This pilot doesn't distinguish "True"
    # (fold DEPEND/BDEPEND/IDEPEND into RDEPEND) from "rdeps" -- and for
    # this EAPI-7+-only fork it never needs to: at EAPI 7+
    # (eapi_attrs.bdepend, depgraph.py:4218-4238) the `--root-deps ==
    # "rdeps"` ignore_depend_deps branch is inside `else: if
    # eapi_attrs.bdepend`, so `=rdeps` is a complete no-op. Every
    # accepted form just enables the one behavior this pilot implements:
    # real running-root satisfiability for DEPEND/BDEPEND/IDEPEND atoms.
    root_deps = False
    with_test_deps = False
    changed_deps_report = False
    verbose_slot_rebuilds = True
    # --ignore-built-slot-operator-deps: real y_or_n (default "n",
    # main.py:470). "Intended only for debugging purposes" -- when y, the
    # slot-operator auto-rebuild scan is skipped entirely.
    ignore_built_slot_operator_deps = False
    # --backtrack=COUNT: real `type=int` / `valid_integers` (main.py). The
    # resolver's retry ceiling after a solvable slot conflict; default
    # (flag absent) is 10, `--backtrack=0` disables backtracking.
    backtrack_max = 10
    # --autounmask/--autounmask-keep-keywords: None means "not explicitly
    # given" -- see the on/off default-resolution logic just below where
    # these are actually consumed, mirroring pretend.rs exactly.
    autounmask = None
    autounmask_keep_keywords = None
    autounmask_use = None
    autounmask_license = None
    autounmask_keep_masks = None
    usepkg = False
    usepkgonly = False
    getbinpkg = False
    getbinpkgonly = False
    binpkg_respect_use = None
    rebuilt_binaries = None
    rebuilt_binaries_timestamp = None
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
        elif arg in ("--oneshot", "-1"):
            oneshot = True
            i += 1
        elif arg in ("--tree", "-t"):
            tree = True
            i += 1
        elif arg == "--unordered-display":
            unordered_display = True
            i += 1
        elif arg == "--columns":
            columns = True
            i += 1
        elif arg == "--alphabetical":
            # Real main.py plain-boolean "options" -- only affects the
            # USE="..." ordering (see use_suffix). Mirrors pretend.rs.
            alphabetical = True
            i += 1
        elif arg == "--color" or arg.startswith("--color="):
            # Real `emerge --color y|n` (main.py:421): the explicit
            # override that wins over NO_COLOR/NOCOLOR/isatty. A required
            # value. Mirrors pretend.rs.
            if arg.startswith("--color="):
                val = arg[len("--color=") :]
                i += 1
            elif i + 1 < len(args):
                val = args[i + 1]
                i += 2
            else:
                print(
                    "emerge: --color requires an argument (y or n)", file=sys.stderr
                )
                return 2
            if val == "y":
                color_opt = True
            elif val == "n":
                color_opt = False
            else:
                print(
                    f"emerge: --color: invalid choice: {val!r} (choose from 'y', 'n')",
                    file=sys.stderr,
                )
                return 2
        elif arg == "--depclean-lib-check" or arg.startswith("--depclean-lib-check="):
            # Real main.py: "choices": true_y_or_n -- a value flag
            # (y/n/True). Bare (no value) is lenient here -> y. Mirrors
            # pretend.rs.
            if arg.startswith("--depclean-lib-check="):
                val = arg[len("--depclean-lib-check=") :]
                i += 1
            elif i + 1 < len(args) and args[i + 1] in ("y", "n", "True"):
                val = args[i + 1]
                i += 2
            else:
                val = "y"
                i += 1
            lib_check = val not in ("n", "N")
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
        elif arg == "--backtrack":
            # Real main.py --backtrack: type=int, and listed in
            # insert_optional_args's valid_integers set, so the next token
            # is consumed only if it parses as a non-negative integer --
            # exactly like --deep/-D above. A bare --backtrack, or one
            # followed by a non-integer, leaves the default (10) in place.
            nxt = args[i + 1] if i + 1 < len(args) else None
            if nxt is not None and nxt.isdigit():
                backtrack_max = int(nxt)
                i += 2
            else:
                i += 1
        elif arg.startswith("--backtrack="):
            # argparse's native "="-form -- a non-integer here is an
            # immediate parse error, unlike a non-integer *next token*.
            value = arg[len("--backtrack=") :]
            if value.isdigit():
                backtrack_max = int(value)
                i += 1
            else:
                print(f'emerge: invalid --backtrack parameter: "{value}"', file=sys.stderr)
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
        elif arg == "--usepkg-exclude":
            # Same "action": "append", space-separated-per-occurrence
            # shape as --exclude above -- no short alias, real main.py
            # never gives it one.
            if i + 1 >= len(args):
                print('emerge: option "--usepkg-exclude" requires an argument', file=sys.stderr)
                return 2
            usepkg_exclude.extend(args[i + 1].split())
            i += 2
        elif arg.startswith("--usepkg-exclude="):
            usepkg_exclude.extend(arg[len("--usepkg-exclude=") :].split())
            i += 1
        elif arg == "--usepkg-include":
            if i + 1 >= len(args):
                print('emerge: option "--usepkg-include" requires an argument', file=sys.stderr)
                return 2
            usepkg_include.extend(args[i + 1].split())
            i += 2
        elif arg.startswith("--usepkg-include="):
            usepkg_include.extend(arg[len("--usepkg-include=") :].split())
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
                deselect_n = True
                i += 2
            else:
                deselect = True
                i += 1
        elif arg == "--deselect=y":
            deselect = True
            i += 1
        elif arg == "--deselect=n":
            deselect = False
            deselect_n = True
            i += 1
        elif arg in ("--unmerge", "-C"):
            # Real main.py: --unmerge/-C is a standalone ACTION
            # (myaction = "unmerge"), dispatched to _run_unmerge_pretend
            # below. Plain boolean. Mirrors pretend.rs.
            unmerge = True
            i += 1
        elif arg in ("--depclean", "-c"):
            # Real main.py: --depclean/-c is a standalone ACTION,
            # dispatched to _run_depclean_pretend below. Mirrors pretend.rs.
            depclean = True
            i += 1
        elif arg in ("--prune", "-P"):
            # Real main.py: --prune/-P is a standalone ACTION routed
            # through the same action_depclean as --depclean, dispatched
            # to _run_prune_pretend below. Mirrors pretend.rs.
            prune = True
            i += 1
        elif arg == "--config":
            # Real main.py: --config is a standalone ACTION (action_config
            # -- run pkg_config for one installed package). Ignores
            # --pretend. This reference has no ebuild-execution machinery,
            # so run() returns 0 for it (see below). Mirrors pretend.rs.
            config_action = True
            i += 1
        elif arg == "--list-sets":
            list_sets = True
            i += 1
        elif arg in ("--search", "-s"):
            search_action = True
            i += 1
        elif arg in ("--searchdesc", "-S"):
            search_action = True
            searchdesc = True
            i += 1
        elif arg == "--check-news":
            check_news = True
            i += 1
        elif arg == "--clean":
            clean_action = True
            i += 1
        elif arg == "--rage-clean":
            rage_clean = True
            i += 1
        elif arg == "--info":
            info_action = True
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
        elif arg == "--verbose-slot-rebuilds":
            # Real y_or_n (default "y"), same optional-value shape.
            nxt = args[i + 1] if i + 1 < len(args) else None
            if nxt == "n":
                verbose_slot_rebuilds = False
                i += 2
            elif nxt == "y":
                verbose_slot_rebuilds = True
                i += 2
            else:
                verbose_slot_rebuilds = True
                i += 1
        elif arg == "--verbose-slot-rebuilds=y":
            verbose_slot_rebuilds = True
            i += 1
        elif arg == "--verbose-slot-rebuilds=n":
            verbose_slot_rebuilds = False
            i += 1
        elif arg == "--ignore-built-slot-operator-deps":
            # Real y_or_n (no default arg); this pilot accepts the bare
            # form as y, same permissive shape as its sibling flags.
            nxt = args[i + 1] if i + 1 < len(args) else None
            if nxt == "n":
                ignore_built_slot_operator_deps = False
                i += 2
            elif nxt == "y":
                ignore_built_slot_operator_deps = True
                i += 2
            else:
                ignore_built_slot_operator_deps = True
                i += 1
        elif arg == "--ignore-built-slot-operator-deps=y":
            ignore_built_slot_operator_deps = True
            i += 1
        elif arg == "--ignore-built-slot-operator-deps=n":
            ignore_built_slot_operator_deps = False
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
        elif arg == "--newrepo":
            newrepo = True
            i += 1
        elif arg == "--emptytree" or arg == "-e":
            # Real main.py plain-boolean "options" (short alias `e`,
            # main.py:58). Mirrors pretend.rs.
            emptytree = True
            i += 1
        elif arg == "--buildpkgonly" or arg == "-B":
            buildpkgonly = True
            i += 1
        elif arg in ("--buildpkg", "-b"):
            # Real true_y_or_n. Only affects a real (non-pretend) source
            # merge on the Rust side (FEATURES=buildpkg / EbuildBinpkg);
            # this reference has no execution machinery, so it's a
            # recognized no-op here. Peek/consume an optional y|n.
            nxt = args[i + 1] if i + 1 < len(args) else None
            i += 2 if nxt in ("y", "n") else 1
        elif arg in ("--buildpkg=y", "--buildpkg=n"):
            i += 1
        elif arg == "--buildpkg-exclude":
            # "action": "append", required space-separated value -- a
            # recognized no-op for --pretend (only affects a real source
            # merge on the Rust side). A missing value is still a usage
            # error, same as --exclude.
            if i + 1 >= len(args):
                print(
                    'emerge: option "--buildpkg-exclude" requires an argument',
                    file=sys.stderr,
                )
                return 2
            i += 2
        elif arg.startswith("--buildpkg-exclude="):
            i += 1
        elif arg in ("--root-deps", "--root-deps=True", "--root-deps=rdeps"):
            root_deps = True
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
        elif arg == "--autounmask-use":
            # Real "--autounmask-use": plain y_or_n, a REQUIRED value --
            # same shape as "--autounmask-keep-keywords" above (real
            # lib/_emerge/main.py's own "choices": y_or_n, not
            # true_y_or_n).
            if i + 1 >= len(args):
                print(
                    'emerge: option "--autounmask-use" requires an argument',
                    file=sys.stderr,
                )
                return 2
            value = args[i + 1]
            if value == "y":
                autounmask_use = True
                i += 2
            elif value == "n":
                autounmask_use = False
                i += 2
            else:
                print(
                    f'emerge: option "--autounmask-use": invalid choice: "{value}" '
                    '(choose from "y", "n")',
                    file=sys.stderr,
                )
                return 2
        elif arg.startswith("--autounmask-use="):
            value = arg[len("--autounmask-use=") :]
            if value == "y":
                autounmask_use = True
                i += 1
            elif value == "n":
                autounmask_use = False
                i += 1
            else:
                print(
                    f'emerge: option "--autounmask-use": invalid choice: "{value}" '
                    '(choose from "y", "n")',
                    file=sys.stderr,
                )
                return 2
        elif arg == "--autounmask-license":
            value = args[i + 1] if i + 1 < len(args) else None
            if value == "y":
                autounmask_license = True
                i += 2
            elif value == "n":
                autounmask_license = False
                i += 2
            elif value is None:
                print(
                    'emerge: option "--autounmask-license" requires an argument',
                    file=sys.stderr,
                )
                return 2
            else:
                print(
                    f'emerge: option "--autounmask-license": invalid choice: "{value}" '
                    '(choose from "y", "n")',
                    file=sys.stderr,
                )
                return 2
        elif arg.startswith("--autounmask-license="):
            value = arg[len("--autounmask-license=") :]
            if value == "y":
                autounmask_license = True
                i += 1
            elif value == "n":
                autounmask_license = False
                i += 1
            else:
                print(
                    f'emerge: option "--autounmask-license": invalid choice: "{value}" '
                    '(choose from "y", "n")',
                    file=sys.stderr,
                )
                return 2
        elif arg == "--autounmask-keep-masks":
            value = args[i + 1] if i + 1 < len(args) else None
            if value == "y":
                autounmask_keep_masks = True
                i += 2
            elif value == "n":
                autounmask_keep_masks = False
                i += 2
            elif value is None:
                print(
                    'emerge: option "--autounmask-keep-masks" requires an argument',
                    file=sys.stderr,
                )
                return 2
            else:
                print(
                    f'emerge: option "--autounmask-keep-masks": invalid choice: "{value}" '
                    '(choose from "y", "n")',
                    file=sys.stderr,
                )
                return 2
        elif arg.startswith("--autounmask-keep-masks="):
            value = arg[len("--autounmask-keep-masks=") :]
            if value == "y":
                autounmask_keep_masks = True
                i += 1
            elif value == "n":
                autounmask_keep_masks = False
                i += 1
            else:
                print(
                    f'emerge: option "--autounmask-keep-masks": invalid choice: "{value}" '
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
        elif arg == "--getbinpkg" or arg == "-g":
            nxt = args[i + 1] if i + 1 < len(args) else None
            if nxt == "y":
                getbinpkg = True
                i += 2
            elif nxt == "n":
                getbinpkg = False
                i += 2
            else:
                getbinpkg = True
                i += 1
        elif arg == "--getbinpkg=y":
            getbinpkg = True
            i += 1
        elif arg == "--getbinpkg=n":
            getbinpkg = False
            i += 1
        elif arg == "--getbinpkgonly" or arg == "-G":
            nxt = args[i + 1] if i + 1 < len(args) else None
            if nxt == "y":
                getbinpkgonly = True
                i += 2
            elif nxt == "n":
                getbinpkgonly = False
                i += 2
            else:
                getbinpkgonly = True
                i += 1
        elif arg == "--getbinpkgonly=y":
            getbinpkgonly = True
            i += 1
        elif arg == "--getbinpkgonly=n":
            getbinpkgonly = False
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
        elif arg == "--rebuilt-binaries":
            nxt = args[i + 1] if i + 1 < len(args) else None
            if nxt == "y":
                rebuilt_binaries = True
                i += 2
            elif nxt == "n":
                rebuilt_binaries = False
                i += 2
            else:
                rebuilt_binaries = True
                i += 1
        elif arg == "--rebuilt-binaries=y":
            rebuilt_binaries = True
            i += 1
        elif arg == "--rebuilt-binaries=n":
            rebuilt_binaries = False
            i += 1
        elif arg == "--rebuilt-binaries-timestamp":
            # Real "action": "store" -- a required value, same shape as
            # --exclude's own required argument, but numeric (a Unix
            # timestamp real BUILD_TIME values are compared against).
            if i + 1 >= len(args):
                print(
                    'emerge: option "--rebuilt-binaries-timestamp" requires an argument',
                    file=sys.stderr,
                )
                return 2
            value = args[i + 1]
            if value.isdigit():
                rebuilt_binaries_timestamp = int(value)
                i += 2
            else:
                print(f'emerge: invalid --rebuilt-binaries-timestamp parameter: "{value}"', file=sys.stderr)
                return 2
        elif arg.startswith("--rebuilt-binaries-timestamp="):
            value = arg[len("--rebuilt-binaries-timestamp=") :]
            if value.isdigit():
                rebuilt_binaries_timestamp = int(value)
                i += 1
            else:
                print(f'emerge: invalid --rebuilt-binaries-timestamp parameter: "{value}"', file=sys.stderr)
                return 2
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
                elif c == "1":
                    oneshot = True
                elif c == "t":
                    tree = True
                elif c == "u":
                    update = True
                elif c == "n":
                    noreplace = True
                elif c == "D":
                    deep = True
                elif c == "e":
                    emptytree = True
                elif c == "k":
                    usepkg = True
                elif c == "K":
                    usepkgonly = True
                elif c == "g":
                    getbinpkg = True
                elif c == "G":
                    getbinpkgonly = True
                elif c == "W":
                    deselect = True
                elif c == "C":
                    unmerge = True
                elif c == "c":
                    depclean = True
                elif c == "P":
                    prune = True
                elif c == "s":
                    search_action = True
                elif c == "S":
                    search_action = True
                    searchdesc = True
                elif c == "B":
                    buildpkgonly = True
                elif c == "b":
                    pass  # --buildpkg: recognized no-op for --pretend
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

    # Real actions.py: "if '--tree' in emerge_config.opts and '--columns'
    # in emerge_config.opts: print(...); return 1" -- checked once
    # parsing finishes (order-independent), right after option parsing
    # and before any other validation, matching real portage's own
    # placement. This pilot's own CLI-usage-error convention (exit 2,
    # stderr) differs deliberately from real portage's literal `return
    # 1`/stdout here, matching every other CLI-usage error this pilot
    # already reports. Mirrors pretend.rs exactly.
    if tree and columns:
        print('emerge: can\'t specify both of "--tree" and "--columns".', file=sys.stderr)
        return 2

    # Real portage resolves COLUMNWIDTH (and warns on an unparsable
    # value) as part of general display setup, unconditionally -- never
    # gated on --columns itself actually being given. Mirrored here the
    # same way, even though the value only ever affects anything below
    # when `columns` is True.
    columnwidth = _columnwidth_from_env()

    # --unmerge/-C, --depclean/-c, --prune/-P and --deselect/-W WITHOUT
    # --pretend are all real writes on the Rust side now (pretend.rs's
    # execute_unmerge / run_deselect's world-file rewrite); this reference
    # has no ebuild-execution machinery, so it just returns 0 below --
    # except --deselect, which is a pure display action and is dispatched
    # here (before the `not pretend` return) so the reference still shows
    # what real portage would remove, just with the `Removing` verb and no
    # actual file write.
    #
    # Real main.py: --deselect is a standalone action only when
    # `myaction is None` -- --depclean/--prune/--unmerge set their own
    # action first, and --deselect=y|n is then just a modifier (real
    # action_depclean's `deselect`).
    if deselect and not depclean and not prune and not unmerge:
        return _run_deselect(atom_args, _root(), pretend)

    # --list-sets / --search: standalone read-only query actions, ignore
    # --pretend entirely (dispatched before the `not pretend` return
    # below). Mirrors pretend.rs.
    if list_sets:
        return _run_list_sets(_config_root())
    if search_action:
        return _run_search(
            atom_args,
            _config_root(),
            _root(),
            searchdesc,
            verbose,
            _Colorizer(_resolve_havecolor(color_opt)),
        )
    if check_news:
        return _run_check_news(
            find_repos(_config_root()),
            _root(),
            False,
            _Colorizer(_resolve_havecolor(color_opt)),
        )
    if info_action:
        _repos = find_repos(_config_root())
        _main = next(r for r in _repos if r["is_main"])
        _info_config = resolve_config(
            _config_root(),
            _main["location"],
            [(r["name"], r["location"]) for r in _repos if not r["is_main"]],
            [
                (a, r["location"])
                for r in _repos
                for a in r.get("aliases", [])
            ],
            _main["name"],
            {r["name"]: r["masters"] for r in _repos},
        )
        return _run_info(_info_config, _repos, _root())

    # --config <atom>: a real action (real action_config runs pkg_config
    # from the vdb). Ignores --pretend entirely. No ebuild-execution
    # machinery here -> nothing to do.
    if config_action:
        return 0

    # Every real, non-dry-run `emerge` execution path -- `--buildpkgonly`
    # (build a binary package), `--getbinpkgonly` (download + merge remote
    # binpkgs), and a plain `emerge <atom>` (real source build + merge) --
    # is implemented on the Rust side (portuale's emerge_build.rs /
    # emerge_getbinpkg.rs). This reference implementation has NO real
    # ebuild-execution machinery at all -- only the dry-run resolution
    # logic every CASES entry in the contract suite exercises -- so there
    # is nothing for it to do here. Return success without output; the
    # non-`--pretend` paths are Rust-black-box-tested (test_portuale.py),
    # never via the shared contract CASES.
    if not pretend:
        return 0

    if (
        not atom_args
        and not unmerge
        and not depclean
        and not prune
        and not clean_action
        and not rage_clean
        and not info_action
    ):
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
        # Real "masters" (see find_repos' own docstring): each repo's own
        # already-resolved masters chain, keyed by name, for
        # resolve_config's own package.mask stacking.
        repo_masters = {r["name"]: r["masters"] for r in all_repos}
        # Every repo's own aliases (repos.conf/layout.conf), each paired
        # with that repo's location -- real
        # repositories.get_location_for_name resolves an aliased
        # reponame:path profile parent (see resolve_config's docstring).
        repo_aliases = [
            (alias, r["location"]) for r in all_repos for alias in r.get("aliases", [])
        ]
        config = resolve_config(
            _config_root(),
            main_repo["location"],
            overlay_repos,
            repo_aliases,
            main_repo["name"],
            repo_masters,
        )
    except ResolutionError as e:
        print(f"emerge: {e}", file=sys.stderr)
        return 1

    # Real actions.py::adjust_configs colour gate -- resolved once here so
    # every action path (the standalone cleanup actions below and the
    # ordinary resolve-graph path) shares one _Colorizer.
    color = _Colorizer(_resolve_havecolor(color_opt))

    # --unmerge/-C: a standalone action; resolved config in hand,
    # dispatch before the ordinary resolve-graph path.
    if unmerge:
        return _run_unmerge_pretend(atom_args, _root(), _config_root(), config, color=color)
    if clean_action:
        return _run_clean_pretend(atom_args, _root(), _config_root(), color)
    if rage_clean:
        return _run_unmerge_pretend(
            atom_args, _root(), _config_root(), config, color=color, action="rage-clean"
        )
    if depclean:
        return _run_depclean_pretend(
            atom_args,
            _root(),
            _config_root(),
            config,
            color,
            verbose=verbose,
            lib_check=lib_check,
            deselect=not deselect_n,
        )
    if prune:
        if nodeps:
            return _run_prune_nodeps_pretend(atom_args, _root(), _config_root(), color)
        return _run_prune_pretend(
            atom_args,
            _root(),
            _config_root(),
            config,
            color,
            verbose=verbose,
            lib_check=lib_check,
        )

    # The built-in set tokens each expand to their own real atom list, in
    # place, at whatever position they appear: @world/@selected
    # (_expand_selected -- the world file's atoms + world_sets' nested
    # sets), @system (the profile chain's packages files), and @installed
    # (_installed_set_atoms -- a cat/pkg:slot atom per vdb package). Any
    # other "@name" token is a user-defined (file-based) package set,
    # expanded recursively via _resolve_custom_set.
    try:
        expanded_atoms = []
        for atom_arg in atom_args:
            if atom_arg in ("@world", "@selected"):
                expanded_atoms.extend(_expand_selected(_config_root(), _root()))
            elif atom_arg == "@system":
                expanded_atoms.extend(config["system_packages"])
            elif atom_arg == "@installed":
                expanded_atoms.extend(_installed_set_atoms(_root()))
            elif atom_arg.startswith("@"):
                expanded_atoms.extend(
                    _resolve_custom_set(_config_root(), atom_arg[1:], set())
                )
            else:
                expanded_atoms.append(atom_arg)
    except ResolutionError as e:
        print(f"emerge: {e}", file=sys.stderr)
        return 1
    atom_args = expanded_atoms

    if not atom_args:
        print(
            "emerge (pilot v1): no package atoms to resolve (the target list, "
            "after expanding any @world/@selected/@system/@installed/@<set>, is empty)",
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

    # Display.pkgprint's @system/world inputs (`color` already resolved
    # above). Mirrors pretend.rs.
    color_system_atoms = config["system_packages"]
    color_world_atoms = _read_world_atoms(_root())

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
    # whole share of real --reinstall's own contribution. --newrepo is
    # one of real create_depgraph_params.py's own listed triggers too
    # (confirmed by reading it, line ~147: "--newrepo" in myopts). An
    # explicit --selective=n unconditionally cancels it regardless of
    # what the other flags computed, matching real
    # create_depgraph_params.py's own unconditional `if myopts.get(
    # "--selective") == "n": pop`, checked last, after every other
    # trigger.
    if selective_flag is None:
        selective = (
            update
            or newuse
            or changed_use
            or changed_deps
            or changed_slot
            or noreplace
            or newrepo
        )
    else:
        selective = selective_flag

    # --autounmask/--autounmask-keep-keywords/--autounmask-use: real
    # create_depgraph_params.py's own default-resolution logic,
    # simplified for this pilot's own v1 scope (--autounmask-license/
    # -masks still aren't read at all). Real logic: autounmask itself
    # defaults to enabled (only --autounmask=n turns the whole feature
    # off). autounmask_keep_keywords (real: "suppress keyword
    # suggestions") defaults to suppressed (True) when --autounmask
    # itself was NOT explicitly given at all, but defaults to *not*
    # suppressed (False, i.e. keyword suggestions ARE generated) once
    # --autounmask itself WAS explicitly given (any value) -- real
    # portage's own "explicitly asking for autounmask implies wanting
    # its keyword suggestions too, but the ambient always-on default
    # doesn't" asymmetry, ported exactly. autounmask_use (real: "allow
    # autounmask to change package.use") has no such asymmetry at all --
    # real myparams["autounmask_keep_use"] = True if autounmask_use ==
    # "n" else False, unconditionally on whenever --autounmask-use isn't
    # explicitly "n", regardless of whether --autounmask itself was ever
    # explicitly given. Mirrors pretend.rs exactly, including its own
    # documented gap: real autounmask_use is also forced to "n" whenever
    # myparams["binpkg_respect_use"] == "y" (an explicit, literal
    # --binpkg-respect-use=y, not the "auto" default) -- not reproduced
    # here either, same reasoning.
    autounmask_enabled = autounmask is not False
    if autounmask_keep_keywords is not None:
        autounmask_suggest_keywords = autounmask_enabled and not autounmask_keep_keywords
    else:
        autounmask_suggest_keywords = autounmask_enabled and autounmask is not None
    autounmask_suggest_use = autounmask_enabled and autounmask_use is not False
    # Real create_depgraph_params.py: autounmask_license defaults to "y"
    # only when --autounmask itself is explicitly True, else "n" -- so OFF
    # by default (unlike USE).
    autounmask_suggest_license = autounmask_enabled and (
        autounmask_license if autounmask_license is not None else (autounmask is True)
    )
    # Real create_depgraph_params.py: masks stay masked unless
    # --autounmask-keep-masks=n is given explicitly.
    autounmask_suggest_masks = autounmask_enabled and autounmask_keep_masks is False

    # Fold the --getbinpkg family into the --usepkg family (see their
    # parsing): --getbinpkgonly implies binary-only; either getbinpkg
    # flag makes binary candidates eligible; getbinpkg additionally
    # turns on remote binrepo candidate loading. Mirrors pretend.rs.
    usepkgonly = usepkgonly or getbinpkgonly
    usepkg = usepkg or getbinpkg or getbinpkgonly
    getbinpkg = getbinpkg or getbinpkgonly

    # Real bintree._populate_local's "no trusted index" branch: when
    # --usepkg/--usepkgonly makes local binary candidates eligible but
    # <PKGDIR>/Packages is absent, walk $PKGDIR for binpkg files and
    # synthesize the index from each file's own embedded metadata. NOT
    # written back to Packages (config["scanned_binpkgs"] instead). A
    # present Packages is always used as is. Mirrors pretend.rs.
    config["scanned_binpkgs"] = None
    if (usepkg or usepkgonly) and not os.path.isfile(
        os.path.join(config["pkgdir"], "Packages")
    ):
        try:
            scanned = _scan_pkgdir(config["pkgdir"])
        except Exception as e:  # noqa: BLE001 -- surface any scan failure
            print(f"emerge: scanning {config['pkgdir']}: {e}", file=sys.stderr)
            return 1
        if scanned:
            config["scanned_binpkgs"] = scanned

    # --binpkg-respect-use: real default is "auto" (effectively on)
    # whenever --usepkgonly is NOT given, left off (unset/falsy) when it
    # IS -- create_depgraph_params.py:47-55. An explicit
    # --binpkg-respect-use=y/=n always wins outright either way.
    resolved_binpkg_respect_use = (
        binpkg_respect_use if binpkg_respect_use is not None else not usepkgonly
    )

    # --rebuilt-binaries's own real default-resolution
    # (create_depgraph_params.py:185-193, confirmed by reading it):
    # "rebuilt_binaries is True or (rebuilt_binaries != "n" and
    # usepkgonly is True and deep is True and "--update" in myopts)" --
    # an explicit "=n" always wins outright (turns the auto-on condition
    # off too, not just the bare flag); an explicit bare/"=y" always
    # wins on; otherwise (never mentioned at all) it still auto-enables
    # once --usepkgonly, bare --deep (no explicit number -- deep is
    # True), and --update are ALL given together.
    resolved_rebuilt_binaries = (
        rebuilt_binaries
        if rebuilt_binaries is not None
        else (usepkgonly and deep is True and update)
    )

    # --root-deps: real running root (see _running_root's own doc comment
    # for why real "/" is the correct default here, and
    # PORTAGE_RUNNING_ROOT's own pilot-specific, test-only override).
    root_deps_running_root = _running_root() if root_deps else None

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
            autounmask_suggest_use,
            autounmask_suggest_license,
            autounmask_suggest_masks,
            usepkg,
            usepkgonly,
            resolved_binpkg_respect_use,
            usepkg_exclude,
            usepkg_include,
            resolved_rebuilt_binaries,
            rebuilt_binaries_timestamp,
            newrepo,
            buildpkgonly,
            root_deps_running_root,
            os.environ.get("DISTDIR", "/var/cache/distfiles"),
            emptytree,
            getbinpkg,
            ignore_built_slot_operator_deps,
            backtrack_max,
        )
    except ResolutionError as e:
        print(f"emerge: {e}", file=sys.stderr)
        return 1
    entries = result["entries"]

    # Real depgraph.py:11192-11235's display_problems() block for a
    # directly-requested atom that matched package.provided -- to stderr,
    # before the merge list. No SetArg tracking here, so the "pulled in
    # by" ref is always 'args' and the real @world/@selected "A) B) C)"
    # solution text is never reached. Mirrors pretend.rs.
    if result["pprovided_atoms"]:
        sys.stderr.write(color.c("BAD", "\nWARNING: "))
        if len(result["pprovided_atoms"]) > 1:
            print(
                "Requested packages will not be merged because they are listed in",
                file=sys.stderr,
            )
        else:
            print(
                "A requested package will not be merged because it is listed in",
                file=sys.stderr,
            )
        print("package.provided:\n", file=sys.stderr)
        for atom in result["pprovided_atoms"]:
            print(f"  {color.c('INFORM', atom)} pulled in by 'args'", file=sys.stderr)
        print(file=sys.stderr)

    # Real Display.blockers: blocker lines are collected while walking the
    # entries and printed as one group *after* every package line (real
    # output.py::display calls print_messages() then print_blockers()),
    # not inline. Mirrors pretend.rs's own deferred `blocker_lines`.
    deferred_blocker_lines = []

    def print_blockers(category, package, owner_version, blockers):
        # Real ResolverOutput._blockers (output.py:75-123). Purely
        # informational (see resolve_pretend_graph's doc comment): v1
        # neither refuses nor changes the exit code for a blocker match.
        # This pilot only ever reports an *unsatisfied* blocker (it never
        # resolves one away), so real `blocker.satisfied` is always False
        # here: the bracket letter is always the red `B` / style
        # PKG_BLOCKER, never the teal `b` / PKG_BLOCKER_SATISFIED branch.
        # `resolved` is real `dep_expand(str(atom).lstrip("!"))` -- a
        # category-qualification only, and every pilot blocker atom is
        # already `cat/pkg[...]`, so it reduces to stripping the leading
        # `!`/`!!`. Real's `(is <desc> <parents>)` alternative
        # (`self.resolved == blocker.atom`) is unreachable: `resolved`
        # drops the `!` while `blocker.atom` keeps it. Real `_blockers`
        # appends `empty_space_in_brackets()` after the five-space `B    `
        # pad, and that adds the mask column's own space whenever
        # `verbosity > 1` -- true at real portage's default `emerge -p`
        # verbosity of 2, so it's always present here (this pilot has no
        # --quiet).
        style = "PKG_BLOCKER"
        pad = "      "
        for b in blockers:
            resolved = b["atom_str"].lstrip("!")
            desc = "hard blocking" if b["strong"] else "soft blocking"
            parents = f"{category}/{package}-{owner_version}"
            deferred_blocker_lines.append(
                f'[{color.c(style, "blocks")} {color.c(style, "B")}{pad}] '
                + color.c(style, resolved)
                + color.c(style, f' ("{resolved}" is {desc} {parents})')
            )

    def _installed_use_state(category, package, outcome):
        # Real _display_use's previous_pkg: the installed version's own
        # recorded USE/IUSE for the */% diff markers -- only for an entry
        # that replaces an installed one (Upgrade/Downgrade from
        # outcome[1], Reinstall at outcome[1]). Mirrors pretend.rs.
        tag = outcome[0]
        if tag in ("upgrade", "downgrade", "reinstall"):
            version = outcome[1]
            old_iuse = _read_vdb_flag_set(_root(), category, package, version, "IUSE")
            old_use = {
                f
                for f in _read_vdb_flag_set(_root(), category, package, version, "USE")
                if f in old_iuse
            }
            return (old_use, old_iuse)
        return None

    def _forced_flags_for_entry(category, package, outcome):
        # Real _display_use's self.forced_flags = pkg.use.force |
        # pkg.use.mask, for the ( ... ) wrap -- re-resolves the displayed
        # candidate (portage-repo computes this inline where the
        # candidate is already resolved; this render loop is separate, so
        # it re-derives from list_candidates). Mirrors pretend.rs.
        tag = outcome[0]
        if tag not in ("new", "upgrade", "downgrade", "reinstall"):
            return set()
        version = outcome[2] if tag in ("upgrade", "downgrade") else outcome[1]
        cands = [
            c
            for c in list_candidates(all_repos, category, package)
            if c["version"] == version
        ]
        if not cands:
            return set()
        resolved = max(cands, key=lambda c: c["repo_priority"])
        try:
            metadata = read_md5_cache(
                resolved["repo_location"], category, f"{package}-{version}"
            )
        except OSError:
            return set()
        candidate_str = (
            f"{category}/{package}-{version}:{resolved['slot']}/"
            f"{resolved['sub_slot']}::{resolved['repo_name']}"
        )
        return _forced_or_masked_flags(
            metadata.get("IUSE", ""),
            resolved["keywords"],
            candidate_str,
            category,
            package,
            config,
        )

    def use_suffix(use_display, installed=None, forced=None, reinst_flags=None):
        # "  USE=\"a -b\" VIDEO_CARDS=\"-amdgpu nvidia\"", matching real
        # --pretend's own line format. Real output.py:_display_use
        # groups the flags by USE_EXPAND (plain USE group, then one
        # VAR="..." per non-hidden USE_EXPAND var, empty groups omitted),
        # for an entry that replaces an installed one appends */% markers
        # vs that installed version's USE/IUSE, and wraps a
        # force-enabled/mask-disabled flag in ( ). Groups render enabled
        # flags first, then disabled; --alphabetical re-sorts each group's
        # rendered tokens into one bare-name-sorted list (real
        # _create_use_string's `conf.alphabetical` branch). Still not
        # shown: real portage's ANSI colorization (documented cut).
        # Mirrors portage-repo/src/lib.rs's build_use_expand_display +
        # pretend.rs's use_suffix.
        #
        # Real _DisplayConfig: print_use_string = verbosity != 1, and
        # real default `emerge -p` verbosity is 2 -- so the USE line is
        # NOT -v-gated. What -v (verbosity 3) changes is all_flags, i.e.
        # WHICH flags render: -pv shows every flag (unchanged ones plain,
        # plus the (-flag%) removed list), plain -p omits an unchanged
        # flag -- so a New package's list is the same at -p and -pv
        # (is_new renders everything), and a Reinstall/Upgrade shows only
        # the changed flags at -p (often none).
        if not use_display:
            return ""
        groups = _build_use_expand_display(
            use_display,
            config["use_expand"],
            config["use_expand_hidden"],
            installed,
            forced,
            verbose,
            reinst_flags,
        )
        if not groups:
            return ""

        def body(rendered):
            toks = rendered.split(" ")
            if alphabetical:
                toks.sort(key=lambda t: _alnum_sort_key(_use_flag_sort_key(t)))
            # Colour (real _create_use_string's red/green/blue/yellow) is
            # applied per token *after* the sort, so the --alphabetical
            # sort key still sees plain tokens. Mirrors pretend.rs.
            return " ".join(_colorize_use_token(t, color) for t in toks)

        # Real print_messages: `myprint += " " + self.verboseadd` -- a
        # single space joins the USE display to the line, which already
        # ends with the (possibly empty) oldbest slot's own trailing
        # space. Mirrors pretend.rs's use_suffix.
        return " " + " ".join(f'{name}="{body(rendered)}"' for name, rendered in groups)

    def root_suffix(targets_running_root):
        # Real lib/_emerge/resolver/output.py:841-862's own
        # darkgreen("to " + pkg.root) suffix: a --root-deps entry that
        # builds against the running root rather than the target ROOT
        # (targets_running_root, the 13th entry-tuple field) is annotated
        # with where it actually installs -- exactly as real portage
        # annotates any entry whose own pkg.root_config.settings["ROOT"]
        # != "/". Deliberately narrower than that real gate, though: this
        # pilot annotates only the running-root build entries, never
        # every entry merged under a non-"/" ROOT, since that would make
        # every fixture test emit its own non-deterministic mktemp -d
        # ROOT path (see pretend.rs's own root_suffix docstring). "" for
        # every ordinary entry, and "" defensively if root_deps_running_root
        # is somehow None. Mirrors pretend.rs's own root_suffix exactly.
        if not targets_running_root or root_deps_running_root is None:
            return ""
        # Returned bare ("to /", no leading space) -- real
        # output.py:856-861 places it right after the always-present
        # space that follows the package string, with oldbest (when
        # non-empty) getting its own trailing space before it;
        # print_entry_line's own emit reproduces that spacing. Mirrors
        # pretend.rs's root_suffix.
        return f"to {root_deps_running_root}"

    if json_output:
        _print_json(
            entries,
            result["slot_conflicts"],
            result["changed_deps_report"],
            result["autounmask_keyword_changes"],
            result["autounmask_use_changes"],
            result["autounmask_license_changes"],
            result["autounmask_mask_changes"],
            result["abi_rebuilds"],
            top_level_pkgs,
            verbose,
            root_deps_running_root,
        )
        return 0

    def print_entry_line(entry, indent):
        # One entry's own display line, `indent` prepended right before
        # the category/package text (empty for flat mode, print_tree's
        # own growing prefix for --tree) -- the exact same per-outcome
        # bracket/reason logic the flat loop always had, just factored
        # out so both display modes share one implementation rather than
        # drifting apart. Mirrors pretend.rs's own print_entry_line
        # exactly.
        (
            category,
            package,
            outcome,
            blockers,
            _slot,
            use_display,
            _required_by,
            source,
            provenance,
            keyword_suggestion,
            use_suggestion,
            parent_use_suggestion,
            targets_running_root,
        ) = entry
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
        # Real Display.check_system_world, narrowed (see pretend.rs's own
        # print_entry_line): world = already a var/lib/portage/world
        # match, OR a directly-requested target (a favorite) that
        # create_world_atom would actually add -- NOT --oneshot/--onlydeps
        # (real _DisplayConfig.oneshot), and not an unslotted @system
        # member. system = a @system atom match (slot-qualified @system
        # atoms match version-only -- cosmetic-only miss, colour only).
        binary = source == "binary"
        is_favorite = (category, package) in top_level_pkgs
        unslotted = (_slot or "0") == "0"

        def classify(version):
            cpv = f"{category}/{package}-{version}"
            system = any(_match_atom_str(a, cpv) for a in color_system_atoms)
            would_add_to_world = (
                is_favorite
                and not (oneshot or onlydeps)
                and not (system and unslotted)
            )
            world = would_add_to_world or any(
                _match_atom_str(a, cpv) for a in color_world_atoms
            )
            return system, world
        # Real output.py:841-862's own "to <root>" annotation for a
        # running-root build entry -- "" for every ordinary entry (see
        # root_suffix). Placed right before use_suffix in each arm below,
        # matching both real portage's own ordering and pretend.rs.
        root = root_suffix(targets_running_root)
        installed = _installed_use_state(category, package, outcome)
        forced = _forced_flags_for_entry(category, package, outcome)
        # Real `reinst_flags_map`: a USE-triggered Reinstall's own
        # `_reinstall_for_flags` set (`outcome[2]`), force-shown in the
        # USE line even at plain -p.
        reinst_flags = set(outcome[2]) if outcome[0] == "reinstall" else set()
        prov = provenance if isinstance(provenance, dict) else {}
        # Real output.py:gen_mask_str's -v one-character mask column, now
        # the 7th column of the fixed-width attr_display field (not an
        # appended suffix). Mirrors pretend.rs.
        km = prov.get("keyword_mask")
        interactive = bool(prov.get("interactive"))
        fetch_restrict = bool(prov.get("fetch_restrict"))
        fetch_restrict_satisfied = bool(prov.get("fetch_restrict_satisfied"))
        remote_binary = bool(prov.get("remote_binary"))
        new_slot_flag = bool(prov.get("new_slot"))

        # Real _append_slot / _append_repository / convert_myoldbest
        # (verbosity 3 -- emerge -pv). Mirrors pretend.rs.
        entry_slot = _slot or "0"
        entry_sub = prov.get("sub_slot") or "0"
        entry_repo = prov.get("repo_name") or ""
        oldbest_refs = prov.get("oldbest") or []
        show_slot = (
            new_slot_flag
            or f"{entry_slot}/{entry_sub}" != "0/0"
            or any(f"{r['slot']}/{r['sub_slot']}" != "0/0" for r in oldbest_refs)
        )

        def disp_version(v):
            if not verbose:
                return v
            return _decorate_version(v, entry_slot, entry_sub, entry_repo, show_slot)

        def oldbest_str():
            if not oldbest_refs:
                return ""
            parts = []
            for r in oldbest_refs:
                v = r["version"][:-3] if r["version"].endswith("-r0") else r["version"]
                parts.append(
                    _decorate_version(v, r["slot"], r["sub_slot"], r["repo"], show_slot)
                    if verbose
                    else v
                )
            return "[" + ", ".join(parts) + "]"

        def field(new=False, new_slot=False, replace=False, new_version=False, downgrade=False):
            # The fixed-width attr_display field flags this entry
            # contributes, shared by every merge outcome below (see
            # _attr_display_field). force_reinstall is always False here;
            # remote_binary (the `g` column) is prov["remote_binary"] --
            # real attr_display.remote_binary = pkg.remote for a
            # --getbinpkg binary not yet in $PKGDIR. Mirrors pretend.rs's
            # own `field` closure.
            return _attr_display_field(
                interactive,
                new,
                False,
                new_slot,
                replace,
                fetch_restrict and not fetch_restrict_satisfied,
                fetch_restrict_satisfied,
                remote_binary,
                new_version,
                downgrade,
                km,
                color,
            )

        def emit(f, version):
            # One merge line, shared by new/upgrade/downgrade/reinstall.
            # Real _set_no_columns: f"[{type} {attr}] {indent}{pkg_str}
            # {oldbest}" -- the space before oldbest is always there even
            # when oldbest is empty. The running-root "to <root>" suffix
            # (real output.py:856-861) and the USE="..." display (real
            # print_messages' own " " + verboseadd) follow, each already
            # carrying its own leading space. Mirrors pretend.rs's emit.
            if onlydeps_suppressed:
                return
            system, world = classify(version)
            disp_ver = disp_version(version)
            oldbest = oldbest_str()
            use_str = use_suffix(use_display, installed, forced, reinst_flags)
            # Real output.py::verbose_size (verbosity 3 only): verboseadd
            # += localized_size(mysize) after the USE string. Rendered
            # only for a --getbinpkg remote binary (see pretend.rs's
            # emit -- the one non-zero case; the wider bare " 0 KiB" that
            # real shows on every -pv line is a pre-existing omission).
            if verbose and remote_binary:
                _bytes = sum(s for _n, s in (prov.get("download_files") or []))
                size_suffix = " " + _localized_size(_bytes)
            else:
                size_suffix = ""
            # Real output.py:856-861: darkgreen("to " + pkg.root).
            root_col = color.c("darkgreen", root) if root else ""
            if columns:
                root_str = f" {root_col}" if root else ""
                print(
                    _columns_line(
                        bracket,
                        f,
                        indent,
                        category,
                        package,
                        disp_ver,
                        oldbest,
                        columnwidth,
                        color,
                        binary,
                        system,
                        world,
                    )
                    + root_str
                    + use_str
                    + size_suffix
                )
                return
            # Real _set_no_columns: f"[{pkgprint(type)} {attr}]
            # {indent}{pkgprint(pkg_str)} {oldbest}".
            bword = color.pkgprint(bracket, binary, system, world)
            pkg_str = color.pkgprint(
                f"{category}/{package}-{disp_ver}", binary, system, world
            )
            tail = " "
            if oldbest:
                tail += color.c("blue", oldbest)
            if root:
                if oldbest:
                    tail += " "
                tail += root_col
            tail += use_str
            tail += size_suffix
            print(f"[{bword} {f}] {indent}{pkg_str}{tail}")

        if tag == "new":
            # Real _get_installed_best: brand-new -> attr.new; into a
            # fresh slot while another slot is installed -> attr.new
            # *and* attr.new_slot (provenance["new_slot"]). No oldbest for
            # a brand-new package; the other-slot version list real
            # portage shows for a new-slot install (myoldbest =
            # installed_versions) is deferred to a follow-up increment
            # (this pilot doesn't carry the other-slot versions yet).
            emit(field(new=True, new_slot=new_slot_flag), outcome[1])
            print_blockers(category, package, outcome[1], blockers)
        elif tag == "upgrade":
            # Real: an in-slot version bump -> attr.new_version only (the
            # exact new cpv isn't installed, so attr.replace stays clear
            # -> U, no R). oldbest = the in-slot installed version.
            emit(field(new_version=True), outcome[2])
            print_blockers(category, package, outcome[2], blockers)
        elif tag == "downgrade":
            # Real: in-slot downgrade -> attr.new_version *and*
            # attr.downgrade (U and D). oldbest as for upgrade.
            emit(field(new_version=True, downgrade=True), outcome[2])
            print_blockers(category, package, outcome[2], blockers)
        elif tag == "reinstall":
            # Real _get_installed_best: the exact cpv is already installed
            # -> attr.replace (the yellow R at column 2), and myoldbest
            # stays empty for a same-slot/same-repo reinstall -> no
            # [from]. Real portage's -pv shows no inline "why" for a
            # reinstall at all -- the pilot's former "(reinstall for
            # changed ...)" prose is dropped here (the USE diff still
            # shows in the USE="..." section for --changed-use;
            # --changed-deps/--changed-slot reasons are genuinely
            # invisible in real -pv too).
            emit(field(replace=True), outcome[1])
            print_blockers(category, package, outcome[1], blockers)
        elif tag == "already_installed":
            # Already-satisfied dependencies aren't shown, matching real
            # emerge's usual "don't clutter the list" behavior -- only a
            # directly-requested (top-level) atom gets its own
            # "is already installed; nothing to do" line, and --onlydeps
            # suppresses that too, same as every other outcome above.
            if (category, package) in top_level_pkgs and not onlydeps_suppressed:
                print(
                    f"{indent}{category}/{package}-{outcome[1]} is already installed; nothing to do"
                )
        else:
            print(
                f'!!! no visible ebuild for dependency "{category}/{package}"',
                file=sys.stderr,
            )
            # --autounmask's own keyword-suggestion sub-feature, extended
            # to a dependency's own no_visible_candidate -- see
            # portage-repo/src/lib.rs's GraphEntry::keyword_suggestion own
            # doc comment. Previously only a top-level atom's own fatal
            # no_visible_candidate got this note (as part of the
            # ResolutionError that aborts the whole call).
            if keyword_suggestion is not None:
                version, keyword = keyword_suggestion
                print(
                    f'!!! note: {category}/{package}-{version} exists but is masked by '
                    f"KEYWORDS; --autounmask-keep-keywords=n suggests adding "
                    f'"{category}/{package} {keyword}" to package.accept_keywords',
                    file=sys.stderr,
                )
            # --autounmask-use's own suggestion sub-feature -- see
            # portage-repo/src/lib.rs's GraphEntry::use_suggestion own
            # doc comment.
            if use_suggestion is not None:
                version, flip = use_suggestion
                adjustments = " ".join(flag if enabled else f"-{flag}" for flag, enabled in flip)
                print(
                    f'!!! note: {category}/{package}-{version} exists but its USE flags '
                    f"don't satisfy this atom; --autounmask-use suggests adding "
                    f'"={category}/{package}-{version} {adjustments}" to package.use',
                    file=sys.stderr,
                )
            # --autounmask-use's own opt=-aware *parent* flip -- see
            # _suggested_parent_use_candidate's own docstring. When that
            # flip resolves the dep, resolve_pretend_graph applies it and
            # this entry is no longer no_visible_candidate, so this is a
            # fallback hint for the (currently unreachable) case where the
            # suggestion exists but wasn't applied.
            if parent_use_suggestion is not None:
                parent_category, parent_package, parent_version, flip = parent_use_suggestion
                adjustments = " ".join(flag if enabled else f"-{flag}" for flag, enabled in flip)
                print(
                    f"!!! note: {parent_category}/{parent_package}-{parent_version}'s own USE "
                    f"flags need to change to satisfy this dependency; --autounmask-use "
                    f'suggests adding "={parent_category}/{parent_package}-{parent_version} '
                    f'{adjustments}" to package.use',
                    file=sys.stderr,
                )

    def print_tree(entries):
        # --tree/-t: indents each entry under whichever other entry's own
        # dependency string reached it. Pilot-specific simplification,
        # NOT a faithful port of real output_helpers.py's own
        # _tree_display -- see pretend.rs's own print_tree docstring for
        # the full grounding on why a faithful port isn't tractable here
        # (no merge scheduler, no real bidirectional digraph) and the
        # design this pilot uses instead: invert each entry's own
        # required_by (already "every distinct owner, sorted") into a
        # children map, walk it from the top-level/requested entries as
        # roots in their own entries order (now real portage's
        # dependency-first merge order, per _topological_merge_order),
        # never rendering (or recursing into) a node more than once
        # anywhere in the tree -- real _unordered_tree_display's own
        # seen_nodes behavior, ported exactly, and what keeps this from
        # looping forever on a genuine dependency cycle too.
        # unordered_display chooses child order at each level: entries'
        # own order when true (merge order now, not raw BFS discovery),
        # versus alphabetical-by-(category, package) when false (this
        # pilot's own deterministic default). Any entry
        # never reached from a root at all (shouldn't normally happen) is
        # still printed, unindented, after the tree itself, rather than
        # silently dropped. Mirrors pretend.rs's own print_tree exactly.
        children = {}
        for i, entry in enumerate(entries):
            for owner in entry[6]:
                children.setdefault(owner, []).append(i)
        if not unordered_display:
            for kids in children.values():
                kids.sort(key=lambda i: (entries[i][0], entries[i][1]))

        rendered = set()

        def render(i, depth):
            if i in rendered:
                return
            rendered.add(i)
            indent = "  " * depth
            print_entry_line(entries[i], indent)
            key = (entries[i][0], entries[i][1])
            for child in children.get(key, []):
                render(child, depth + 1)

        for i, entry in enumerate(entries):
            if (entry[0], entry[1]) in top_level_pkgs:
                render(i, 0)

        # Safety net, not expected to ever trigger in practice (see this
        # function's own docstring) -- prints anything the tree walk
        # somehow never reached, flat, rather than silently dropping it.
        for i, entry in enumerate(entries):
            if i not in rendered:
                print_entry_line(entry, "")

    if tree:
        print_tree(entries)
    else:
        for entry in entries:
            print_entry_line(entry, "")

    # Real Display.print_blockers(): the collected `[blocks B ...]` lines,
    # printed as one group after every package line and before the
    # counters. Mirrors pretend.rs.
    for line in deferred_blocker_lines:
        print(line)

    # Real output.py::display: `if self.conf.verbosity == 3:
    # self.print_verbose(...)` -- the `Total: ...` counters line, printed
    # after every entry (and blocker) line, only under -v, for the
    # tree/columns/flat layouts alike. Real emits f"\n{self.counters}\n"
    # (a leading blank line). Mirrors pretend.rs.
    if verbose:
        print()
        print(_package_counters_summary(entries, top_level_pkgs, onlydeps, color))

    # Real depgraph._show_slot_collision_notice -> slot_conflict_handler.
    # get_conflict() (lib/_emerge/resolver/slot_collision.py): the
    # "!!! Multiple package instances within a single package slot ..."
    # block, then the advisory paragraph. Simplified transcription -- the
    # preamble, per-instance "(<cpv>, ebuild scheduled for merge) pulled in
    # by" + "<atom> required by (<parent>)" / "<atom> (Argument)" lines,
    # and the advisory (with the --backtrack=30 hint gated the real way:
    # shown unless --backtrack is >=30 or 0). Cut (documented, fixtures
    # don't exercise them): collision_reasons grouping / best-atom
    # selection, pkg_use_display, --verbose-conflicts USE markers, "omitted
    # N similar parents", operator colorization. Purely informational -- v1
    # neither refuses nor changes the exit code. Mirrors pretend.rs.
    if result["slot_conflicts"]:
        print()
        print(
            "!!! Multiple package instances within a single package slot have been pulled"
        )
        print("!!! into the dependency graph, resulting in a slot conflict:")
        for c in result["slot_conflicts"]:
            print()
            print(f"{c['category']}/{c['package']}:{c['slot']}")
            for inst in c["instances"]:
                print()
                print(
                    f"  ({c['category']}/{c['package']}-{inst['version']}:{c['slot']}"
                    f"/{inst['sub_slot']}::{inst['repo_name']}, ebuild scheduled for merge)"
                    " pulled in by"
                )
                for parent_cpv, atom in inst["parents"]:
                    if not parent_cpv:
                        print(f"    {atom} (Argument)")
                    else:
                        print(
                            f"    {atom} required by ({parent_cpv}, ebuild scheduled for merge)"
                        )
        print()
        for line in (
            "It may be possible to solve this problem by using package.mask to",
            "prevent one of those packages from being selected. However, it is also",
            "possible that conflicting dependencies exist such that they are",
            "impossible to satisfy simultaneously.  If such a conflict exists in",
            "the dependencies of two different packages, then those packages can",
        ):
            print(line)
        if 0 < backtrack_max < 30:
            print("not be installed simultaneously. You may want to try a larger value of")
            print("the --backtrack option, such as --backtrack=30, in order to see if")
            print("that will solve this conflict automatically.")
        else:
            print("not be installed simultaneously.")
        print()
        print("For more information, see MASKED PACKAGES section in the emerge man")
        print("page or refer to the Gentoo Handbook.")
        print()

    # Real depgraph.py::_display_autounmask (:10625): --autounmask
    # applied keyword / USE changes to make the graph resolve, so they
    # are reported after the merge list. Real _writemsg: `\nThe following
    # <BAD>{reason}</BAD> are necessary to proceed:\n (see "{file}" in the
    # portage(5) man page for more details)\n`; then format_msg
    # (`#`-prefixed dep-chain comment lines stay plain, the `{atom}
    # {token}` line is INFORM-coloured -- the atom carries its own op
    # prefix: `=` for keywords, `>=`/`>=...:slot`/`=` for USE). One
    # header covers every change of a kind; keyword block first, then
    # USE (real order). Real portage does NOT print the "Use
    # --autounmask-write" hint under --pretend (:11084 `not pretend`),
    # and `emerge --pretend` still exits 0 (real actions.py:563). Mirrors
    # pretend.rs.
    def _print_autounmask_block(reason, cfg_file, changes):
        if not changes:
            return
        print(
            f'\nThe following {color.c("BAD", reason)} are necessary to proceed:',
            file=sys.stderr,
        )
        print(
            f' (see "{cfg_file}" in the portage(5) man page for more details)',
            file=sys.stderr,
        )
        for change in changes:
            for line in change["dep_chain"]:
                print(f"# {line}", file=sys.stderr)
            atom_line = (
                change["atom"]
                if not change["token"]
                else f'{change["atom"]} {change["token"]}'
            )
            print(color.c("INFORM", atom_line), file=sys.stderr)

    # Real _display_autounmask _writemsg order: keyword, mask, USE, license.
    _print_autounmask_block(
        "keyword changes",
        "package.accept_keywords",
        result["autounmask_keyword_changes"],
    )
    _print_autounmask_block(
        "mask changes", "package.unmask", result["autounmask_mask_changes"]
    )
    _print_autounmask_block(
        "USE changes", "package.use", result["autounmask_use_changes"]
    )
    _print_autounmask_block(
        "license changes", "package.license", result["autounmask_license_changes"]
    )

    # Real _show_abi_rebuild_info (depgraph.py:1210), gated on
    # --verbose-slot-rebuilds != "n" (default on, NOT --verbose), after
    # the merge list / autounmask blocks and before the changed-deps
    # report. writemsg_stdout -> stdout. Mirrors pretend.rs.
    if verbose_slot_rebuilds and result["abi_rebuilds"]:
        print()
        print("The following packages are causing rebuilds:")
        print()
        provider = None
        for child, parent in result["abi_rebuilds"]:
            if child != provider:
                print(f"  {child} causes rebuilds for:")
                provider = child
            print(f"    {parent}")

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

    # Real depgraph.py's own display_problems(): shown *after* the merge
    # list above (real `_show_merge_list()` runs first), then the whole
    # action fails -- see resolve_pretend_graph's own
    # "buildpkgonly_deps_unsatisfied" comment for the exact real check
    # this mirrors.
    if result["buildpkgonly_deps_unsatisfied"]:
        print(file=sys.stderr)
        print("!!! --buildpkgonly requires all dependencies to be merged.", file=sys.stderr)
        print("!!! Cannot merge requested packages. Merge deps and try again.", file=sys.stderr)
        print(file=sys.stderr)
        return 1

    return 0


if __name__ == "__main__":
    sys.exit(run(sys.argv[1:]))
