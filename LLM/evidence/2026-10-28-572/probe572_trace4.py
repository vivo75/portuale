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

VIRT_RDEPEND = "|| ( =dev-lang/perl-5.42* =dev-lang/perl-5.40* ~perl-core/Encode-3.210.0 )"

ebuilds = {
    "dev-lang/perl-5.42.2": {"EAPI": "8", "SLOT": "0/5.42"},
    "dev-lang/perl-5.44.0": {"EAPI": "8", "SLOT": "0/5.44"},
    "perl-core/Encode-3.210.0": {"EAPI": "8", "SLOT": "0"},
    "dev-perl/Encode-Locale-1.5": {
        "EAPI": "8",
        "SLOT": "0",
        "RDEPEND": ">=virtual/perl-Encode-2.0.0",
    },
    "virtual/perl-Encode-3.210.0-r2": {"EAPI": "8", "SLOT": "0", "RDEPEND": VIRT_RDEPEND},
}
installed = {
    "dev-lang/perl-5.42.2": {"EAPI": "8", "SLOT": "0/5.42"},
    "dev-perl/Encode-Locale-1.5": {
        "EAPI": "8",
        "SLOT": "0",
        "RDEPEND": ">=virtual/perl-Encode-2.0.0",
    },
    "virtual/perl-Encode-3.210.0-r2": {"EAPI": "8", "SLOT": "0", "RDEPEND": VIRT_RDEPEND},
}
world = ["dev-perl/Encode-Locale", "dev-lang/perl"]

orig = dcm.dep_zapdeps


def interesting(unreduced):
    return (
        bool(unreduced)
        and str(unreduced[0]) == "||"
        and any("5.42" in str(x) for x in unreduced)
    )


def patched(unreduced, reduced, myroot, use_binaries=0, trees=None, minimize_slots=False):
    show = interesting(unreduced)
    out = orig(
        unreduced,
        reduced,
        myroot,
        use_binaries=use_binaries,
        trees=trees,
        minimize_slots=minimize_slots,
    )
    if show:
        print("\nZAPDEPS-IN ", unreduced, flush=True)
        print("ZAPDEPS-OUT", unreduced, "->", out, flush=True)
        if trees is not None:
            t = trees[myroot]
            vardb = t["vartree"].dbapi
            port = t["porttree"].dbapi
            for arm in unreduced[1:]:
                if str(arm) in ("||", "(", ")"):
                    continue
                at = Atom(str(arm))
                print(
                    "   arm",
                    arm,
                    "| vardb:",
                    vardb.match(at.without_use),
                    "| portdb:",
                    [p.cpv for p in port.match_pkgs(at.without_use)],
                    flush=True,
                )
    return out


dcm.dep_zapdeps = patched

playground = ResolverPlayground(ebuilds=ebuilds, installed=installed, world=world)
try:
    result = playground.run(["=" + "dev-lang/perl-5.44.0", "dev-perl/Encode-Locale"], options={})
    print("SUCCESS", result.success)
    print("MERGELIST", result.mergelist)
finally:
    playground.cleanup()
