import sys; sys.path.insert(0, "/tmp/claude-1000/-home-vivo-repo-PORTUALE-portuale/31e72e06-5910-492b-8d0e-7bb237e53e32/scratchpad")
from probe import *
for pm in ("portage", "portuale"):
    r = stage(f"md-{pm}", pm=pm)
    A = ["--package-moves=n"] if pm == "portage" else []
    show(f"{pm} tty missingdep", tty("missingdep-tty", pm, [*A, "--pretend", "dev-libs/missingdep"], r, color=True), r)
    show(f"{pm} pipe missingdep", pipe("missingdep-pipe", pm, [*A, "--pretend", "dev-libs/missingdep"], r), r)
