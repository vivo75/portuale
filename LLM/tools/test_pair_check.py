#!/usr/bin/env python3
"""Regression checks for pair_check.sh on two throwaway git repos (no cargo).
Run: python3 -m pytest LLM/tools/test_pair_check.py

Each case pins one rule: (a) branch-name mismatch, and PAIR_STANDALONE as the
only way past it, (b) a merge-path diff with no LLM/evidence/merge-gate-<branch>.md
naming a glibc and a bash result (a slash in the branch becomes a dash),
(c) a pmtest corpus commit without "corpus bless", (d) the warning-only
missing-pmtest-sha case, and the clean case.
"""

import os
import shutil
import subprocess

import pytest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "pair_check.sh")
MERGE_FILE = "crates/portuale/src/ebuild_merge/tree.rs"
GIT_ENV = {
    "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t",
    "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t",
    "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_SYSTEM": "/dev/null",
}


def git(repo, *args):
    env = dict(os.environ, **GIT_ENV)
    return subprocess.run(["git", "-C", str(repo), *args], check=True, env=env,
                          capture_output=True, text=True).stdout.strip()


def commit(repo, path, msg, text="x\n"):
    full = os.path.join(str(repo), path)
    os.makedirs(os.path.dirname(full), exist_ok=True)
    with open(full, "a") as f:
        f.write(text)
    git(repo, "add", "-A")
    git(repo, "commit", "-q", "-m", msg)
    return git(repo, "rev-parse", "--short=7", "HEAD")


def make_pair(tmp_path, pbranch="feat", tbranch="feat"):
    """portuale + pmtest repos, each with a base commit on main, then branched."""
    por, pmt = tmp_path / "portuale", tmp_path / "pmtest"
    for repo, br in ((por, pbranch), (pmt, tbranch)):
        os.makedirs(str(repo))
        git(repo, "init", "-q", "-b", "main")
        commit(repo, "README", "base")
        if br != "main":
            git(repo, "checkout", "-q", "-b", br)
    os.makedirs(str(por / "LLM" / "tools"))
    shutil.copy(SCRIPT, str(por / "LLM" / "tools" / "pair_check.sh"))
    git(por, "add", "-A")
    git(por, "commit", "-q", "-m", "tools")
    return por, pmt


def run(por, pmt, **extra):
    env = dict(os.environ, PMTEST_DIR=str(pmt))
    for k in ("PAIR_STANDALONE", "BASE", "PMTEST_BASE", "PAIR_INTEGRATION"):
        env.pop(k, None)
    env.update(extra)
    # BASE must exclude the "tools" setup commit on the branch under test.
    env.setdefault("BASE", git(por, "rev-parse", "HEAD"))
    p = subprocess.run(["sh", str(por / "LLM" / "tools" / "pair_check.sh")], env=env,
                       capture_output=True, text=True)
    return p.returncode, p.stdout + p.stderr


def make_integration_pair(tmp_path, pbranch="feat", tbranch="feat"):
    """main -> opencode (with a merge-path commit) -> the slice branch.

    The shape that made #758 visible: a branch cut from the integration branch
    inherits nothing, while `main` as the base would make it inherit the
    merge-path change that lives on `opencode`.
    """
    por, pmt = tmp_path / "portuale", tmp_path / "pmtest"
    for repo, br in ((por, pbranch), (pmt, tbranch)):
        os.makedirs(str(repo))
        git(repo, "init", "-q", "-b", "main")
        commit(repo, "README", "base")
        if repo is por:
            os.makedirs(str(repo / "LLM" / "tools"))
            shutil.copy(SCRIPT, str(repo / "LLM" / "tools" / "pair_check.sh"))
            git(repo, "add", "-A")
            git(repo, "commit", "-q", "-m", "tools; pmtest pair")
            # The integration branch is where a merge-path change lives.
            git(repo, "checkout", "-q", "-b", "opencode")
            commit(repo, MERGE_FILE, "merge path on the integration branch")
        else:
            git(repo, "checkout", "-q", "-b", "opencode")
            commit(repo, "fixtures/integration", "fixture")
        git(repo, "checkout", "-q", "-b", br)
    return por, pmt


def test_758_a_slice_cut_from_the_integration_branch_is_green(tmp_path):
    """The defect: `main` as the base made this branch inherit `opencode`'s
    merge-path change and demand evidence for a docs-only slice."""
    por, pmt = make_integration_pair(tmp_path)
    commit(por, "docs/note.md", "docs only")
    commit(pmt, "fixtures/a", "fixture; portuale docs only")
    rc, out = run(por, pmt, BASE="")
    assert rc == 0, out
    assert "anchor refs/heads/opencode" in out, out


