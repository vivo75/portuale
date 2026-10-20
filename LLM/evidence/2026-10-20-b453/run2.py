import sys; sys.path.insert(0, "/tmp/claude-1000/-home-vivo-repo-PORTUALE-portuale/31e72e06-5910-492b-8d0e-7bb237e53e32/scratchpad")
from probe import *
pm = sys.argv[1]
r = stage(f"cond-{pm}", pm=pm)
P = ["--pretend", "dev-libs/newpkg"]
show("tty --nospinner", tty("nospinner-tty", pm, ["--nospinner", *P], r), r)
show("tty TERM=dumb", tty("dumb-tty", pm, P, r, extra={"TERM": "dumb"}), r)
show("tty TERM=vt100", tty("vt100-tty", pm, P, r, extra={"TERM": "vt100"}), r)
show("tty FEATURES=notitles", tty("notitles-tty", pm, P, r, extra={"FEATURES": "notitles"}), r)
show("tty FEATURES=candy", tty("candy-tty", pm, P, r, extra={"FEATURES": "candy"}), r)
show("tty --debug (grep)", tty("debug-tty", pm, ["--debug", *P], r), r) if pm == "x" else None
show("tty, stderr to file", tty_sh("stderrfile-tty", pm, "EMERGE --pretend dev-libs/newpkg 2>/var/tmp/b453/err.txt", r), r)
print("stderr file:", repr(open("/var/tmp/b453/err.txt").read()[-300:]))
show("tty, stdout to file", tty_sh("stdoutfile-tty", pm, "EMERGE --pretend dev-libs/newpkg >/var/tmp/b453/out.txt", r), r)
print("stdout file:", repr(open("/var/tmp/b453/out.txt").read()[-400:]))
show("tty colour default (NOCOLOR unset)", tty("color-tty", pm, P, r, color=True), r)
show("tty --color=n", tty("colorn-tty", pm, ["--color=n", *P], r, color=True), r)
show("tty NO_COLOR=1", tty("nocolorenv-tty", pm, P, r, color=True, extra={"NO_COLOR": "1"}), r)
show("pipe colour default (NOCOLOR unset)", pipe("color-pipe", pm, P, r, color=True), r)
show("pipe --color=y", pipe("colory-pipe", pm, ["--color=y", *P], r, color=True), r)
show("pipe --color=y NOCOLOR=true", pipe("colory-nocolor-pipe", pm, ["--color=y", *P], r), r)
