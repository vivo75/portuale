"""#647 oracle lock (S0 #645 follow-up). Portage 3.0.83 is the oracle.

Runs the S0 probe645_mutations.py M6b/M5/M4/M5b definitions VERBATIM (copied,
not re-derived — every ebuild dict, RDEPEND string and request below is
byte-identical to the S0 file; only this docstring and the runner tail
differ: the tail runs exactly the four lock mutations instead of taking
argv). Run: python3 probe647_oracle.py

Recorded 2026-11-03, Portage 3.0.83, bed tree @ e07309ba6:
- M6b (leg A, atom request): SUCCESS True,
  MERGELIST ['dev-perl/Sub-Name-0.280.0-r1', 'dev-lang/perl-5.44.0',
  'dev-perl/Try-Tiny-0.320.0'] — virtual stays installed at 1.680.100_rc.
- M5 (leg B, atom request): SUCCESS True,
  MERGELIST ['dev-lang/perl-5.44.0', 'perl-core/Encode-3.210.0',
  'dev-perl/Encode-Locale-1.50.0-r1'] — virtual stays installed at 3.210.0-r2.
- M4 (not the cell, @world --update --deep): SUCCESS True,
  MERGELIST ['dev-lang/perl-5.44.0',
  'virtual/perl-Scalar-List-Utils-1.700.0', 'dev-perl/Try-Tiny-0.320.0'] —
  the virtual UPGRADES to 1.700.0 (scope boundary, do not "fix").
- M5b (not the cell, @world --update --deep): SUCCESS True,
  MERGELIST ['dev-lang/perl-5.44.0', 'virtual/perl-Encode-3.240.0'] —
  the virtual UPGRADES to 3.240.0 (scope boundary, do not "fix").

Axes (vs p03 = probe572_p03.py, p02 = probe572.py):
- M0 baseline p03 (Try-Tiny `|| (>=virtual/... dev-perl/Sub-Name)`, single
  pure-`||` virtual, @world --update --deep). Expected (S0 #643):
  Sub-Name + perl-5.44.0 + Try-Tiny.
- M1 USE-conditional wrapping: Try-Tiny RDEPEND wrapped in `!minimal? (...)`
  (IUSE=minimal, default off so the conditional is ACTIVE), verbatim real
  nesting depth (RDEPEND -> `!minimal?` -> `||`).
- M2 multi-version virtual, purity held: three extra tree virtuals with the
  SAME doubly-dead pure-`||` shape as p03 (the shared `~` pin is deliberate:
  the ONLY delta from M0 is the tree version count 1 -> 4).
- M3 conjunction extras, count held: the single virtual carries the VERBATIM
  real 1.680.100_rc RDEPEND (`|| (...) dev-lang/perl:= !<... !>...`).
- M4 full real leg-A shape: M1 + the four tree virtuals with VERBATIM real
  RDEPENDs (1.630.0-r1, 1.680.0 with NO `||` at all, 1.680.100_rc, 1.700.0
  naming `=perl-5.44*`). Try-Tiny keeps M1's wrapped `||` only: the real
  Carp/Exporter/Except conjuncts are omitted as non-interacting (plain
  conjuncts beside the group; Portage appends them as-is).
- M5 real leg-B shape: Encode-Locale (plain `>=virtual/perl-Encode-2.0.0`,
  NOT installed) + explicit `=perl-5.44.0` atom request; both tree Encode
  virtuals with VERBATIM real RDEPENDs (3.210.0-r2, 3.240.0 naming
  `=perl-5.44*`); tree has Encode-3.190.0 + 3.210.0 (no 3.240.0, as in the
  real tree).
- M5b M5 ebuilds under the p02 request (@world --update --deep, leaf
  installed) for continuity with the S0 #572 pin.
- M6a M0 ebuilds under an atom request (no --update/--deep).
- M6b M4 ebuilds under an atom request (closest to the real cell's leg A).
"""
import grp
import os
import pwd
import shutil
import sys
import tempfile
from pathlib import Path

LIB = str(Path(__file__).resolve().parents[3] / "3rdparty" / "portage" / "lib")
sys.path.insert(0, LIB)

import portage  # noqa: E402
import portage.const  # noqa: E402
import portage.output  # noqa: E402
import portage.util  # noqa: E402

os.environ["PORTAGE_USERNAME"] = pwd.getpwuid(os.getuid()).pw_name
os.environ["PORTAGE_GRPNAME"] = grp.getgrgid(os.getgid()).gr_name
path = [p for p in os.environ.get("PATH", "").split(":") if p]
path.insert(0, portage.const.PORTAGE_BIN_PATH)
os.environ["PATH"] = ":".join(path)

portage._internal_caller = True
portage._disable_legacy_globals()
if portage.util.no_color(os.environ):
    portage.output.nocolor()

gpg_path = tempfile.mkdtemp(prefix="gpg_")
shutil.copytree(os.path.join(LIB, "portage", "tests", ".gnupg"), gpg_path, dirs_exist_ok=True)
os.chmod(gpg_path, 0o700)
os.environ["PORTAGE_GNUPGHOME"] = gpg_path

