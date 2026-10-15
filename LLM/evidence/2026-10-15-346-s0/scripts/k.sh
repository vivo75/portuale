#!/bin/bash
# K family (346-KW, S2): package.keywords — user file (accept + warning),
# user-profile node (adds to the package's own KEYWORDS), chain node, `-` removal,
# and the cross-file `-` cancellation (package.keywords first, accept_keywords extends).
set -u
H=$(cd "$(dirname "$0")" && pwd); . "$H/lib.sh"

K() { # <cell> <target> <setup-fn>
    local cell=$1
    local target=$2
    local setup=$3
    for pm in portage portuale; do
        local cfg
        cfg=$(mkcfg "k-$cell-$pm")
        "$setup" "$cfg"
        if [ "$pm" = portage ]; then
            run_cell k "$cell" "$pm" env PORTAGE_CONFIGROOT="$cfg" ROOT="$cfg" \
                emerge -p --package-moves=n "$target"
        else
            run_cell k "$cell" "$pm" env PORTAGE_CONFIGROOT="$cfg" ROOT="$cfg" \
                "$PEM" -p --package-moves=n "$target"
        fi
    done
}

s_user_keywords() { # user config FILE (not profile): accept + deprecation warning
    echo "dev-libs/bareacceptkeywordspkg ~amd64" > "$1/etc/portage/package.keywords"
}
s_userprofile_keywords_amd64() { # profile node: adds stable keyword to KEYWORDS
    mkdir -p "$1/etc/portage/profile"
    echo "dev-libs/bareacceptkeywordspkg amd64" > "$1/etc/portage/profile/package.keywords"
}
s_chain_keywords() { # chain node via scratch profile with relative parent
    local chain=$S346/chainkw
    rm -rf "$chain"; mkdir -p "$chain"
    realpath --relative-to="$chain" "$FIX/repo/profiles/default" > "$chain/parent"
    echo "dev-libs/bareacceptkeywordspkg amd64" > "$chain/package.keywords"
    ln -sfn "$chain" "$1/etc/portage/make.profile"
}
s_profile_minus() { # `-~amd64` removal of the package's own keyword
    mkdir -p "$1/etc/portage/profile"
    echo "dev-libs/bareacceptkeywordspkg -~amd64" > "$1/etc/portage/profile/package.keywords"
}
s_cross_minus() { # keywords first, accept_keywords extends with `-`: stays masked
    echo "dev-libs/bareacceptkeywordspkg ~amd64" > "$1/etc/portage/package.keywords"
    echo "dev-libs/bareacceptkeywordspkg -~amd64" > "$1/etc/portage/package.accept_keywords"
}

TGT=dev-libs/bareacceptkeywordspkg
K k01-user-keywords $TGT s_user_keywords
K k02-userprofile-amd64 $TGT s_userprofile_keywords_amd64
K k03-chain-amd64 $TGT s_chain_keywords
K k04-profile-minus $TGT s_profile_minus
K k05-cross-minus $TGT s_cross_minus
