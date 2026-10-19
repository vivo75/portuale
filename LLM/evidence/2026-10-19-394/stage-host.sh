#!/bin/bash
# #394 host oracle staging. Adapts
# differential-test-bed/layers/l0-fixture-oracle/stage.sh for the host:
# copy $SRC (pmtest fixtures) to $STAGE/fixtures, apply the container
# deltas that matter for --pretend, then inject the staged-only scratch
# ebuilds from $SCRATCH before Manifest generation. Never touches the
# checked-in fixture tree.
#
# Usage: stage-host.sh <src-fixtures> <stage-dir> <scratch-dir>
set -euo pipefail

SRC=${1:?src fixtures dir}
STAGE=${2:?stage dir}
SCRATCH=${3:?scratch ebuild dir}
FX="$STAGE/fixtures"

rm -rf "$STAGE"
mkdir -p "$STAGE"
cp -r "$SRC" "$FX"

# 1. absolute repo locations (Portage rejects relative `location = repo`)
sed -i "s|^location = |location = $FX/|" "$FX/etc/portage/repos.conf/repos.conf"
# 2. resolve the portuale-only interpolation
sed -i "s|\${PORTAGE_CONFIGROOT}|$FX|g" "$FX/etc/portage/binrepos.conf"
# 5. Portage rejects the fixture news format
rm -rf "$FX/repo/metadata/news"

# scratch overlay: staged-only ebuilds (never committed), before Manifests
if [ -d "$SCRATCH/repo" ]; then
  cp -r "$SCRATCH/repo/." "$FX/repo/"
fi

# 3. categories Portage will accept
CATEGORIES=$(
  for repo in repo overlay independentoverlay layoutmasteroverlay repnamerepo; do
    [ -d "$FX/$repo" ] || continue
    for cat in "$FX/$repo"/*/; do
      [ -f "$cat" ] && continue
      basename "$cat"
    done
  done | sort -u
)
printf '%s\n' "$CATEGORIES" > "$FX/etc/portage/categories"
for repo in repo overlay independentoverlay layoutmasteroverlay repnamerepo; do
  [ -d "$FX/$repo" ] || continue
  mkdir -p "$FX/$repo/profiles"
  printf '%s\n' "$CATEGORIES" > "$FX/$repo/profiles/categories"
done

# 7. stale-entry case (#46 S4)
stale="$FX/repo/metadata/md5-cache/dev-libs/common-1.0"
if [ -f "$stale" ]; then
  sed -i \
    -e 's/^KEYWORDS=.*/KEYWORDS=~amd64/' \
    -e 's/^_md5_=.*/_md5_=00000000000000000000000000000000/' \
    "$stale"
fi

# 8. layout.conf masters
for repo in overlay independentoverlay layoutmasteroverlay repnamerepo; do
  [ -d "$FX/$repo" ] || continue
  mkdir -p "$FX/$repo/metadata"
  if ! grep -q '^masters' "$FX/$repo/metadata/layout.conf" 2>/dev/null; then
    echo "masters = testrepo" >> "$FX/$repo/metadata/layout.conf"
  fi
done

# 9. comments in profiles/updates files are an error to Portage
for upd in "$FX"/*/profiles/updates/*; do
  [ -f "$upd" ] || continue
  sed -i '/^[[:space:]]*#/d' "$upd"
done

# 10. `*/<pkg>` is portuale-only syntax
python3 - "$FX" <<'PY'
import re
import sys
from pathlib import Path

fx = Path(sys.argv[1])
categories: dict[str, str] = {}
for repo in ("repo", "overlay", "independentoverlay", "layoutmasteroverlay", "repnamerepo"):
    root = fx / repo
    if not root.is_dir():
        continue
    for pkg_dir in root.glob("*/*/"):
        categories.setdefault(pkg_dir.name, pkg_dir.parent.name)

pattern = re.compile(r"(?m)^\*/(" + "|".join(map(re.escape, categories)) + r")(?=\s|$)")
for path in sorted(fx.glob("*/profiles/package.use.*")) + sorted(fx.glob("*/profiles/*/package.use.*")):
    if not path.is_file():
        continue
    text = path.read_text()
    fixed = pattern.sub(lambda m: f"{categories[m.group(1)]}/{m.group(1)}", text)
    if fixed != text:
        path.write_text(fixed)
PY

# 11. repo_name files
for repo in overlay independentoverlay layoutmasteroverlay; do
  [ -d "$FX/$repo" ] || continue
  if [ ! -s "$FX/$repo/profiles/repo_name" ]; then
    mkdir -p "$FX/$repo/profiles"
    printf '%s\n' "$repo" > "$FX/$repo/profiles/repo_name"
  fi
done

# 4. one EBUILD Manifest record per staged ebuild (covers scratch too)
python3 - "$FX" <<'PY'
import hashlib
import sys
from collections import defaultdict
from pathlib import Path

fx = Path(sys.argv[1])
records: dict[Path, list[str]] = defaultdict(list)
for repo in ("repo", "overlay", "independentoverlay", "layoutmasteroverlay", "repnamerepo", "repo-341", "repo-341-formats", "repo-348", "repo-355"):
    root = fx / repo
    if not root.is_dir():
        continue
    for ebuild in root.glob("*/*/*.ebuild"):
        data = ebuild.read_bytes()
        records[ebuild.parent].append(
            "EBUILD {name} {size} BLAKE2B {b2} SHA512 {s5}\n".format(
                name=ebuild.name,
                size=len(data),
                b2=hashlib.blake2b(data, digest_size=64).hexdigest(),
                s5=hashlib.sha512(data).hexdigest(),
            )
        )
for pkgdir, lines in records.items():
    (pkgdir / "Manifest").write_text("".join(sorted(lines)))
PY

echo "staged $FX"