from portage.tests.resolver.ResolverPlayground import ResolverPlayground  # noqa: E402

PERL = {
    "dev-lang/perl-5.42.2": {"EAPI": "8", "SLOT": "0/5.42"},
    "dev-lang/perl-5.44.0": {"EAPI": "8", "SLOT": "0/5.44"},
}
SLU_TREE = {
    "perl-core/Scalar-List-Utils-1.630.0": {"EAPI": "8", "SLOT": "0"},
    "perl-core/Scalar-List-Utils-1.680.0": {"EAPI": "8", "SLOT": "0"},
}
SUBNAME = {"dev-perl/Sub-Name-0.280.0-r1": {"EAPI": "8", "SLOT": "0"}}

TRY_PURE_RDEPEND = "|| ( >=virtual/perl-Scalar-List-Utils-1.400.0 dev-perl/Sub-Name )"
# Verbatim real nesting: RDEPEND -> !minimal? (...) -> || (Task 1 report #5).
TRY_WRAP_RDEPEND = (
    "!minimal? ( || ( >=virtual/perl-Scalar-List-Utils-1.400.0 dev-perl/Sub-Name ) )"
)

SLU_PURE_RDEPEND = (
    "|| ( =dev-lang/perl-5.42* ~perl-core/Scalar-List-Utils-1.680.100_rc )"
)
# Doubly-dead pure-|| for the M2 extra versions: `=perl-5.40*` matches
# nothing in-tree, the `~` pin matches nothing in-tree. Shared pin text is
# deliberate (M2's only delta from M0 is the version count).
SLU_PURE_DEAD = "|| ( =dev-lang/perl-5.40* ~perl-core/Scalar-List-Utils-1.680.100_rc )"

# Verbatim real virtual RDEPENDs (bed tree @ e07309ba6, Task 1 report #6).
# ${PN#perl-} expanded by hand; `dev-lang/perl:=` + blockers included.
SLU_REAL = {
    "1.630.0-r1": "|| ( =dev-lang/perl-5.40* =dev-lang/perl-5.38*"
    " ~perl-core/Scalar-List-Utils-1.630.0 ) dev-lang/perl:="
    " !<perl-core/Scalar-List-Utils-1.630.0"
    " !>perl-core/Scalar-List-Utils-1.630.0-r999",
    "1.680.0": "~perl-core/Scalar-List-Utils-1.680.0 dev-lang/perl:="
    " !<perl-core/Scalar-List-Utils-1.680.0"
    " !>perl-core/Scalar-List-Utils-1.680.0-r999",
    "1.680.100_rc": "|| ( =dev-lang/perl-5.42*"
    " ~perl-core/Scalar-List-Utils-1.680.100_rc ) dev-lang/perl:="
    " !<perl-core/Scalar-List-Utils-1.680.100_rc"
    " !>perl-core/Scalar-List-Utils-1.680.100_rc-r999",
    "1.700.0": "|| ( =dev-lang/perl-5.44* ~perl-core/Scalar-List-Utils-1.700.0 )"
    " dev-lang/perl:= !<perl-core/Scalar-List-Utils-1.700.0"
    " !>perl-core/Scalar-List-Utils-1.700.0-r999",
}
ENC_REAL = {
    "3.210.0-r2": "|| ( =dev-lang/perl-5.42* =dev-lang/perl-5.40*"
    " ~perl-core/Encode-3.210.0 ) dev-lang/perl:= !<perl-core/Encode-3.210.0"
    " !>perl-core/Encode-3.210.0-r999",
    "3.240.0": "|| ( =dev-lang/perl-5.44* ~perl-core/Encode-3.240.0 )"
    " dev-lang/perl:= !<perl-core/Encode-3.240.0 !>perl-core/Encode-3.240.0-r999",
}


def _try_tiny(rdepend, iuse=""):
    d = {"EAPI": "8", "SLOT": "0", "RDEPEND": rdepend}
    if iuse:
        d["IUSE"] = iuse
    return d


def _virt(cat, ver, rdepend):
    return {"EAPI": "8", "SLOT": "0", "RDEPEND": rdepend}


def _m0():
    ebuilds = dict(PERL)
    ebuilds.update(SLU_TREE)
    ebuilds.update(SUBNAME)
    ebuilds["dev-perl/Try-Tiny-0.320.0"] = _try_tiny(TRY_PURE_RDEPEND)
    ebuilds["virtual/perl-Scalar-List-Utils-1.680.100_rc"] = _virt(
        "virtual/perl-Scalar-List-Utils", "1.680.100_rc", SLU_PURE_RDEPEND
    )
    installed = {
        "dev-lang/perl-5.42.2": dict(PERL["dev-lang/perl-5.42.2"]),
        "virtual/perl-Scalar-List-Utils-1.680.100_rc": _virt(
            "virtual/perl-Scalar-List-Utils", "1.680.100_rc", SLU_PURE_RDEPEND
        ),
    }
    return {
        "note": "p03 baseline",
        "ebuilds": ebuilds,
        "installed": installed,
        "world": ["dev-perl/Try-Tiny", "dev-lang/perl"],
        "request": ["@world"],
        "options": {"--update": True, "--deep": True},
    }


