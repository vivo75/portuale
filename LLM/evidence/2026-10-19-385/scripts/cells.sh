#!/bin/bash
# cells.sh [pm] [cell ...]: #385 oracle cells, each run for Portage and portuale.
# Cell tag: <shape><eapi><feat>[-pg|-pl], shape in {a,b,c,d}, eapi in {7,8},
# feat n (no strict-keepdir) | s (FEATURES=strict-keepdir, via make.conf append).
# Every cell also carries noclean so the post-QA image survives for find $D
# (Portage's merge tail would otherwise run the clean phase; `clean` is not
# implemented by `portuale ebuild`, so without noclean the two sides would
# differ in image survival for a reason unrelated to #385).
# Usage: ./cells.sh              # everything, both PMs
#        ./cells.sh portage a7n   # subset for one PM
set -u
S=/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-19-385/scripts
B=/var/tmp/s385
SHAPES="a b c d"
EAPIS="7 8"
FEATS="n s"

docell() { # $1=celltag $2=pm-name
    local cell=$1 pm=$2 shape=${1:0:1} eapi=${1:1:1} feat=${1:2:1}
    local ebf=$B/repo/app-misc/t385-$shape/t385-$shape-$eapi.ebuild
    local extra=noclean
    [[ $feat == s ]] && extra="noclean strict-keepdir"
    local tag=$cell-$([[ $pm == portuale ]] && echo pl || echo pg)
    echo "=== cell $cell PM=$pm ==="
    PM=$pm "$S/run.sh" "$tag" "$ebf" "$pm" "$extra"
}

if [[ $# -ge 1 ]]; then PMS="$1"; shift; else PMS="portage portuale"; fi
if [[ $# -ge 1 ]]; then WANT="$*"; else
    WANT=""
    for s in $SHAPES; do for e in $EAPIS; do for f in $FEATS; do WANT="$WANT $s$e$f"; done; done; done
fi
for pm in $PMS; do
    for c in $WANT; do
        docell "$c" "$pm"
    done
done
