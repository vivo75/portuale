#!/bin/bash
# E family (349, S4): direct-`ebuild` configuration — PORTAGE_TMPDIR, DISTDIR,
# FEATURES from make.conf vs process env, make.globals copy (binpkg layout),
# emerge --fetchonly DISTDIR, and the die `::repo` text.
# Base probe: repo-348 t348-b-env-8 `setup` (dumps everything to T348_LOGDIR),
# env -u PORTAGE_TMPDIR -u DISTDIR -u FEATURES, like the §0.3 probe.
set -u
H=$(cd "$(dirname "$0")" && pwd); . "$H/lib.sh"

EB8=$FIX/repo-348/app-misc/t348-b-env/t348-b-env-8.ebuild
ENOA=$FIX/repo-348/app-misc/t348-b-envnoeapply/t348-b-envnoeapply-7.ebuild
FBASE="-sandbox -usersandbox -userpriv -network-sandbox -ipc-sandbox -pid-sandbox"


clean_host_builddir() { # $1 = pf; portuale ignores make.conf PORTAGE_TMPDIR
    # (the 349-TMP bug), so its E runs share host /var/tmp/portage and would
    # hit each other's stamps. Remove only our own probe builddir.
    rm -rf "/var/tmp/portage/app-misc/$1"
}

E_SETUP() { # <cell> <pm> <make.conf-extra> : run `ebuild EB8 setup`, save env log
    local cell=$1
    local pm=$2
    local extra=$3
    local base=$S346/e-$cell-$pm
    local cfg; cfg=$(mkcfg "e-$cell-$pm")
    mkdir -p "$base/logs"
    extra=$(printf "$extra" | sed "s|e-X|e-$cell-$pm|g")
    printf '%s\n' "$extra" >> "$cfg/etc/portage/make.conf"
    clean_host_builddir t348-b-env-8
    if [ "$pm" = portage ]; then
        run_cell e "$cell" "$pm" env -u PORTAGE_TMPDIR -u DISTDIR -u FEATURES \
            PORTAGE_CONFIGROOT="$cfg" T348_LOGDIR="$base/logs" \
            ebuild "$EB8" setup
    else
        run_cell e "$cell" "$pm" env -u PORTAGE_TMPDIR -u DISTDIR -u FEATURES \
            PORTAGE_CONFIGROOT="$cfg" T348_LOGDIR="$base/logs" \
            "$PEB" "$EB8" setup
    fi
    cp "$base/logs"/env-pkg_setup.log "$EVDIR/e/$cell-$pm.stagelog" 2>/dev/null || \
        echo "(no env-pkg_setup.log)" > "$EVDIR/e/$cell-$pm.stagelog"
    (cd "$base" && find . -maxdepth 2 | sort) > "$EVDIR/e/$cell-$pm ScratchFind.txt" 2>/dev/null
    rm -rf "$base"
}

mkextra() { # <base> : make.conf lines for DISTDIR/TMPDIR/FEATURES/T348 keys
    local base=$1
    printf 'DISTDIR="%s"\nPORTAGE_TMPDIR="%s"\nFEATURES="%s %s"\nT348_LOGDIR="%s"\n' \
        "$S346/e-X/dist" "$S346/e-X/tmp" "$FBASE" "$2" "$S346/e-X/logs"
}

