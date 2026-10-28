#!/bin/bash
# runall.sh: every R6 S0 cell, Portage vs portuale; prints count + per-pass sequences.
S=/var/tmp/claude-r6; P=$S/tools/pair.sh
run() { local n=$1; $P "$@" > /dev/null
  bt() { grep -ho 'backtrack: [0-9]*/[0-9]*' "$@" | head -1 | sed 's/backtrack: //'; }
  printf '%-28s | Portage %-6s rc %s | %-40s | portuale %-6s rc %s | %s\n' "$n" "$(bt $S/captures/$n.portage.{out,err})" "$(cat $S/captures/$n.portage.rc)" \
    "$(python3 $S/tools/seq.py $S/captures/$n.portage.err)" "$(bt $S/captures/$n.portuale.{out,err})" "$(cat $S/captures/$n.portuale.rc)" "$(python3 $S/tools/pseq.py $S/captures/$n.portuale.err)"; }
if [ "${1:-}" = aub ]; then
for b in y n; do for o in "c b a" "c a b" "b c a" "b a c" "a c b" "a b c" "b a" "a b"; do
  args=(); for x in $o; do args+=(dev-libs/aub0$x); done; run aub-$b-${o// /} fx --autounmask-backtrack=$b "${args[@]}"; done; done
fi
for a in slotconflictunsolvable slotconfgroup scuseparent slotusegroup ormisstop ormissbad abort-masked-mid abort-unsat-mid abort-masked-last abort-masked-cycle r594etop r594gtop r594qtop r594ctop r594ftop r594jtop p569pull p563pull p589apull p589bpull p589cpull t8x needer; do run fx-$a fx dev-libs/$a; done
run fx-g0 fx dev-libs/p569pull dev-libs/abort-masked-mid
run fx-g0-bt1 fx --backtrack=1 dev-libs/p569pull dev-libs/abort-masked-mid
run fx-c-aun fx --autounmask=n dev-libs/r594ctop
run fx-q-bt1 fx --backtrack=1 dev-libs/r594qtop
run fx-p569pull_t8x fx dev-libs/p569pull dev-libs/t8x
run fx-triangle fx dev-libs/needer dev-libs/othermod
run fx-triangle-rev fx dev-libs/othermod dev-libs/needer
run fx-paired-2 fx =dev-libs/paired-2.0
run keeper-arg keeper =dev-libs/paired-2.0
run keeper-hard keeper dev-libs/needer
run keeper-triangle keeper dev-libs/needer dev-libs/othermod
run igw-ge igw '>=dev-libs/igw0x-2'
run igw-lt igw '<dev-libs/igw0x-1'
run cgpver-ge cgpver '>=dev-libs/cgp1x-2'
run cgpver-lt cgpver '<dev-libs/cgp1x-1'
