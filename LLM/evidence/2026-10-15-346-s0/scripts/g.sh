#!/bin/bash
# G family (347-GB/PX, S9): --getbinpkg-exclude/include filter the remote index,
# per-binrepo keys (+ conflict warnings), bad atoms, and the partly-excluded
# top-level target. Hermetic file:// binhost with two REMOTE-ONLY binaries
# (no ebuilds anywhere: exclusion is visible as resolve/abort, like the
# remotebinpkg precedent). --pretend refreshes the index (#192) but never
# opens archives (PATHs are fake).
set -u
H=$(cd "$(dirname "$0")" && pwd); . "$H/lib.sh"

G_PKGDIR_TEXT='VERSION: 0\nTIMESTAMP: 0\nPACKAGES: 2\n\n'

g_entry() { # <pn>
    printf 'CPV: app-misc/%s-1\nDEFINED_PHASES: -\nDESCRIPTION: s346 G oracle remote binary %s\nEAPI: 8\nIUSE:\nKEYWORDS: amd64\nSLOT: 0\nUSE:\nREPO: s346g\nPATH: app-misc/%s-1.gpkg.tar\nBUILD_ID: 1\nBUILD_TIME: 1000\nSIZE: 100\n\n' "$1" "$1" "$1"
}

# G_SETUP <cell> <pm> : fresh cfg + scratch repo s346g (profiles only, NO ebuilds)
# + scratch file:// binhost with both binaries. Echoes cfg; sets G_BASE/G_BINHOST.
G_SETUP() {
    local cell=$1
    local pm=$2
    G_BASE=$S346/g-$cell-$pm
    local cfg; cfg=$(mkcfg "g-$cell-$pm")
    mkdir -p "$G_BASE/repo/profiles" "$G_BASE/repo/metadata" "$G_BASE/binhost"
    printf 'masters = testrepo\n' > "$G_BASE/repo/metadata/layout.conf"
    printf 's346g\n' > "$G_BASE/repo/profiles/repo_name"
    printf 'app-misc\n' > "$G_BASE/repo/profiles/categories"
    printf '\n[s346g]\nlocation = %s/repo\npriority = 70\n' "$G_BASE" >> "$cfg/etc/portage/repos.conf"
    { printf "$G_PKGDIR_TEXT"; g_entry s346-gbin-a; g_entry s346-gbin-b; } > "$G_BASE/binhost/Packages"
    printf '[s346gbin]\nsync-uri = file://%s/binhost\npriority = 1\n' "$G_BASE" > "$cfg/etc/portage/binrepos.conf"
    echo "$cfg"
}

G_RUN() { # <cell> <target...> with per-cell env in G_ENV (array) + G_BREPO extra
    local cell=$1
    shift
    for pm in portage portuale; do
        local cfg; cfg=$(G_SETUP "$cell" "$pm")
        if [ -n "${G_BREPO:-}" ]; then
            printf '%s\n' "$G_BREPO" >> "$cfg/etc/portage/binrepos.conf"
        fi
        if [ "$pm" = portage ]; then
            run_cell g "$cell" "$pm" env PORTAGE_CONFIGROOT="$cfg" ROOT="$cfg" \
                emerge -pv --package-moves=n --getbinpkg "${G_ENV[@]}" "$@"
        else
            run_cell g "$cell" "$pm" env PORTAGE_CONFIGROOT="$cfg" ROOT="$cfg" \
                "$PEM" -pv --package-moves=n --getbinpkg "${G_ENV[@]}" "$@"
        fi
    done
    unset G_ENV G_BREPO
}

T2="app-misc/s346-gbin-a app-misc/s346-gbin-b"

# g00: dedicated cell WITHOUT --package-moves (351-UPD coupling demo).
for pm in portage portuale; do
    cfg=$(G_SETUP "g00-nomoves" "$pm")
    if [ "$pm" = portage ]; then
        run_cell g g00-nomoves "$pm" env PORTAGE_CONFIGROOT="$cfg" ROOT="$cfg" \
            emerge -pv --getbinpkg $T2
    else
        run_cell g g00-nomoves "$pm" env PORTAGE_CONFIGROOT="$cfg" ROOT="$cfg" \
            "$PEM" -pv --getbinpkg $T2
    fi
done

G_ENV=(--getbinpkg-exclude app-misc/s346-gbin-a); G_RUN g01-exclude $T2
G_ENV=(--getbinpkg-exclude app-misc/s346-gbin-a); G_RUN g01b-exclude-other-target app-misc/s346-gbin-b
G_ENV=(--getbinpkg-include app-misc/s346-gbin-a); G_RUN g02-include $T2
G_ENV=(--getbinpkg-include app-misc/s346-gbin-a); G_RUN g02b-include-only-target app-misc/s346-gbin-a
G_ENV=(--getbinpkg-exclude app-misc/s346-gbin-a --getbinpkg-include app-misc/s346-gbin-a); G_RUN g03-both $T2
G_BREPO='getbinpkg-exclude = app-misc/s346-gbin-a'; G_RUN g04-repokey $T2
G_BREPO='getbinpkg-exclude = =bad/atom-1'; G_RUN g05-repobadatom $T2
# g06: CLI include overrides per-repo exclude (and the reverse), each with a warning.
G_BREPO='getbinpkg-exclude = app-misc/s346-gbin-b'; G_ENV=(--getbinpkg-include app-misc/s346-gbin-b); G_RUN g06-cli-overrides-repo $T2
# g07: partly-excluded top-level — --exclude names gbin-a, gbin-b user-masked.
g07() {
    for pm in portage portuale; do
        cfg=$(G_SETUP "g07-partly-excluded" "$pm")
        echo "app-misc/s346-gbin-b" > "$cfg/etc/portage/package.mask"
        if [ "$pm" = portage ]; then
            run_cell g g07-partly-excluded "$pm" env PORTAGE_CONFIGROOT="$cfg" ROOT="$cfg" \
                emerge -pv --package-moves=n --getbinpkg --exclude app-misc/s346-gbin-a $T2
        else
            run_cell g g07-partly-excluded "$pm" env PORTAGE_CONFIGROOT="$cfg" ROOT="$cfg" \
                "$PEM" -pv --package-moves=n --getbinpkg --exclude app-misc/s346-gbin-a $T2
        fi
    done
}
g07
