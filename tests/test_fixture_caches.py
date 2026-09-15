"""Every committed fixture `metadata/md5-cache` entry must be valid for
its ebuild (backlog #46 S1).

Real `portdbapi._pull_valid_cache` validates a pregen entry's `_md5_`
against the ebuild's content before trusting it; a stale entry makes
real run the `depend` phase (C0's P8 oracle), and after #46 S3 portuale
does the same. This guard keeps the checked-in tree valid so the
contract suite keeps testing a cached repo rather than a regenerated
one, and so a new fixture that forgets `_md5_` (or edits the ebuild
without refreshing the entry) fails here instead of changing resolver
results.

md5 only, no phase run: a few hundred files, milliseconds. The entry
path is `<repo>/metadata/md5-cache/<cat>/<pf>`, the ebuild
`<repo>/<cat>/<pn>/<pf>.ebuild`; dot-dirs are pytest junk and skipped.
"""

from __future__ import annotations

import hashlib
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[1]
FIXTURE_ROOTS = [
    REPO_ROOT / "fixtures" / name
    for name in (
        "repo",
        "overlay",
        "independentoverlay",
        "layoutmasteroverlay",
        "repnamerepo",
    )
] + [REPO_ROOT / "TEST" / "images" / "overlay" / "porttest"]


def _entries(root: Path):
    cache = root / "metadata" / "md5-cache"
    if not cache.is_dir():
        return
    for cat in sorted(cache.iterdir()):
        if not cat.is_dir() or cat.name.startswith("."):
            continue
        for entry in sorted(cat.iterdir()):
            if not entry.is_file() or entry.name.startswith("."):
                continue
            yield cat.name, entry.name, entry


def _find_ebuild(root: Path, category: str, pf: str) -> Path | None:
    pkg_root = root / category
    if not pkg_root.is_dir():
        return None
    for pkg in sorted(pkg_root.iterdir()):
        if pkg.is_dir() and not pkg.name.startswith("."):
            ebuild = pkg / f"{pf}.ebuild"
            if ebuild.is_file():
                return ebuild
    return None


def _md5_of_entry(entry: Path) -> str | None:
    for line in entry.read_text(errors="replace").splitlines():
        if line.startswith("_md5_="):
            return line[len("_md5_="):]
    return None


@pytest.mark.parametrize("root", FIXTURE_ROOTS,
                         ids=lambda p: str(p.relative_to(REPO_ROOT)))
def test_committed_fixture_md5_cache_entries_match_their_ebuilds(root: Path):
    problems: list[str] = []
    checked = 0
    for category, pf, entry in _entries(root):
        ebuild = _find_ebuild(root, category, pf)
        if ebuild is None:
            problems.append(f"{category}/{pf}: no ebuild")
            continue
        want = _md5_of_entry(entry)
        got = hashlib.md5(ebuild.read_bytes()).hexdigest()
        checked += 1
        if want != got:
            problems.append(f"{category}/{pf}: _md5_={want} but the ebuild is {got}")
    assert checked, f"{root}: no cache entries found"
    assert not problems, (
        f"{root}: {len(problems)} stale md5-cache entr(y/ies) -- refresh with real "
        f"`egencache --update` (backlog #46 S1):\n" + "\n".join(problems)
    )
