import grp
import os
import pwd
import shutil
import sys
import tempfile

LIB = "/home/vivo/repo/PORTUALE/portuale/3rdparty/portage/lib"
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

import portage.dep.dep_check as dcm  # noqa: E402
from portage.dep import Atom  # noqa: E402
from portage.tests.resolver.ResolverPlayground import ResolverPlayground  # noqa: E402
import _emerge.depgraph as dgmod  # noqa: E402

SLU_RDEPEND = "|| ( =dev-lang/perl-5.42* ~perl-core/Scalar-List-Utils-1.680.100_rc )"

ebuilds = {
    "dev-lang/perl-5.42.2": {"EAPI": "8", "SLOT": "0/5.42"},
    "dev-lang/perl-5.44.0": {"EAPI": "8", "SLOT": "0/5.44"},
    "perl-core/Scalar-List-Utils-1.630.0": {"EAPI": "8", "SLOT": "0"},
    "perl-core/Scalar-List-Utils-1.680.0": {"EAPI": "8", "SLOT": "0"},
    "dev-perl/Sub-Name-0.280.0-r1": {"EAPI": "8", "SLOT": "0"},
    "dev-perl/Try-Tiny-0.320.0": {
        "EAPI": "8",
        "SLOT": "0",
        "RDEPEND": "|| ( >=virtual/perl-Scalar-List-Utils-1.400.0 dev-perl/Sub-Name )",
    },
    "virtual/perl-Scalar-List-Utils-1.680.100_rc": {
        "EAPI": "8",
        "SLOT": "0",
        "RDEPEND": SLU_RDEPEND,
    },
}

installed = {
    "dev-lang/perl-5.42.2": {"EAPI": "8", "SLOT": "0/5.42"},
    "virtual/perl-Scalar-List-Utils-1.680.100_rc": {
        "EAPI": "8",
        "SLOT": "0",
        "RDEPEND": SLU_RDEPEND,
    },
}

world = ["dev-perl/Try-Tiny", "dev-lang/perl"]

BIN_NAMES = (
    "preferred_in_graph", "preferred_non_installed",
    "unsat_use_in_graph", "unsat_use_installed", "unsat_use_non_installed",
    "other_installed", "other_installed_some", "other_installed_any_slot", "other",
)


def interesting(unreduced):
    s = " ".join(str(x) for x in unreduced)
    return ("Sub-Name" in s or "Scalar-List-Utils" in s) and str(unreduced[0]) == "||"


def classify_arm(nested_unreduced, nested_reduced, myroot, trees):
    """Replicate dep_check.py:432-541 + :599-736 for one outer-|| arm."""
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
            nested_unreduced, nested_reduced, myroot, trees=trees)
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
            parent and graph_interface
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
            (str(atom), str(avail_pkg), str(avail_slot)))
        if not replacing and graph_db is not None and downgrade_probe is not None:
            slot_matches = graph_db.match_pkgs(avail_slot)
            if (len(slot_matches) > 1 and avail_pkg < slot_matches[-1]
                    and not downgrade_probe(avail_pkg)):
                conflict_downgrade = True
                detail["conflict_downgrade_atom"] = str(atom)
        if not replacing and downgrade_probe is not None and graph is not None:
            highest = mydbapi.match_pkgs(avail_slot)
            highest = highest[-1] if highest else None
            if (avail_pkg and highest and avail_pkg < highest
                    and not downgrade_probe(avail_pkg)
                    and (highest.installed or highest in graph)):
                installed_downgrade = True
                detail["installed_downgrade_atom"] = (
                    str(atom), str(avail_pkg), str(highest),
                    bool(highest.installed), bool(highest in graph))
    detail.update(all_available=all_available,
                  conflict_downgrade=conflict_downgrade,
                  installed_downgrade=installed_downgrade)
    if not all_available:
        # :713-736 branch
        all_inst = all(vardb.match(a) for a in atoms if not a.blocker) if atoms else True
        some_inst = any(vardb.match(a) for a in atoms if not a.blocker) if atoms else False
        fuzzy = any(vardb.match(Atom(a.cp)) for a in atoms if not a.blocker) if atoms else False
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
            b = "preferred_non_installed" if not all_installed else "preferred_installed?"
            # :697-701: all_installed -> preferred_installed / preferred_any_slot
            if all_installed:
                all_installed_slots = True
                # slot_map unavailable here; approximate below
                b = "preferred_installed-or-any_slot (all_installed=True)"
    else:
        b = "unsat_use_*"
    detail["bin"] = b
    detail["all_in_graph"] = all_in_graph
    return detail


orig_zapdeps = dcm.dep_zapdeps


def patched_zapdeps(unreduced, reduced, myroot, use_binaries=0, trees=None, minimize_slots=False):
    show = bool(unreduced) and interesting(unreduced)
    out = orig_zapdeps(
        unreduced, reduced, myroot,
        use_binaries=use_binaries, trees=trees, minimize_slots=minimize_slots,
    )
    if show:
        print("\nZAPDEPS-IN ", unreduced, flush=True)
        print("ZAPDEPS-REDUCED", reduced, flush=True)
        print("ZAPDEPS-OUT", unreduced, "->", out, flush=True)
        if trees is not None:
            t = trees[myroot]
            print("   parent:", t.get("parent"),
                  "| virt_parent:", t.get("virt_parent"),
                  "| graph_db is None:", t.get("graph_db") is None, flush=True)
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
    if "Scalar-List-Utils" in repr(mysplit) or "Sub-Name" in repr(mysplit):
        print("\nEXPAND-IN  ", mysplit, flush=True)
        print("EXPAND-OUT ", out, flush=True)
    return out


dcm._expand_new_virtuals = patched_expand

orig_add_dep = dgmod.depgraph._add_dep


def patched_add_dep(self, dep, allow_unsatisfied=False):
    atom_s = str(getattr(dep, "atom", ""))
    if "Sub-Name" in atom_s or "Scalar-List-Utils" in atom_s:
        print(
            "\nADD_DEP atom:", dep.atom,
            "| parent:", dep.parent,
            "| child:", dep.child,
            flush=True,
        )
    return orig_add_dep(self, dep, allow_unsatisfied=allow_unsatisfied)


dgmod.depgraph._add_dep = patched_add_dep

playground = ResolverPlayground(ebuilds=ebuilds, installed=installed, world=world)
try:
    result = playground.run(["@world"], options={"--update": True, "--deep": True})
    print("SUCCESS", result.success)
    print("MERGELIST", result.mergelist)
    print("UNSAT", getattr(result, "unsatisfied_deps", None))
finally:
    playground.cleanup()
