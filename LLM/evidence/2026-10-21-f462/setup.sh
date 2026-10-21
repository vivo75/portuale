#!/bin/bash
# #462 oracle root: stages the checked-in fixture tree for host Portage
# 3.0.82.2, adapting `differential-test-bed/layers/l0-fixture-oracle/stage.sh`
# (which expects a `/fixtures` mount inside the test container) to this host:
# `$FIXTURES_SRC` is copied to `$S/fixtures` and the same deltas applied,
# except touching `/etc/make.local` (it already exists on this host and the
# fixture `make.conf` only sources it).
set -euo pipefail

S=${S7:-/var/tmp/f462/stage}
FX_SRC=${FIXTURES_SRC:-/home/vivo/worktrees/f462/pmtest/fixtures}
FX="$S/fixtures"

rm -rf "$S"
mkdir -p "$S"
cp -r "$FX_SRC" "$FX"

# 1. absolute repo locations
sed -i "s|^location = |location = $FX/|" "$FX/etc/portage/repos.conf/repos.conf"
# 2. resolve the portuale-only interpolation
sed -i "s|\${PORTAGE_CONFIGROOT}|$FX|g" "$FX/etc/portage/binrepos.conf"
# 5. Portage rejects the fixture news format
rm -rf "$FX/repo/metadata/news"

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

# 8. layout.conf masters (the fixture repos omit it; Portage warns otherwise)
for repo in repo overlay independentoverlay layoutmasteroverlay repnamerepo; do
  [ -d "$FX/$repo" ] || continue
  mkdir -p "$FX/$repo/metadata"
  if ! grep -q '^masters' "$FX/$repo/metadata/layout.conf" 2>/dev/null; then
    echo "masters = testrepo" >> "$FX/$repo/metadata/layout.conf"
  fi
done

#   9. comments in a staged `profiles/updates/` file are an error to
#      Portage's parser; drop them.
for upd in "$FX"/*/profiles/updates/*; do
  [ -f "$upd" ] || continue
  sed -i '/^[[:space:]]*#/d' "$upd"
done

#  10. `*/<pkg>` is portuale-only syntax Portage rejects; rewrite it.
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

#  11. repos whose profiles lack `repo_name` make Portage warn on every run
for repo in repo overlay independentoverlay layoutmasteroverlay; do
  [ -d "$FX/$repo" ] || continue
  if [ ! -s "$FX/$repo/profiles/repo_name" ]; then
    mkdir -p "$FX/$repo/profiles"
    printf '%s\n' "$repo" > "$FX/$repo/profiles/repo_name"
  fi
done

# 4. one EBUILD record per staged ebuild (all versions in one Manifest)
python3 - "$FX" <<'PY'
import hashlib
import sys
from collections import defaultdict
from pathlib import Path

fx = Path(sys.argv[1])
records: dict[Path, list[str]] = defaultdict(list)
for repo in ("repo", "overlay", "independentoverlay", "layoutmasteroverlay", "repnamerepo"):
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
