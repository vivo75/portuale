#!/bin/bash
# Capture real portage's output for fixture atoms inside the TEST
# container (backlog #19 oracle, docs/abort-path-spec.md §3).
#
#   TEST/run/abort-capture.sh [-o OUTDIR] ATOM...
#
# Method (identical to the Slice 1 captures): the committed fixtures/
# tree is copied to a scratch dir, the copy gets (a) a repos.conf with
# absolute /fixtures/... locations (real portage rejects the fixture
# tree's relative ones) and (b) generated Manifest files (real masks
# every Manifest-less ebuild "masked by: corruption"; portuale reads
# md5-cache directly). The copy is mounted at /fixtures, /etc/make.local
# is bind-mounted (the fixture make.conf sources it by absolute path),
# (empty by default: the fixture make.conf `source`s it by absolute
# path, which portuale resolves chroot-style to the non-existent
# fixtures/etc/make.local -- mounting the host's own file would leak its
# EMERGE_DEFAULT_OPTS into the oracle; ABORTCAP_MAKE_LOCAL=/etc/make.local
# opts back in), and each atom runs as `emerge -pv`, `-pvt`, `-pv --columns`,
# `--pretend --debug` with PORTAGE_CONFIGROOT=/fixtures ROOT=/fixtures
# PYTHONHASHSEED=0 LC_ALL=C.UTF-8 TZ=UTC. Per probe-mode:
# <cat>_<pkg>.<mode>.{cmd,exit,stdout,stderr}. The committed fixtures/
# are never touched.
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=lib.sh
source "$here/lib.sh"

OUTDIR="$REPO_ROOT/fixtures/abort-captures"
if [ "${1:-}" = "-o" ]; then OUTDIR=$2; shift 2; fi
[ $# -ge 1 ] || { echo "usage: $0 [-o OUTDIR] ATOM..." >&2; exit 2; }
ensure_image

WORK=${ABORTCAP_WORK:-$(mktemp -d /tmp/abortcap.XXXXXX)}
rm -rf "$WORK/fixtures"
mkdir -p "$WORK/fixtures"
# Everything but the captures themselves.
tar -C "$REPO_ROOT/fixtures" --exclude=./abort-captures -cf - . | tar -C "$WORK/fixtures" -xf -

sed -i -E 's#^location = ([^/].*)$#location = /fixtures/\1#' "$WORK/fixtures/etc/portage/repos.conf/repos.conf"

python3 - "$WORK/fixtures" <<'PY'
import hashlib, os, sys
root = sys.argv[1]
for repo in ("repo", "overlay", "independentoverlay", "layoutmasteroverlay", "repnamerepo"):
    base = os.path.join(root, repo)
    if not os.path.isdir(base):
        continue
    for cat in sorted(os.listdir(base)):
        catdir = os.path.join(base, cat)
        if not os.path.isdir(catdir) or cat in ("metadata", "profiles", "eclass"):
            continue
        for pkg in sorted(os.listdir(catdir)):
            pkgdir = os.path.join(catdir, pkg)
            if not os.path.isdir(pkgdir):
                continue
            lines = []
            for f in sorted(os.listdir(pkgdir)):
                if not f.endswith(".ebuild"):
                    continue
                data = open(os.path.join(pkgdir, f), "rb").read()
                lines.append(
                    f"EBUILD {f} {len(data)} BLAKE2B {hashlib.blake2b(data).hexdigest()} "
                    f"SHA512 {hashlib.sha512(data).hexdigest()}\n"
                )
            if lines:
                with open(os.path.join(pkgdir, "Manifest"), "w") as fh:
                    fh.writelines(lines)
PY

rm -rf "$WORK/out"
mkdir -p "$WORK/out" "$OUTDIR"
MAKE_LOCAL=${ABORTCAP_MAKE_LOCAL:-$WORK/make.local.empty}
: > "$WORK/make.local.empty"
cat > "$WORK/run.sh" <<'EOS'
#!/bin/bash
export PORTAGE_CONFIGROOT=/fixtures ROOT=/fixtures PYTHONHASHSEED=0 LC_ALL=C.UTF-8 TZ=UTC
python3 -c 'import portage; print("portage", portage.VERSION)' > /out/version.txt
for atom in "$@"; do
  tag=${atom//\//_}
  while read -r mode args; do
    p="/out/$tag.$mode"
    echo "cmdline: emerge $args $atom" > "$p.cmd"
    set +e
    # shellcheck disable=SC2086
    emerge $args "$atom" > "$p.stdout" 2> "$p.stderr"
    printf '%s' "$?" > "$p.exit"
    set -e
  done <<'MODES'
pv -pv
pvt -pvt
pv-columns -pv --columns
pretend-debug --pretend --debug
MODES
done
EOS
chmod +x "$WORK/run.sh"

"$PODMAN" run --rm --entrypoint /bin/bash \
  --security-opt seccomp=unconfined \
  -v "$WORK/fixtures:/fixtures" \
  -v "$WORK/out:/out" \
  -v "$WORK/run.sh:/run.sh:ro" \
  -v "$MAKE_LOCAL":/etc/make.local:ro \
  "$IMAGE" /run.sh "$@"

cp "$WORK/out"/*.{cmd,exit,stdout,stderr} "$OUTDIR"/
echo ">>> $(cat "$WORK/out/version.txt"); captures in $OUTDIR (work dir $WORK)"
