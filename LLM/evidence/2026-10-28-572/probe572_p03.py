import grp
import os
import pwd
import shutil
import sys
import tempfile

LIB = "/home/vivo/repo/PORTUALE/portuale/3rdparty/portage/lib"
sys.path.insert(0, LIB)

import portage
import portage.const
import portage.output
import portage.util

os.environ["PORTAGE_USERNAME"] = pwd.getpwuid(os.getuid()).pw_name
os.environ["PORTAGE_GRPNAME"] = grp.getgrgid(os.getgid()).gr_name
path = os.environ.get("PATH", "").split(":")
path.insert(0, portage.const.PORTAGE_BIN_PATH)
os.environ["PATH"] = ":".join([p for p in path if p])

portage._internal_caller = True
portage._disable_legacy_globals()
if portage.util.no_color(os.environ):
    portage.output.nocolor()

gpg_path = tempfile.mkdtemp(prefix="gpg_")
shutil.copytree(os.path.join(LIB, "portage", "tests", ".gnupg"), gpg_path, dirs_exist_ok=True)
os.chmod(gpg_path, 0o700)
os.environ["PORTAGE_GNUPGHOME"] = gpg_path

from portage.tests.resolver.ResolverPlayground import ResolverPlayground  # noqa: E402

# S0 p03 shape (LLM/evidence/2026-10-25-567/ probes p03/p10): Try-Tiny's
# || names the installed virtual first, but the virtual's own || is doubly
# dead once perl-5.44.0 is co-scheduled, so the choice falls through to
# dev-perl/Sub-Name.
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

playground = ResolverPlayground(ebuilds=ebuilds, installed=installed, world=world)
try:
    result = playground.run(
        ["@world"], options={"--update": True, "--deep": True}
    )
    print("SUCCESS", result.success)
    print("MERGELIST", result.mergelist)
    print("UNSAT", getattr(result, "unsatisfied_deps", None))
finally:
    playground.cleanup()
