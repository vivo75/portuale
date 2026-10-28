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

VIRT_RDEPEND = "perl-core/Encode-3.210.0"

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

playground = ResolverPlayground(ebuilds=ebuilds, installed=installed, world=world)
try:
    result = playground.run(
        ["=" + "dev-lang/perl-5.44.0", "dev-perl/Encode-Locale"], options={}
    )
    print("SUCCESS", result.success)
    print("MERGELIST", result.mergelist)
    print("UNSAT", getattr(result, "unsatisfied_deps", None))
finally:
    playground.cleanup()
