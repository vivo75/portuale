"""S0 #649 Step 1: mirror the dev-lang_perl over-fire cell in the playground.

Portage 3.0.83 is the oracle per mutation. Run:
  python3 probe649_mutations.py [B0|N0|N1|N2]
(No arg = all four.)

Hermeticity: `ResolverPlayground` builds the repo/vdb/world in a temp dir
from the dicts below -- no host tree is read. (The #645 Task-2 host-gentoo
leak was a scratch-world `PORTAGE_REPOSITORIES` miss; the playground has no
repos.conf to leak through, and the Task-2 gate check pins
`PORTAGE_REPOSITORIES` on its scratch world -- see the gate probe header.)

Recorded cell (Task 1 report): `emerge -pv dev-lang/perl`, no --update.
Real Total 1 (perl-5.42.2 reinstall); the merged perl-5.42.2 carries the
LIVE in-graph consumer PDEPEND (`dev-lang/perl-5.42.2.ebuild:72-82`, bed
tree @ `e07309ba6`):
  PDEPEND="!minimal? ( ... >=virtual/perl-Encode-3.120.0 ... virtual/perl-Test-Harness )"
(`minimal` OFF in the merged row, so the block is active.) Both virtuals
match an installed virtual (Encode-3.210.0-r2, Test-Harness-3.500.0-r1)
plus tree versions; portuale merges the `~perl-core/` pins out of the
INSTALLED versions' 5.42-arm conjunctions (merge-rows 2,3); real merges
neither. Both outputs schedule-then-skip perl-5.44.0 via the dev-lang/perl:0
conflict (WARNING block).

Entry lead candidate under test: the per-version selection's
installed-wins/downgrade-guard interaction. Already killed by inspection
before the first mutation: the "installed virtual version has no tree
ebuild" sub-form does NOT match this cell -- host tree @ the pin carries
`virtual/perl-Encode-3.210.0-r2.ebuild` AND
`virtual/perl-Test-Harness-3.500.0-r1.ebuild` (both installed versions have
tree ebuilds). The mutations below test the guard interaction itself:
does the transient co-scheduled 5.44.0 demote the installed `=perl-5.42*`
arm where real holds the installed binding?

Axes (B0 = probe647_oracle.py M5 verbatim, the control):
- B0 M5 verbatim (Encode-Locale NEW + explicit `=perl-5.44.0` atom
  request). Oracle: mergelist `[perl-5.44.0, perl-core/Encode-3.210.0,
  Encode-Locale-1.50.0-r1]`. Proves the harness reproduces the oracle.
- N0 live-only: consumer := dev-lang/perl-5.42.2 with the cell's PDEPEND
  (`!minimal?` block, IUSE=minimal, minimal off so the block is ACTIVE --
  the probe645-M1 precedent), request `[dev-lang/perl]` bare, NO 5.44.0 in
  tree. Tree has both Encode virtuals + both Test-Harness virtuals with
  oracle-proven arm text (see ARMS note) and perl-core pins only for the
  OLD versions (Encode-3.190.0 + 3.210.0, no 3.240.0; Test-Harness-3.500.0,
  no 3.520.0 -- as in the real tree). Installed: perl-5.42.2 + both
  5.42-arm virtuals. If Portage holds the binding here (reinstall only),
  the transient 5.44.0 is NOT needed for the hold.
- N1 N0 + perl-5.44.0 in tree (same PDEPEND text -- identical shape, like
  the real tree). Tests whether the transient scheduled-then-skipped
  5.44.0 changes Portage's mergelist or its demoted conjunction.

ARMS note (probe647_oracle.py:36-55 divergent-arms table): the RDEPENDs
below carry the ORACLE spelling (`~`/`!<` without the installed revision,
probe-form `!>`); the TRUE tree text differs only in the revisioned `~`/
`!<` (`~perl-core/Encode-3.210.0-r2`, `!<...-r2` blocking tree 3.190.0) and
the INVALID `!>` (`!>...-3.210.0-r2-r999`). Mergelist-neutral here:
nothing merges a perl-core in the oracle legs, and the real cell with TRUE
text still holds the binding -- the axis under test (guard vs installed
arm) never touches the blocked range. If N0/N1 surprise, the TRUE spelling
is the first follow-up axis (see the task-2 report).

TRACE (probe643_trace.py style): `dep_zapdeps` is patched to print + bin
every `||` group mentioning Encode/Test-Harness/perl-core (bin ladder
replicates dep_check.py:432-541 + :599-736); `_expand_new_virtuals`
prints the per-version conjunctions Portage builds for each virtual atom
(`dep_check.py:197,220,264`); `depgraph._add_dep` prints every merge
addition of an Encode/Test-Harness/perl-core package with its parent.
Read the winner per group from the bins, and whether a perl-core package
is ever added (oracle: never in N0/N1).
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

from portage.dep import Atom  # noqa: E402
import portage.dep.dep_check as dcm  # noqa: E402
from portage.tests.resolver.ResolverPlayground import ResolverPlayground  # noqa: E402
import _emerge.depgraph as dgmod  # noqa: E402


def _rdepend_or(rdepend):
    return {"EAPI": "8", "SLOT": "0", "RDEPEND": rdepend}


# Oracle-proven arm text (probe647 ENC_REAL + the Test-Harness twin built
# the same way: installed 5.42-arm with the live `=perl-5.42*` leg first,
# tree-newer 5.44-arm naming `=perl-5.44*` with a `~` pin at a version that
# has NO perl-core ebuild -- the cell's attribution lock: the merged pins
# 3.210.0/3.500.0 exist ONLY in the installed arms' conjunctions).
ENC_REAL = {
    "3.210.0-r2": "|| ( =dev-lang/perl-5.42* =dev-lang/perl-5.40*"
    " ~perl-core/Encode-3.210.0 ) dev-lang/perl:= !<perl-core/Encode-3.210.0"
    " !>perl-core/Encode-3.210.0-r999",
    "3.240.0": "|| ( =dev-lang/perl-5.44* ~perl-core/Encode-3.240.0 )"
    " dev-lang/perl:= !<perl-core/Encode-3.240.0 !>perl-core/Encode-3.240.0-r999",
}
TH_REAL = {
    "3.500.0-r1": "|| ( =dev-lang/perl-5.42*"
    " ~perl-core/Test-Harness-3.500.0 ) dev-lang/perl:="
    " !<perl-core/Test-Harness-3.500.0 !>perl-core/Test-Harness-3.500.0-r999",
    "3.520.0": "|| ( =dev-lang/perl-5.44* ~perl-core/Test-Harness-3.520.0 )"
    " dev-lang/perl:= !<perl-core/Test-Harness-3.520.0"
    " !>perl-core/Test-Harness-3.520.0-r999",
}

PERL_PDEPEND = (
    "!minimal? ( >=virtual/perl-Encode-3.120.0 virtual/perl-Test-Harness )"
)


def _perl(version, slot):
    return {
        "EAPI": "8",
        "SLOT": f"0/{slot}",
        "IUSE": "minimal",
        "PDEPEND": PERL_PDEPEND,
    }


def _n_ebuilds(with_544):
    ebuilds = {
        "perl-core/Encode-3.190.0": {"EAPI": "8", "SLOT": "0"},
        "perl-core/Encode-3.210.0": {"EAPI": "8", "SLOT": "0"},
        "perl-core/Test-Harness-3.500.0": {"EAPI": "8", "SLOT": "0"},
    }
    ebuilds["dev-lang/perl-5.42.2"] = _perl("5.42.2", "5.42")
    if with_544:
        ebuilds["dev-lang/perl-5.44.0"] = _perl("5.44.0", "5.44")
    for ver, rdepend in ENC_REAL.items():
        ebuilds[f"virtual/perl-Encode-{ver}"] = _rdepend_or(rdepend)
    for ver, rdepend in TH_REAL.items():
        ebuilds[f"virtual/perl-Test-Harness-{ver}"] = _rdepend_or(rdepend)
    return ebuilds


def _n_installed():
    return {
        "dev-lang/perl-5.42.2": _perl("5.42.2", "5.42"),
        "virtual/perl-Encode-3.210.0-r2": _rdepend_or(ENC_REAL["3.210.0-r2"]),
        "virtual/perl-Test-Harness-3.500.0-r1": _rdepend_or(TH_REAL["3.500.0-r1"]),
    }


def _b0():
    # probe647 M5 verbatim (Encode-Locale NEW + explicit =perl-5.44.0).
    ebuilds = {
        "dev-lang/perl-5.42.2": {"EAPI": "8", "SLOT": "0/5.42"},
        "dev-lang/perl-5.44.0": {"EAPI": "8", "SLOT": "0/5.44"},
        "perl-core/Encode-3.190.0": {"EAPI": "8", "SLOT": "0"},
        "perl-core/Encode-3.210.0": {"EAPI": "8", "SLOT": "0"},
        "dev-perl/Encode-Locale-1.50.0-r1": {
            "EAPI": "8",
            "SLOT": "0",
            "RDEPEND": ">=virtual/perl-Encode-2.0.0",
        },
    }
    for ver, rdepend in ENC_REAL.items():
        ebuilds[f"virtual/perl-Encode-{ver}"] = _rdepend_or(rdepend)
    return {
        "note": "M5 verbatim control (oracle: perl-5.44.0 + perl-core/Encode-3.210.0 + Encode-Locale)",
        "ebuilds": ebuilds,
        "installed": {
            "dev-lang/perl-5.42.2": {"EAPI": "8", "SLOT": "0/5.42"},
            "virtual/perl-Encode-3.210.0-r2": _rdepend_or(ENC_REAL["3.210.0-r2"]),
        },
        "world": ["dev-perl/Encode-Locale", "dev-lang/perl"],
        "request": ["dev-perl/Encode-Locale", "=dev-lang/perl-5.44.0"],
        "options": {},
    }


def _n0():
    return {
        "note": "live-only: perl-5.42.2 PDEPEND consumer, bare atom request, no 5.44.0 in tree",
        "ebuilds": _n_ebuilds(False),
        "installed": _n_installed(),
        "world": ["dev-lang/perl"],
        "request": ["dev-lang/perl"],
        "options": {},
    }


def _n1():
    m = _n0()
    m["ebuilds"] = _n_ebuilds(True)
    m["note"] = "N0 + perl-5.44.0 in tree (identical PDEPEND: the transient axis)"
    return m


# The cell-posture axis: one installed reverse-edge holder pins the 5.42
# subslot the way the real cell's ~100 `dev-lang/perl:0/5.42=` vdb edges do
# (real WARNING block: every installed virtual + dev-perl package requires
# `dev-lang/perl:0/5.42=` and/or `=dev-lang/perl-5.42*`, so the transient
# 5.44.0 is scheduled-then-SKIPPED and perl reinstalls). N2 = N1 + that
# holder, installed only (no tree ebuild: it is background, never merged).
def _n2():
    m = _n1()
    m["installed"]["dev-perl/s649pin-1"] = {
        "EAPI": "8",
        "SLOT": "0",
        "RDEPEND": "dev-lang/perl:0/5.42=",
    }
    m["note"] = (
        "N1 + installed subslot pin-holder (cell posture: 5.44.0 must skip,"
        " perl reinstalls)"
    )
    return m


MUTS = {"B0": _b0, "N0": _n0, "N1": _n1, "N2": _n2}


def interesting(unreduced):
    s = " ".join(str(x) for x in unreduced)
    return ("perl-core" in s or "Test-Harness" in s or "perl-Encode" in s) and str(
        unreduced[0]
    ) == "||"


def classify_arm(nested_unreduced, nested_reduced, myroot, trees):
    """Replicate dep_check.py:432-541 + :599-736 for one outer-|| arm
    (probe643_trace.py's classifier, unchanged)."""
    t = trees[myroot]
    vardb = t["vartree"].dbapi
    mydbapi = t["porttree"].dbapi
    graph_db = t.get("graph_db")
    graph = t.get("graph")
    downgrade_probe = t.get("downgrade_probe")
    parent = t.get("parent")
    graph_interface = t.get("graph_interface")
    if isinstance(nested_unreduced, list):
        atoms = dcm.dep_zapdeps(
            nested_unreduced, nested_reduced, myroot, trees=trees
        )
    else:
        atoms = [nested_unreduced]
    detail = {"atoms_after_recurse": [str(a) for a in atoms]}
    all_available = True
    all_use_satisfied = True
    conflict_downgrade = False
    installed_downgrade = False
    for atom in atoms:
        if atom.blocker:
            continue
        replacing = (
            parent
            and graph_interface
            and graph_interface.will_replace_child(parent, myroot, atom)
        )
        avail = mydbapi.match_pkgs(atom.without_use)
        if avail:
            avail_pkg = avail[-1]
            avail_slot = Atom(f"{atom.cp}:{avail_pkg.slot}")
        else:
            all_available = False
            break
        detail.setdefault("avail", []).append(
            (str(atom), str(avail_pkg), str(avail_slot))
        )
        if not replacing and graph_db is not None and downgrade_probe is not None:
            slot_matches = graph_db.match_pkgs(avail_slot)
            if (
                len(slot_matches) > 1
                and avail_pkg < slot_matches[-1]
                and not downgrade_probe(avail_pkg)
            ):
                conflict_downgrade = True
                detail["conflict_downgrade_atom"] = str(atom)
        if not replacing and downgrade_probe is not None and graph is not None:
            highest = mydbapi.match_pkgs(avail_slot)
            highest = highest[-1] if highest else None
            if (
                avail_pkg
                and highest
                and avail_pkg < highest
                and not downgrade_probe(avail_pkg)
                and (highest.installed or highest in graph)
            ):
                installed_downgrade = True
                detail["installed_downgrade_atom"] = (
                    str(atom),
                    str(avail_pkg),
                    str(highest),
                    bool(highest.installed),
                    bool(highest in graph),
                )
    detail.update(
        all_available=all_available,
        conflict_downgrade=conflict_downgrade,
        installed_downgrade=installed_downgrade,
    )
    if not all_available:
        all_inst = (
            all(vardb.match(a) for a in atoms if not a.blocker) if atoms else True
        )
        some_inst = (
            any(vardb.match(a) for a in atoms if not a.blocker) if atoms else False
        )
        fuzzy = (
            any(vardb.match(Atom(a.cp)) for a in atoms if not a.blocker)
            if atoms
            else False
        )
        if all_inst:
            b = "other_installed"
        elif some_inst:
            b = "other_installed_some"
        elif fuzzy:
            b = "other_installed_any_slot"
        else:
            b = "other"
        detail["bin"] = b + " (via all_available=False)"
        return detail
    if graph_db is None:
        detail["bin"] = "graph_db-None branch (unexpected here)"
        return detail
    if conflict_downgrade or installed_downgrade:
        detail["bin"] = "other (via downgrade guard :634-635)"
        return detail
    all_in_graph = True
    for atom in atoms:
        if atom.blocker or atom.category == "virtual":
            continue
        if not any(pkg in graph for pkg in graph_db.match_pkgs(atom)):
            all_in_graph = False
            detail["not_in_graph"] = str(atom)
            break
    if all_use_satisfied:
        if all_in_graph:
            b = "preferred_in_graph"
        else:
            all_installed = True
            for a in {Atom(a.cp) for a in atoms if not a.blocker}:
                if not vardb.match(a) and a.category != "virtual":
                    all_installed = False
                    break
            b = (
                "preferred_non_installed"
                if not all_installed
                else "preferred_installed-or-any_slot (all_installed=True)"
            )
    else:
        b = "unsat_use_*"
    detail["bin"] = b
    detail["all_in_graph"] = all_in_graph
    return detail


orig_zapdeps = dcm.dep_zapdeps


def patched_zapdeps(unreduced, reduced, myroot, use_binaries=0, trees=None, minimize_slots=False):
    show = bool(unreduced) and interesting(unreduced)
    out = orig_zapdeps(
        unreduced,
        reduced,
        myroot,
        use_binaries=use_binaries,
        trees=trees,
        minimize_slots=minimize_slots,
    )
    if show:
        print("\nZAPDEPS-IN ", unreduced, flush=True)
        print("ZAPDEPS-REDUCED", reduced, flush=True)
        print("ZAPDEPS-OUT", unreduced, "->", out, flush=True)
        if trees is not None:
            t = trees[myroot]
            print(
                "   parent:", t.get("parent"),
                "| virt_parent:", t.get("virt_parent"),
                "| graph_db is None:", t.get("graph_db") is None, flush=True,
            )
            for x, sat in zip(unreduced[1:], reduced[1:]):
                print("   ARM", x, "| reduced:", sat, flush=True)
                print("      =>", classify_arm(x, sat, myroot, trees), flush=True)
    return out


dcm.dep_zapdeps = patched_zapdeps

orig_expand = dcm._expand_new_virtuals


def patched_expand(mysplit, edebug, mydbapi, mysettings, myroot="/", trees=None,
                   use_mask=None, use_force=None, **kwargs):
    out = orig_expand(
        mysplit, edebug, mydbapi, mysettings, myroot=myroot, trees=trees,
        use_mask=use_mask, use_force=use_force, **kwargs,
    )
    if "perl-Encode" in repr(mysplit) or "Test-Harness" in repr(mysplit):
        print("\nEXPAND-IN  ", mysplit, flush=True)
        print("EXPAND-OUT ", out, flush=True)
    return out


dcm._expand_new_virtuals = patched_expand

orig_add_dep = dgmod.depgraph._add_dep


def patched_add_dep(self, dep, allow_unsatisfied=False):
    atom_s = str(getattr(dep, "atom", ""))
    if "perl-core" in atom_s or "Test-Harness" in atom_s or "perl-Encode" in atom_s:
        print(
            "\nADD_DEP atom:", dep.atom,
            "| parent:", dep.parent,
            "| child:", dep.child,
            flush=True,
        )
    return orig_add_dep(self, dep, allow_unsatisfied=allow_unsatisfied)


dgmod.depgraph._add_dep = patched_add_dep

names = sys.argv[1:] or ["B0", "N0", "N1", "N2"]
for name in names:
    m = MUTS[name]()
    print("=" * 70)
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
