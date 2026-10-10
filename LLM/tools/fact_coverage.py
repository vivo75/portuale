#!/usr/bin/env python3
"""Check that shortening comments lost no fact (#336 Phase 7).

Usage: fact_coverage.py BASE [--head REV] [-v] FILE...

BASE is the revision before the comment pass (e.g. HEAD); the after side
is the work tree, or REV with --head. For each comment line of FILE at
BASE that is gone after, the concrete facts on it -- backticked spans,
*.py/*.rs/*.md/*.sh/*.conf names (with :line), #NNN numbers, quoted
strings, dates, versions -- must appear in the file after or in its
notes file, LLM/code-notes/<crate>/<path without src/ and .rs>.md
(case- and backtick-insensitive). Prints each miss with the old line
(-v) and exits 1 if there is any.
"""
import os, re, subprocess, sys

FACT = re.compile(r"`[^`\n]{2,80}`|\b[\w/.-]+\.(?:py|rs|md|sh|conf)\b(?::\d+(?:-\d+)?)?|#\d{2,}\b"
                  r"|\"[^\"\n]{3,80}\"|\b20\d\d-\d\d-\d\d\b|\b\d+\.\d+\.\d+(?:\.\d+)?\b")


def git(*a):
    return subprocess.run(["git", *a], capture_output=True, text=True).stdout


def norm(s):
    # "real" (meaning Portage) became "Portage" in the pass; match either
    s = re.sub(r"\s+", " ", s.replace("`", "")).strip().lower()
    return re.sub(r"\breal\b", "portage", s)


def comments(text):
    return [l.strip() for l in text.split("\n") if l.lstrip().startswith("//")]


def notes_path(f):
    _, crate, rest = f.split("/", 2)
    rest = rest[4:] if rest.startswith("src/") else rest
    return f"LLM/code-notes/{crate}/{rest[:-3]}.md"


def misses(base, head, f):
    old = git("show", f"{base}:{f}")
    new = git("show", f"{head}:{f}") if head else open(f).read()
    note = notes_path(f)
    notes = git("show", f"{head}:{note}") if head else (open(note).read() if os.path.exists(note) else "")
    hay = norm(new + "\n" + notes)
    kept = set(comments(new))
    out, seen = [], set()
    for line in comments(old):
        if line in kept:
            continue
        for m in FACT.findall(line):
            fact = m.strip("`\"")
            if len(fact) >= 3 and fact not in seen and norm(fact) not in hay:
                seen.add(fact)
                out.append((fact, line))
    return out


def main():
    a = sys.argv[1:]
    verbose = "-v" in a
    a = [x for x in a if x != "-v"]
    head = None
    if "--head" in a:
        i = a.index("--head")
        head = a[i + 1]
        del a[i:i + 2]
    base, files = a[0], a[1:]
    total = 0
    for f in files:
        ms = misses(base, head, f)
        total += len(ms)
        if ms:
            print(f"{f}: {len(ms)} facts not found (notes: {notes_path(f)})")
            if verbose:
                for fact, line in ms:
                    print(f"    {fact!r}  <-  {line}")
    print(f"{total} facts not found")
    sys.exit(1 if total else 0)


if __name__ == "__main__":
    main()
