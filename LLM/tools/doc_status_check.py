#!/usr/bin/env python3
"""Check plan `Status:` headers against the backlog, and backlog hygiene.

Usage: doc_status_check.py [--backlog F ...] [--plans-dir D] [--allow-base F]
Exit 0 when clean, 1 with one line per violation on stdout otherwise.

Inputs: backlog files (default glob LLM/backlog-tasks*.md) and plan files
LLM/[0-9]*.opus.md named `<tier>.<item>-<slug>.opus.md`.

Rules (a violation prints as `file:line: (x) #N message`):
  (a) plan `done` => its entry is DONE/CLOSED/WITHDRAWN; plan `proposed` /
      `in progress` => the entry is not DONE unless the Status paragraph names
      the open residues; a plan whose number has no entry is a violation; an
      UNKNOWN entry status (no leading status word) is a violation.
  (b) a number defined twice, within or across the backlog files.
  (c) entries numbered above FIRST_ENFORCED need `Oracle:` or `Premise:`
      (`Premise: n/a — <reason>` is accepted). This proves the line exists,
      not that the premise is right.
  (d) the plan's tier prefix must match the `## Tier N` the entry sits in.
  (m) a malformed item line `N. N. ...` (doubled number).
Legacy violations live in ALLOW, keyed by (rule, number) with one reason each.
ALLOW is shrink-only: a unit test pins its size, a stale row is itself a
violation, and `--allow-base F` (the main-branch copy of this file, used by
gate.sh) fails if ALLOW gained a key.
"""

import argparse
import ast
import glob
import os
import re
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
LLM = os.path.join(ROOT, "LLM")

FIRST_ENFORCED = 493

ITEM = re.compile(r"^(\d+)\. \*\*(DONE|OPEN|WITHDRAWN|CLOSED|SPLIT)?")
MALFORMED = re.compile(r"^(\d+)\. \d+\.")
TIER = re.compile(r"^## Tier (\d+)")
PLAN_NAME = re.compile(r"^(\d+)\.(\d+)-")
PLAN_STATUS = re.compile(r"^Status:\s*(?:\*\*)?(done|proposed|in progress)", re.I)
ORACLE = re.compile(r"\b(?:Oracle|Premise):")
PREMISE_NA = re.compile(r"Premise:\s*n/a(?!\s*[—–-]+\s*\S)")
DONE_PLAN_OK = {"DONE", "CLOSED", "WITHDRAWN"}

# (rule, number) -> one-line reason. Shrink only; see the module docstring.
ALLOW = {
    ("a", 447): "multi-item plan 06.447 (#447-#451) stays `in progress` while #447 itself is DONE; #451 optional",
    ("d", 443): "plan 06.443 predates the tier rule; its entry sits in Tier 1 by design, the prefix is the plan's theme (tests)",
    ("d", 452): "plan 06.452 predates the tier rule; its entry sits in Tier 1 by design, the prefix is the plan's theme (tests)",
}


def parse_backlog(path):
    """Return (entries, violations): entries are dicts with number, status,
    tier, file, line, text; violations are (rule, n, location, message)."""
    entries, found = [], []
    tier = None
    cur = None
    with open(path) as f:
        lines = f.read().splitlines()
    for lineno, text in enumerate(lines, 1):
        t = TIER.match(text)
        if t:
            tier = int(t.group(1))
        if text.startswith("#"):
            cur = None
            continue
        m = MALFORMED.match(text)
        if m:
            found.append(("m", int(m.group(1)), f"{path}:{lineno}",
                          "malformed item line (doubled number)"))
            cur = None
            continue
        m = ITEM.match(text)
        if m:
            cur = {"n": int(m.group(1)), "status": m.group(2) or "UNKNOWN",
                   "tier": tier, "file": path, "line": lineno, "text": text}
            entries.append(cur)
        elif cur is not None and text.strip():
            cur["text"] += "\n" + text
        elif not text.strip():
            cur = None
    return entries, found


def parse_plan(path):
    """Return (tier, number, status word or None, status paragraph, line)."""
    m = PLAN_NAME.match(os.path.basename(path))
    if not m:
        return None
    with open(path) as f:
        lines = f.read().splitlines()
    for i, text in enumerate(lines):
        s = PLAN_STATUS.match(text)
        if s:
            para = [text]
            for nxt in lines[i + 1:]:
                if not nxt.strip():
                    break
                para.append(nxt)
            return (int(m.group(1)), int(m.group(2)), s.group(1).lower(),
                    "\n".join(para), i + 1)
    return (int(m.group(1)), int(m.group(2)), None, "", 1)


