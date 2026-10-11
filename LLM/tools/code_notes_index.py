"""Regenerate LLM/code-notes/INDEX.md from the per-file notes.

Every notes file LLM/code-notes/**/*.md except INDEX.md starts with a
line ``# <source path>`` followed by a one-line summary. This script
collects those two lines from each file and rewrites INDEX.md grouped by
crate (the first directory under LLM/code-notes): crates in alphabetical
order, one ``- [<source path>](<notes-relative path>)`` bullet per file
sorted by notes-relative path. A summary line ending with the boilerplate
sentence ``Context moved out of the code comments (#336 Phase 7); the
code keeps only the short why.`` has that sentence dropped.

How to run (from the repo root)::

    python3 LLM/tools/code_notes_index.py          # rewrite LLM/code-notes/INDEX.md
    python3 LLM/tools/code_notes_index.py --check  # exit 1 if INDEX.md is stale
"""

from __future__ import annotations

import pathlib
import sys

TITLE = (
    "# Code notes (#336): context moved out of code comments, "
    "keyed by file and symbol"
)
BOILERPLATE = (
    "Context moved out of the code comments (#336 Phase 7); "
    "the code keeps only the short why."
)

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
NOTES_DIR = REPO_ROOT / "LLM" / "code-notes"
INDEX_PATH = NOTES_DIR / "INDEX.md"


def parse_note(path: pathlib.Path) -> tuple[str, str]:
    """Return (source path, summary) from a notes file's first two lines."""
    lines = path.read_text(encoding="utf-8").splitlines()
    if not lines or not lines[0].startswith("# "):
        raise ValueError(f"{path}: missing '# <source path>' title line")
    if len(lines) < 2:
        raise ValueError(f"{path}: missing one-line summary")
    source = lines[0][2:].strip()
    summary = lines[1].strip()
    if summary.endswith(BOILERPLATE):
        summary = summary[: -len(BOILERPLATE)].rstrip()
    return source, summary


def render_index() -> str:
    """Render the full INDEX.md text from the notes files on disk."""
    notes = sorted(
        (p for p in NOTES_DIR.rglob("*.md") if p.name != "INDEX.md"),
        key=lambda p: p.relative_to(NOTES_DIR).as_posix(),
    )
    by_crate: dict[str, list[tuple[str, str, str]]] = {}
    for path in notes:
        rel = path.relative_to(NOTES_DIR).as_posix()
        crate = rel.split("/", 1)[0]
        source, summary = parse_note(path)
        by_crate.setdefault(crate, []).append((rel, source, summary))
    out = [TITLE, ""]
    for crate in sorted(by_crate):
        out.append(f"## {crate}")
        for rel, source, summary in by_crate[crate]:
            out.append(f"- [{source}]({rel}) \u2014 {summary}")
        out.append("")
    return "\n".join(out)


def main(argv: list[str]) -> int:
    check = "--check" in argv[1:]
    rendered = render_index()
    if check:
        current = INDEX_PATH.read_text(encoding="utf-8")
        if current != rendered:
            print(f"{INDEX_PATH}: stale, re-run code_notes_index.py")
            return 1
        print(f"{INDEX_PATH}: up to date")
        return 0
    INDEX_PATH.write_text(rendered, encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
