#!/bin/bash
# C family (347-ERR, S8): every portuale error site from §0.2.3 — full stderr
# tail after the usage block (verbatim whole stderr), with PYTHONHASHSEED=0
# and once without (the usage-block order is hashseed-dependent; the error
# line itself must not be). rc 2 on both sides.
set -u
H=$(cd "$(dirname "$0")" && pwd); . "$H/lib.sh"

C() { # <cell> [emerge args...] (target appended by caller convention below)
    local cell=$1
    shift
    for pm in portage portuale; do
        local cfg; cfg=$(mkcfg "c-$cell-$pm-seed")
        if [ "$pm" = portage ]; then
            run_cell c "$cell-seed" "$pm" env PORTAGE_CONFIGROOT="$cfg" ROOT="$cfg" \
                emerge "$@"
            local cfg2; cfg2=$(mkcfg "c-$cell-$pm-noseed")
            run_cell c "$cell-noseed" "$pm" env -u PYTHONHASHSEED \
                PORTAGE_CONFIGROOT="$cfg2" ROOT="$cfg2" \
                emerge "$@"
        else
            run_cell c "$cell-seed" "$pm" env PORTAGE_CONFIGROOT="$cfg" ROOT="$cfg" \
                "$PEM" "$@"
            local cfg2; cfg2=$(mkcfg "c-$cell-$pm-noseed")
            run_cell c "$cell-noseed" "$pm" env -u PYTHONHASHSEED \
                PORTAGE_CONFIGROOT="$cfg2" ROOT="$cfg2" \
                "$PEM" "$@"
        fi
    done
}

TGT=dev-libs/t341-dep
C c01-jobs -p --package-moves=n --jobs=x $TGT
C c02-j-short -p --package-moves=n -j x $TGT
C c03-load-average -p --package-moves=n --load-average=x $TGT
C c04-deep -p --package-moves=n --deep=x $TGT
C c05-backtrack -p --package-moves=n --backtrack=x $TGT
C c06-color -p --package-moves=n --color=maybe $TGT
C c07-with-bdeps -p --package-moves=n --with-bdeps=x $TGT
C c08-with-bdeps-auto -p --package-moves=n --with-bdeps-auto=x $TGT
C c09-onlydeps-with-rdeps -p --package-moves=n --onlydeps-with-rdeps=x $TGT
C c10-onlydeps-with-ideps -p --package-moves=n --onlydeps-with-ideps=x $TGT
C c11-autounmask-backtrack -p --package-moves=n --autounmask-backtrack=x $TGT
C c12-frobnicate --frobnicate $TGT
C c13-exclude-badatom -p --package-moves=n --exclude '=a/b-1' $TGT
C c14-buildpkg-exclude-badatom -p --package-moves=n --buildpkg-exclude '!dev-libs/t341-dep' $TGT
