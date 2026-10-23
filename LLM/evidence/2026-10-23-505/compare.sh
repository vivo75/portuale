#!/bin/bash
# Phase-environment oracle for backlog #505 (see README.md next to it).
# Needs /var/tmp/ev505/{cfg,repo,oldbin,loc} set up as in README.md.
W=/var/tmp/ev505; D=$(dirname "$0")
for k in portage portuale portuale-old; do bash "$D/run-env-probe.sh" $k $k >/dev/null 2>&1; done
echo "== Portage 3.0.83-r1 vs portuale (S6): lines differing / total"
diff $W/portage.env $W/portuale.env | grep -c '^[<>]'
echo "== portuale before S6 vs after S6 (the effect of the re-sync)"
diff $W/portuale-old.env $W/portuale.env
echo "== USERLAND / LOCPATH / QEMU_* in the three saved environments"
grep -H "USERLAND\|LOCPATH\|QEMU_" $W/portage.env $W/portuale.env $W/portuale-old.env
