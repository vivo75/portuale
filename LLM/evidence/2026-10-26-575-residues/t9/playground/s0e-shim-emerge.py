#!/usr/bin/env python3
import os, sys, subprocess, json, re
REAL = "/home/vivo/repo/PORTUALE/portuale/target/release/portuale"
LOG = "/var/tmp/claude-s0e/bt.log"
env = dict(os.environ); env["PORTUALE_BT_TRACE"] = "1"
kind = os.path.basename(sys.argv[0])
r = subprocess.run([kind, *sys.argv[1:]], executable=REAL, env=env, stdin=sys.stdin,
                   capture_output=True)
err = r.stderr.decode("utf-8", "replace")
bt = [l for l in err.split("\n") if l.startswith("BT ")]
keep = "\n".join(l for l in err.split("\n") if not l.startswith("BT "))
out = r.stdout.decode("utf-8", "replace")
m = re.search(r"backtrack: (\d+)/(\d+)", out + err)
heal = any(bt[i].startswith("BT try=") and "kind=Feedback:MissingDep" in bt[i] and
           any(x.startswith("BT   mask") and "SlotConflict" in x for x in bt[i+1:i+8] if not x.startswith("BT try="))
           for i in range(len(bt)))
tries = sum(1 for l in bt if l.startswith("BT try="))
rec = {"test": os.environ.get("PYTEST_CURRENT_TEST"), "kind": kind, "argv": sys.argv[1:], "rc": r.returncode,
       "env": {k: v for k, v in os.environ.items() if k in ("PORTAGE_CONFIGROOT","ROOT","PORTAGE_RUNNING_ROOT","DISTDIR","EMERGE_DEFAULT_OPTS","FEATURES","USE","PORTAGE_TMPDIR","PKGDIR","PORTAGE_BINHOST","ACCEPT_KEYWORDS")},
       "cwd": os.getcwd(), "heal": heal, "tries": tries, "bt_line": m.group(0) if m else None,
       "resid": ("Multiple package instances" in out and "installed in '" in out), "conflict": any(l.startswith("BT   conflict") for l in bt), "bt": bt if (heal or any(l.startswith("BT   conflict") or "missing_dep_trigger" in l for l in bt)) else None}
try:
    with open(LOG, "a") as f:
        f.write(json.dumps(rec) + "\n")
except Exception:
    pass
sys.stdout.buffer.write(r.stdout)
sys.stderr.write(keep); sys.stderr.flush()
sys.exit(r.returncode)
