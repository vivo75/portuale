"""#652 oracle: controls + the two runs the cell traces call for.

Portage 3.0.83 is the oracle. Run:
  python3 probe652_oracle.py
(No args: N1, N2, T1, NFORM, in that order.)

N1/N2 come from S0 `probe649_mutations.py` VERBATIM -- imported, not
re-derived (its import-time runner executes the requested mutations, so
`sys.argv` is set to `["probe652_oracle.py", "N1", "N2"]` before the
import: the N1/N2 blocks below ARE that import's verbatim output). They
are the controls the plan's Constraints box demands re-run first.

T1 (M5-TH analogue, the libreoffice split in minimal form): N1's dicts
verbatim, but the request pins the upgrade (`=dev-lang/perl-5.44.0`,
M5-style) so the transient perl-5.44.0 PDEPEND -- the libreoffice cell's
only live `virtual/perl-Test-Harness` consumer -- is evaluated with the
upgrade co-scheduled. M5 proves Portage merges `perl-core/Encode-3.210.0`
on this posture; T1 asks whether `perl-core/Test-Harness-3.500.0` follows
(the bed cell says it does not: real merges Encode, holds TH).

NFORM (the entry's three-version sketch rendered with real versions):
installed perl-5.40.2 + tree N perl-5.42.2 + tree D perl-5.44.0, the
N2-style installed subslot pin-holder (`dev-lang/perl:0/5.42=`) masking
only D, so post-skip N is selected (upgrade 5.40.2 -> 5.42.2) while every
recorded demoter (D) ends masked. Asks the Step-2 question: does
Portage's re-fire merge the fall-through here (corner exists in
principle: portuale's void-iff-ALL-recorded-demoters-masked would hold)?

Expected verbatim (locked 2026-11-04, Portage 3.0.83):
- N1: SUCCESS True, MERGELIST ['dev-lang/perl-5.42.2']
- N2: SUCCESS True, MERGELIST ['dev-lang/perl-5.42.2']
- T1: recorded below (the run IS the oracle for the follow-up S0)
- NFORM: recorded below (Step-2 verdict on the sketch in principle)

Hermeticity: playground legs build repo/vdb/world in a temp dir (no host
tree is read).
"""

import os
import pwd
import grp
import shutil
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent

# N1/N2 verbatim: the import below executes these two mutations itself.
sys.argv = ["probe652_oracle.py", "N1", "N2"]
sys.path.insert(0, str(HERE.parent / "2026-11-03-649"))
import probe649_mutations as m649  # noqa: E402

from portage.tests.resolver.ResolverPlayground import (  # noqa: E402
    ResolverPlayground,
)


def _t1():
    m = m649._n1()
    m["world"] = ["dev-lang/perl"]
    m["request"] = ["=dev-lang/perl-5.44.0"]
    m["note"] = (
        "N1 dicts verbatim, M5-style pinned-upgrade request: the transient "
        "perl-5.44.0 PDEPEND (the libreoffice cell's only live "
        "virtual/perl-Test-Harness consumer) evaluated with the upgrade "
        "co-scheduled -- M5 proves Encode-3.210.0 merges here; does "
        "Test-Harness-3.500.0 follow?"
    )
    return m


def _nform():
    m = m649._n1()
    m["ebuilds"]["dev-lang/perl-5.40.2"] = m649._perl("5.40.2", "5.40")
    m["ebuilds"]["dev-perl/s652pin-1"] = {
        "EAPI": "8",
        "SLOT": "0",
        "RDEPEND": "dev-lang/perl:0/5.42=",
    }
    installed = dict(m["installed"])
    installed["dev-lang/perl-5.40.2"] = m649._perl("5.40.2", "5.40")
    del installed["dev-lang/perl-5.42.2"]
    installed["dev-perl/s652pin-1"] = {
        "EAPI": "8",
        "SLOT": "0",
        "RDEPEND": "dev-lang/perl:0/5.42=",
    }
    m["installed"] = installed
    m["world"] = ["dev-lang/perl"]
    m["request"] = ["dev-lang/perl"]
    m["note"] = (
        "entry sketch with real versions: installed 5.40.2, tree N 5.42.2, "
        "tree D 5.44.0, installed subslot pin-holder masking only D -- "
        "post-skip N is selected (5.40.2 -> 5.42.2 upgrade) while every "
        "recorded demoter (D) ends masked"
    )
    return m


for _name, _mk in (("T1", _t1), ("NFORM", _nform)):
    print("=" * 70)
    print("MUT", _name, "-", _mk()["note"])
    pg = ResolverPlayground(
        ebuilds=_mk()["ebuilds"],
        installed=_mk()["installed"],
        world=_mk()["world"],
    )
    try:
        result = pg.run(_mk()["request"], options=_mk()["options"])
        print("SUCCESS", result.success)
        print("MERGELIST", result.mergelist)
        print("UNSAT", getattr(result, "unsatisfied_deps", None))
    finally:
        pg.cleanup()
