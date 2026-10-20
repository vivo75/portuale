import sys; sys.path.insert(0, "/tmp/claude-1000/-home-vivo-repo-PORTUALE-portuale/31e72e06-5910-492b-8d0e-7bb237e53e32/scratchpad")
from probe import *
r = stage("nd")
show("portage tty --nodeps", tty("nodeps-tty", "portage", ["--package-moves=n", "--pretend", "--nodeps", "dev-libs/newpkg"], r), r)
argv, rc, data, n = tty("debug-tty", "portage", ["--package-moves=n", "--pretend", "--debug", "dev-libs/newpkg"], r)
print("=== portage tty --debug\nargv:", argv, "rc", rc)
for k in ("These are the packages", "Calculating dependencies", "Dependency resolution took", "\x1b[?25l", "\x1b]0;"):
    print(f"  {k!r}: {k in data}")
