#!/bin/bash
# S family (351-STAMP, S5): stamp/skip lines, environment notice, WORKDIR lines,
# checksum line. Cumulative chain per PM (s01-s05 share one builddir, like the
# §0.4 probe); s06/s07/s08/s09 on fresh builddirs. Hermetic: PORTAGE_TMPDIR,
# DISTDIR, FEATURES exported (same values both sides) and mirrored in make.conf.
set -u
H=$(cd "$(dirname "$0")" && pwd); . "$H/lib.sh"

PH8=$FIX/repo-348/app-misc/t348-b-phases/t348-b-phases-8.ebuild
FBASE="-sandbox -usersandbox -userpriv -network-sandbox -ipc-sandbox -pid-sandbox -news"

S_RUN() { # <cell> <pm> <cfgbase-tag> <ebuild> <phase...> [with FEATURES-ov via S_FEAT]
    local cell=$1
    local pm=$2
    local tag=$3
    local eb=$4
    shift 4
    local base=$S346/s-$tag
    local cfg=$base/cfg
    if [ "$pm" = portage ]; then
        run_cell s "$cell" "$pm" env \
            PORTAGE_CONFIGROOT="$cfg" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="${S_FEAT:-$FBASE}" \
            ebuild "$eb" "$@"
    else
        run_cell s "$cell" "$pm" env \
            PORTAGE_CONFIGROOT="$cfg" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="${S_FEAT:-$FBASE}" \
            "$PEB" "$eb" "$@"
    fi
}

S_CHAIN() { # cumulative s01-s05 per PM on one scratch (clean builddir start)
    local pm=$1
    local base=$S346/s-chain-$pm
    local cfg; cfg=$(mkcfg "s-chain-$pm")
    printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\n' \
        "$base/tmp" "$base/dist" "$FBASE" >> "$cfg/etc/portage/make.conf"
    S_RUN s01-unpack "$pm" "chain-$pm" "$PH8" unpack
    S_RUN s02-unpack-again "$pm" "chain-$pm" "$PH8" unpack
    S_RUN s03-compile "$pm" "chain-$pm" "$PH8" compile
    S_RUN s04-test "$pm" "chain-$pm" "$PH8" test
    S_RUN s05-install "$pm" "chain-$pm" "$PH8" install
}

S_CHAIN portage
S_CHAIN portuale

# s06: full sequence, then FEATURES=noauto install (stamp skip incl. install?).
s06() {
    for pm in portage portuale; do
        local base=$S346/s-s06-$pm
        local cfg; cfg=$(mkcfg "s-s06-$pm")
        printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\n' \
            "$base/tmp" "$base/dist" "$FBASE" >> "$cfg/etc/portage/make.conf"
        if [ "$pm" = portage ]; then EBC=ebuild; else EBC=$PEB; fi
        env PORTAGE_CONFIGROOT="$cfg" PORTAGE_TMPDIR="$base/tmp" DISTDIR="$base/dist" \
            FEATURES="$FBASE" $EBC "$PH8" unpack >/dev/null 2>&1
        env PORTAGE_CONFIGROOT="$cfg" PORTAGE_TMPDIR="$base/tmp" DISTDIR="$base/dist" \
            FEATURES="$FBASE" $EBC "$PH8" compile >/dev/null 2>&1
        env PORTAGE_CONFIGROOT="$cfg" PORTAGE_TMPDIR="$base/tmp" DISTDIR="$base/dist" \
            FEATURES="$FBASE" $EBC "$PH8" install >/dev/null 2>&1
        S_FEAT="$FBASE noauto" S_RUN s06-noauto-install "$pm" "s06-$pm" "$PH8" install
        unset S_FEAT
    done
}
s06

# s07: `setup unpack` doubling (the #350 cut: pretend+setup re-run, recorded).
s07() {
    for pm in portage portuale; do
        local base=$S346/s-s07-$pm
        local cfg; cfg=$(mkcfg "s-s07-$pm")
        printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\n' \
            "$base/tmp" "$base/dist" "$FBASE" >> "$cfg/etc/portage/make.conf"
        S_RUN s07a-setup-unpack "$pm" "s07-$pm" "$PH8" setup unpack
        S_RUN s07b-setup-unpack-again "$pm" "s07-$pm" "$PH8" setup unpack
    done
}
s07

# s08: scratch overlay copy WITH Manifest (checksum line); s09: fixture copy
# WITHOUT Manifest. Same package, same path shape.
s08s09() {
    for pm in portage portuale; do
        local base=$S346/s-s08-$pm
        local cfg; cfg=$(mkcfg "s-s08-$pm")
        rm -rf "$base/ov"; mkdir -p "$base/ov/app-misc" "$base/ov/metadata" "$base/ov/profiles"
        cp -a "$FIX/repo-348/app-misc/t348-b-phases" "$base/ov/app-misc/"
        printf '\n[s346m]\nlocation = %s/ov\npriority = 70\nmasters = testrepo\n' \
            "$base" >> "$cfg/etc/portage/repos.conf"
        printf 'masters = testrepo\n' > "$base/ov/metadata/layout.conf"
        mkdir -p "$base/ov/profiles"
        printf 's346m\n' > "$base/ov/profiles/repo_name"
        printf 'app-misc\n' > "$base/ov/profiles/categories"
        printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\n' \
            "$base/tmp" "$base/dist" "$FBASE" >> "$cfg/etc/portage/make.conf"
        if [ "$pm" = portage ]; then EBC=ebuild; else EBC=$PEB; fi
        env PORTAGE_CONFIGROOT="$cfg" PORTAGE_TMPDIR="$base/tmp" DISTDIR="$base/dist" \
            FEATURES="$FBASE" $EBC "$base/ov/app-misc/t348-b-phases/t348-b-phases-8.ebuild" manifest \
            > "$EVDIR/s/s08-manifest-gen-$pm.stdout" 2> "$EVDIR/s/s08-manifest-gen-$pm.stderr"
        S_RUN s08-manifest-unpack "$pm" "s08-$pm" "$base/ov/app-misc/t348-b-phases/t348-b-phases-8.ebuild" unpack
        # s09: same copy minus the Manifest
        rm -f "$base/ov/app-misc/t348-b-phases/Manifest"
        rm -rf "$base/tmp"
        S_RUN s09-nomanifest-unpack "$pm" "s08-$pm" "$base/ov/app-misc/t348-b-phases/t348-b-phases-8.ebuild" unpack
    done
}
s08s09
