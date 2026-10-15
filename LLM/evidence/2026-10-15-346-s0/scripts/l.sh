#!/bin/bash
# L + CL families (owner-gated S10/S11, captured now so the owner decides on
# evidence): merge/unmerge file listing on every merge path, and auto-clean
# tied to a user-set AUTOCLEAN=yes. t348 packages (thin-manifests repo, so
# Portage's manifest verification passes; the fixture main repo has no
# Manifests and Portage refuses real merges there — see a05 note in matrix.md).
# l00: the dedicated no---package-moves cell (351-UPD coupling).
set -u
H=$(cd "$(dirname "$0")" && pwd); . "$H/lib.sh"

PH7=$FIX/repo-348/app-misc/t348-b-phases/t348-b-phases-7.ebuild
FBASE="-sandbox -usersandbox -userpriv -network-sandbox -ipc-sandbox -pid-sandbox -news"

L_EBUILD() { # <cell> <pm> <ebuild> <phase...> (direct path, scratch ROOT for merge)
    local cell=$1
    local pm=$2
    local eb=$3
    shift 3
    local base=$S346/l-$cell-$pm
    local cfg; cfg=$(mkcfg "l-$cell-$pm")
    printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\nCLEAN_DELAY="0"\n' \
        "$base/tmp" "$base/dist" "$FBASE" >> "$cfg/etc/portage/make.conf"
    if [ "$pm" = portage ]; then
        run_cell l "$cell" "$pm" env \
            PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            ebuild "$eb" "$@"
    else
        run_cell l "$cell" "$pm" env \
            PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            "$PEB" "$eb" "$@"
    fi
    echo "$base" > "$EVDIR/l/$cell-$pm.basepath"
}

L_EMERGE() { # <cell> <pm> <autoclean:yes|no> <emerge args...>
    local cell=$1
    local pm=$2
    local ac=$3
    shift 3
    local base=$S346/l-$cell-$pm
    local cfg; cfg=$(mkcfg "l-$cell-$pm")
    {
        printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\nCLEAN_DELAY="0"\n' \
            "$base/tmp" "$base/dist" "$FBASE"
        [ "$ac" = yes ] && printf 'AUTOCLEAN="yes"\n'
    } >> "$cfg/etc/portage/make.conf"
    if [ "$pm" = portage ]; then
        run_cell l "$cell" "$pm" env \
            PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            emerge "$@"
    else
        run_cell l "$cell" "$pm" env \
            PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            "$PEM" "$@"
    fi
    echo "$base" > "$EVDIR/l/$cell-$pm.basepath"
}

# l00: no --package-moves (UPD coupling) on a real merge.
L_EMERGE l00-nomoves portage no --oneshot app-misc/t348-b-phases
L_EMERGE l00-nomoves portuale no --oneshot app-misc/t348-b-phases
# l01: direct `ebuild merge` on a fresh ROOT.
L_EBUILD l01-ebuild-merge portage "$PH7" merge
L_EBUILD l01-ebuild-merge portuale "$PH7" merge
# l02: emerge --oneshot new install.
L_EMERGE l02-emerge-new portage no --oneshot --package-moves=n app-misc/t348-b-phases
L_EMERGE l02-emerge-new portuale no --oneshot --package-moves=n app-misc/t348-b-phases
# l03: replace — install 7 via ebuild merge, then emerge --oneshot (same ROOT).
l03() {
    for pm in portage portuale; do
        local base=$S346/l-l03-replace-$pm
        local cfg; cfg=$(mkcfg "l-l03-replace-$pm")
        printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\nCLEAN_DELAY="0"\n' \
            "$base/tmp" "$base/dist" "$FBASE" >> "$cfg/etc/portage/make.conf"
        if [ "$pm" = portage ]; then EBC=ebuild; EME=emerge; else EBC=$PEB; EME=$PEM; fi
        env PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            $EBC "$PH7" merge >/dev/null 2>&1
        run_cell l l03-replace "$pm" env \
            PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            $EME --oneshot --package-moves=n app-misc/t348-b-phases
        echo "$base" > "$EVDIR/l/l03-replace-$pm.basepath"
    done
}
l03
# l04: emerge -C listing (install first, then unmerge).
l04() {
    for pm in portage portuale; do
        local base=$S346/l-l04-unmerge-$pm
        local cfg; cfg=$(mkcfg "l-l04-unmerge-$pm")
        printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\nCLEAN_DELAY="0"\n' \
            "$base/tmp" "$base/dist" "$FBASE" >> "$cfg/etc/portage/make.conf"
        if [ "$pm" = portage ]; then EME=emerge; else EME=$PEM; fi
        env PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            $EME --oneshot --package-moves=n app-misc/t348-b-phases >/dev/null 2>&1
        run_cell l l04-unmerge "$pm" env \
            PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            $EME -C --package-moves=n app-misc/t348-b-phases
        echo "$base" > "$EVDIR/l/l04-unmerge-$pm.basepath"
    done
}
l04
# cl01: AUTOCLEAN=yes in make.conf + outdated same-slot copy (7 installed,
# then emerge 8 which upgrades 7->8 and auto-cleans).
cl01() {
    for pm in portage portuale; do
        local base=$S346/l-cl01-autoclean-$pm
        local cfg; cfg=$(mkcfg "l-cl01-autoclean-$pm")
        printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\nCLEAN_DELAY="0"\nAUTOCLEAN="yes"\n' \
            "$base/tmp" "$base/dist" "$FBASE" >> "$cfg/etc/portage/make.conf"
        if [ "$pm" = portage ]; then EBC=ebuild; EME=emerge; else EBC=$PEB; EME=$PEM; fi
        env PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            $EBC "$PH7" merge >/dev/null 2>&1
        run_cell l cl01-autoclean-outdated "$pm" env \
            PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            $EME --oneshot --package-moves=n app-misc/t348-b-phases
        echo "$base" > "$EVDIR/l/cl01-autoclean-outdated-$pm.basepath"
    done
}
cl01
# cl02: AUTOCLEAN=yes, no outdated copy (fresh merge).
L_EMERGE cl02-autoclean-fresh portage yes --oneshot --package-moves=n app-misc/t348-b-phases
L_EMERGE cl02-autoclean-fresh portuale yes --oneshot --package-moves=n app-misc/t348-b-phases

