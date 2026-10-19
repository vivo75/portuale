#!/usr/bin/env python3
"""Write the per-cell $R/pkgdir/Packages index and package.use. Usage: gen.py A|B|C"""
import sys

R = "/var/tmp/w0/s10/R"
cell = sys.argv[1]

TOP = """CPV: app-misc/cell-top-1
DEFINED_PHASES: -
DESCRIPTION: cell top
EAPI: 8
IUSE:
KEYWORDS: amd64
SLOT: 0
USE:
REPO: cellrepo
PATH: app-misc/cell-top-1.tbz2
BUILD_ID: 1
BUILD_TIME: 1000
"""

T341 = """CPV: dev-libs/t341-dep-1
DEFINED_PHASES: -
DESCRIPTION: backlog #346 S3 oracle build {bid}
EAPI: 8
IUSE: x
KEYWORDS: amd64
SLOT: 0/1
USE: {use}
REPO: t341
PATH: dev-libs/t341-dep-1-{bid}.tbz2
BUILD_ID: {bid}
BUILD_TIME: {bt}
{extra}"""

if cell == "A":
    # dep respect-use: both builds USE="" but user asks x -> both mismatch
    entries = [TOP, T341.format(bid="1", bt=1000, use="", extra=""),
               T341.format(bid="2", bt=2000, use="", extra="")]
    use_line = "dev-libs/t341-dep x\n"
elif cell in ("B", "C"):
    # changed-deps: binary records RDEPEND the ebuild (no DEPEND at all) lacks
    entries = [T341.format(bid="1", bt=1000, use="",
                           extra="RDEPEND: dev-libs/changed-probe\n")]
    if cell == "C":
        entries.append(TOP)
    use_line = ""
else:
    raise SystemExit("unknown cell")

n = len(entries)
with open(f"{R}/pkgdir/Packages", "w") as f:
    f.write("VERSION: 0\nTIMESTAMP: 0\nPACKAGES: %d\n\n" % n)
    f.write("\n".join(entries) + "\n")
with open(f"{R}/etc/portage/package.use", "w") as f:
    f.write(use_line)
print(f"cell {cell}: {n} binaries, package.use={use_line!r}")
