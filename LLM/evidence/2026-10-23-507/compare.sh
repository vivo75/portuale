#!/bin/bash
# Compare portage vs portuale captures in <dir> for --circular-deps-report=json:
# rc, stderr byte for byte, stdout with the wall-clock seconds of the
# "Dependency resolution took" line blanked (portuale cuts them). The keys of
# each "use_changes" object come out in hash order from Portage (Q6), so they
# are sorted before comparing; nothing else is normalised.
d=$1
norm() { python3 -I -c '
import re,sys,json
t=open(sys.argv[1]).read()
# wall-clock seconds are cut on the portuale side (determinism convention)
t=re.sub(r"took (\x1b\[[0-9;]*m)?[0-9.]*(\x1b\[[0-9;]*m)?( s)? ?\(backtrack", "took <T> (backtrack", t)
i=t.find("{\n")
if i<0 or "\"cycles\"" not in t: print(t,end=""); sys.exit()
j=json.loads(t[i:])
for s in j["solutions"]: s["use_changes"]=[dict(sorted(u.items())) for u in s["use_changes"]]
print(t[:i]+json.dumps(j,indent=2)+"\n",end="")' "$1"; }
for f in "$d"/*.portage.rc; do id=$(basename ${f%.portage.rc})
  a=$(cat $d/$id.portage.rc); b=$(cat $d/$id.portuale.rc 2>/dev/null)
  if diff -q <(norm $d/$id.portage.out) <(norm $d/$id.portuale.out) >/dev/null; then so=same; else so=DIFF; fi
  if cmp -s $d/$id.portage.err $d/$id.portuale.err; then se=same; else se=DIFF; fi
  printf '%-18s rc %s/%s  stdout %-4s stderr %s\n' $id $a $b $so $se
done
