#!/usr/bin/env bash
# Task 5 oracle capture (backlog #576).
#
# Runs Portage 3.0.83's own `emerge -p` (from portuale/3rdparty/portage,
# never the host emerge) over a scratch copy of the pmtest
# fixtures/global-updates tree and snapshots everything
# `_do_global_updates` touches: the vdb tree, world, package.*
# (incl. ._cfg files), the Packages index + binpkgs, mtimedb, and stdout.
#
# Hermeticity: ROOT and PORTAGE_CONFIGROOT point inside the scratch dir
# (only the target root is updated; see run_updates), and
# PORTAGE_REPOSITORIES pins exactly the fixture repo, so the host's
# /etc/portage/repos.conf, profile and /var/db/pkg can neither leak in
# nor be written to. The scratch copy uses `cp -a`: the committed
# Packages index records binpkg MTIMEs, and only mtime-preserving copies
# keep a plain populate() from rewriting it before the moves run.
#
# Outputs in this directory: stdout.txt, stderr.txt, rc.txt (the pretend
# run), stdout.quiet.txt, stderr.quiet.txt, rc.quiet.txt (second run on
# a fresh copy with --quiet), command.txt (secpass goes to stderr.txt),
# portage-version.txt, before/ and after/ tree snapshots.
set -euo pipefail

T5_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" >/dev/null && pwd -P)"
PORTUALE="$(cd "$T5_DIR/../../../../.." >/dev/null && pwd -P)"
PMTEST="$PORTUALE/pmtest"
FIX="$PMTEST/fixtures/global-updates"
PORTAGE="$PORTUALE/portuale/3rdparty/portage"

T="$(mktemp -d /var/tmp/gu-capture.XXXXXX)"
trap 'rm -rf "$T"' EXIT

setup_tree() {
    rm -rf "$T/root" "$T/repo"
    cp -a "$FIX/root" "$T/root"
    cp -a "$FIX/repo" "$T/repo"
}

snapshot() {
    # $1 = before|after
    local dest="$T5_DIR/$1"
    rm -rf "$dest"
    mkdir -p "$dest"
    (cd "$T/root" && find var/db/pkg var/lib/portage etc/portage pkgdir var/cache -type f | LC_ALL=C sort > "$dest/tree.txt")
    while IFS= read -r f; do
        mkdir -p "$dest/$(dirname "$f")"
        cp -a "$T/root/$f" "$dest/$f"
    done < "$dest/tree.txt"
    (cd "$T/root" && md5sum $(cat "$dest/tree.txt") > "$dest/md5sums.txt")
}

run_updates() {
    # $1 = extra emerge args (""/"--quiet"), $2 = stdout, $3 = stderr, $4 = rc
    # Drives _global_updates for the TARGET root only, exactly as
    # run_action does per root (actions.py: _global_updates, then
    # mtimedb.commit() when it reports applied updates). The running
    # root ("/" here) is deliberately not processed: real Portage has
    # no PORTAGE_RUNNING_ROOT knob, so going through `emerge` would
    # run the updates against the host root as well (and trip this
    # host's profile gate before printing any pretend output).
    export ROOT="$T/root" PORTAGE_CONFIGROOT="$T/root"
    unset PORTAGE_RUNNING_ROOT
    export PORTAGE_REPOSITORIES="[gutest]
location = $T/repo"
    export PYTHONPATH="$PORTAGE/lib"
    unset SANDBOX_ACTIVE SANDBOX_PID SANDBOX_LOG 2>/dev/null || true
    set +e
    # shellcheck disable=SC2086
    python3 - $1 >"$2" 2>"$3" <<'PYEOF'
import sys
quiet = len(sys.argv) > 1 and sys.argv[1] == "--quiet"
sys.argv = ["emerge", "-p"] + (["--quiet"] if quiet else []) + ["dev-libs/newpkg"]
from portage import data
from portage._global_updates import _global_updates
from _emerge.actions import load_emerge_config
import portage.output
# emerge_main disables color at startup and only re-enables it in
# adjust_configs, which runs after the updates: the updates block is
# never colored in a real run.
portage.output.havecolor = 0
emerge_config = load_emerge_config(action="pretend")
target = emerge_config.target_config
print("secpass =", data.secpass, file=sys.stderr)
updated = _global_updates(
    target.root,
    emerge_config.trees,
    target.mtimedb["updates"],
    quiet=quiet,
)
if updated:
    target.mtimedb.commit()
print("updated =", updated, file=sys.stderr)
PYEOF
    echo -n "$?" > "$4"
    set -e
}

{
    echo "commit: $(git -C "$PORTAGE" describe --tags --always 2>/dev/null || git -C "$PORTAGE" rev-parse --short HEAD)"
    echo "host: $(emerge --version 2>/dev/null | head -1 || echo n/a)"
    echo "python: $(python3 --version 2>&1)"
} > "$T5_DIR/portage-version.txt"

{
    echo "ROOT=\$T/root PORTAGE_CONFIGROOT=\$T/root (single target root only)"
    echo "PORTAGE_REPOSITORIES=<[gutest] location=\$T/repo>"
    echo "probe: load_emerge_config(action=pretend) then"
    echo "  _global_updates(target.root, trees, target.mtimedb[updates], quiet)"
    echo "  target.mtimedb.commit() when updates were applied"
    echo "(mirrors _emerge/actions.py run_action per-root; the running-root"
    echo "pass, the config reload and the AUTOCLEAN force belong to Task 11)"
} > "$T5_DIR/command.txt"

# The unprivileged-mode oracle claim (data.py:_unprivileged_mode): the
# scratch EROOT is owned by this user, so secpass must be 2 and the
# updates must run. Recorded by the probe on stderr (see *.stderr).

# Run 1 (normal): snapshot before-state, run, snapshot after-state.
setup_tree
snapshot before
run_updates "" "$T5_DIR/stdout.txt" "$T5_DIR/stderr.txt" "$T5_DIR/rc.txt"
snapshot after

# Run 2 (--quiet): fresh copy, stdout only.
setup_tree
run_updates "--quiet" "$T5_DIR/stdout.quiet.txt" "$T5_DIR/stderr.quiet.txt" "$T5_DIR/rc.quiet.txt"

echo "rc=$(cat "$T5_DIR/rc.txt") rc.quiet=$(cat "$T5_DIR/rc.quiet.txt")"
