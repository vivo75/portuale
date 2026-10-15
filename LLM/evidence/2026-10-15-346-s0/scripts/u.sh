#!/bin/bash
# U family (346-UP, S1): the eleven §0.1.1 rows + the S0 adds.
# Target dev-libs/t341-dep (IUSE x) unless noted. argv: emerge -p --package-moves=n <target>.
# User profile = $cfg/etc/portage/profile (Portage's CUSTOM_PROFILE_PATH; appended
# only when make.profile resolves, LocationsManager.py:169).
set -u
H=$(cd "$(dirname "$0")" && pwd); . "$H/lib.sh"

U() { # <cell> <target> <setup-fn> [extra-emerge-args...]
    local cell=$1
    local target=$2
    local setup=$3
    shift 3
    for pm in portage portuale; do
        local cfg; cfg=$(mkcfg "u-$cell-$pm")
        "$setup" "$cfg"
        if [ "$pm" = portage ]; then
            run_cell u "$cell" "$pm" env PORTAGE_CONFIGROOT="$cfg" ROOT="$cfg" \
                emerge -p --package-moves=n "$@" "$target"
        else
            run_cell u "$cell" "$pm" env PORTAGE_CONFIGROOT="$cfg" ROOT="$cfg" \
                "$PEM" -p --package-moves=n "$@" "$target"
        fi
    done
}

s_none() { :; }
s_useforce() { mkdir -p "$1/etc/portage/profile"; echo "x" > "$1/etc/portage/profile/use.force"; }
s_makedefaults() { mkdir -p "$1/etc/portage/profile"; printf 'USE="x"\n' > "$1/etc/portage/profile/make.defaults"; }
s_packageuse() { mkdir -p "$1/etc/portage/profile"; echo "dev-libs/t341-dep x" > "$1/etc/portage/profile/package.use"; }
s_useforcemask() {
    mkdir -p "$1/etc/portage/profile"
    echo "dev-libs/t341-dep x" > "$1/etc/portage/profile/package.use.mask"
    echo "dev-libs/t341-dep x" > "$1/etc/portage/profile/package.use"
}
s_useforcefile() { mkdir -p "$1/etc/portage/profile"; echo "dev-libs/t341-dep x" > "$1/etc/portage/profile/package.use.force"; }
s_usemask() {
    mkdir -p "$1/etc/portage/profile"
    echo "x" > "$1/etc/portage/profile/use.mask"
    echo "dev-libs/t341-dep x" > "$1/etc/portage/profile/package.use"
}
s_acceptkeywords() { mkdir -p "$1/etc/portage/profile"; echo "dev-libs/bareacceptkeywordspkg ~amd64" > "$1/etc/portage/profile/package.accept_keywords"; }
s_packages() { mkdir -p "$1/etc/portage/profile"; echo "dev-libs/t341-dep" > "$1/etc/portage/profile/packages"; }
s_eapi5wild() {
    mkdir -p "$1/etc/portage/profile"
    echo "5" > "$1/etc/portage/profile/eapi"
    echo "*/t341-dep x" > "$1/etc/portage/profile/package.use"
}
s_noeapiwild() { mkdir -p "$1/etc/portage/profile"; echo "*/t341-dep x" > "$1/etc/portage/profile/package.use"; }
s_usestablemask() {
    mkdir -p "$1/etc/portage/profile"
    echo "x" > "$1/etc/portage/profile/use.stable.mask"
    echo "dev-libs/t341-dep x" > "$1/etc/portage/profile/package.use"
}
s_usestableforce() { mkdir -p "$1/etc/portage/profile"; echo "dev-libs/t341-dep x" > "$1/etc/portage/profile/package.use.stable.force"; }
s_chainmask_userunmask() {
    # Scratch chain profile (relative parent -> fixture default) masking
    # t341-dep; user profile unmasks it. Proves the user node is read
    # against chain masks. (A `repo:path` parent needs profile-formats
    # portage-2 in a repo layout; a bare dir has no repo, so relative.)
    local chain=$S346/chainmask
    rm -rf "$chain"; mkdir -p "$chain"
    realpath --relative-to="$chain" "$FIX/repo/profiles/default" > "$chain/parent"
    echo "dev-libs/t341-dep" > "$chain/package.mask"
    ln -sfn "$chain" "$1/etc/portage/make.profile"
    mkdir -p "$1/etc/portage/profile"; echo "dev-libs/t341-dep" > "$1/etc/portage/profile/package.unmask"
}
s_parent_ignored() {
    # User profile holds ONLY a parent pointer to a scratch profile that
    # would enable x. Portage appends the user node but never walks it, so
    # the parent's package.use must have no effect (USE stays "-x").
    local up
    up=$(dirname "$1")/uparent
    rm -rf "$up"; mkdir -p "$up"
    echo "dev-libs/t341-dep x" > "$up/package.use"
    mkdir -p "$1/etc/portage/profile"
    realpath --relative-to="$1/etc/portage/profile" "$up" > "$1/etc/portage/profile/parent"
}
s_profile_starred() {
    mkdir -p "$1/etc/portage/profile"
    printf '*dev-libs/t341-dep\ndev-libs/bareacceptkeywordspkg\n' > "$1/etc/portage/profile/packages"
}
s_system_unstarred() { mkdir -p "$1/etc/portage/profile"; echo "dev-libs/t341-dep" > "$1/etc/portage/profile/packages"; }

