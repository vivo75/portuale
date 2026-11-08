#!/usr/bin/env python3
"""Check the skills under LLM/skills/.

Usage: skills_check.py [--skills DIR] [--claude-skills DIR]
Exit 0 when clean, 1 with one line per violation otherwise.

Rules, for every LLM/skills/<name>/SKILL.md:
  (a) the front matter has `name:` equal to the directory name and a
      `description:` that starts with "Use when" or "Use before";
  (b) every backticked path that starts with LLM/, docs/, crates/, 3rdparty/ or
      ../pmtest/ exists (paths with `*`, `<`, `…` or `{` are patterns and are
      skipped; a `:line` suffix is ignored; `../pmtest/` is skipped when the
      sibling checkout is absent);
  (c) a cited path that is gone and listed in LLM/ARCHIVE.manifest.tsv is reported
      as archived, not just missing: a skill an agent cannot follow is worse than
      none;
  (d) .claude/skills/<name> is a symlink to the skill's directory;
  (e) SKILL.md is at most MAX_BYTES, because a skill is read whole.
"""

import argparse
import os
import re
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
MAX_BYTES = 10_000
PATH = re.compile(r"`((?:LLM|docs|crates|3rdparty|\.\./pmtest)/[^`\s]+)`")
PATTERN = re.compile(r"[*<…{]")
LINE_SUFFIX = re.compile(r":\d+(?:[-,]\d+)*$")


def archived_paths(root):
    manifest = os.path.join(root, "LLM", "ARCHIVE.manifest.tsv")
    if not os.path.exists(manifest):
        return set()
    with open(manifest) as f:
        return {line.split("\t", 1)[0] for line in f.read().splitlines()[1:]}


def front_matter(text):
    m = re.match(r"---\n(.*?)\n---\n", text, re.S)
    return dict(re.findall(r"^(\w+):\s*(.*)$", m.group(1), re.M)) if m else {}


def check(skills_dir, claude_dir, root=ROOT):
    problems, archived = [], archived_paths(root)
    have_pmtest = os.path.isdir(os.path.join(root, "..", "pmtest"))
    for name in sorted(os.listdir(skills_dir)):
        path = os.path.join(skills_dir, name, "SKILL.md")
        if not os.path.exists(path):
            continue
        with open(path) as f:
            text = f.read()
        fm = front_matter(text)
        if fm.get("name") != name:
            problems.append(f"{path}: front matter name {fm.get('name')!r} != directory {name!r}")
        if not re.match(r"Use (when|before)\b", fm.get("description", "")):
            problems.append(f"{path}: description must start with 'Use when' or 'Use before'")
        if len(text.encode()) > MAX_BYTES:
            problems.append(f"{path}: {len(text.encode())} bytes, over {MAX_BYTES}")
        for lineno, line in enumerate(text.splitlines(), 1):
            for cited in PATH.findall(line):
                rel = LINE_SUFFIX.sub("", cited.rstrip(".,;)"))
                if PATTERN.search(rel) or (rel.startswith("../pmtest/") and not have_pmtest):
                    continue
                if os.path.exists(os.path.join(root, rel)):
                    continue
                kind = "archived" if rel in archived else "missing"
                problems.append(f"{path}:{lineno}: cites {kind} {rel}")
        link = os.path.join(claude_dir, name)
        want = os.path.realpath(os.path.join(skills_dir, name))
        if not os.path.islink(link) or os.path.realpath(link) != want:
            problems.append(f"{link}: not a symlink to {os.path.relpath(want, root)}")
    return problems


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--skills", default=os.path.join(ROOT, "LLM", "skills"))
    ap.add_argument("--claude-skills", default=os.path.join(ROOT, ".claude", "skills"))
    args = ap.parse_args(argv)
    problems = check(args.skills, args.claude_skills)
    for p in problems:
        print(p)
    print(f"skills check: {'OK' if not problems else f'{len(problems)} violation(s)'}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