def test_758_the_old_base_is_still_available_and_still_red(tmp_path):
    por, pmt = make_integration_pair(tmp_path)
    commit(por, "docs/note.md", "docs only")
    rc, out = run(por, pmt, BASE="", PAIR_INTEGRATION="main")
    assert rc == 1 and "merge-gate-feat.md" in out, out


def test_758_a_slice_that_touches_the_merge_path_is_still_red(tmp_path):
    por, pmt = make_integration_pair(tmp_path)
    commit(por, MERGE_FILE, "the slice itself touches the merge path")
    rc, out = run(por, pmt, BASE="")
    assert rc == 1 and "merge-gate-feat.md" in out, out


def test_758_a_clone_with_only_the_remote_ref_still_resolves(tmp_path):
    """A fresh clone has refs/remotes/origin/opencode and no local branch;
    `--fork-point` fails empty there, so the resolution must fall back to
    plain merge-base against the remote ref rather than to `main`."""
    por, pmt = make_integration_pair(tmp_path)
    commit(por, "docs/note.md", "docs only")
    for repo in (por, pmt):
        tip = git(repo, "rev-parse", "refs/heads/opencode")
        git(repo, "branch", "-D", "opencode")
        git(repo, "update-ref", "refs/remotes/origin/opencode", tip)
        logdir = os.path.join(str(repo), ".git", "logs", "refs", "remotes", "origin")
        shutil.rmtree(logdir, ignore_errors=True)
    rc, out = run(por, pmt, BASE="")
    assert rc == 0, out
    assert "anchor refs/remotes/origin/opencode" in out, out


def test_clean_case(tmp_path):
    por, pmt = make_pair(tmp_path)
    t = commit(pmt, "fixtures/a", "fixture")
    commit(por, "crates/other/x.rs", "change; pmtest %s" % t)
    rc, out = run(por, pmt, BASE=git(por, "rev-parse", "HEAD~1"))
    assert rc == 0, out
    assert "WARN" not in out


def test_a_branch_mismatch_fails(tmp_path):
    por, pmt = make_pair(tmp_path, "feat", "other")
    rc, out = run(por, pmt)
    assert rc == 1 and "branch" in out, out


def test_a_standalone_accepted(tmp_path):
    por, pmt = make_pair(tmp_path, "feat", "other")
    rc, out = run(por, pmt, PAIR_STANDALONE="pmtest-only fixture fix")
    assert rc == 0, out


def test_a_standalone_needs_reason(tmp_path):
    por, pmt = make_pair(tmp_path, "feat", "other")
    rc, out = run(por, pmt, PAIR_STANDALONE="")
    assert rc == 1, out


def test_b_merge_path_without_evidence_fails(tmp_path):
    por, pmt = make_pair(tmp_path)
    base = git(por, "rev-parse", "HEAD")
    commit(por, MERGE_FILE, "touch merge path")
    rc, out = run(por, pmt, BASE=base)
    assert rc == 1 and "merge-gate-feat.md" in out, out


def test_b_evidence_missing_bash_fails(tmp_path):
    por, pmt = make_pair(tmp_path)
    base = git(por, "rev-parse", "HEAD")
    commit(por, MERGE_FILE, "touch merge path")
    commit(por, "LLM/evidence/merge-gate-feat.md", "evidence", "sys-libs/glibc merged ok\n")
    rc, out = run(por, pmt, BASE=base)
    assert rc == 1, out


def test_b_slashed_branch_with_evidence_passes(tmp_path):
    por, pmt = make_pair(tmp_path, "batch/x", "batch/x")
    base = git(por, "rev-parse", "HEAD")
    commit(por, MERGE_FILE, "touch merge path")
    commit(por, "LLM/evidence/merge-gate-batch-x.md", "evidence",
           "sys-libs/glibc merged ok\napp-shells/bash merged ok\n")
    rc, out = run(por, pmt, BASE=base)
    assert rc == 0, out


def test_c_corpus_commit_without_phrase_fails(tmp_path):
    por, pmt = make_pair(tmp_path)
    commit(pmt, "pytests-contract-suite/corpus/c.json", "update corpus")
    rc, out = run(por, pmt)
    assert rc == 1 and "corpus" in out, out


def test_c_corpus_commit_with_phrase_passes(tmp_path):
    por, pmt = make_pair(tmp_path)
    commit(pmt, "pytests-contract-suite/corpus/c.json", "corpus bless after review")
    rc, out = run(por, pmt)
    assert rc == 0, out


def test_d_missing_pmtest_sha_warns_only(tmp_path):
    por, pmt = make_pair(tmp_path)
    commit(pmt, "fixtures/a", "fixture")
    base = git(por, "rev-parse", "HEAD")
    commit(por, "crates/other/x.rs", "change without the sibling sha")
    rc, out = run(por, pmt, BASE=base)
    assert rc == 0, out
    assert "WARN" in out, out
