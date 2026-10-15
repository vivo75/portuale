#!/bin/bash
# B family (346-BID, S3): build-ids in user package.accept_keywords / package.use.
# Two-instance synthetic PKGDIR (the #341 S9 shape); KEYWORDS/USE varied per cell.
# argv: emerge -pv --package-moves=n --usepkgonly=y [...] dev-libs/t341-dep.
set -u
H=$(cd "$(dirname "$0")" && pwd); . "$H/lib.sh"

PKG_BASE='VERSION: 0\nTIMESTAMP: 0\nPACKAGES: 2\n\n'

pkg_entry() { # <bid> <buildtime> <keywords> <use> <iuse>
    printf 'CPV: dev-libs/t341-dep-1\nDEFINED_PHASES: -\nDESCRIPTION: s346 B oracle build %s\nEAPI: 8\nIUSE: %s\nKEYWORDS: %s\nSLOT: 0/1\nUSE: %s\nREPO: t341\nPATH: dev-libs/t341-dep-1-%s.tbz2\nBUILD_ID: %s\nBUILD_TIME: %s\n\n' \
        "$1" "$5" "$3" "$4" "$1" "$1" "$2"
}

B() { # <cell> <keywords> <use1> <use2> <userfile-content> [extra args...]
    local cell=$1
    local kw=$2
    local use1=$3
    local use2=$4
    local content=$5
    shift 5
    for pm in portage portuale; do
        local cfg
        cfg=$(mkcfg "b-$cell-$pm")
        { printf "$PKG_BASE"; pkg_entry 1 1000 "$kw" "$use1" x; pkg_entry 2 2000 "$kw" "$use2" x; } \
            > "$cfg/pkgdir/Packages"
        local uf=$cfg/etc/portage/package.use
        : > "$cfg/etc/portage/package.accept_keywords"
        : > "$uf"
        printf '%s\n' "$content" > "$cfg/etc/portage/package.accept_keywords"
        if [ "$pm" = portage ]; then
            run_cell b "$cell" "$pm" env PORTAGE_CONFIGROOT="$cfg" ROOT="$cfg" \
                emerge -pv --package-moves=n --usepkgonly=y "$@" dev-libs/t341-dep
        else
            run_cell b "$cell" "$pm" env PORTAGE_CONFIGROOT="$cfg" ROOT="$cfg" \
                "$PEM" -pv --package-moves=n --usepkgonly=y "$@" dev-libs/t341-dep
        fi
    done
}

# NOTE: B() always writes the content into package.accept_keywords; the
# package.use cells overwrite afterwards via B_USE. Simpler: dedicated fns.
B_USE() { # <cell> <keywords> <use1> <use2> <package.use-line> [extra args...]
    local cell=$1
    local kw=$2
    local use1=$3
    local use2=$4
    local line=$5
    shift 5
    for pm in portage portuale; do
        local cfg
        cfg=$(mkcfg "b-$cell-$pm")
        { printf "$PKG_BASE"; pkg_entry 1 1000 "$kw" "$use1" x; pkg_entry 2 2000 "$kw" "$use2" x; } \
            > "$cfg/pkgdir/Packages"
        : > "$cfg/etc/portage/package.accept_keywords"
        printf '%s\n' "$line" > "$cfg/etc/portage/package.use"
        if [ "$pm" = portage ]; then
            run_cell b "$cell" "$pm" env PORTAGE_CONFIGROOT="$cfg" ROOT="$cfg" \
                emerge -pv --package-moves=n --usepkgonly=y "$@" dev-libs/t341-dep
        else
            run_cell b "$cell" "$pm" env PORTAGE_CONFIGROOT="$cfg" ROOT="$cfg" \
                "$PEM" -pv --package-moves=n --usepkgonly=y "$@" dev-libs/t341-dep
        fi
    done
}

B b01-ak-bid2 '~amd64' '' '' '=dev-libs/t341-dep-1-2 ~amd64'
B b02-ak-bid1 '~amd64' '' '' '=dev-libs/t341-dep-1-1 ~amd64'
B_USE b03-use-bid2-respect 'amd64' 'x' '' '=dev-libs/t341-dep-1-2 x' --binpkg-respect-use=y
B_USE b04-use-bid1-respect 'amd64' 'x' '' '=dev-libs/t341-dep-1-1 x' --binpkg-respect-use=y
B b05-range-ignored '~amd64' '' '' '>=dev-libs/t341-dep-1-2 ~amd64'
B_USE b06-use-bid-norespect 'amd64' 'x' '' '=dev-libs/t341-dep-1-2 x'