# e01: all keys in make.conf (FEATURES=test added to the sandbox-off base).
e05() {
    for pm in portage portuale; do
        local base=$S346/e-e05-$pm
        local cfg; cfg=$(mkcfg "e-e05-$pm")
        mkdir -p "$base/logs" "$base/envtmp" "$base/envdist"
        printf 'DISTDIR="%s"\nPORTAGE_TMPDIR="%s"\nFEATURES="%s test"\n' \
            "$base/dist" "$base/tmp" "$FBASE" >> "$cfg/etc/portage/make.conf"
        if [ "$pm" = portage ]; then
            run_cell e e05-envwins "$pm" env \
                PORTAGE_CONFIGROOT="$cfg" T348_LOGDIR="$base/logs" \
                PORTAGE_TMPDIR="$base/envtmp" DISTDIR="$base/envdist" FEATURES="-test" \
                ebuild "$EB8" setup
        else
            run_cell e e05-envwins "$pm" env \
                PORTAGE_CONFIGROOT="$cfg" T348_LOGDIR="$base/logs" \
                PORTAGE_TMPDIR="$base/envtmp" DISTDIR="$base/envdist" FEATURES="-test" \
                "$PEB" "$EB8" setup
        fi
        cp "$base/logs"/env-pkg_setup.log "$EVDIR/e/e05-envwins-$pm.stagelog" 2>/dev/null || \
            echo "(no env-pkg_setup.log)" > "$EVDIR/e/e05-envwins-$pm.stagelog"
        rm -rf "$base"
    done
}
e06() {
    for pm in portage portuale; do
        local base=$S346/e-e06-$pm
        local cfg; cfg=$(mkcfg "e-e06-$pm")
        mkdir -p "$base/logs"
        printf 'DISTDIR="%s"\nPORTAGE_TMPDIR="%s"\nFEATURES="%s test"\n' \
            "$base/dist" "$base/tmp" "$FBASE" >> "$cfg/etc/portage/make.conf"
        mkdir -p "$cfg/usr/share/portage/config"
        cp /usr/share/portage/config/make.globals "$cfg/usr/share/portage/config/make.globals"
        clean_host_builddir t348-b-env-8
        if [ "$pm" = portage ]; then
            run_cell e e06-makeglobals-package "$pm" env -u PORTAGE_TMPDIR -u DISTDIR -u FEATURES \
                PORTAGE_CONFIGROOT="$cfg" T348_LOGDIR="$base/logs" \
                ebuild "$EB8" package
        else
            run_cell e e06-makeglobals-package "$pm" env -u PORTAGE_TMPDIR -u DISTDIR -u FEATURES \
                PORTAGE_CONFIGROOT="$cfg" T348_LOGDIR="$base/logs" \
                "$PEB" "$EB8" package
        fi
        cp "$base/logs"/env-pkg_setup.log "$EVDIR/e/e06-makeglobals-package-$pm.stagelog" 2>/dev/null || \
            echo "(no env-pkg_setup.log)" > "$EVDIR/e/e06-makeglobals-package-$pm.stagelog"
        (cd "$cfg/pkgdir" && find . | sort) > "$EVDIR/e/e06-makeglobals-package-$pm.pkgfind" 2>&1
        rm -rf "$base"
    done
}
e07() {
    local url=http://10.48.0.229:8080/distfiles/d3/zc_lockfile-4.0.tar.gz
    for pm in portage portuale; do
        local base=$S346/e-e07-$pm
        local cfg; cfg=$(mkcfg "e-e07-$pm")
        mkdir -p "$base/ov/app-misc/s346-fetch" "$base/ov/metadata" "$base/ov/profiles"
        cat > "$base/ov/app-misc/s346-fetch/s346-fetch-1.ebuild" <<EOF
EAPI=8
DESCRIPTION="s346 S0 probe: SRC_URI fetch landing cell (349-DIST emerge half)"
HOMEPAGE="https://example.invalid/s346-fetch"
LICENSE="MIT"
SLOT="0"
KEYWORDS="amd64"
SRC_URI="$url -> zc_lockfile-4.0.tar.gz"
S="\${WORKDIR}"
src_unpack() { :; }
src_install() { :; }
EOF
        read -r EB_SIZE EB_BLAKE EB_SHA <<<"$(python3 -c "
import hashlib
d = open('$base/ov/app-misc/s346-fetch/s346-fetch-1.ebuild','rb').read()
print(len(d), hashlib.blake2b(d, digest_size=64).hexdigest(), hashlib.sha512(d).hexdigest())")"
        { printf 'DIST zc_lockfile-4.0.tar.gz 10956 BLAKE2B ad5a9b27dad3ea3c523c5f2d182e28ff9c45f5a989140cf5b235ef87df3b320c5819bf521129f285b5a6b55a9fb0b562c9bffaaa7f3c8010d7de8f082f41df98 SHA512 d6f9a01676710aa50b7a422fa11fd5617ee2806ad6f1d301b2e18460a3a5708eef0d8272aa7a4523b81ea2ca55280b88323fb9c00cb8230b490d63478906ec77\n'
          printf 'EBUILD s346-fetch-1.ebuild %s BLAKE2B %s SHA512 %s\n' "$EB_SIZE" "$EB_BLAKE" "$EB_SHA"; } \
            > "$base/ov/app-misc/s346-fetch/Manifest"
        printf 'masters = testrepo\n' > "$base/ov/metadata/layout.conf"
        mkdir -p "$base/ov/profiles"
        printf 's346f\n' > "$base/ov/profiles/repo_name"
        printf 'app-misc\n' > "$base/ov/profiles/categories"
        printf '\n[s346f]\nlocation = %s/ov\npriority = 70\n' "$base" >> "$cfg/etc/portage/repos.conf"
        printf 'DISTDIR="%s"\n' "$base/dist" >> "$cfg/etc/portage/make.conf"
        if [ "$pm" = portage ]; then
            run_cell e e07-fetchonly-distdir "$pm" env -u PORTAGE_TMPDIR -u DISTDIR -u FEATURES \
                PORTAGE_CONFIGROOT="$cfg" ROOT="$cfg" \
                emerge --oneshot --fetchonly --package-moves=n app-misc/s346-fetch
        else
            run_cell e e07-fetchonly-distdir "$pm" env -u PORTAGE_TMPDIR -u DISTDIR -u FEATURES \
                PORTAGE_CONFIGROOT="$cfg" ROOT="$cfg" \
                "$PEM" --oneshot --fetchonly --package-moves=n app-misc/s346-fetch
        fi
        (cd "$base/dist" && find . | sort) > "$EVDIR/e/e07-fetchonly-distdir-$pm.distfind" 2>&1
        rm -rf "$base"
    done
}
e08() {
    for pm in portage portuale; do
        local base=$S346/e-e08-$pm
        local cfg; cfg=$(mkcfg "e-e08-$pm")
        mkdir -p "$base/logs"
        printf 'DISTDIR="%s"\nPORTAGE_TMPDIR="%s"\nFEATURES="%s test"\n' \
            "$base/dist" "$base/tmp" "$FBASE" >> "$cfg/etc/portage/make.conf"
        clean_host_builddir t348-b-envnoeapply-7
        if [ "$pm" = portage ]; then
            run_cell e e08-die-repo "$pm" env -u PORTAGE_TMPDIR -u DISTDIR -u FEATURES \
                PORTAGE_CONFIGROOT="$cfg" T348_LOGDIR="$base/logs" \
                ebuild "$ENOA" prepare
        else
            run_cell e e08-die-repo "$pm" env -u PORTAGE_TMPDIR -u DISTDIR -u FEATURES \
                PORTAGE_CONFIGROOT="$cfg" T348_LOGDIR="$base/logs" \
                "$PEB" "$ENOA" prepare
        fi
        (cd "$base/tmp" && find . -maxdepth 5 | sort) > "$EVDIR/e/e08-die-repo-$pm.tmpfind" 2>&1
        find "$base/tmp" -name build.log | sort > "$EVDIR/e/e08-die-repo-$pm.buildlogs" 2>&1
        rm -rf "$base"
    done
}
case "${1:-all}" in
  all)
