"""#651 oracle lock: N1/N2/H1 re-run verbatim + the C1 chained holder.

Portage 3.0.83 is the oracle. Run:
  python3 probe651_oracle.py
(No args: N1, N2, H1, then C1, in that order.)

N1/N2 come from S0 `probe649_mutations.py` VERBATIM -- imported, not
re-derived (its import-time runner executes the requested mutations, so
`sys.argv` is set to `["probe651_oracle.py", "N1", "N2"]` before the
import: the N1/N2 blocks below ARE that import's verbatim output). H1 is
#650's holder shape (`probe650_oracle.py`) copied VERBATIM, not
re-derived (N1's dicts plus a tree+installed `dev-perl/holder-1` carrying
the cell's subslot reverse edge, requested alongside perl). C1 is #651's
commit-before-conflict shape: H1's holder pulled TRANSITIVELY through a
requested tree+installed `dev-perl/top-1` (RDEPEND `dev-perl/holder`) --
the direct request resolves the conflict too early (the #650 withhold
gate declines: conflict already open at commit time), while the chained
request commits the seeds in the first pass and the conflict surfaces
only as try-2 `SlotConflict` feedback (recorded in the binary leg below).

Deltas vs the real cell (stated, all outcome-neutral for the verdict):
- the `=perl-5.40*` leg dangles (no perl-5.40.2 in the playground tree;
  the real tree has it, but the arm is guard-demoted there and
  unavailable here -- non-winning on both sides);
- the non-interacting PDEPEND conjuncts (perl-cleaner/CPAN/File-Temp/
  Data-Dumper/Math-BigInt) are omitted, like the M4 probe's Carp omission;
- the holder carries the plain pin `dev-lang/perl:0/5.42`, not the cell's
  vdb `dev-lang/perl:0/5.42=`: the trailing-`=` rebuild form is rejected in
  tree RDEPEND context (the H1b scratch run proves the plain pin renders
  the posture);
- the holder is REQUESTED in H1 (`["dev-lang/perl", "dev-perl/holder"]`)
  but TRANSITIVE in C1 (`["dev-lang/perl", "dev-perl/top"]`), where the
  cell's holders are background world packages: portuale's walk never
  sees unreferenced installed leaves, so the edge must be in the closure
  to argue against the transient. The resolution posture (R+skip + extras)
  matches the cell exactly; the request shape differs, and says so.
- the 5.44 arms' `~` pins (`~perl-core/Encode-3.240.0`,
  `~perl-core/Test-Harness-3.520.0`) dangle hermetically: no such tree
  ebuilds exist here, matching the real tree (S0 attribution lock: "for
  which NO tree ebuild exists"); omitted as non-interacting (the `~` arm
  is non-winning on both sides either way).

Expected verbatim (locked 2026-11-04, Portage 3.0.83):
- N1: SUCCESS True, MERGELIST ['dev-lang/perl-5.42.2']
- N2: SUCCESS True, MERGELIST ['dev-lang/perl-5.42.2']
- H1: SUCCESS True, MERGELIST ['dev-lang/perl-5.42.2', 'dev-perl/holder-1']
  (reinstalls only; the transient skipped; no fall-through merged).
- C1: SUCCESS True, MERGELIST ['dev-perl/top-1', 'dev-lang/perl-5.42.2']
  (reinstalls only; the transient skipped; no fall-through merged).

The portuale-side C1 run (committed fixtures, `emerge --pretend` with
`PORTUALE_BT_TRACE=1`) is appended after the playground blocks: both
`SEED commit` lines (demoter `p651prov-5.44.0`) precede any `SlotConflict`
mask involving the demoter, with zero `SEED decline` lines -- the seeds
commit in the first pass, before the transient's slot conflict opens (it
surfaces as try-2 feedback, then the try-3 settle mask), and the final
rows show the WARNING-proven skip with the extras still merged = RED
(seeds saw pressure AND survived it). Hermeticity: playground legs build
repo/vdb/world in a temp dir (no host tree is read); the binary leg
points PORTAGE_CONFIGROOT/ROOT/PORTAGE_RUNNING_ROOT at the committed
fixtures and strips the suite's `_ENV_CONFIG_VARS` (copied from
probe649_gate.py) so host USE/KEYWORDS cannot leak in.
"""

import os
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
PM = HERE.parents[2]
BINARY = PM / "target" / "release" / "portuale"
FIXTURES = Path(os.environ.get("PORTUALE_FIXTURES", PM / "fixtures"))

# N1/N2 verbatim: the import below executes these two mutations itself.
sys.argv = ["probe651_oracle.py", "N1", "N2"]
sys.path.insert(0, str(HERE.parent / "2026-11-03-649"))
import probe649_mutations as m649  # noqa: E402