# cl01b: AUTOCLEAN=yes with a genuinely outdated package in the vdb (phaseslim
# merged directly, in no world set and depended on by nothing), then a merge of
# an unrelated package triggers the clean unmerge arm.
cl01b() {
    for pm in portage portuale; do
        local base=$S346/l-cl01b-autoclean-unmerge-$pm
        local cfg; cfg=$(mkcfg "l-cl01b-autoclean-unmerge-$pm")
        printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\nCLEAN_DELAY="0"\nAUTOCLEAN="yes"\n' \
            "$base/tmp" "$base/dist" "$FBASE" >> "$cfg/etc/portage/make.conf"
        if [ "$pm" = portage ]; then EBC=ebuild; EME=emerge; else EBC=$PEB; EME=$PEM; fi
        env PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            $EBC "$FIX/repo-348/app-misc/t348-b-phaseslim/t348-b-phaseslim-8.ebuild" merge >/dev/null 2>&1
        run_cell l cl01b-autoclean-unmerge "$pm" env \
            PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            $EME --oneshot --package-moves=n app-misc/t348-b-phases
        echo "$base" > "$EVDIR/l/cl01b-autoclean-unmerge-$pm.basepath"
    done
}
cl01b

# cl01c: AUTOCLEAN=yes with a reconstructed leftover (7's vdb entry saved before
# the 8 upgrade replaced it, then restored AFTER all merges: same CP+slot, older
# counter — the shape an interrupted replace leaves behind). The final merge is
# an UNRELATED package (phaseslim), so its replace step cannot consume the
# leftover; only the post-merge auto-clean can. World holds phases (clean needs
# a world atom to consider a package). Restored CONTENTS is faithful, so the
# unmerge may also remove paths now shared with 8 — scratch root, documented.
cl01c() {
    for pm in portage portuale; do
        local base=$S346/l-cl01c-autoclean-leftover-$pm
        local cfg; cfg=$(mkcfg "l-cl01c-autoclean-leftover-$pm")
        printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\nCLEAN_DELAY="0"\nAUTOCLEAN="yes"\n' \
            "$base/tmp" "$base/dist" "$FBASE" >> "$cfg/etc/portage/make.conf"
        if [ "$pm" = portage ]; then EBC=ebuild; EME=emerge; else EBC=$PEB; EME=$PEM; fi
        env PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            $EBC "$FIX/repo-348/app-misc/t348-b-phases/t348-b-phases-7.ebuild" merge >/dev/null 2>&1
        cp -a "$base/root/var/db/pkg/app-misc/t348-b-phases-7" "$base/saved7"
        env PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            $EBC "$FIX/repo-348/app-misc/t348-b-phases/t348-b-phases-8.ebuild" merge >/dev/null 2>&1
        cp -a "$base/saved7" "$base/root/var/db/pkg/app-misc/t348-b-phases-7"
        mkdir -p "$base/root/var/lib/portage"
        printf 'app-misc/t348-b-phases\napp-misc/t348-b-phaseslim\n' > "$base/root/var/lib/portage/world"
        run_cell l cl01c-autoclean-leftover "$pm" env \
            PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            $EME --oneshot --package-moves=n app-misc/t348-b-phaseslim
        (cd "$base/root/var/db/pkg" && find . -maxdepth 2 | sort) > "$EVDIR/l/cl01c-autoclean-leftover-$pm.vdbfind" 2>&1
        echo "$base" > "$EVDIR/l/cl01c-autoclean-leftover-$pm.basepath"
    done
}
cl01c
