#!/bin/bash
# S404 shared harness (backlog #404, 02.346 S11: auto-clean).
# Portage 3.0.82.2 ("Portage", never "real") vs target/release/portuale,
# hermetic per-cell PORTAGE_CONFIGROOT; scratch under /var/tmp/s404.
# Layout mirrors the S0/S403 harnesses. Differences from S0:
# - CLEAN_DELAY is NOT set anywhere (neither env nor make.conf), so the
#   default 5 applies: any countdown line in a capture is a real finding.
# - FBASE has no CLEAN_DELAY entry for the same reason.
set -u
S404=/var/tmp/s404
EVDIR=/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-18-404
FIX=/home/vivo/repo/PORTUALE/pmtest/fixtures
PBIN=/home/vivo/repo/PORTUALE/portuale/target/release/portuale
PEM=$S404/bin/emerge
PEB=$S404/bin/ebuild
export GENTOO_MIRRORS=http://10.48.0.229:8080
export PYTHONHASHSEED=0
export EMERGE_DEFAULT_OPTS=

mkcfg() {
    # mkcfg <tag> : fresh hermetic configroot set; echoes the cfg path.
    local tag=$1
    local base=$S404/$tag
    rm -rf "$base"
    mkdir -p "$base/cfg/etc/portage" "$base/root" "$base/tmp" "$base/dist" "$base/cfg/pkgdir"
    cat > "$base/cfg/etc/portage/repos.conf" <<EOF
[DEFAULT]
main-repo = testrepo
[testrepo]
location = $FIX/repo
[overlay]
location = $FIX/overlay
priority = 10
[independentoverlay]
location = $FIX/independentoverlay
priority = 20
masters = overlay
[layoutmasteroverlay]
location = $FIX/layoutmasteroverlay
priority = 30
[repnamesection]
location = $FIX/repnamerepo
priority = 40
[t341]
location = $FIX/repo-341
priority = 50
[t348]
location = $FIX/repo-348
priority = 60
EOF
    ln -sfn "$FIX/repo/profiles/default" "$base/cfg/etc/portage/make.profile"
    printf 'PKGDIR="%s"\nDISTDIR="%s"\nFEATURES="-news"\n' \
        "$base/cfg/pkgdir" "$base/dist" > "$base/cfg/etc/portage/make.conf"
    echo "$base/cfg"
}

run_cell() {
    # run_cell <family> <cell> <pm> <command...>
    local fam=$1
    local cell=$2
    local pm=$3
    shift 3
    local d=$EVDIR/$fam
    local base
    mkdir -p "$d"
    base=$d/$cell-$pm
    { echo "cwd: $S404"; printf 'cmd:'; printf ' %q' "$@"; printf '\n'; } > "$base.argv"
    timeout 600 "$@" > "$base.stdout" 2> "$base.stderr"
    echo $? > "$base.rc"
}
