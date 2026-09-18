"""Expectation-free checks on portuale's resolver output
(`docs/second_python_copy_removal.md` §1, §2, §9).

* §1 -- every contract case and every fixture package, re-run with
  `--json`, satisfies `output_invariants.check_json`;
* §2 -- plain, `--tree`, `--quiet` and `--json` describe the same graph,
  and `-v` rows / `Total:` counters are self-consistent;
* §4 -- the resolver dropped no dependency token as unparseable;
* §9 -- the same command gives byte-identical output on repeated runs
  (Rust's per-process hash seed varies, so this catches `HashMap`
  iteration order leaking into output).

Runs are fanned out over a thread pool and reported as one failure per
group, so the whole grid stays a few seconds.
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
from concurrent.futures import ThreadPoolExecutor

import pytest

import output_invariants as inv
from conftest import FIXTURES_ROOT
from test_emerge_pretend_contract import CASES

REPO_ROOTS = [FIXTURES_ROOT / r for r in
              ("repo", "overlay", "independentoverlay", "layoutmasteroverlay")]
MODE_FLAGS = {"--tree", "-t", "--json", "--quiet", "-q", "--verbose", "-v"}
WORKERS = max(2, min(16, (os.cpu_count() or 2)))


def _fixture_atoms() -> list[str]:
    atoms = set()
    for root in REPO_ROOTS:
        for ebuild in root.glob("*/*/*.ebuild"):
            atoms.add(f"{ebuild.parent.parent.name}/{ebuild.parent.name}")
    return sorted(atoms)


def _run(emerge, args, env):
    return subprocess.run([str(emerge), *args], capture_output=True, text=True,
                          env=env, check=False)


def _cross_mode_applies(args: list[str]) -> bool:
    for a in args:
        if a in MODE_FLAGS or "color" in a or "columns" in a:
            return False
        if a.startswith("-") and not a.startswith("--") and set(a[1:]) & set("tqv"):
            return False
    return True


def _check(emerge, args, env) -> list[str] | None:
    base = [a for a in args if a not in MODE_FLAGS]
    json_run = _run(emerge, [*base, "--json"],
                    {**env, "PORTUALE_REPORT_UNPARSED_DEP_TOKENS": "1"})
    doc = inv.parse_json_stdout(json_run.stdout)
    if doc is None:
        return None  # not a resolver command (or a CLI error): nothing to check
    problems = inv.check_json(doc, FIXTURES_ROOT, REPO_ROOTS, base)
    problems += inv.check_unparsed_dep_tokens(json_run.stderr)
    if _cross_mode_applies(args):
        plain, tree, quiet, verbose = (
            _run(emerge, [*base, *extra], env).stdout
            for extra in ([], ["--tree"], ["--quiet"], ["--verbose"])
        )
        problems += inv.check_cross_mode(plain, tree, quiet, doc, base)
        problems += inv.check_plain_use(verbose) + inv.check_summary(verbose)
    return problems


def _report(results: list[tuple[str, list[str] | None]]) -> None:
    lines = [f"{label}: {p}" for label, problems in results for p in problems or []]
    checked = sum(1 for _, problems in results if problems is not None)
    assert checked, "no case produced --json output"
    assert not lines, f"{len(lines)} invariant violation(s):\n" + "\n".join(lines)


def test_contract_cases_satisfy_output_invariants(emerge_binary, fixture_env):
    def one(case):
        description, args, _ = case
        return description, _check(emerge_binary, args, fixture_env)

    with ThreadPoolExecutor(WORKERS) as pool:
        _report(list(pool.map(one, CASES)))


@pytest.mark.parametrize(
    "options",
    [["--pretend"], ["--pretend", "--update", "--deep"], ["--pretend", "--emptytree"]],
    ids=["p", "puD", "pe"],
)
def test_fixture_packages_satisfy_output_invariants(options, emerge_binary, fixture_env):
    def one(atom):
        return atom, _check(emerge_binary, [*options, atom], fixture_env)

    with ThreadPoolExecutor(WORKERS) as pool:
        _report(list(pool.map(one, _fixture_atoms())))


def _determinism_args() -> list[list[str]]:
    return [args for _, args, _ in CASES] + [
        ["--pretend", "--update", "--deep", "--newuse", s]
        for s in ("@world", "@system", "@selected")
    ]


def test_repeated_runs_are_byte_identical(emerge_binary, fixture_env):
    """§9: each contract case three times; any difference is iteration
    order leaking into output."""

    def one(args):
        runs = [_run(emerge_binary, args, fixture_env) for _ in range(3)]
        first = runs[0]
        same = all((r.returncode, r.stdout, r.stderr) ==
                   (first.returncode, first.stdout, first.stderr) for r in runs[1:])
        return " ".join(args), None if same else ["output differs between identical runs"]

    with ThreadPoolExecutor(WORKERS) as pool:
        results = list(pool.map(one, _determinism_args()))
    lines = [label for label, problems in results if problems]
    assert not lines, "non-deterministic output for:\n" + "\n".join(lines)


def test_output_is_identical_under_shuffled_directory_order(emerge_binary, fixture_env):
    """§9's shuffle half (backlog #51): `PORTUALE_SHUFFLE_DIRS` makes every
    production directory read return a seeded, deliberately shuffled order
    (`portage-util`'s `read_dir_entries`). Output must not depend on it."""

    seeds = ("1", "17", "4242")

    def one(args):
        runs = [
            _run(emerge_binary, args, {**fixture_env, "PORTUALE_SHUFFLE_DIRS": seed})
            for seed in seeds
        ]
        first = runs[0]
        same = all((r.returncode, r.stdout, r.stderr) ==
                   (first.returncode, first.stdout, first.stderr) for r in runs[1:])
        return " ".join(args), None if same else [
            "output differs between PORTUALE_SHUFFLE_DIRS seeds " + ", ".join(seeds)]

    with ThreadPoolExecutor(WORKERS) as pool:
        results = list(pool.map(one, _determinism_args()))
    lines = [label for label, problems in results if problems]
    assert not lines, "readdir-order-dependent output for:\n" + "\n".join(lines)


def _reversed_repos_conf(tmp_path):
    """A configroot copy of `fixtures/` whose `repos.conf` `[section]`
    blocks are in reverse order (the `[DEFAULT]` header stays first); the
    repos themselves are symlinked, so ROOT can stay the real fixtures."""
    root = tmp_path / "configroot"
    shutil.copytree(FIXTURES_ROOT / "etc", root / "etc", symlinks=True)
    for entry in FIXTURES_ROOT.iterdir():
        if entry.name != "etc":
            (root / entry.name).symlink_to(entry)
    conf = root / "etc" / "portage" / "repos.conf" / "repos.conf"
    sections = re.split(r"(?m)^(?=\[)", conf.read_text())
    header, blocks = sections[0], sections[1:]
    assert len(blocks) > 1, "fixture repos.conf must define several repos"
    conf.write_text(header + "".join(reversed(blocks)))
    return root


def test_repos_conf_section_order_does_not_change_output(emerge_binary, fixture_env, tmp_path):
    """Backlog #51 S1: the repo list is priority-sorted, not file-order
    dependent, so reversing the `[repo]` sections changes nothing."""
    configroot = _reversed_repos_conf(tmp_path)
    env = dict(fixture_env)
    env["PORTAGE_CONFIGROOT"] = str(configroot)

    def one(args):
        want = _run(emerge_binary, args, fixture_env)
        got = _run(emerge_binary, args, env)
        same = (got.returncode, got.stdout, got.stderr) == \
               (want.returncode, want.stdout, want.stderr)
        return " ".join(args), None if same else ["output differs with repos.conf sections reversed"]

    with ThreadPoolExecutor(WORKERS) as pool:
        results = list(pool.map(one, _determinism_args()))
    lines = [label for label, problems in results if problems]
    assert not lines, "repos.conf file-order-dependent output for:\n" + "\n".join(lines)


# -- the checker itself --------------------------------------------------
# Each synthetic output below carries exactly one past bug class; the
# checker must flag it (a checker that goes blind passes everything).


def _entry(pkg, order, required_by=(), requested=False, outcome="new", slot="0"):
    return {
        "category": "dev-libs", "package": pkg, "merge_order": order,
        "outcome": outcome, "version": "1.0", "slot": slot, "requested": requested,
        "required_by": [{"category": "dev-libs", "package": p} for p in required_by],
        "builds_against_running_root": None,
    }


def _doc(*entries, slot_conflicts=()):
    return {"entries": list(entries), "slot_conflicts": list(slot_conflicts),
            "aborted": None, "_dup_keys": []}


def test_checker_flags_a_wiped_required_by():
    # what-this-proves.md: a destructive required_by_map.remove left later
    # slots with `required_by: []`.
    doc = _doc(_entry("leaf", 0, ()), _entry("root", 1, requested=True))
    problems = inv.check_json(doc)
    assert any("required_by is empty" in p for p in problems)
    assert any("unreachable" in p for p in problems)


def test_checker_flags_an_owner_that_is_neither_entry_nor_installed(tmp_path):
    doc = _doc(_entry("leaf", 0, ("ghost",)), _entry("root", 1, requested=True))
    assert any("neither an entry nor installed" in p
               for p in inv.check_json(doc, root=tmp_path))


def test_checker_flags_a_duplicate_slot_without_a_conflict():
    doc = _doc(_entry("dup", 0, ("root",)), _entry("dup", 1, ("root",)),
               _entry("root", 2, requested=True))
    assert any("appears 2 times" in p for p in inv.check_json(doc))
    conflict = [{"category": "dev-libs", "package": "dup"}]
    assert not inv.check_json(_doc(*_doc(
        _entry("dup", 0, ("root",)), _entry("dup", 1, ("root",)),
        _entry("root", 2, requested=True))["entries"], slot_conflicts=conflict))


def test_checker_flags_a_dependency_merged_after_its_owner():
    doc = _doc(_entry("root", 0, requested=True), _entry("leaf", 1, ("root",)))
    assert any("merges after its owner" in p for p in inv.check_json(doc))


def test_checker_tolerates_a_multi_instance_child_satisfied_earlier():
    # The cp-level required_by can't say which instance owns the edge: the
    # L0 firefox/thunderbird clang/llvm/rust-bin rows are the later
    # instance of a cp whose earlier instance already merged before the
    # owner (backlog #48).
    doc = _doc(
        _entry("multi", 0, ("root",), slot="22"),
        _entry("root", 1, requested=True),
        _entry("multi", 2, ("root",), slot="21"),
    )
    assert not [p for p in inv.check_json(doc) if "merge order" in p]
    # Neither instance before the owner: still flagged.
    late = _doc(_entry("root", 0, requested=True),
                _entry("multi", 2, ("root",), slot="21"),
                _entry("multi", 3, ("root",), slot="22"))
    assert len([p for p in inv.check_json(late) if "merge order" in p]) == 2


def test_checker_tolerates_soft_edges_only():
    # Real's `ignore_priority` ladder may relax RDEPEND (runtime) and
    # PDEPEND (runtime_post) edges; DEPEND/BDEPEND/IDEPEND never.
    doc = _doc(_entry("root", 0, requested=True), _entry("leaf", 1, ("root",)))
    key = (("dev-libs", "leaf"), ("dev-libs", "root"))
    assert not [p for p in inv.check_json(doc, dep_vars={key: {"RDEPEND"}})
                if "merge order" in p]
    assert not [p for p in inv.check_json(doc, dep_vars={key: {"PDEPEND"}})
                if "merge order" in p]
    assert [p for p in inv.check_json(doc, dep_vars={key: {"BDEPEND"}})
            if "merge order" in p]
    assert [p for p in inv.check_json(doc, dep_vars={key: {"RDEPEND", "BDEPEND"}})
            if "merge order" in p]
    # No metadata: stays strict.
    assert [p for p in inv.check_json(doc, dep_vars={}) if "merge order" in p]


def test_checker_honours_a_vdb_list_snapshot():
    # L0 cannot expose the container's vdb; `parse_vdb_list` is the
    # snapshot the checker takes `installed` from (backlog #48).
    installed = inv.parse_vdb_list("dev-libs/leaf-1.0\ndev-libs/other-2.0-r1\n")
    assert installed == {("dev-libs", "leaf"), ("dev-libs", "other")}
    doc = _doc(_entry("root", 0, requested=True), _entry("leaf", 1, ("root",)))
    assert not [p for p in inv.check_json(doc, installed=installed) if "merge order" in p]


def test_checker_tolerates_a_cycle():
    doc = _doc(_entry("a", 0, ("b",), requested=True), _entry("b", 1, ("a",)))
    assert not [p for p in inv.check_json(doc) if "merge order" in p]


def test_checker_flags_a_repeated_use_flag():
    # The IUSE `x x` double render both implementations shared.
    out = '[ebuild  N     ] dev-libs/foo-1.0::testrepo  USE="x x -y"\n'
    assert inv.check_plain_use(out) == ["dev-libs/foo: USE repeats ['x']"]


def test_checker_flags_wrong_total_counters():
    out = ("[ebuild  N     ] dev-libs/foo-1.0 \n"
           "[ebuild     U  ] dev-libs/bar-2.0 [1.0]\n\n"
           "Total: 2 packages (2 new), Size of downloads: 0 KiB\n")
    problems = inv.check_summary(out)
    assert "Total says 2 new, merge list has 1" in problems
    assert "Total says 0 upgrade, merge list has 1" in problems


def test_checker_flags_a_flush_left_never_reached_tree_row():
    # The first required_by bug surfaced as a flush-left --tree row; the
    # cross-mode check catches the opposite mis-nesting.
    doc = _doc(_entry("leaf", 0, ("other",)), _entry("other", 1, requested=True),
               _entry("root", 2, requested=True))
    tree = ("[ebuild  N     ] dev-libs/root-1.0 \n"
            "[ebuild  N     ]   dev-libs/leaf-1.0 \n"
            "[ebuild  N     ] dev-libs/other-1.0 \n")
    plain = "".join(line.replace("  dev", "dev") + "\n" for line in tree.splitlines())
    problems = inv.check_cross_mode(plain, tree, plain, doc)
    assert any("required_by" in p for p in problems)


def test_checker_parses_binary_build_ids():
    assert inv.split_cpv("dev-libs/foo-1.0-3") == ("dev-libs", "foo", "1.0")
    assert inv.split_cpv("dev-libs/foo-1.0-r3:0::repo") == ("dev-libs", "foo", "1.0-r3")