U u01-baseline dev-libs/t341-dep s_none
U u02-useforce dev-libs/t341-dep s_useforce
U u03-makedefaults dev-libs/t341-dep s_makedefaults
U u04-packageuse dev-libs/t341-dep s_packageuse
U u05-useforcemask dev-libs/t341-dep s_useforcemask
U u06-useforcefile dev-libs/t341-dep s_useforcefile
U u07-usemask dev-libs/t341-dep s_usemask
U u08-acceptkeywords dev-libs/bareacceptkeywordspkg s_acceptkeywords
U u10-eapi5wild dev-libs/t341-dep s_eapi5wild
U u11-noeapiwild dev-libs/t341-dep s_noeapiwild
U u12-usestablemask dev-libs/t341-dep s_usestablemask
U u13-usestableforce dev-libs/t341-dep s_usestableforce
U u14-chainmask-userunmask dev-libs/t341-dep s_chainmask_userunmask
U u16-parent-ignored dev-libs/t341-dep s_parent_ignored
U u18-profile-starred @profile s_profile_starred
U u19-system-unstarred @system s_system_unstarred

# u14b: control for u14 — chain mask WITHOUT the user unmask (Portage stays masked).
s_chainmask_only() {
    local chain=$S346/chainmask
    rm -rf "$chain"; mkdir -p "$chain"
    realpath --relative-to="$chain" "$FIX/repo/profiles/default" > "$chain/parent"
    echo "dev-libs/t341-dep" > "$chain/package.mask"
    ln -sfn "$chain" "$1/etc/portage/make.profile"
}
U u14b-chainmask-only dev-libs/t341-dep s_chainmask_only

# u09: @profile with a single unstarred line (Portage merges t341-dep).
U u09-packages-atprofile @profile s_packages

# u15: no make.profile at all + user use.force (Portage ignores the user
# profile then, LocationsManager.py:169). mkcfg's symlink is removed.
u15() {
    for pm in portage portuale; do
        local cfg; cfg=$(mkcfg "u-u15-$pm")
        rm "$cfg/etc/portage/make.profile"
        mkdir -p "$cfg/etc/portage/profile"; echo "x" > "$cfg/etc/portage/profile/use.force"
        if [ "$pm" = portage ]; then
            run_cell u u15-nomakeprofile "$pm" env PORTAGE_CONFIGROOT="$cfg" ROOT="$cfg" \
                emerge -p --package-moves=n dev-libs/t341-dep
        else
            run_cell u u15-nomakeprofile "$pm" env PORTAGE_CONFIGROOT="$cfg" ROOT="$cfg" \
                "$PEM" -p --package-moves=n dev-libs/t341-dep
        fi
    done
}
u15

# u17: emerge --info with a user-profile make.defaults (USE/FEATURES lines).
u17() {
    for pm in portage portuale; do
        local cfg; cfg=$(mkcfg "u-u17-$pm")
        mkdir -p "$cfg/etc/portage/profile"
        printf 'USE="x"\nFEATURES="test"\n' > "$cfg/etc/portage/profile/make.defaults"
        if [ "$pm" = portage ]; then
            run_cell u u17-info "$pm" env PORTAGE_CONFIGROOT="$cfg" ROOT="$cfg" \
                emerge --info --package-moves=n
        else
            run_cell u u17-info "$pm" env PORTAGE_CONFIGROOT="$cfg" ROOT="$cfg" \
                "$PEM" --info --package-moves=n
        fi
    done
}
u17
