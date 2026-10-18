#!/bin/bash
# L re-capture for backlog #403 (S10): merge/unmerge listing.
# Cells l01 (fresh `ebuild merge`) + l05 (qmerge over installed older)
# mirror the S0 recipe; l06+ are extra shapes for every marker.
# Evidence: LLM/evidence/2026-10-18-403/l/.
set -u
H=$(cd "$(dirname "$0")" && pwd); . "$H/lib.sh"

PH7=$FIX/repo-348/app-misc/t348-b-phases/t348-b-phases-7.ebuild
PH8=$FIX/repo-348/app-misc/t348-b-phases/t348-b-phases-8.ebuild
FBASE="-sandbox -usersandbox -userpriv -network-sandbox -ipc-sandbox -pid-sandbox -news"
SYNOVERLAY=$S403/s403-overlay

L_EBUILD() { # <cell> <pm> <ebuild> <phase...> (direct path, scratch ROOT for merge)
    local cell=$1
    local pm=$2
    local eb=$3
    shift 3
    local base=$S403/l-$cell-$pm
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
    local base=$S403/l-$cell-$pm
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

# l01: direct `ebuild merge` on a fresh ROOT.
L_EBUILD l01-ebuild-merge portage "$PH7" merge
L_EBUILD l01-ebuild-merge portuale "$PH7" merge
# l02: emerge --oneshot new install (build.log oracle: where the listing goes).
L_EMERGE l02-emerge-new portage no --oneshot --package-moves=n app-misc/t348-b-phases
L_EMERGE l02-emerge-new portuale no --oneshot --package-moves=n app-misc/t348-b-phases

# l05: install 7 via `ebuild merge`, build 8's image, then qmerge 8 over 7.
l05() {
    for pm in portage portuale; do
        local base=$S403/l-l05-qmerge-$pm
        local cfg; cfg=$(mkcfg "l-l05-qmerge-$pm")
        printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\nCLEAN_DELAY="0"\n' \
            "$base/tmp" "$base/dist" "$FBASE" >> "$cfg/etc/portage/make.conf"
        if [ "$pm" = portage ]; then EBC=ebuild; else EBC=$PEB; fi
        env PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            $EBC "$PH7" merge >/dev/null 2>&1
        env PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            $EBC "$PH8" install >/dev/null 2>&1
        run_cell l l05-qmerge "$pm" env \
            PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            $EBC "$PH8" qmerge
        echo "$base" > "$EVDIR/l/l05-qmerge-$pm.basepath"
    done
}
l05

# Synthetic overlay (built by mkoverlay below); thin-manifests repos skip
# the Manifest check, like repo-348 in S0.
l06() {
    for pm in portage portuale; do
        local base=$S403/l-l06-symlink-$pm
        local cfg; cfg=$(mkcfg "l-l06-symlink-$pm")
        cat >> "$cfg/etc/portage/repos.conf" <<EOF
[s403]
location = $SYNOVERLAY
priority = 70
EOF
        printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\nCLEAN_DELAY="0"\n' \
            "$base/tmp" "$base/dist" "$FBASE" >> "$cfg/etc/portage/make.conf"
        if [ "$pm" = portage ]; then EBC=ebuild; else EBC=$PEB; fi
        run_cell l l06-symlink "$pm" env \
            PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            $EBC "$SYNOVERLAY/app-misc/s403-link/s403-link-1.0.ebuild" merge
        echo "$base" > "$EVDIR/l/l06-symlink-$pm.basepath"
    done
}

# --- synthetic overlay for l06/l07/l08/l09 (built once, no Manifests needed:
# thin-manifests repos skip the check, like repo-348 in S0) ---
mkoverlay() {
    rm -rf "$SYNOVERLAY"
    mkdir -p "$SYNOVERLAY/app-misc/s403-cfg" "$SYNOVERLAY/app-misc/s403-collide" \
        "$SYNOVERLAY/app-misc/s403-mod" "$SYNOVERLAY/app-misc/s403-link" \
        "$SYNOVERLAY/profiles" "$SYNOVERLAY/metadata"
    cat > "$SYNOVERLAY/profiles/repo_name" <<'EOF'
s403
EOF
    cat > "$SYNOVERLAY/profiles/categories" <<'EOF'
app-misc
EOF
    cat > "$SYNOVERLAY/metadata/layout.conf" <<'EOF'
masters = testrepo
thin-manifests = true
EOF
    # l07: config file under /etc. v1 ships "one", v2 ships "two".
    cat > "$SYNOVERLAY/app-misc/s403-cfg/s403-cfg-1.0.ebuild" <<'EOF'
EAPI=8
DESCRIPTION="s403 config-protect probe v1"
SLOT="0"
KEYWORDS="amd64"
S="${WORKDIR}"
src_unpack() { :; }
src_install() {
    insinto /etc
    echo "s403-cfg version one" > "${T}/s403-cfg.conf" || die
    doins "${T}/s403-cfg.conf" || die
}
EOF
    cat > "$SYNOVERLAY/app-misc/s403-cfg/s403-cfg-2.0.ebuild" <<'EOF'
EAPI=8
DESCRIPTION="s403 config-protect probe v2"
SLOT="0"
KEYWORDS="amd64"
S="${WORKDIR}"
src_unpack() { :; }
src_install() {
    insinto /etc
    echo "s403-cfg version two" > "${T}/s403-cfg.conf" || die
    doins "${T}/s403-cfg.conf" || die
}
EOF
    # l08: colliding file. s403-collide-1.0 owns /usr/share/s403-owned.txt;
    # s403-mod-1.0 ships the same path with different content.
    cat > "$SYNOVERLAY/app-misc/s403-collide/s403-collide-1.0.ebuild" <<'EOF'
EAPI=8
DESCRIPTION="s403 collision owner probe"
SLOT="0"
KEYWORDS="amd64"
S="${WORKDIR}"
src_unpack() { :; }
src_install() {
    insinto /usr/share
    echo "owned by s403-collide" > "${T}/s403-owned.txt" || die
    doins "${T}/s403-owned.txt" || die
}
EOF
    cat > "$SYNOVERLAY/app-misc/s403-mod/s403-mod-1.0.ebuild" <<'EOF'
EAPI=8
DESCRIPTION="s403 collision intruder probe"
SLOT="0"
KEYWORDS="amd64"
S="${WORKDIR}"
src_unpack() { :; }
src_install() {
    insinto /usr/share
    echo "intruder content from s403-mod" > "${T}/s403-owned.txt" || die
    doins "${T}/s403-owned.txt" || die
    insinto /lib/modules/s403-test
    echo "dummy module" > "${T}/s403-test.ko" || die
    doins "${T}/s403-test.ko" || die
}
EOF
    cat > "$SYNOVERLAY/app-misc/s403-mod/s403-mod-2.0.ebuild" <<'EOF'
EAPI=8
DESCRIPTION="s403 collision intruder probe v2 (drops the .ko)"
SLOT="0"
KEYWORDS="amd64"
S="${WORKDIR}"
src_unpack() { :; }
src_install() {
    insinto /usr/share
    echo "intruder content from s403-mod" > "${T}/s403-owned.txt" || die
    doins "${T}/s403-owned.txt" || die
}
EOF
    # l06: symlinks (absolute + relative target).
    cat > "$SYNOVERLAY/app-misc/s403-link/s403-link-1.0.ebuild" <<'EOF'
EAPI=8
DESCRIPTION="s403 symlink listing probe"
SLOT="0"
KEYWORDS="amd64"
S="${WORKDIR}"
src_unpack() { :; }
src_install() {
    insinto /usr/share/s403-link
    echo "s403-link payload" > "${T}/payload.txt" || die
    doins "${T}/payload.txt" || die
    ln -s payload.txt "${D}/usr/share/s403-link/rel-link" || die
    ln -s /usr/share/s403-link/payload.txt "${D}/usr/share/s403-link/abs-link" || die
}
EOF
}
mkoverlay
l06

# l10: NOCOLOR=yes on the direct path: the ` * ` line goes plain.
l10() {
    for pm in portage portuale; do
        local base=$S403/l-l10-nocolor-$pm
        local cfg; cfg=$(mkcfg "l-l10-nocolor-$pm")
        printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\nCLEAN_DELAY="0"\n' \
            "$base/tmp" "$base/dist" "$FBASE" >> "$cfg/etc/portage/make.conf"
        if [ "$pm" = portage ]; then EBC=ebuild; else EBC=$PEB; fi
        run_cell l l10-nocolor "$pm" env \
            PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" NOCOLOR=yes \
            $EBC "$PH7" merge
        echo "$base" > "$EVDIR/l/l10-nocolor-$pm.basepath"
    done
}
l10
L_SYN_EBUILD() { # <cell> <pm> <ebuild-rel> <phase...> + optional extra make.conf lines
    local cell=$1
    local pm=$2
    local ebrel=$3
    shift 3
    local base=$S403/l-$cell-$pm
    local cfg; cfg=$(mkcfg "l-$cell-$pm")
    cat >> "$cfg/etc/portage/repos.conf" <<EOF
[s403]
location = $SYNOVERLAY
priority = 70
EOF
    {
        printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\nCLEAN_DELAY="0"\n' \
            "$base/tmp" "$base/dist" "$FBASE"
        printf 'CONFIG_PROTECT="/etc"\nCONFIG_PROTECT_MASK="/etc/env.d"\n'
    } >> "$cfg/etc/portage/make.conf"
    local eb="$SYNOVERLAY/$ebrel"
    if [ "$pm" = portage ]; then
        run_cell l "$cell" "$pm" env \
            PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" CONFIG_PROTECT="/etc" \
            CONFIG_PROTECT_MASK="/etc/env.d" \
            ebuild "$eb" "$@"
    else
        run_cell l "$cell" "$pm" env \
            PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" CONFIG_PROTECT="/etc" \
            CONFIG_PROTECT_MASK="/etc/env.d" \
            "$PEB" "$eb" "$@"
    fi
    echo "$base" > "$EVDIR/l/$cell-$pm.basepath"
}

# l07: merge cfg v1 fresh, user-modify the live conf, merge v2 over it
# (replace path: diverted `._cfgNNNN_` + `--- replaced` on the old entry).
l07() {
    for pm in portage portuale; do
        local base=$S403/l-l07-cfgdivert-$pm
        local cfg; cfg=$(mkcfg "l-l07-cfgdivert-$pm")
        cat >> "$cfg/etc/portage/repos.conf" <<EOF
[s403]
location = $SYNOVERLAY
priority = 70
EOF
        {
            printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\nCLEAN_DELAY="0"\n' \
                "$base/tmp" "$base/dist" "$FBASE"
            printf 'CONFIG_PROTECT="/etc"\nCONFIG_PROTECT_MASK="/etc/env.d"\n'
        } >> "$cfg/etc/portage/make.conf"
        if [ "$pm" = portage ]; then EBC=ebuild; else EBC=$PEB; fi
        env PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" CONFIG_PROTECT="/etc" \
            CONFIG_PROTECT_MASK="/etc/env.d" \
            $EBC "$SYNOVERLAY/app-misc/s403-cfg/s403-cfg-1.0.ebuild" merge >/dev/null 2>&1
        echo "user-edited s403-cfg.conf" >> "$base/root/etc/s403-cfg.conf"
        run_cell l l07-cfgdivert "$pm" env \
            PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" CONFIG_PROTECT="/etc" \
            CONFIG_PROTECT_MASK="/etc/env.d" \
            $EBC "$SYNOVERLAY/app-misc/s403-cfg/s403-cfg-2.0.ebuild" merge
        echo "$base" > "$EVDIR/l/l07-cfgdivert-$pm.basepath"
    done
}
l07

# l07b: merge cfg v2 content a second time (already-offered update):
# confmem rejects it (`---` on the merge side).
l07b() {
    for pm in portage portuale; do
        local base=$S403/l-l07b-cfgskip-$pm
        local cfg; cfg=$(mkcfg "l-l07b-cfgskip-$pm")
        cat >> "$cfg/etc/portage/repos.conf" <<EOF
[s403]
location = $SYNOVERLAY
priority = 70
EOF
        {
            printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\nCLEAN_DELAY="0"\n' \
                "$base/tmp" "$base/dist" "$FBASE"
            printf 'CONFIG_PROTECT="/etc"\nCONFIG_PROTECT_MASK="/etc/env.d"\n'
        } >> "$cfg/etc/portage/make.conf"
        if [ "$pm" = portage ]; then EBC=ebuild; else EBC=$PEB; fi
        env PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" CONFIG_PROTECT="/etc" \
            CONFIG_PROTECT_MASK="/etc/env.d" \
            $EBC "$SYNOVERLAY/app-misc/s403-cfg/s403-cfg-1.0.ebuild" merge >/dev/null 2>&1
        echo "user-edited s403-cfg.conf" >> "$base/root/etc/s403-cfg.conf"
        env PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" CONFIG_PROTECT="/etc" \
            CONFIG_PROTECT_MASK="/etc/env.d" \
            $EBC "$SYNOVERLAY/app-misc/s403-cfg/s403-cfg-2.0.ebuild" merge >/dev/null 2>&1
        run_cell l l07b-cfgskip "$pm" env \
            PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" CONFIG_PROTECT="/etc" \
            CONFIG_PROTECT_MASK="/etc/env.d" \
            $EBC "$SYNOVERLAY/app-misc/s403-cfg/s403-cfg-2.0.ebuild" merge
        echo "$base" > "$EVDIR/l/l07b-cfgskip-$pm.basepath"
    done
}
l07b

# l08: file owned by another package, protect-owned on: both sides abort
# (rc 1). The S0 FBASE lacks protect-owned while Portage's make.globals
# has it, so it is set explicitly here for a same-shape abort.
l08() {
    for pm in portage portuale; do
        local base=$S403/l-l08-collision-$pm
        local cfg; cfg=$(mkcfg "l-l08-collision-$pm")
        cat >> "$cfg/etc/portage/repos.conf" <<EOF
[s403]
location = $SYNOVERLAY
priority = 70
EOF
        printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s protect-owned"\nCLEAN_DELAY="0"\n' \
            "$base/tmp" "$base/dist" "$FBASE" >> "$cfg/etc/portage/make.conf"
        if [ "$pm" = portage ]; then EBC=ebuild; else EBC=$PEB; fi
        env PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE protect-owned" \
            $EBC "$SYNOVERLAY/app-misc/s403-collide/s403-collide-1.0.ebuild" merge >/dev/null 2>&1
        run_cell l l08-collision "$pm" env \
            PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE protect-owned" \
            $EBC "$SYNOVERLAY/app-misc/s403-mod/s403-mod-1.0.ebuild" merge
        echo "$base" > "$EVDIR/l/l08-collision-$pm.basepath"
    done
}
l08

# l08b: same collision with protect-owned off: the merge goes through
# ("merged despite file collisions"); the intruder file gets a plain
# `>>>` line -- there is no `!owner` marker in 3.0.82.2.
l08b() {
    for pm in portage portuale; do
        local base=$S403/l-l08b-mergethrough-$pm
        local cfg; cfg=$(mkcfg "l-l08b-mergethrough-$pm")
        cat >> "$cfg/etc/portage/repos.conf" <<EOF
[s403]
location = $SYNOVERLAY
priority = 70
EOF
        printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s -protect-owned"\nCLEAN_DELAY="0"\n' \
            "$base/tmp" "$base/dist" "$FBASE" >> "$cfg/etc/portage/make.conf"
        if [ "$pm" = portage ]; then EBC=ebuild; else EBC=$PEB; fi
        env PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE -protect-owned" \
            $EBC "$SYNOVERLAY/app-misc/s403-collide/s403-collide-1.0.ebuild" merge >/dev/null 2>&1
        run_cell l l08b-mergethrough "$pm" env \
            PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE -protect-owned" \
            $EBC "$SYNOVERLAY/app-misc/s403-mod/s403-mod-1.0.ebuild" merge
        echo "$base" > "$EVDIR/l/l08b-mergethrough-$pm.basepath"
    done
}
l08b

# l09: UNINSTALL_IGNORE'd path dropped by the new version: the old entry's
# file is kept with `--- cfgpro` (the only `cfgpro` source in vartree.py).
l09() {
    for pm in portage portuale; do
        local base=$S403/l-l09-cfgpro-$pm
        local cfg; cfg=$(mkcfg "l-l09-cfgpro-$pm")
        cat >> "$cfg/etc/portage/repos.conf" <<EOF
[s403]
location = $SYNOVERLAY
priority = 70
EOF
        printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\nCLEAN_DELAY="0"\n' \
            "$base/tmp" "$base/dist" "$FBASE" >> "$cfg/etc/portage/make.conf"
        if [ "$pm" = portage ]; then EBC=ebuild; else EBC=$PEB; fi
        env PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            $EBC "$SYNOVERLAY/app-misc/s403-mod/s403-mod-1.0.ebuild" merge >/dev/null 2>&1
        run_cell l l09-cfgpro "$pm" env \
            PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            $EBC "$SYNOVERLAY/app-misc/s403-mod/s403-mod-2.0.ebuild" merge
        echo "$base" > "$EVDIR/l/l09-cfgpro-$pm.basepath"
    done
}
l09

# l12: direct standalone `ebuild unmerge` (same `_unmerge_pkgfiles`
# listing as the replace loop, without the frame lines).
l12() {
    for pm in portage portuale; do
        local base=$S403/l-l12-unmerge-$pm
        local cfg; cfg=$(mkcfg "l-l12-unmerge-$pm")
        printf 'PORTAGE_TMPDIR="%s"\nDISTDIR="%s"\nFEATURES="%s"\nCLEAN_DELAY="0"\n' \
            "$base/tmp" "$base/dist" "$FBASE" >> "$cfg/etc/portage/make.conf"
        if [ "$pm" = portage ]; then EBC=ebuild; else EBC=$PEB; fi
        env PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            $EBC "$PH8" merge >/dev/null 2>&1
        run_cell l l12-unmerge "$pm" env \
            PORTAGE_CONFIGROOT="$cfg" ROOT="$base/root" PORTAGE_TMPDIR="$base/tmp" \
            DISTDIR="$base/dist" FEATURES="$FBASE" \
            $EBC "$PH8" unmerge
        echo "$base" > "$EVDIR/l/l12-unmerge-$pm.basepath"
    done
}
l12