E_SETUP e01-full portage "$(mkextra X test)"
E_SETUP e01-full portuale "$(mkextra X test)"
E_SETUP e02-nodistdir portage "$(printf 'PORTAGE_TMPDIR="%s"\nFEATURES="%s test"\n' "$S346/e-X/tmp" "$FBASE")"
E_SETUP e02-nodistdir portuale "$(printf 'PORTAGE_TMPDIR="%s"\nFEATURES="%s test"\n' "$S346/e-X/tmp" "$FBASE")"
E_SETUP e03-notmpdir portage "$(printf 'DISTDIR="%s"\nFEATURES="%s test"\n' "$S346/e-X/dist" "$FBASE")"
E_SETUP e03-notmpdir portuale "$(printf 'DISTDIR="%s"\nFEATURES="%s test"\n' "$S346/e-X/dist" "$FBASE")"
E_SETUP e04-nofeatures portage "$(printf 'DISTDIR="%s"\nPORTAGE_TMPDIR="%s"\n' "$S346/e-X/dist" "$S346/e-X/tmp")"
E_SETUP e04-nofeatures portuale "$(printf 'DISTDIR="%s"\nPORTAGE_TMPDIR="%s"\n' "$S346/e-X/dist" "$S346/e-X/tmp")"
ls -la /var/tmp/portage > "$EVDIR/e/e03-host-var-tmp-portage.txt" 2>&1
find /var/tmp/portage -maxdepth 4 | sort >> "$EVDIR/e/e03-host-var-tmp-portage.txt" 2>&1
e05
e06
e07
e08
;;
  e01) E_SETUP e01-full portage "$(mkextra X test)"; E_SETUP e01-full portuale "$(mkextra X test)" ;;
  e02) E_SETUP e02-nodistdir portage "$(printf 'PORTAGE_TMPDIR=\"%s\"\nFEATURES=\"%s test\"\n' "$S346/e-X/tmp" "$FBASE")"; E_SETUP e02-nodistdir portuale "$(printf 'PORTAGE_TMPDIR=\"%s\"\nFEATURES=\"%s test\"\n' "$S346/e-X/tmp" "$FBASE")" ;;
  e07) e07 ;;
  e08) e08 ;;
esac

