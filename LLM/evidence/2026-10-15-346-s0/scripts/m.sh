#!/bin/bash
# M family (351-MARK/DIE, S6): stream split (stdout vs stderr carry), RESTRICT=test,
# FEATURES=-test, die block + build.log + ${T} listing, PORT_LOGDIR, and the
# emerge foreground (--jobs=1/2, --quiet-build=y). Fresh builddir per cell.
# Same hermetic env as S (exported TMPDIR/DISTDIR/FEATURES + make.conf mirror).
set -u
H=$(cd "$(dirname "$0")" && pwd); . "$H/lib.sh"

PH8=$FIX/repo-348/app-misc/t348-b-phases/t348-b-phases-8.ebuild
PHRT8=$FIX/repo-348/app-misc/t348-b-phasert/t348-b-phasert-8.ebuild
ENOA=$FIX/repo-348/app-misc/t348-b-envnoeapply/t348-b-envnoeapply-7.ebuild
FBASE="-sandbox -usersandbox -userpriv -network-sandbox -ipc-sandbox -pid-sandbox -news"

M_EBUILD() { # <cell> <pm> <features> <ebuild> <phase...>
    local cell=$1
    local pm=$2
    local feat=$3
    local eb=$4
    shift 4
    local base=$S346/m-$cell-$pm
    local cfg; cfg=$(mkcfg "m-$cell-$pm")
    printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\n' \
        "$base/tmp" "$base/dist" "$feat" >> "$cfg/etc/portage/make.conf"
    if [ "$pm" = portage ]; then
        run_cell m "$cell" "$pm" env \
            PORTAGE_CONFIGROOT="$cfg" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$feat" \
            ebuild "$eb" "$@"
    else
        run_cell m "$cell" "$pm" env \
            PORTAGE_CONFIGROOT="$cfg" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$feat" \
            "$PEB" "$eb" "$@"
    fi
    echo "$base" > "$EVDIR/m/$cell-$pm.basepath"
}
case "${1:-all}" in
  all)
    M_EBUILD m01-unpack portage "$FBASE" "$PH8" unpack
    M_EBUILD m01-unpack portuale "$FBASE" "$PH8" unpack
    # fresh `install` chain without test in FEATURES (-> [not enabled] on Portage).
    M_EBUILD m02-fresh-install portage "$FBASE" "$PH8" install
    M_EBUILD m02-fresh-install portuale "$FBASE" "$PH8" install
    # RESTRICT=test package, test phase, FEATURES=test (-> runs).
    M_EBUILD m03-phasert-test portage "$FBASE test" "$PHRT8" test
    M_EBUILD m03-phasert-test portuale "$FBASE test" "$PHRT8" test
    # RESTRICT=test package, no test feature (-> [not enabled] or [disabled]?).
    M_EBUILD m04-phasert-notest portage "$FBASE" "$PHRT8" test
    M_EBUILD m04-phasert-notest portuale "$FBASE" "$PHRT8" test
    # no-RESTRICT package, FEATURES=-test (-> forced? or disabled?).
    M_EBUILD m05-phases-minus-test portage "$FBASE -test" "$PH8" test
    M_EBUILD m05-phases-minus-test portuale "$FBASE -test" "$PH8" test
    # die block + ${T} listing + build.log presence (streams already split by harness).
    M_EBUILD m06-die portage "$FBASE test" "$ENOA" prepare
    M_EBUILD m06-die portuale "$FBASE test" "$ENOA" prepare
    for pm in portage portuale; do
        base=$(cat "$EVDIR/m/m06-die-$pm.basepath")
        (cd "$base/tmp" && find . -maxdepth 6 | sort) > "$EVDIR/m/m06-die-$pm.tmpfind" 2>&1
        find "$base/tmp" -name build.log | sort > "$EVDIR/m/m06-die-$pm.buildlogs" 2>&1
    done
    # PORT_LOGDIR (G4): log under PORT_LOGDIR + the die convenience-symlink line.
    for pm in portage portuale; do
        cell=m07-port-logdir
        base=$S346/m-$cell-$pm
        cfg=$(mkcfg "m-$cell-$pm")
        printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s test"\n' \
            "$base/tmp" "$base/dist" "$FBASE" >> "$cfg/etc/portage/make.conf"
        mkdir -p "$base/plogs"
        if [ "$pm" = portage ]; then
            run_cell m "$cell" "$pm" env \
                PORTAGE_CONFIGROOT="$cfg" PORTAGE_TMPDIR="$base/tmp" \
                DISTDIR="$base/dist" FEATURES="$FBASE test" PORT_LOGDIR="$base/plogs" \
                ebuild "$ENOA" prepare
        else
            run_cell m "$cell" "$pm" env \
                PORTAGE_CONFIGROOT="$cfg" PORTAGE_TMPDIR="$base/tmp" \
                DISTDIR="$base/dist" FEATURES="$FBASE test" PORT_LOGDIR="$base/plogs" \
                "$PEB" "$ENOA" prepare
        fi
        (cd "$base/plogs" && find . | sort) > "$EVDIR/m/$cell-$pm.plogfind" 2>&1
        echo "$base" > "$EVDIR/m/$cell-$pm.basepath"
    done
    # emerge foreground: real --oneshot merges into a scratch ROOT.
    for pm in portage portuale; do
        for j in 1 2; do
            cell=m08-emerge-jobs$j
            base=$S346/m-$cell-$pm
            cfg=$(mkcfg "m-$cell-$pm")
            printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\n' \
                "$base/tmp" "$base/dist" "$FBASE" >> "$cfg/etc/portage/make.conf"
            if [ "$pm" = portage ]; then
                run_cell m "$cell" "$pm" env \
                    PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
                    DISTDIR="$base/dist" FEATURES="$FBASE" \
                    emerge --oneshot --package-moves=n --jobs=$j app-misc/t348-b-phases
            else
                run_cell m "$cell" "$pm" env \
                    PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
                    DISTDIR="$base/dist" FEATURES="$FBASE" \
                    "$PEM" --oneshot --package-moves=n --jobs=$j app-misc/t348-b-phases
            fi
            echo "$base" > "$EVDIR/m/$cell-$pm.basepath"
        done
        cell=m09-emerge-quietbuild
        base=$S346/m-$cell-$pm
        cfg=$(mkcfg "m-$cell-$pm")
        printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\n' \
            "$base/tmp" "$base/dist" "$FBASE" >> "$cfg/etc/portage/make.conf"
        if [ "$pm" = portage ]; then
            run_cell m "$cell" "$pm" env \
                PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
                DISTDIR="$base/dist" FEATURES="$FBASE" \
                emerge --oneshot --package-moves=n --quiet-build=y app-misc/t348-b-phases
        else
            run_cell m "$cell" "$pm" env \
                PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
                DISTDIR="$base/dist" FEATURES="$FBASE" \
                "$PEM" --oneshot --package-moves=n --quiet-build=y app-misc/t348-b-phases
        fi
        echo "$base" > "$EVDIR/m/$cell-$pm.basepath"
    done
    ;;

esac
