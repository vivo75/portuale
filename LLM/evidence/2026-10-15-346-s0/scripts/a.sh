#!/bin/bash
# A family (351-ABORT, S7): masked-abort resolution notices — `emerge --oneshot`,
# `-p`, `-pv`, `--ask=n -v` on (a) top-level all-masked, (b) masked dependency,
# (c) unsatisfied dependency. stdout and stderr captured separately by the harness.
set -u
H=$(cd "$(dirname "$0")" && pwd); . "$H/lib.sh"

A() { # <cell> <target> [extra args...] (A_MASK exported by caller masks the dep)
    local cell=$1
    local target=$2
    shift 2
    for pm in portage portuale; do
        local cfg; cfg=$(mkcfg "a-$cell-$pm")
        if [ -n "${A_MASK:-}" ]; then
            echo "$A_MASK" > "$cfg/etc/portage/package.mask"
        fi
        if [ "$pm" = portage ]; then
            run_cell a "$cell" "$pm" env PORTAGE_CONFIGROOT="$cfg" ROOT="$cfg" \
                emerge "$@" --package-moves=n "$target"
        else
            run_cell a "$cell" "$pm" env PORTAGE_CONFIGROOT="$cfg" ROOT="$cfg" \
                "$PEM" "$@" --package-moves=n "$target"
        fi
    done
}

A a01-masked-top-oneshot dev-libs/bareacceptkeywordspkg --oneshot
A a02-masked-top-p dev-libs/bareacceptkeywordspkg -p
A a03-masked-top-pv dev-libs/bareacceptkeywordspkg -pv
A a04-masked-top-ask dev-libs/bareacceptkeywordspkg --ask=n -v
A_MASK=dev-libs/maskeddep A a05-maskeddep-oneshot dev-libs/abort-masked-last --oneshot
A_MASK=dev-libs/maskeddep A a06-maskeddep-p dev-libs/abort-masked-last -p
A_MASK=dev-libs/maskeddep A a07-maskeddep-pv dev-libs/abort-masked-last -pv
A_MASK=dev-libs/maskeddep A a08-maskeddep-ask dev-libs/abort-masked-last --ask=n -v
A a09-unsat-oneshot dev-libs/abort-unsat-last --oneshot
A a10-unsat-p dev-libs/abort-unsat-last -p
A a11-unsat-pv dev-libs/abort-unsat-last -pv
A a12-unsat-ask dev-libs/abort-unsat-last --ask=n -v
