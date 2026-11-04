"""#652b oracle: (b) bins + N-form (leg A) hold + H-form (leg B) playground record.

Portage 3.0.83 is the oracle. Run:
  python3 probe652b_oracle.py
(No args: N1, N2, NF, HF, in that order.)

N1/N2 come from S0 `probe649_mutations.py` VERBATIM -- imported, not
re-derived (its import-time runner executes the requested mutations, so
`sys.argv` is set to `["probe652b_oracle.py", "N1", "N2"]` before the
import: the N1/N2 blocks below ARE that import's verbatim output). They
are the controls the plan's Constraints box demands re-run first.

NF (leg-A N-form, NEW here): Try-Tiny-shaped fresh consumer
(`|| ( >=virtual/perl-SLU-1.400.0 dev-perl/Sub-Name )` + `dev-lang/perl`)
over the installed dead SLU virtual (5.42 arm + `~` pin at a version with
NO perl-core ebuild, bed-text family) with the N2-style installed subslot
pin-holder co-scheduled. This is the texlive-cell consumer path in minimal
form (Try-Tiny binds the dead installed SLU virtual). Oracle: the virtual
holds -- no Sub-Name merged, no perl row at all (verbatim below). Bed
ground truth: L0 `l0-20261104T075942Z` real `app-text_texlive.txt` carries
NO `dev-perl/Sub-Name` row (the 5 Sub-Name `[extra]` rows are
portuale-only); real holds everywhere Portage does.

HF (leg-B H-form, NEW here): B9 shape -- N1 closure (valid probe-form
texts, visible KEYWORDS) + installed new-text driver virtual
(`|| ( =perl-5.44* =perl-5.42* ~missing )`, bed Exporter family, the
libreoffice/gimp upgrade driver) + early fresh TH consumer + late
Encode/TH consumer. Oracle (verbatim below): the playground does NOT
reproduce the live hold -- both pins merge with same-version virtual
reinstalls (slot-op set path). The LIVE (b) oracle is the bed, not the
playground: L0 `l0-20261104T075942Z` real `app-office_libreoffice.txt`
merges `perl-core/Encode-3.210.0` (row 33) and carries NO
`perl-core/Test-Harness` row and upgrades neither `virtual/perl-Encode`
nor `virtual/perl-Test-Harness` (other fresh virtuals do merge; gimp
identical: Encode row 22, no TH row, `dev-lang/perl-5.44.0` U). The (b)
bins below are the playground record Task 1 established: under an
identical posture both fall-through arms land in identical bins (the
perl arm `other` via the downgrade guard, the pin arm
`preferred_non_installed` wins) -- the live split comes from evaluation
posture (which parent/time), recorded in the Task-1 report, not from a
distinguishing arm bin.

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
sys.argv = ["probe652b_oracle.py", "N1", "N2"]
sys.path.insert(0, str(HERE.parent / "2026-11-03-649"))
import probe649_mutations as m649  # noqa: E402

from portage.tests.resolver.ResolverPlayground import (  # noqa: E402
    ResolverPlayground,
)

K = {"KEYWORDS": "x86"}


def _rdep(rdep):
    d = {"EAPI": "8", "SLOT": "0", "RDEPEND": rdep}
    d.update(K)
    return d


def _perl(v, s):
    d = dict(m649._perl(v, s))
    d.update(K)
    return d


# Leg-A N-form: Try-Tiny-shaped consumer over the installed dead SLU
# virtual (bed-text family: 5.42 arm + ~pin with no perl-core ebuild).
SLU_OLD = (
    "|| ( =dev-lang/perl-5.42* ~perl-core/Scalar-List-Utils-1.680.100_rc ) "
    "dev-lang/perl:= !<perl-core/Scalar-List-Utils-1.680.100_rc "
    "!>perl-core/Scalar-List-Utils-1.680.100_rc-r999"
)
SLU_OLD_INST = SLU_OLD.replace("dev-lang/perl:=", "dev-lang/perl:0/5.42=")


def _nf():
    m = m649._n1()
    ebuilds = dict(m["ebuilds"])
    for cpv in list(ebuilds):
        ebuilds[cpv] = dict(ebuilds[cpv])
        ebuilds[cpv].update(K)
    installed = dict(m["installed"])
    for cpv in list(installed):
        installed[cpv] = dict(installed[cpv])
        installed[cpv].update(K)
    ebuilds["virtual/perl-Scalar-List-Utils-1.680.100_rc"] = _rdep(SLU_OLD)
    ebuilds["dev-perl/Sub-Name-0.280.0-r1"] = {"EAPI": "8", "SLOT": "0", **K}
    # Fresh driver virtual (no installed instance): 5.44-first-arm || with
    # a ~pin at a version with no ebuild -- forces =perl-5.44*, scheduling
    # the upgrade the holder then skips (libreoffice/texlive driver shape).
    # Multi-arm text (5.42 fallback): mirrors the live new-text virtuals
    # (Exporter/Carp/File-Path carry both arms), so the post-skip graph
    # stays satisfiable the way the live backtrack masking leaves it.
    ebuilds["virtual/perl-s652foo-2.223.0-r1"] = _rdep(
        "|| ( =dev-lang/perl-5.44* =dev-lang/perl-5.42*"
        " ~perl-core/s652foo-2.223.0 ) dev-lang/perl:="
    )
    ebuilds["dev-perl/s652try-1"] = {
        "EAPI": "8",
        "SLOT": "0",
        "RDEPEND": (
            "|| ( >=virtual/perl-Scalar-List-Utils-1.400.0"
            " dev-perl/Sub-Name ) dev-lang/perl virtual/perl-s652foo"
        ),
        **K,
    }
    installed["virtual/perl-Scalar-List-Utils-1.680.100_rc"] = _rdep(SLU_OLD_INST)
    installed["dev-perl/s652pin-1"] = {
        "EAPI": "8",
        "SLOT": "0",
        "RDEPEND": "dev-lang/perl:0/5.42=",
        **K,
    }
    return {
        "note": (
            "N-form (leg A): Try-Tiny-shaped consumer over the installed "
            "dead SLU virtual + subslot pin-holder; the driver schedules "
            "the 5.44 upgrade the walk then skips. Oracle: the virtual "
            "holds (no Sub-Name merged)."
        ),
        "ebuilds": ebuilds,
        "installed": installed,
        "world": ["dev-perl/s652try"],
        "request": ["dev-perl/s652try"],
        "options": {"--backtrack": 30},
    }


# Leg-B H-form: B9 shape -- N1 closure + installed new-text driver +
# early TH consumer + late Encode/TH consumer, NO tree-newer virtuals
# (never selected on live: avoid_update + self-pin protection keep the
# installed ones; omitting them removes only the never-taken slot-op
# upgrade branch while keeping every evaluated bin identical).
EXP_NEW = "|| ( =dev-lang/perl-5.44* =dev-lang/perl-5.42* ~perl-core/Exporter-5.79 ) dev-lang/perl:="
EXP_NEW_INST = EXP_NEW.replace("dev-lang/perl:=", "dev-lang/perl:0/5.42=")


def _hf():
    m = m649._n1()
    ebuilds = dict(m["ebuilds"])
    for cpv in list(ebuilds):
        ebuilds[cpv] = dict(ebuilds[cpv])
        ebuilds[cpv].update(K)
    installed = dict(m["installed"])
    for cpv in list(installed):
        installed[cpv] = dict(installed[cpv])
        installed[cpv].update(K)
    ENC_OLD = m649.ENC_REAL["3.210.0-r2"]
    TH_OLD = m649.TH_REAL["3.500.0-r1"]
    ebuilds["virtual/perl-Encode-3.210.0-r2"] = _rdep(ENC_OLD)
    ebuilds["virtual/perl-Test-Harness-3.500.0-r1"] = _rdep(TH_OLD)
    ebuilds["virtual/perl-Exporter-5.790.0-r1"] = _rdep(EXP_NEW)
    ebuilds["dev-perl/s652early-1"] = {
        "EAPI": "8",
        "SLOT": "0",
        "RDEPEND": "virtual/perl-Test-Harness",
        **K,
    }
    ebuilds["dev-perl/s652cons-1"] = {
        "EAPI": "8",
        "SLOT": "0",
        "RDEPEND": (
            "virtual/perl-Exporter >=virtual/perl-Encode-3.120.0"
            " virtual/perl-Test-Harness"
        ),
        **K,
    }
    installed["virtual/perl-Encode-3.210.0-r2"] = _rdep(ENC_OLD)
    installed["virtual/perl-Test-Harness-3.500.0-r1"] = _rdep(TH_OLD)
    installed["virtual/perl-Exporter-5.790.0-r1"] = _rdep(EXP_NEW_INST)
    return {
        "note": (
            "H-form (leg B): live merging-transient shape (installed "
            "new-text driver pulls 5.44; early TH consumer + late "
            "Encode/TH consumer). Playground record: both pins merge "
            "with same-version virtual reinstalls (diverges from the "
            "bed, which holds TH) -- see the Task-1 report for the "
            "posture analysis. Bed oracle: libreoffice/gimp merge "
            "Encode-3.210.0, hold TH, upgrade neither the Encode nor the TH virtual."
        ),
        "ebuilds": ebuilds,
        "installed": installed,
        "world": ["dev-perl/s652early", "dev-perl/s652cons"],
        "request": ["dev-perl/s652early", "dev-perl/s652cons"],
        "options": {"--backtrack": 30},
    }


for _name, _mk in (("NF", _nf), ("HF", _hf)):
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
