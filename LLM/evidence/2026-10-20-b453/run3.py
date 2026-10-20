import sys, re; sys.path.insert(0, "/tmp/claude-1000/-home-vivo-repo-PORTUALE-portuale/31e72e06-5910-492b-8d0e-7bb237e53e32/scratchpad")
from probe import *
def gu(label, res, root):
    argv, rc, data, n = res
    data = data.replace(str(root), "$ROOT")
    lines = [l.strip("\r") for l in data.split("\n") if "updates/" in l or "Global Updates" in l]
    print(f"=== {label}\nargv: {argv}\nrc={rc}\n" + "\n".join(lines[:4]) + (f"\n... ({len(lines)} update lines)" if len(lines) > 4 else "") + "\n")
P = ["--pretend", "dev-libs/newpkg"]
# A: cast_cells.py warm-up (--pretend --version) then recorded run
r = stage("gu-a", warm=False)
p = subprocess.run([PMS["portage"], "--pretend", "--version"], env=env_for(r), capture_output=True, text=True)
print("warm-up --pretend --version stdout:", repr(p.stdout[:120]))
gu("A after --version warm-up, tty", tty("gu-a1", "portage", P, r), r)
gu("A second run, tty", tty("gu-a2", "portage", P, r), r)
gu("A third run, pipe", pipe("gu-a3", "portage", P, r), r)
gu("A --package-moves=n, tty", tty("gu-a4", "portage", ["--package-moves=n", *P], r), r)
# B: fresh stage, --package-moves=n first
r = stage("gu-b", warm=False)
gu("B fresh, --package-moves=n, tty", tty("gu-b1", "portage", ["--package-moves=n", *P], r), r)
# C: fresh stage, fixture updates comment line removed (stage copy only)
r = stage("gu-c", warm=False)
f = r / "repo/profiles/updates/2Q-2024"; f.write_text("".join(l for l in f.read_text().splitlines(True) if not l.startswith("#")))
gu("C comment stripped, run 1", pipe("gu-c1", "portage", P, r), r)
gu("C comment stripped, run 2", pipe("gu-c2", "portage", P, r), r)
# portuale fresh
r = stage("gu-d", warm=False, pm="portuale")
gu("D portuale fresh, tty", tty("gu-d1", "portuale", P, r), r)
# portuale colour discriminators
r = stage("pc", pm="portuale")
show("portuale tty masked colour", tty("masked-color-tty", "portuale", ["--pretend", "dev-libs/maskedpkg"], r, color=True), r)
show("portuale tty TERM=dumb colour", tty("dumb-color-tty", "portuale", P, r, color=True, extra={"TERM": "dumb"}), r)
r = stage("pc2")
show("portage tty TERM=dumb colour", tty("dumb-color-tty", "portage", P, r, color=True, extra={"TERM": "dumb"}), r)
