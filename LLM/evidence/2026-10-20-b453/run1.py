import sys; sys.path.insert(0, "/tmp/claude-1000/-home-vivo-repo-PORTUALE-portuale/31e72e06-5910-492b-8d0e-7bb237e53e32/scratchpad")
from probe import *
pm = sys.argv[1]
r = stage(f"core-{pm}", pm=pm)
P = ["--pretend", "dev-libs/newpkg"]
show("tty -p", tty("p-tty", pm, P, r), r)
show("pipe -p", pipe("p-pipe", pm, P, r), r)
show("tty -pv", tty("pv-tty", pm, ["--pretend", "--verbose", "dev-libs/newpkg"], r), r)
show("pipe -pv", pipe("pv-pipe", pm, ["--pretend", "--verbose", "dev-libs/newpkg"], r), r)
show("tty --tree", tty("tree-tty", pm, ["--pretend", "--tree", "dev-libs/diamond"], r), r)
show("pipe --tree", pipe("tree-pipe", pm, ["--pretend", "--tree", "dev-libs/diamond"], r), r)
show("tty masked", tty("masked-tty", pm, ["--pretend", "dev-libs/maskedpkg"], r), r)
show("pipe masked", pipe("masked-pipe", pm, ["--pretend", "dev-libs/maskedpkg"], r), r)
show("tty --quiet", tty("q-tty", pm, ["--pretend", "--quiet", "dev-libs/newpkg"], r), r)
show("pipe --quiet", pipe("q-pipe", pm, ["--pretend", "--quiet", "dev-libs/newpkg"], r), r)