def _m1():
    m = _m0()
    m["ebuilds"]["dev-perl/Try-Tiny-0.320.0"] = _try_tiny(TRY_WRAP_RDEPEND, "minimal")
    m["note"] = "M0 + !minimal? USE-wrap on Try-Tiny (minimal off: active)"
    return m


def _m2():
    m = _m0()
    for ver in ("1.630.0-r1", "1.680.0", "1.700.0"):
        m["ebuilds"][f"virtual/perl-Scalar-List-Utils-{ver}"] = _virt(
            "virtual/perl-Scalar-List-Utils", ver, SLU_PURE_DEAD
        )
    m["note"] = "M0 + 3 extra pure-|| virtuals (count 1 -> 4, shape held)"
    return m


def _m3():
    m = _m0()
    cpv = "virtual/perl-Scalar-List-Utils-1.680.100_rc"
    m["ebuilds"][cpv] = _virt("virtual/perl-Scalar-List-Utils", "1.680.100_rc",
                               SLU_REAL["1.680.100_rc"])
    m["installed"][cpv] = _virt("virtual/perl-Scalar-List-Utils", "1.680.100_rc",
                                 SLU_REAL["1.680.100_rc"])
    m["note"] = "M0 + verbatim real extras on the single virtual (count held)"
    return m


def _m4():
    m = _m1()
    for ver, rdepend in SLU_REAL.items():
        cpv = f"virtual/perl-Scalar-List-Utils-{ver}"
        m["ebuilds"][cpv] = _virt("virtual/perl-Scalar-List-Utils", ver, rdepend)
        if ver == "1.680.100_rc":
            m["installed"][cpv] = _virt("virtual/perl-Scalar-List-Utils", ver, rdepend)
    m["note"] = "full real leg-A shape: wrapped Try-Tiny + 4 verbatim virtuals"
    return m


def _m5_ebuilds():
    ebuilds = dict(PERL)
    ebuilds["perl-core/Encode-3.190.0"] = {"EAPI": "8", "SLOT": "0"}
    ebuilds["perl-core/Encode-3.210.0"] = {"EAPI": "8", "SLOT": "0"}
    ebuilds["dev-perl/Encode-Locale-1.50.0-r1"] = {
        "EAPI": "8",
        "SLOT": "0",
        "RDEPEND": ">=virtual/perl-Encode-2.0.0",
    }
    for ver, rdepend in ENC_REAL.items():
        ebuilds[f"virtual/perl-Encode-{ver}"] = _virt("virtual/perl-Encode", ver, rdepend)
    return ebuilds


def _m5():
    installed = {
        "dev-lang/perl-5.42.2": dict(PERL["dev-lang/perl-5.42.2"]),
        "virtual/perl-Encode-3.210.0-r2": _virt(
            "virtual/perl-Encode", "3.210.0-r2", ENC_REAL["3.210.0-r2"]
        ),
    }
    return {
        "note": "real leg-B shape: Encode-Locale NEW, atom request + upgrade",
        "ebuilds": _m5_ebuilds(),
        "installed": installed,
        "world": ["dev-perl/Encode-Locale", "dev-lang/perl"],
        "request": ["dev-perl/Encode-Locale", "=dev-lang/perl-5.44.0"],
        "options": {},
    }


def _m5b():
    m = _m5()
    m["installed"]["dev-perl/Encode-Locale-1.50.0-r1"] = {
        "EAPI": "8",
        "SLOT": "0",
        "RDEPEND": ">=virtual/perl-Encode-2.0.0",
    }
    m["note"] = "M5 ebuilds under the p02 request (@world --update --deep)"
    m["request"] = ["@world"]
    m["options"] = {"--update": True, "--deep": True}
    return m


def _m6a():
    m = _m0()
    m["note"] = "M0 ebuilds under an atom request (no --update/--deep)"
    m["request"] = ["dev-perl/Try-Tiny", "=dev-lang/perl-5.44.0"]
    m["options"] = {}
    return m


def _m6b():
    m = _m4()
    m["note"] = "M4 ebuilds under an atom request (closest to the real cell)"
    m["request"] = ["dev-perl/Try-Tiny", "=dev-lang/perl-5.44.0"]
    m["options"] = {}
    return m


MUTS = {
    "M0": _m0,
    "M1": _m1,
    "M2": _m2,
    "M3": _m3,
    "M4": _m4,
    "M5": _m5,
    "M5b": _m5b,
    "M6a": _m6a,
    "M6b": _m6b,
}

for name in ("M6b", "M5", "M4", "M5b"):
    m = MUTS[name]()
    print("MUT", name, "-", m["note"])
    playground = ResolverPlayground(
        ebuilds=m["ebuilds"], installed=m["installed"], world=m["world"]
    )
    try:
        result = playground.run(m["request"], options=m["options"])
        print("SUCCESS", result.success)
        print("MERGELIST", result.mergelist)
        print("UNSAT", getattr(result, "unsatisfied_deps", None))
    finally:
        playground.cleanup()
