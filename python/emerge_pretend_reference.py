#!/usr/bin/env python3
"""Python reference implementation for the `emerge --pretend` pilot slice
(see PORTING/PROMPT.md and PORTING/rust/portage-repo/src/lib.rs for the
full scope writeup). Mirrors the exact same restricted v1 algorithm as the
Rust side -- hardcoded ACCEPT_KEYWORDS=amd64, no profile/make.conf
stacking, main repo only, no dependency recursion -- so the two can be
contract-tested against each other, argv-for-argv and byte-for-byte on
stdout, the same way every other pilot slice is.

This is NOT a wrapper around the real `emerge` binary (unlike the
Python-side harnesses for versions/atom/use_reduce, which wrap real
production code): the whole point of this slice is that config.py's and
depgraph.py's real machinery is deliberately not being exercised yet, so
there is no real code to wrap for the parts this script implements. It
does reuse the real portage.versions.vercmp for version ordering.

Usage mirrors the real emerge CLI (and the Rust multicall's `emerge`
applet) directly:
    emerge_pretend_reference.py --pretend <category/package>

Config/target roots come from the real PORTAGE_CONFIGROOT/ROOT environment
variables, defaulting to "/" -- see lib/portage/const.py.
"""

import configparser
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "..", "lib"))

from portage.dep import Atom
from portage.exception import InvalidAtom
from portage.versions import vercmp

ACCEPT_KEYWORDS = "amd64"


class ResolutionError(Exception):
    pass


def _config_root():
    return os.environ.get("PORTAGE_CONFIGROOT") or "/"


def _root():
    return os.environ.get("ROOT") or "/"


def find_main_repo(config_root):
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

    location = parser.get(main_repo, "location", fallback=None)
    if location is None:
        raise ResolutionError(f'no location for repo "{main_repo}" in repos.conf')

    # Real repos.conf always uses absolute locations; relative ones are a
    # pilot/testing convenience -- see the matching comment in
    # portage-repo/src/lib.rs.
    if not os.path.isabs(location):
        location = os.path.join(config_root, location)

    return main_repo, location


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


def list_candidates(repo_location, category, package):
    pkg_dir = os.path.join(repo_location, category, package)
    if not os.path.isdir(pkg_dir):
        return []
    candidates = []
    for name in os.listdir(pkg_dir):
        if not name.endswith(".ebuild"):
            continue
        stem = name[: -len(".ebuild")]
        version = _strip_version_prefix(stem, package)
        if version is None:
            continue
        try:
            metadata = read_md5_cache(repo_location, category, stem)
        except OSError:
            continue
        keywords = metadata.get("KEYWORDS", "").split()
        slot = metadata.get("SLOT", "0").split("/")[0]
        candidates.append({"version": version, "keywords": keywords, "slot": slot})
    return candidates


def is_visible(candidate):
    return ACCEPT_KEYWORDS in candidate["keywords"]


def _max_version(versions):
    best = versions[0]
    for v in versions[1:]:
        if (vercmp(v, best) or 0) > 0:
            best = v
    return best


def select_best_visible(candidates):
    visible = [c["version"] for c in candidates if is_visible(c)]
    if not visible:
        return None
    return _max_version(visible)


def installed_versions(root, category, package):
    cat_dir = os.path.join(root, "var", "db", "pkg", category)
    if not os.path.isdir(cat_dir):
        return []
    versions = []
    for name in os.listdir(cat_dir):
        if not os.path.isdir(os.path.join(cat_dir, name)):
            continue
        v = _strip_version_prefix(name, package)
        if v is not None:
            versions.append(v)
    return versions


def resolve_pretend(config_root, root, category, package):
    """Returns a tuple whose first element is the outcome tag: "new",
    "upgrade", "already_installed", or "no_visible_candidate"."""
    _, repo_location = find_main_repo(config_root)
    candidates = list_candidates(repo_location, category, package)
    best = select_best_visible(candidates)
    if best is None:
        return ("no_visible_candidate",)

    installed = installed_versions(root, category, package)
    if best in installed:
        return ("already_installed", best)
    if installed:
        return ("upgrade", _max_version(installed), best)
    return ("new", best)


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
    category, package = atom.cp.split("/", 1)

    try:
        outcome = resolve_pretend(_config_root(), _root(), category, package)
    except ResolutionError as e:
        print(f"emerge: {e}", file=sys.stderr)
        return 1

    tag = outcome[0]
    if tag == "new":
        print(f"[ebuild  N] {category}/{package}-{outcome[1]}")
        return 0
    if tag == "upgrade":
        print(f"[ebuild  U] {category}/{package}-{outcome[2]} (upgrade from {outcome[1]})")
        return 0
    if tag == "already_installed":
        print(f"{category}/{package}-{outcome[1]} is already installed; nothing to do")
        return 0
    print(f'!!! no visible ebuild for "{category}/{package}"', file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(run(sys.argv[1:]))