def check(backlogs, plans, allow=None):
    """Return a list of violation strings (empty when clean)."""
    allow = ALLOW if allow is None else allow
    found, entries = [], []
    for path in backlogs:
        e, v = parse_backlog(path)
        entries += e
        found += v

    by_num = {}
    for e in entries:
        by_num.setdefault(e["n"], []).append(e)
    for n, es in sorted(by_num.items()):
        if len(es) > 1:
            where = ", ".join(f"{x['file']}:{x['line']}" for x in es)
            found.append(("b", n, f"{es[1]['file']}:{es[1]['line']}",
                          f"number defined {len(es)} times ({where})"))

    for e in entries:
        if e["n"] > FIRST_ENFORCED:
            t = e["text"]
            if not ORACLE.search(t) or PREMISE_NA.search(t):
                found.append(("c", e["n"], f"{e['file']}:{e['line']}",
                              "no `Oracle:`/`Premise:` token (or `Premise: n/a` "
                              "without a reason)"))

    for path in plans:
        p = parse_plan(path)
        if p is None:
            continue
        tier, n, word, para, pline = p
        loc = f"{path}:{pline}"
        es = by_num.get(n)
        if not es:
            found.append(("a", n, loc, "plan has no backlog entry"))
            continue
        e = es[0]
        if word is None:
            found.append(("a", n, loc, "plan has no recognisable `Status:` header"))
        elif e["status"] == "UNKNOWN":
            found.append(("a", n, loc, "backlog entry status is UNKNOWN "
                          f"({e['file']}:{e['line']} has no leading status word)"))
        elif word == "done" and e["status"] not in DONE_PLAN_OK:
            found.append(("a", n, loc, f"plan is done but entry is {e['status']} "
                          f"({e['file']}:{e['line']})"))
        elif word != "done" and e["status"] == "DONE" and "residue" not in para.lower():
            found.append(("a", n, loc, f"plan is {word} but entry is DONE "
                          f"({e['file']}:{e['line']}) and the header names no residues"))
        if e["tier"] is not None and e["tier"] != tier:
            found.append(("d", n, loc, f"plan tier prefix {tier:02d} but entry sits "
                          f"in Tier {e['tier']} ({e['file']}:{e['line']})"))

    out, used = [], set()
    for rule, n, loc, msg in found:
        if (rule, n) in allow:
            used.add((rule, n))
            continue
        out.append(f"{loc}: ({rule}) #{n} {msg}")
    for key in sorted(set(allow) - used):
        out.append(f"doc_status_check.py: stale ALLOW row {key}: no such violation "
                   "any more, delete it")
    return out


def load_allow(source):
    """Return the ALLOW dict literal assigned in python source text."""
    for node in ast.parse(source).body:
        if isinstance(node, ast.Assign) and any(
                isinstance(t, ast.Name) and t.id == "ALLOW" for t in node.targets):
            return ast.literal_eval(node.value)
    return {}


def allow_growth(base_source, current):
    """Return keys in `current` that the base source's ALLOW does not have."""
    base = load_allow(base_source)
    return sorted(set(current) - set(base))


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--backlog", action="append")
    ap.add_argument("--plans-dir", default=LLM)
    ap.add_argument("--allow-base", help="main-branch copy of this file; fail if ALLOW grew")
    args = ap.parse_args(argv)
    backlogs = args.backlog or sorted(glob.glob(os.path.join(LLM, "backlog-tasks*.md")))
    plans = sorted(glob.glob(os.path.join(args.plans_dir, "[0-9]*.opus.md")))
    problems = check(backlogs, plans)
    if args.allow_base:
        with open(args.allow_base) as f:
            for key in allow_growth(f.read(), ALLOW):
                problems.append(f"doc_status_check.py: ALLOW grew by {key}: fix the "
                                "staleness instead of allowing it")
    for p in problems:
        print(p)
    if problems:
        print(f"doc status: {len(problems)} violation(s)")
        return 1
    print("doc status: OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