from portage.tests.resolver.ResolverPlayground import (  # noqa: E402
    ResolverPlayground,
)


def _h1():
    m = m649._n1()
    m["ebuilds"]["dev-perl/holder-1"] = {
        "EAPI": "8",
        "SLOT": "0",
        "RDEPEND": "dev-lang/perl:0/5.42",
    }
    m["installed"]["dev-perl/holder-1"] = {
        "EAPI": "8",
        "SLOT": "0",
        "RDEPEND": "dev-lang/perl:0/5.42",
    }
    m["request"] = ["dev-lang/perl", "dev-perl/holder"]
    m["note"] = (
        "N1 + REFERENCED subslot pin-holder (cell posture: transient "
        "scheduled-then-skipped, perl reinstalls; the N2 leaf is inert)"
    )
    return m


def _c1():
    m = _h1()
    m["ebuilds"]["dev-perl/top-1"] = {
        "EAPI": "8",
        "SLOT": "0",
        "RDEPEND": "dev-perl/holder",
    }
    m["installed"]["dev-perl/top-1"] = {
        "EAPI": "8",
        "SLOT": "0",
        "RDEPEND": "dev-perl/holder",
    }
    m["request"] = ["dev-lang/perl", "dev-perl/top"]
    m["note"] = (
        "H1 with the holder behind a requested top (commit-before-conflict:"
        " seeds commit before the holder edge opens the conflict)"
    )
    return m


for _name, _mk in (("H1", _h1), ("C1", _c1)):
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

# --- portuale side: the committed p651 fixture family, both legs --------
# (copied from probe649_gate.py: stripped so the host config cannot leak
# into the fixture-configroot run; PORTUALE_BT_TRACE=1 records the
# commit-before-conflict ordering verbatim).
_ENV_STRIP = (
    "USE ACCEPT_KEYWORDS USE_EXPAND USE_EXPAND_UNPREFIXED USE_EXPAND_IMPLICIT "
    "USE_EXPAND_HIDDEN IUSE_IMPLICIT ACCEPT_LICENSE ACCEPT_PROPERTIES "
    "ACCEPT_RESTRICT PKGDIR PORTAGE_LOGDIR PORTAGE_BINHOST PORTAGE_NICENESS "
    "PORTAGE_IONICE_COMMAND PORTAGE_SCHEDULING_POLICY PORTAGE_SCHEDULING_PRIORITY "
    "PORTAGE_ELOG_SYSTEM PORTAGE_ELOG_CLASSES PORTAGE_ELOG_MAILURI FEATURES CHOST "
    "CBUILD CTARGET CFLAGS CXXFLAGS CPPFLAGS LDFLAGS FFLAGS FCFLAGS MAKEOPTS "
    "EMERGE_DEFAULT_OPTS PORTAGE_RSYNC_EXTRA_OPTS GENTOO_MIRRORS VIDEO_CARDS "
    "PYTHON_TARGETS PYTHON_SINGLE_TARGET LINGUAS L10N CPU_FLAGS_X86 ELIBC KERNEL "
    "USERLAND ABI_X86 BINPKG_FORMAT BINPKG_COMPRESS BINPKG_COMPRESS_FLAGS "
    "PORTAGE_BZIP2_COMMAND PORTAGE_BUNZIP2_COMMAND CONFIG_PROTECT "
    "CONFIG_PROTECT_MASK ENV_UNSET PORTAGE_REPOSITORIES"
).split()


def _binary_run(label, atoms):
    env = dict(os.environ)
    for k in _ENV_STRIP:
        env.pop(k, None)
    env["PORTAGE_CONFIGROOT"] = str(FIXTURES)
    env["ROOT"] = str(FIXTURES)
    env["PORTAGE_RUNNING_ROOT"] = str(FIXTURES)
    env["DISTDIR"] = str(FIXTURES / "distfiles")
    env["PORTUALE_BT_TRACE"] = "1"
    env.setdefault("PMTEST_ALLOW_ROOT", "1")
    assert BINARY.is_file(), f"portuale binary missing: {BINARY}"
    proc = subprocess.run(
        [str(BINARY), "emerge", "--pretend", *atoms],
        capture_output=True, text=True, env=env, check=False,
    )
    print("=" * 70)
    print(f"PORTUALE {label} atoms={list(atoms)}")
    print(f"--- rc={proc.returncode} stdout ---")
    print(proc.stdout, end="")
    print(f"--- stderr ({len(proc.stderr)} bytes) ---")
    print(proc.stderr, end="")


_binary_run("N1-fixture (bare leg)", ["dev-libs/p651prov"])
_binary_run("C1-fixture (chained-holder leg)", ["dev-libs/p651prov", "dev-perl/p651top"])
