#!/bin/bash
# Real `emerge` as a fixture oracle (backlog #49 / second_python_copy_removal.md
# §6) -- the staging step, run inside the test container.
#
# Copies the checked-in `fixtures/` tree (mounted read-only at /fixtures)
# into a writable stage dir and applies the minimal deltas real Portage
# needs. The checked-in fixtures stay untouched; the deltas are:
#
#   1. `repos.conf` locations become absolute in-container paths (real
#      rejects the relative `location = repo` portuale accepts);
#   2. `binrepos.conf`'s `${PORTAGE_CONFIGROOT}` is resolved to the
#      staged configroot (portuale-only interpolation);
#   3. an `etc/portage/categories` file is added, listing every staged
#      fixture category (real masks a package whose category is not
#      listed);
#   4. one `Manifest` per staged package dir with every ebuild's own
#      BLAKE2B/SHA512 (real masks a digest-less ebuild as corruption, and
#      "not listed in the Manifest" for a missing multi-version line);
#      distfile records are not needed for `--pretend`;
#   5. `repo/metadata/news` is dropped (the fixture news items use a
#      portuale-only format real rejects);
#   6. an empty `/etc/make.local` is created in the container, because
#      the fixture `make.conf` sources it (the only host-root path the
#      staged run touches, and only in the throwaway container);
#   7. backquotes in staged ebuilds become single quotes: fixture
#      DESCRIPTIONS use `` `flag` `` for prose, which bash evaluates as
#      command substitution when real's depend phase sources the ebuild
#      (metadata is prose, so this does not change any resolution input);
#   8. a `metadata/layout.conf` with `masters = testrepo` is added where
#      the fixture repo lacks one (real warns on every run otherwise).
#
# `FX_WORLD_EXTRA` (backlog #54 S0): space-separated atoms appended to
# the staged `var/lib/portage/world` before either package manager runs
# -- lets a probe test a world state the checked-in fixture world does
# not encode (e.g. a consumer package added to world) without touching
# the shared fixture.
#
# Usage: stage.sh <stage-dir>   (fixtures must be mounted at /fixtures)
set -euo pipefail

STAGE=${1:?stage dir}
FX="$STAGE/fixtures"

rm -rf "$STAGE"
mkdir -p "$STAGE"
cp -r /fixtures "$FX"

# 1. absolute repo locations
sed -i "s|^location = |location = $FX/|" "$FX/etc/portage/repos.conf/repos.conf"
# 2. resolve the portuale-only interpolation
sed -i "s|\${PORTAGE_CONFIGROOT}|$FX|g" "$FX/etc/portage/binrepos.conf"
# 6. silence the fixture make.conf's `source /etc/make.local`
touch /etc/make.local
# 5. real rejects the fixture news format
rm -rf "$FX/repo/metadata/news"

# FX_WORLD_EXTRA: append atoms to the staged world (#54 S0)
if [ -n "${FX_WORLD_EXTRA:-}" ]; then
  mkdir -p "$FX/var/lib/portage"
  for atom in $FX_WORLD_EXTRA; do
    printf '%s\n' "$atom" >> "$FX/var/lib/portage/world"
  done
fi

# 3. categories real will accept. Written to /etc/portage/categories
#    *and* to each staged repo's own profiles/categories: a BDEPEND
#    resolved against the running root uses a `local_config=False`
#    settings object (real EAPI 7+ semantics deliberately isolate the
#    build root's config from the target root's `/etc/portage`
#    overrides), which never reads /etc/portage/categories at all --
#    only a repo's own profile-chain categories file, unconditionally
#    (#53 S3: the go/go-md2man repro is the first fixture-oracle case
#    with a BDEPEND chain, which is what exposed this).
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

# 7. backquotes -> single quotes in staged ebuilds
for ebuild in "$FX"/*/*/*/*.ebuild; do
  [ -f "$ebuild" ] || continue
  if grep -q '`' "$ebuild"; then
    sed -i "s/\`/'/g" "$ebuild"
  fi
done

# 8. layout.conf masters (the fixture repos omit it; real warns otherwise)
for repo in overlay independentoverlay layoutmasteroverlay repnamerepo; do
  [ -d "$FX/$repo" ] || continue
  mkdir -p "$FX/$repo/metadata"
  if ! grep -q '^masters' "$FX/$repo/metadata/layout.conf" 2>/dev/null; then
    echo "masters = testrepo" >> "$FX/$repo/metadata/layout.conf"
  fi
done

#   9. comments in a staged `profiles/updates/` file are an error to
#      real's parser (`Update type not recognized '# ...'`); drop them.
for upd in "$FX"/*/profiles/updates/*; do
  [ -f "$upd" ] || continue
  sed -i '/^[[:space:]]*#/d' "$upd"
done

#  10. `*/<pkg>` is portuale-only syntax real rejects ("Invalid atom");
#      rewrite it to the package's real category in the staged copy. The
#      rewritten atom is the same target, so no resolution input changes.
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

#  11. repos whose profiles lack `repo_name` make real warn on every run;
#      the file content is the section name real would otherwise use.
for repo in overlay independentoverlay layoutmasteroverlay; do
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
