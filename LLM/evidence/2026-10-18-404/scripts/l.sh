#!/bin/bash
# CL re-capture for backlog #404 (02.346 S11: auto-clean, 351-CLEAN).
# Cells: the S0 cl01/cl01c/cl02 shapes plus the scope inputs the brief
# demands (AUTOCLEAN unset, =no, a different-slot instance that must stay
# protected, --pretend, a failed merge). No CLEAN_DELAY anywhere (lib.sh):
# the 5-second default applies, so any countdown line is a finding.
# Evidence: LLM/evidence/2026-10-18-404/l/.
set -u
H=$(cd "$(dirname "$0")" && pwd); . "$H/lib.sh"

PH7=$FIX/repo-348/app-misc/t348-b-phases/t348-b-phases-7.ebuild
PH8=$FIX/repo-348/app-misc/t348-b-phases/t348-b-phases-8.ebuild
PLIM=$FIX/repo-348/app-misc/t348-b-phaseslim/t348-b-phaseslim-8.ebuild
FBASE="-sandbox -usersandbox -userpriv -network-sandbox -ipc-sandbox -pid-sandbox -news"

L_EMERGE() { # <cell> <pm> <autoclean:yes|no|unset> <emerge args...>
    local cell=$1
    local pm=$2
    local ac=$3
    shift 3
    local base=$S404/l-$cell-$pm
    local cfg; cfg=$(mkcfg "l-$cell-$pm")
    {
        printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\n' \
            "$base/tmp" "$base/dist" "$FBASE"
        [ "$ac" = yes ] && printf 'AUTOCLEAN="yes"\n'
        [ "$ac" = no ] && printf 'AUTOCLEAN="no"\n'
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

# cl01: AUTOCLEAN=yes + outdated same-slot copy (7 installed, then emerge 8
# which upgrades 7->8; the replace consumes 7 so autoclean finds nothing).
cl01() {
    for pm in portage portuale; do
        local base=$S404/l-cl01-autoclean-$pm
        local cfg; cfg=$(mkcfg "l-cl01-autoclean-$pm")
        printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\nAUTOCLEAN="yes"\n' \
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

# cl01c: AUTOCLEAN=yes with a reconstructed leftover (7's vdb entry saved
# before the 8 upgrade replaced it, then restored AFTER all merges:
# same CP+slot, older counter — the shape an interrupted replace leaves
# behind). The final merge is an UNRELATED package (phaseslim), so its
# replace step cannot consume the leftover; only the post-merge auto-clean
# can. World holds phases (clean needs a world atom to consider a package).
cl01c() {
    for pm in portage portuale; do
        local base=$S404/l-cl01c-autoclean-leftover-$pm
        local cfg; cfg=$(mkcfg "l-cl01c-autoclean-leftover-$pm")
        printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\nAUTOCLEAN="yes"\n' \
            "$base/tmp" "$base/dist" "$FBASE" >> "$cfg/etc/portage/make.conf"
        if [ "$pm" = portage ]; then EBC=ebuild; EME=emerge; else EBC=$PEB; EME=$PEM; fi
        env PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            $EBC "$PH7" merge >/dev/null 2>&1
        cp -a "$base/root/var/db/pkg/app-misc/t348-b-phases-7" "$base/saved7"
        env PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            $EBC "$PH8" merge >/dev/null 2>&1
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

# cl03: AUTOCLEAN unset (no line in make.conf), fresh merge: no block.
L_EMERGE cl03-autoclean-unset portage unset --oneshot --package-moves=n app-misc/t348-b-phases
L_EMERGE cl03-autoclean-unset portuale unset --oneshot --package-moves=n app-misc/t348-b-phases

# cl04: AUTOCLEAN=no explicitly, fresh merge: no block.
L_EMERGE cl04-autoclean-no portage no --oneshot --package-moves=n app-misc/t348-b-phases
L_EMERGE cl04-autoclean-no portuale no --oneshot --package-moves=n app-misc/t348-b-phases

# cl05: AUTOCLEAN=yes, leftover 7 restored with SLOT edited to "1"
# (synthetic: same cp, different slot). Both slots hold a single
# instance, so each is protected and nothing is outdated.
cl05() {
    for pm in portage portuale; do
        local base=$S404/l-cl05-autoclean-slot-$pm
        local cfg; cfg=$(mkcfg "l-cl05-autoclean-slot-$pm")
        printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\nAUTOCLEAN="yes"\n' \
            "$base/tmp" "$base/dist" "$FBASE" >> "$cfg/etc/portage/make.conf"
        if [ "$pm" = portage ]; then EBC=ebuild; EME=emerge; else EBC=$PEB; EME=$PEM; fi
        env PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            $EBC "$PH7" merge >/dev/null 2>&1
        cp -a "$base/root/var/db/pkg/app-misc/t348-b-phases-7" "$base/saved7"
        env PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            $EBC "$PH8" merge >/dev/null 2>&1
        cp -a "$base/saved7" "$base/root/var/db/pkg/app-misc/t348-b-phases-7"
        printf '1\n' > "$base/root/var/db/pkg/app-misc/t348-b-phases-7/SLOT"
        # Portage consolidates vdb fields into <entry>/metadata and serves
        # it while its stamp is valid (same-mtime-second edits stay
        # invisible); drop it so the edited SLOT is actually read.
        rm -f "$base/root/var/db/pkg/app-misc/t348-b-phases-7/metadata"
        mkdir -p "$base/root/var/lib/portage"
        printf 'app-misc/t348-b-phases\napp-misc/t348-b-phaseslim\n' > "$base/root/var/lib/portage/world"
        run_cell l cl05-autoclean-slot "$pm" env \
            PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            $EME --oneshot --package-moves=n app-misc/t348-b-phaseslim
        (cd "$base/root/var/db/pkg" && find . -maxdepth 2 | sort) > "$EVDIR/l/cl05-autoclean-slot-$pm.vdbfind" 2>&1
        echo "$base" > "$EVDIR/l/cl05-autoclean-slot-$pm.basepath"
    done
}
cl05

# cl06: AUTOCLEAN=yes + --pretend: no block (Portage gates on pretend).
L_EMERGE cl06-autoclean-pretend portage yes --pretend --oneshot --package-moves=n app-misc/t348-b-phases
L_EMERGE cl06-autoclean-pretend portuale yes --pretend --oneshot --package-moves=n app-misc/t348-b-phases

# cl07: AUTOCLEAN=yes + a FAILED merge (die in src_install, scratch repo
# with a real md5-cache entry like the failpkg pmtest pin): no block,
# rc 1 on both sides.
cl07() {
    for pm in portage portuale; do
        local base=$S404/l-cl07-autoclean-fail-$pm
        local cfg; cfg=$(mkcfg "l-cl07-autoclean-fail-$pm")
        cat >> "$cfg/etc/portage/repos.conf" <<EOF
[s404]
location = $base/repo
priority = 70
EOF
        mkdir -p "$base/repo/app-misc/s404-fail" "$base/repo/profiles" "$base/repo/metadata/md5-cache/app-misc"
        printf 's404\n' > "$base/repo/profiles/repo_name"
        printf 'app-misc\n' > "$base/repo/profiles/categories"
        printf 'masters = testrepo\nthin-manifests = true\n' > "$base/repo/metadata/layout.conf"
        printf 'EAPI=8\nDESCRIPTION="s404 failing probe"\nSLOT="0"\nKEYWORDS="amd64"\nsrc_install() {\n\tdie "boom"\n}\n' \
            > "$base/repo/app-misc/s404-fail/s404-fail-1.0.ebuild"
        md5=$(md5sum "$base/repo/app-misc/s404-fail/s404-fail-1.0.ebuild" | cut -d' ' -f1)
        printf 'DEFINED_PHASES=install\nDESCRIPTION=s404 failing probe\nEAPI=8\nKEYWORDS=amd64\nSLOT=0\n_md5_=%s\n' "$md5" \
            > "$base/repo/metadata/md5-cache/app-misc/s404-fail-1.0"
        printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\nAUTOCLEAN="yes"\n' \
            "$base/tmp" "$base/dist" "$FBASE" >> "$cfg/etc/portage/make.conf"
        if [ "$pm" = portage ]; then EME=emerge; else EME=$PEM; fi
        run_cell l cl07-autoclean-fail "$pm" env \
            PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            $EME --oneshot --package-moves=n app-misc/s404-fail
        echo "$base" > "$EVDIR/l/cl07-autoclean-fail-$pm.basepath"
    done
}
cl07
