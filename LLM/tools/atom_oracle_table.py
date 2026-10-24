#!/usr/bin/env python3
"""Generate the S2 oracle TSV from the installed Portage (do not hand-copy).

Re-runs the ``probe_atoms.py`` battery (86 strings x 9 modes) against the
*installed* Portage (3.0.83, native C parser on by default) and writes
``crates/portage-dep/src/testdata/atoms-<version>.tsv`` with columns
``atom \\t mode \\t verdict \\t fields-or-message``.

Portage is still the oracle at test-generation time (AGENTS.md rule 4):
every expected value below comes out of ``portage.dep.Atom`` live, never
out of a transcript. The atom list itself is read from
``LLM/evidence/2026-10-11-341-oracle/probe_atoms.py`` so the battery cannot
drift from the oracle; the verdicts/fields are re-probed.

Verdicts: ``OK`` / ``ERR``. Rows whose ``wild`` mode parses through the
wildcard branch (``extended_syntax``) get ``S3OK``: S2 ports only the
strict grammar, so those rows stay skipped until S3 fills the wildcard
branch. ``OK`` detail renders the same field list the Rust test's
``render_fields`` prints
(``cp op ver slot sub sop repo bid use blocker ext``); ``ERR`` detail is
``str(InvalidAtom)`` (a bare atom string means the generic
``InvalidAtom(self._string)``, oracle Q1).
"""

import os
import re
import sys

from portage.dep import Atom
from portage.exception import InvalidAtom

import portage

EXPECTED_PORTAGE_VERSION = "3.0.83"

ORACLE_DIR = os.path.join(
    os.path.dirname(os.path.abspath(__file__)),
    "..",
    "evidence",
    "2026-10-11-341-oracle",
)
OUT = os.path.join(
    os.path.dirname(os.path.abspath(__file__)),
    "..",
    "..",
    "crates",
    "portage-dep",
    "src",
    "testdata",
    f"atoms-{EXPECTED_PORTAGE_VERSION}.tsv",
)

MODES = [
    ("None", {}),
    ("e5", {"eapi": "5"}),
    ("e7", {"eapi": "7"}),
    ("e8", {"eapi": "8"}),
    ("e9", {"eapi": "9"}),
    ("eX", {"eapi": "10"}),
    ("e8+bid", {"eapi": "8", "allow_build_id": True}),
    ("None-repo", {"allow_repo": False, "allow_build_id": False}),
    ("wild", {"allow_wildcard": True, "allow_repo": True}),
]


def read_cases():
    """The atom list, parsed out of probe_atoms.py (not duplicated here)."""
    src = open(os.path.join(ORACLE_DIR, "probe_atoms.py")).read()
    m = re.search(r'cases = """(.*?)"""', src, re.S)
    assert m, "probe_atoms.py battery block not found"
    return m.group(1).split("\n")


def render(atom):
    blocker = ""
    if atom.blocker:
        blocker = "!!" if atom.blocker.overlap.forbid else "!"
    fields = [
        "cp=%s" % atom.cp,
        "op=%s" % (atom.operator or ""),
        "ver=%s" % (atom.version or ""),
        "slot=%s" % (atom.slot or ""),
        "sub=%s" % (atom.sub_slot or ""),
        "sop=%s" % (atom.slot_operator or ""),
        "repo=%s" % (atom.repo or ""),
        "bid=%s" % ("" if atom.build_id is None else atom.build_id),
        "use=%s" % (",".join(atom.use.tokens) if atom.use else ""),
        "blocker=%s" % blocker,
        "ext=%s" % (1 if atom.extended_syntax else 0),
    ]
    return " ".join(fields)


def main():
    if portage.VERSION != EXPECTED_PORTAGE_VERSION:
        sys.exit(
            f"host Portage is {portage.VERSION}, tool expects "
            f"{EXPECTED_PORTAGE_VERSION}: refusing to write a misnamed table"
        )
    cases = read_cases()
    assert len(cases) == 86, len(cases)
    n_s3 = 0
    with open(OUT, "w") as out:
        out.write("atom\tmode\tverdict\tdetail\n")
        for s in cases:
            assert "\t" not in s and "\n" not in s, repr(s)
            for name, kw in MODES:
                try:
                    atom = Atom(s, **kw)
                except InvalidAtom as e:
                    out.write("%s\t%s\tERR\t%s\n" % (s, name, str(e)))
                except Exception as e:  # pragma: no cover - oracle drift alarm
                    sys.exit("UNEXPECTED %s [%s]: %s: %s" % (s, name, type(e).__name__, e))
                else:
                    if name == "wild" and atom.extended_syntax:
                        verdict = "S3OK"
                        n_s3 += 1
                    else:
                        verdict = "OK"
                    out.write("%s\t%s\t%s\t%s\n" % (s, name, verdict, render(atom)))
    print("wrote %s (%d atoms x %d modes, %d S3 rows)" % (OUT, len(cases), len(MODES), n_s3))


if __name__ == "__main__":
    main()
