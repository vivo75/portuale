#!/bin/bash
# Last stderr line and rc of Portage vs portuale for each argv (backlog #405).
PL=/home/vivo/repo/PORTUALE/portuale/target/release/portuale
bad=0
while read -r line; do
  [ -z "$line" ] && continue
  pout=$(emerge $line 2>&1 >/dev/null </dev/null; echo "rc=$?"); prc=${pout##*rc=}; p=$(printf '%s' "${pout%rc=*}" | grep -v '^$' | tail -1)
  qout=$($PL emerge $line 2>&1 >/dev/null </dev/null; echo "rc=$?"); qrc=${qout##*rc=}; q=$(printf '%s' "${qout%rc=*}" | grep -v '^$' | tail -1)
  if [ "$p" = "$q" ] && [ "$prc" = "$qrc" ]; then echo "OK   $line  rc=$prc  $p"; else echo "DIFF $line  portage(rc=$prc): $p  |  portuale(rc=$qrc): $q"; bad=1; fi
done <<'ARGS'
--color
--exclude
-X
-pX
-vX
--reinstall-atoms
--useoldpkg-atoms
--rebuild-exclude
--rebuild-ignore
--usepkg-exclude
--usepkg-include
--getbinpkg-exclude
--getbinpkg-include
--buildpkg-exclude
--with-bdeps
--with-bdeps-auto
--autounmask-backtrack
--autounmask-use
--autounmask-license
--rebuilt-binaries-timestamp
--quickpkg-direct
--quickpkg-direct-root
-jx
-j2x
-jx2
--jobs=x
--load-average=x
-l2x
-lx
-zp
-pzv
-pz
ARGS
exit $bad
