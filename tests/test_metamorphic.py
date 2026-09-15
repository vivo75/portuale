"""Metamorphic tests (`docs/second_python_copy_removal.md` §5): input
transforms that must not change portuale's resolver output.

Five transforms, each applied to a private copy of `fixtures/`:

1. duplicate an IUSE token in a package the case resolves (the
   motivating bug -- IUSE `x x` rendered twice -- was invisible to the
   removed Python mirror because both copies shared it);
2. add an unrelated package to the main repo;
3. add an empty overlay repo to `repos.conf`;
4. rename a fixture category consistently, everywhere including the
   vdb and every atom string (output normalised back);
5. reorder two non-overlapping `package.use` lines.

A representative subset of `CASES` (the IUSE/IUSE_EXPAND/config-file
heavy fixtures, at most three cases per fixture) runs pristine and
transformed; `(exit, stdout, stderr)` must be byte-identical. This is a
property test, not an exhaustive one (the plan's own wording). A
transform that changes output is a genuine bug: fix it and keep the
failing case as a permanent regression fixture rather than relaxing the
transform.
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
from collections import Counter
from collections.abc import Callable
from pathlib import Path

import pytest

from conftest import FIXTURES_ROOT
from test_emerge_pretend_contract import CASES

REPO_NAMES = ("repo", "overlay", "independentoverlay", "layoutmasteroverlay", "repnamerepo")

SUBSET_ATOMS = (
    "dev-libs/useflagpkg",
    "dev-libs/useexpandpkg",
    "dev-libs/iusedefaultpkg",
    "dev-libs/packageuseenablepkg",
    "dev-libs/packageusedisablepkg",
    "dev-libs/requireduseokpkg",
    "dev-libs/wildexpandpkg",
    "dev-libs/usebuildpkg",
)


def _subset() -> list[tuple[str, list[str], int]]:
    picked: list[tuple[str, list[str], int]] = []
    counts: Counter[str] = Counter()
    for case in CASES:
        _, args, _ = case
        joined = " ".join(args)
        for atom in SUBSET_ATOMS:
            if atom in joined:
                if counts[atom] < 3:
                    counts[atom] += 1
                    picked.append(case)
                break
    return picked


SUBSET = _subset()

_CAT_PKG = re.compile(r"(?<![\w/.-])([a-z0-9][\w.+-]*)/([\w.+-]+)")


def _atoms(args: list[str]) -> list[tuple[str, str]]:
    seen: list[tuple[str, str]] = []
    for arg in args:
        for m in _CAT_PKG.finditer(arg):
            cp = (m.group(1), m.group(2))
            if cp not in seen:
                seen.append(cp)
    return seen


def _materialize(dst: Path) -> Path:
    root = dst / "fixtures"
    shutil.copytree(FIXTURES_ROOT, root)
    return root


def _atomic_write(path: Path, text: str) -> None:
    tmp = path.with_name(path.name + ".mt-tmp")
    tmp.write_text(text)
    os.replace(tmp, path)


def _tree_fingerprint(root: Path) -> tuple:
    return tuple(
        (str(p.relative_to(root)), p.stat().st_size, p.stat().st_mtime_ns)
        for p in sorted(root.rglob("*")) if p.is_file()
    )


def _run(emerge: Path, root: Path, args: list[str]) -> subprocess.CompletedProcess:
    env = dict(os.environ)
    env["PORTAGE_CONFIGROOT"] = str(root)
    env["ROOT"] = str(root)
    env["PORTAGE_RUNNING_ROOT"] = str(root)
    env["DISTDIR"] = str(root / "distfiles")
    return subprocess.run([str(emerge), *args], capture_output=True, text=True,
                          env=env, check=False)


_PRISTINE: dict[tuple[str, ...], subprocess.CompletedProcess] = {}


def _pristine(emerge: Path, root: Path, args: list[str]) -> subprocess.CompletedProcess:
    key = tuple(args)
    if key not in _PRISTINE:
        _PRISTINE[key] = _run(emerge, root, args)
    return _PRISTINE[key]


def _normalised(result: subprocess.CompletedProcess, roots: list[Path],
                rewrite=None) -> tuple:
    out = []
    for field in (result.returncode, result.stdout, result.stderr):
        if isinstance(field, str):
            for root in roots:
                field = field.replace(str(root), "<ROOT>")
            if rewrite is not None:
                field = rewrite(field)
        out.append(field)
    return tuple(out)


# -- transforms ----------------------------------------------------------


def _duplicate_iuse(root: Path, args: list[str]) -> tuple[list[str], None]:
    for cat, pkg in _atoms(args):
        cache = [
            f for repo in REPO_NAMES
            for f in sorted((root / repo / "metadata" / "md5-cache" / cat).glob(f"{pkg}-*"))
            if f.is_file()
        ]
        ebuilds = sorted(root.glob(f"*/{cat}/{pkg}/*.ebuild"))
        changed = False
        for path in [*cache, *ebuilds]:
            text = path.read_text()
            lines = text.splitlines(keepends=True)
            for i, line in enumerate(lines):
                m = re.match(r"^(IUSE=)(['\"]?)([^'\"]*)\2\s*$", line)
                if not m or not m.group(3).split():
                    continue
                tokens = m.group(3).split()
                dup = " ".join([tokens[0], *tokens])
                lines[i] = f"{m.group(1)}{m.group(2)}{dup}{m.group(2)}\n"
                changed = True
                break
            if changed:
                _atomic_write(path, "".join(lines))
        if changed:
            return args, None
    pytest.skip("no fixture package with a non-empty IUSE in this case's atoms")


_UNRELATED_EBUILD = """EAPI=8
DESCRIPTION="fixture package: metamorphic control, referenced by nothing"
SLOT="0"
KEYWORDS="amd64"
"""

_UNRELATED_CACHE = """DEFINED_PHASES=-
DESCRIPTION=fixture package: metamorphic control, referenced by nothing
EAPI=8
IUSE=
KEYWORDS=amd64
SLOT=0
_md5_=0000000000000000000000000000000
"""


def _add_unrelated_package(root: Path, args: list[str]) -> tuple[list[str], None]:
    pkgdir = root / "repo" / "dev-libs" / "mtunrelated"
    pkgdir.mkdir(parents=True)
    (pkgdir / "mtunrelated-1.0.ebuild").write_text(_UNRELATED_EBUILD)
    (root / "repo" / "metadata" / "md5-cache" / "dev-libs" / "mtunrelated-1.0").write_text(
        _UNRELATED_CACHE)
    return args, None


def _add_empty_overlay(root: Path, args: list[str]) -> tuple[list[str], None]:
    overlay = root / "mtoverlay"
    (overlay / "metadata").mkdir(parents=True)
    (overlay / "profiles").mkdir(parents=True)
    (overlay / "profiles" / "repo_name").write_text("mtoverlay\n")
    repos = root / "etc" / "portage" / "repos.conf" / "repos.conf"
    text = repos.read_text()
    if not text.endswith("\n"):
        text += "\n"
    _atomic_write(repos, text + "\n[mtoverlay]\nlocation = mtoverlay\npriority = 50\n")
    return args, None


_CATEGORY_OLD = "dev-libs"
_CATEGORY_NEW = "dev-libs-rn"


def _rename_category(root: Path, args: list[str]) -> tuple[list[str], Callable]:
    old, new = _CATEGORY_OLD, _CATEGORY_NEW
    for repo in REPO_NAMES:
        for base in (root / repo, root / repo / "metadata" / "md5-cache"):
            d = base / old
            if d.is_dir():
                d.rename(base / new)
    vdb = root / "var" / "db" / "pkg" / old
    if vdb.is_dir():
        vdb.rename(vdb.parent / new)
    for path in sorted(root.rglob("*")):
        if not path.is_file():
            continue
        try:
            text = path.read_text()
        except (UnicodeDecodeError, OSError):
            continue
        if old + "/" in text:
            _atomic_write(path, text.replace(old + "/", new + "/"))
    rewritten = [a.replace(old + "/", new + "/") for a in args]
    return rewritten, lambda s: s.replace(new + "/", old + "/")


def _reorder_package_use(root: Path, args: list[str]) -> tuple[list[str], None]:
    path = root / "etc" / "portage" / "package.use"
    lines = path.read_text().splitlines(keepends=True)
    for i in range(len(lines) - 1):
        a = lines[i].split()
        b = lines[i + 1].split()
        if a and b and a[0] != b[0]:
            lines[i], lines[i + 1] = lines[i + 1], lines[i]
            _atomic_write(path, "".join(lines))
            return args, None
    pytest.skip("package.use has no two adjacent non-overlapping lines")


TRANSFORMS = [
    pytest.param(_duplicate_iuse, id="duplicate-iuse"),
    pytest.param(_add_unrelated_package, id="add-unrelated-package"),
    pytest.param(_add_empty_overlay, id="add-empty-overlay"),
    pytest.param(_rename_category, id="rename-category"),
    pytest.param(_reorder_package_use, id="reorder-package-use"),
]


# -- tests ---------------------------------------------------------------


@pytest.fixture(scope="session")
def pristine_fixtures(tmp_path_factory: pytest.TempPathFactory) -> Path:
    return _materialize(tmp_path_factory.mktemp("metamorphic-pristine"))


@pytest.mark.parametrize("case", SUBSET, ids=[d for d, _, _ in SUBSET])
@pytest.mark.parametrize("transform", TRANSFORMS)
def test_transform_does_not_change_output(case, transform, emerge_binary,
                                          pristine_fixtures, tmp_path):
    description, args, _ = case
    root = _materialize(tmp_path)
    before = _tree_fingerprint(root)
    transformed_args, rewrite = transform(root, list(args))
    assert _tree_fingerprint(root) != before, (
        f"transform {description!r} did not modify the fixture tree")

    want = _pristine(emerge_binary, pristine_fixtures, args)
    got = _run(emerge_binary, root, transformed_args)
    assert _normalised(got, [root, pristine_fixtures], rewrite) == \
        _normalised(want, [root, pristine_fixtures], rewrite), (
        f"{description}: transform changed the output")
