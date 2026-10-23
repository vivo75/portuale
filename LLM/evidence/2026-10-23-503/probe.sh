#!/bin/bash
# Records argv acceptance for the three S4 options on Portage (host emerge)
# and portuale. Usage: probe.sh [portuale-binary]   (casts + .txt go next to this script)
cd "$(dirname "$0")"
PT=${1:-../../../target/release/portuale}
i=0
while IFS= read -r args; do
  i=$((i+1)); n=$(printf '%02d' $i)
  for pm in portage portuale; do
    if [ $pm = portage ]; then bin=emerge; else bin="$PT emerge"; fi
    cmd="$bin $args; echo rc=\$?"
    asciinema rec --headless --overwrite -q -c "bash -c '$cmd' 2>&1" $n-$pm.cast >/dev/null 2>&1
    { echo "\$ $bin $args"; bash -c "$cmd" 2>&1; } > $n-$pm.txt
  done
done <<'ARGS'
-p app-misc/nothing-here --binpkg-respect-user-patches
-p app-misc/nothing-here --binpkg-respect-user-patches=y
-p app-misc/nothing-here --binpkg-respect-user-patches=n
-p app-misc/nothing-here --binpkg-respect-user-patches=true
-p app-misc/nothing-here --binpkg-respect-user-patches=maybe
-p app-misc/nothing-here --binpkg-respect-user-patches n
-p --binpkg-respect-user-patches y app-misc/nothing-here
-p app-misc/nothing-here --circular-deps-report
-p app-misc/nothing-here --circular-deps-report=text
-p app-misc/nothing-here --circular-deps-report=json
-p app-misc/nothing-here --circular-deps-report=xml
-p app-misc/nothing-here --circular-deps-report json
-p app-misc/nothing-here --circular-deps-report=
-p app-misc/nothing-here --merge-wait-scope=deep
-p app-misc/nothing-here --merge-wait-scope=system
-p app-misc/nothing-here --merge-wait-scope=toolchain
-p app-misc/nothing-here --merge-wait-scope=none
-p app-misc/nothing-here --merge-wait-scope=all
-p app-misc/nothing-here --merge-wait-scope
-p app-misc/nothing-here --merge-wait-scope none
-p app-misc/nothing-here --cycle-break
-p app-misc/nothing-here --cycle-break=y
-p app-misc/nothing-here --binpkg-respect-user-patches=y --circular-deps-report=json --merge-wait-scope=none
--help
ARGS
