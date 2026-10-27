#!/bin/bash
# R4 probe namespace (adapted from pmtest pytests-contract-suite/repos_filter.sh, #564).
# Usage: NS_FX=<fixture copy> NS_GLOBAL=<comments-only repos.conf> ns-run.sh <real> <argv0> <args...>
# In a private mount namespace (host untouched) it binds:
#   NS_GLOBAL            -> /usr/share/portage/config/repos.conf  (no host gentoo repo, as the suite)
#   $NS_ETC             -> /etc/portage  (a dir holding only make.profile -> $NS_FX/repo/profiles/default:
#                            Portage parses the "/" config's profile against the shared repo set,
#                            and the host profile needs gentoo; no host make.conf/package.* either)
#   $NS_FX/var/db        -> /var/db          (Portage's running root "/" sees the fixture vdb,
#                            like portuale's PORTAGE_RUNNING_ROOT=<fx>)
#   $NS_FX/var/cache/edb -> /var/cache/edb   (nothing can reach the host edb)
set -u
SELF=$0
if [ "${1:-}" = "--inner" ]; then
    U=$2; G=$3; NAME=$4; REAL=$5; shift 5
    /usr/bin/mount --bind "$NS_GLOBAL" /usr/share/portage/config/repos.conf || exit 99
    /usr/bin/mount --bind "$NS_ETC" /etc/portage || exit 98
    /usr/bin/mount --bind "$NS_FX/var/db" /var/db || exit 97
    mkdir -p "$NS_FX/var/cache/edb"
    /usr/bin/mount --bind "$NS_FX/var/cache/edb" /var/cache/edb || exit 96
    exec /usr/bin/unshare --user --map-user="$U" --map-group="$G" -- "$SELF" --exec "$NAME" "$REAL" "$@"
fi
if [ "${1:-}" = "--exec" ]; then
    NAME=$2; REAL=$3; shift 3
    exec -a "$NAME" "$REAL" "$@"
fi
REAL=$1; NAME=$2; shift 2
ME_U=$(/usr/bin/id -u)
G=$(/usr/bin/getent group portage | cut -d: -f3); G=${G:-$(/usr/bin/id -g)}
exec /usr/bin/unshare -r --mount --propagation private -- "$SELF" --inner "$ME_U" "$G" "$NAME" "$REAL" "$@"
