"""#656 leg-(a) adjudication: H0/H1 (NEW for Task 3).

Portage 3.0.83 is the oracle. Run:
  python3 probe656h_oracle.py
(H0, H1, in that order.)

P2 (Task 1) showed N1 + three INSTALLED-ONLY `=5.42*` holders under the
bare request reinstalls cleanly (SUCCESS True, ['dev-lang/perl-5.42.2'],
no skip): the hermetic playground cannot render the bed skip from
installed-only holders. The (a) RED world, however, carries its holders
installed AND tree-present (the `p656hold` ebuilds exist in the fixture
tree), and H-form adds a REQUESTED early TH consumer on top. Portage
reads reverse deps from the installed db (tree-presence is invisible to
it), so H0 is predicted to match P2 -- but the prediction is recorded,
not assumed. The load-bearing question is H1.

H0 (leg-(a) tree-presence control): N1 + three holders that are BOTH
installed and in the tree (`=dev-lang/perl-5.42*`, the `p656hold` twins),
bare `dev-lang/perl` request. Oracle question: does tree-presence change
P2's reinstall (a skip would render leg (a) hermetically; a clean
reinstall keeps leg (a) bed-oracled but confirms the holder-win
posture the (a) gate reproduces)?

H1 (H-form adjudication): H0 + a fresh (tree-only) early TH consumer
(`dev-perl/s652early-1`, bare `virtual/perl-Test-Harness` dep -- the
hermetic `p652early` twin), request `[s652early, perl]` bare. Oracle
question: does the requested early consumer save the transient (U +
Encode, the H-form posture portuale pins) or do the holders win (R,
the (a) posture)? A reinstall here means hermetic-H-form-U is a
tolerate artifact and the (a) gate legitimately flips it (re-pin with
this oracle); U+Encode means the requested puller genuinely saves the
transient in Portage and the (a) gate must spare that shape (STOP:
no grounded narrowing exists -- file (a) as residue per the plan).

Hermeticity: playground legs build repo/vdb/world in a temp dir (no host
tree is read). N1 comes from S0 `probe649_mutations.py` VERBATIM.
"""

import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent

sys.path.insert(0, str(HERE.parent / "2026-11-03-649"))
import probe649_mutations as m649  # noqa: E402

from portage.tests.resolver.ResolverPlayground import (  # noqa: E402
    ResolverPlayground,
)

K = {"KEYWORDS": "x86"}


def _base():
    m = m649._n1()
    ebuilds = {cpv: dict(d) for cpv, d in m["ebuilds"].items()}
    for cpv in list(ebuilds):
        ebuilds[cpv].update(K)
    installed = {cpv: dict(d) for cpv, d in m["installed"].items()}
    for cpv in list(installed):
        installed[cpv].update(K)
    return ebuilds, installed


def _h0():
    ebuilds, installed = _base()
    # Bed-holder twins, installed AND tree-present (the p656hold shape:
    # Portage reads the installed db, so tree-presence is predicted to be
    # invisible -- recorded, not assumed).
    for i in range(1, 4):
        d = {
            "EAPI": "8",
            "SLOT": "0",
            "RDEPEND": "=dev-lang/perl-5.42*",
            **K,
        }
        installed[f"dev-perl/s656hold{i}-1"] = dict(d)
        ebuilds[f"dev-perl/s656hold{i}-1"] = dict(d)
    return {
        "note": (
            "N1 + three installed+tree =5.42* holders under the bare "
            "request. Oracle: skip (renders leg (a)) or clean reinstall "
            "(P2 posture, holder-win confirmed)."
        ),
        "ebuilds": ebuilds,
        "installed": installed,
        "world": ["dev-lang/perl"],
        "request": ["dev-lang/perl"],
        "options": {},
    }


def _h1():
    h0 = _h0()
    ebuilds, installed = h0["ebuilds"], h0["installed"]
    # Requested early TH consumer (the hermetic p652early twin: fresh,
    # tree-only, bare virtual dep).
    ebuilds["dev-perl/s652early-1"] = {
        "EAPI": "8",
        "SLOT": "0",
        "RDEPEND": "virtual/perl-Test-Harness",
        **K,
    }
    return {
        "note": (
            "H0 + requested fresh early TH consumer, bare request. "
            "Oracle: U+Encode (H-form posture stands, requested puller "
            "saves the transient) or R (holders win, H-form-U is a "
            "tolerate artifact)."
        ),
        "ebuilds": ebuilds,
        "installed": installed,
        "world": ["dev-lang/perl"],
        "request": ["dev-perl/s652early", "dev-lang/perl"],
        "options": {},
    }


for _name, _mk in (("H0", _h0), ("H1", _h1)):
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
