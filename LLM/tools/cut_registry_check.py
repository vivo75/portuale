#!/usr/bin/env python3
"""Check the scope-cut registry (LLM/scope-cuts.md) against the backlog and
against the deferred-cut markers in LLM/what-this-proves.md.

Usage: cut_registry_check.py [--registry F] [--backlog F ...] [--proof F]
(--backlog repeats; default is every LLM/backlog-tasks*.md)
Exit 0 when clean, 1 with one line per violation on stdout otherwise.

Registry grammar (one entry per line, `- ` bullet at column 0):
  - CUT-<n>: <one line> → #<backlog>             open; follow-up is backlog #<n>
  - CUT-<n>: <one line> → closed #<backlog>      closed; follow-up shipped
  - CUT-<n>: <one line> → none (accepted): why   no follow-up; reason required

Rules:
  (a) an open entry (`→ #N`) whose backlog entry N starts DONE: the cut is
      closed, so the line must say `→ closed #N`.
  (b) a deferred-cut marker in what-this-proves.md (MARKERS below) on line L
      needs an acknowledgment: a CUT-n within 3 lines of L in the proof, or a
      registry line citing `what-this-proves.md:L`. The proof file is never
      written, so the registry is where orphans are acknowledged.
  (c) an entry naming a backlog number that does not exist in backlog-tasks.md.
  Malformed entries (no `→` target) and duplicate CUT numbers are reported too.
"""

import argparse
import glob
import os
import re
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
DEFAULT_REGISTRY = os.path.join(ROOT, "LLM", "scope-cuts.md")
DEFAULT_BACKLOGS = sorted(glob.glob(os.path.join(ROOT, "LLM", "backlog-tasks*.md")))
DEFAULT_PROOF = os.path.join(ROOT, "LLM", "what-this-proves.md")

REG_LINE = re.compile(r"^- CUT-(\d+):\s*(.*)$")
TARGET = re.compile(r"→ (none \(accepted\)|closed #(\d+)|#(\d+))")
BACKLOG_ENTRY = re.compile(r"^(\d+)\. \*\*(?:([A-Z]+)\b)?")
CUT_REF = re.compile(r"CUT-\d+")
# "KNOWN, DOCUMENTED SCOPE CUT (still open)" contains "still open)", so the
# shorter pattern covers it; both spellings are kept for the reader.
MARKERS = [re.compile(p) for p in (
    r"Still deferred",
    r"remains open",
    r"still open\)",
    r"KNOWN, DOCUMENTED SCOPE CUT \(still open\)",
)]


def parse_registry(path):
    """Return ({n: (line, open_backlog_or_None)}, problems)."""
    entries, problems = {}, []
    with open(path) as f:
        lines = f.read().splitlines()
    for lineno, text in enumerate(lines, 1):
        if not text.startswith("- CUT-"):
            continue
        m = REG_LINE.match(text)
        if not m:
            problems.append(f"{path}:{lineno}: malformed CUT entry: {text!r}")
            continue
        n = int(m.group(1))
        if n in entries:
            problems.append(f"{path}:{lineno}: CUT-{n} duplicate (first at line {entries[n][0]})")
            continue
        t = TARGET.search(m.group(2))
        if not t:
            problems.append(f"{path}:{lineno}: CUT-{n} has no → target "
                            "(use → #N, → closed #N or → none (accepted))")
            continue
        if t.group(2):
            backlog, kind = int(t.group(2)), "closed"
        elif t.group(3):
            backlog, kind = int(t.group(3)), "open"
        else:
            backlog, kind = None, "accepted"
        entries[n] = (lineno, backlog, kind)
    return entries, problems


def parse_backlog(paths):
    """Return {number: status word or ""} for each `N. **` entry (first wins).
    `paths` is one file or a list (the backlog is split over several files).

    An entry without a leading status word (e.g. `5. **brush PRs: ...`) exists
    but has no status, so it is never treated as DONE."""
    status = {}
    for path in [paths] if isinstance(paths, str) else paths:
        with open(path) as f:
            for text in f:
                m = BACKLOG_ENTRY.match(text)
                if m:
                    status.setdefault(int(m.group(1)), m.group(2) or "")
    return status


def check(registry, backlog, proof):
    """Return a list of violation strings (empty when clean)."""
    entries, problems = parse_registry(registry)
    status = parse_backlog(backlog)

    for n, (lineno, num, kind) in sorted(entries.items()):
        if num is None:
            continue
        if num not in status:
            problems.append(f"{registry}:{lineno}: CUT-{n} points at backlog #{num}, "
                            "which is not in backlog-tasks*.md")
        elif kind == "open" and status[num] == "DONE":
            problems.append(f"{registry}:{lineno}: CUT-{n} is open but backlog #{num} "
                            "is DONE (if the follow-up shipped, write → closed #"
                            f"{num})")

    with open(proof) as f:
        proof_lines = f.read().splitlines()
    name = os.path.basename(proof)
    with open(registry) as f:
        cited = {int(m.group(1)) for m in
                 re.finditer(re.escape(name) + r":(\d+)\b", f.read())}

    for lineno, text in enumerate(proof_lines, 1):
        hit = next((m for m in (p.search(text) for p in MARKERS) if m), None)
        if hit is None:
            continue
        window = proof_lines[max(0, lineno - 4):lineno + 3]
        if lineno in cited or any(CUT_REF.search(w) for w in window):
            continue
        problems.append(f"{proof}:{lineno}: deferred-cut marker {hit.group(0)!r} has no "
                        f"CUT-n acknowledgment (add a registry line citing {name}:{lineno})")
    return problems


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--registry", default=DEFAULT_REGISTRY)
    ap.add_argument("--backlog", action="append")
    ap.add_argument("--proof", default=DEFAULT_PROOF)
    args = ap.parse_args(argv)
    problems = check(args.registry, args.backlog or DEFAULT_BACKLOGS, args.proof)
    for p in problems:
        print(p)
    if problems:
        print(f"cut registry: {len(problems)} violation(s)")
        return 1
    print("cut registry: OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
