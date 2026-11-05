"""#656 oracle: controls + the two Task-1 decision legs.

Portage 3.0.83 is the oracle. Run:
  python3 probe656_oracle.py
(No args: N1, N2, P1, P2, in that order.)

N1/N2 come from S0 `probe649_mutations.py` VERBATIM -- imported, not
re-derived (its import-time runner executes the requested mutations, so
`sys.argv` is set to `["probe656_oracle.py", "N1", "N2"]` before the
import: the N1/N2 blocks below ARE that import's verbatim output). They
are the controls the plan's Constraints box demands re-run first, and
the tripwire the task brief keeps green.

P1 (leg (b1), NEW here): the M5 posture with the TH virtual present --
N1 closure (installed perl-5.42.2 + installed 5.42-arm Encode/TH
virtuals, tree 5.44.0 with the textually-identical PDEPEND) + a fresh
Encode consumer (`dev-perl/s656zip-1`, `>=virtual/perl-Encode-2.0.0`,
the M5 Encode-Locale / bed Archive-Zip twin) + a PINNED transient
request. The transient is merging (replacing) while the only TH puller
is that same transient. Oracle question: does the replacing parent's
`||` judgment keep the installed TH arm (Portage `dep_check.py:480`
/`:531` `if not replacing` bypass) while the fresh leg merges Encode?
Bed ground truth: L0 `l0-20261104T163402Z` real
`app-office_libreoffice.txt` / `media-gfx_gimp.txt` merge
`perl-core/Encode-3.210.0` and carry NO `perl-core/Test-Harness` row.

P2 (leg (a) render attempt, NEW here): N1 + installed `=5.42*`
version-pin holders (the bed cell's holder shape: real
`dev-lang_perl.txt` carries ~48 installed `=dev-lang/perl-5.42*`
requirers where N2 carries one `:0/5.42=` subslot pin) under the bare
`dev-lang/perl` request. Oracle question: does Portage open the
conflict and skip the transient (R+skip, the bed posture) on a
hermetic holder set, or does the bare shape reinstall without a skip
(N1/N2 posture)? A skip here renders leg (a) hermetically; a reinstall
without a skip means the hermetic playground cannot render (a) and the
bed cell stays the (a) oracle per plan section 0.

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
sys.argv = ["probe656_oracle.py", "N1", "N2"]
sys.path.insert(0, str(HERE.parent / "2026-11-03-649"))
import probe649_mutations as m649  # noqa: E402

from portage.tests.resolver.ResolverPlayground import (  # noqa: E402
    ResolverPlayground,
)

K = {"KEYWORDS": "x86"}


def _p1():
    m = m649._n1()
    ebuilds = {cpv: dict(d) for cpv, d in m["ebuilds"].items()}
    for cpv in list(ebuilds):
        ebuilds[cpv].update(K)
    installed = {cpv: dict(d) for cpv, d in m["installed"].items()}
    for cpv in list(installed):
        installed[cpv].update(K)
    ebuilds["dev-perl/s656zip-1"] = {
        "EAPI": "8",
        "SLOT": "0",
        "RDEPEND": ">=virtual/perl-Encode-2.0.0",
        **K,
    }
    return {
        "note": (
            "M5 posture with the TH virtual present: fresh Encode "
            "consumer + pinned transient; the transient is the only TH "
            "puller. Oracle: replacing parent keeps TH, fresh leg merges "
            "Encode (bed libreoffice/gimp posture)."
        ),
        "ebuilds": ebuilds,
        "installed": installed,
        "world": ["dev-perl/s656zip", "dev-lang/perl"],
        "request": ["dev-perl/s656zip", "=dev-lang/perl-5.44.0"],
        "options": {},
    }


def _p2():
    m = m649._n1()
    ebuilds = {cpv: dict(d) for cpv, d in m["ebuilds"].items()}
    for cpv in list(ebuilds):
        ebuilds[cpv].update(K)
    installed = {cpv: dict(d) for cpv, d in m["installed"].items()}
    for cpv in list(installed):
        installed[cpv].update(K)
    # Bed-holder twins: installed-only version-pin requirers of the 5.42
    # slot (never merged -- background pressure only).
    for i in range(1, 4):
        installed[f"dev-perl/s656hold{i}-1"] = {
            "EAPI": "8",
            "SLOT": "0",
            "RDEPEND": "=dev-lang/perl-5.42*",
            **K,
        }
    return {
        "note": (
            "N1 + three installed =5.42* version-pin holders under the "
            "bare request. Oracle: skip (bed R+skip posture, renders leg "
            "(a)) or clean reinstall (N1/N2 posture, leg (a) stays "
            "bed-oracled)."
        ),
        "ebuilds": ebuilds,
        "installed": installed,
        "world": ["dev-lang/perl"],
        "request": ["dev-lang/perl"],
        "options": {},
    }


for _name, _mk in (("P1", _p1), ("P2", _p2)):
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
