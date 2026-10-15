#!/bin/bash
# S0 shared harness (plan LLM/02.346-ebuild-config-options-parity.opus.md, slice S0).
# Oracle: host Portage 3.0.82.2 vs target/release/portuale (prebuilt, never rebuilt here).
# Every emerge cell runs with --package-moves=n on both sides (351-UPD cut),
# except the one dedicated coupling cell (l00). Scratch lives under /var/tmp/s346
# and is removed by run-all.sh at the end; evidence files are verbatim captures.
set -u
S346=/var/tmp/s346
EVDIR=/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-15-346-s0
FIX=/home/vivo/repo/PORTUALE/pmtest/fixtures
PBIN=/home/vivo/repo/PORTUALE/portuale/target/release/portuale
PEM=$S346/bin/emerge
PEB=$S346/bin/ebuild
export GENTOO_MIRRORS=http://10.48.0.229:8080
export PYTHONHASHSEED=0
export EMERGE_DEFAULT_OPTS=

mkcfg() {
    # mkcfg <tag> : fresh hermetic configroot set; echoes the cfg path.
    # Layout mirrors pmtest's _t341_configroot (fixture repos + t341 + t348,
    # make.profile -> fixture repo/profiles/default), plus per-tag root/tmp/dist.
    local tag=$1
    local base=$S346/$tag
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
    printf 'PKGDIR="%s"\nDISTDIR="%s"\nCLEAN_DELAY="0"\nFEATURES="-news"\n' \
        "$base/cfg/pkgdir" "$base/dist" > "$base/cfg/etc/portage/make.conf"
    echo "$base/cfg"
}

run_cell() {
    # run_cell <family> <cell> <pm> <command...>
    # Verbatim capture: <cell>-<pm>.{argv,rc,stdout,stderr}. argv holds the
    # full env assignments + command (callers pass VAR=val through `env`).
    local fam=$1
    local cell=$2
    local pm=$3
    shift 3
    local d=$EVDIR/$fam
    local base
    mkdir -p "$d"
    base=$d/$cell-$pm
    { echo "cwd: $S346"; printf 'cmd:'; printf ' %q' "$@"; printf '\n'; } > "$base.argv"
    timeout 600 "$@" > "$base.stdout" 2> "$base.stderr"
    echo $? > "$base.rc"
}

std_env() {
    # std_env <cfg> : prints the standard hermetic env assignments for an
    # emerge cell (ROOT = cfg, like _t341_configroot).
    local cfg=$1
    printf 'PORTAGE_CONFIGROOT=%s ROOT=%s' "$cfg" "$cfg"
}
