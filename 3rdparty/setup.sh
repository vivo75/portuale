#!/usr/bin/env bash
# Clone/update the third-party working checkouts under 3rdparty/ to the
# exact refs pinned in 3rdparty/repos.toml. Gitignored, never committed.
# Idempotent -- safe to re-run after a repos.toml pin bump.
#
#   ./3rdparty/setup.sh            # all repos
#   ./3rdparty/setup.sh portage    # just one
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repos_toml="${here}/repos.toml"
root="$(cd "${here}/.." && pwd)"

[[ -f "${repos_toml}" ]] || { echo "!!! ${repos_toml} not found" >&2; exit 1; }
command -v python3 >/dev/null || { echo "!!! python3 is required to parse repos.toml" >&2; exit 1; }

# Emit "<name>\t<url>\t<checkout_path>\t<ref>\t<commit>" per repo.
mapfile -t entries < <(python3 - "${repos_toml}" <<'PY'
import sys, tomllib
with open(sys.argv[1], "rb") as f:
    data = tomllib.load(f)
for name, r in data.items():
    cp = r.get("checkout_path")
    if not cp:
        continue
    print("\t".join((name, r["url"], cp, r.get("ref", ""), r.get("commit", ""))))
PY
)

want=("$@")
in_want() { [[ ${#want[@]} -eq 0 ]] && return 0; local x; for x in "${want[@]}"; do [[ $x == "$1" ]] && return 0; done; return 1; }

for line in "${entries[@]}"; do
    IFS=$'\t' read -r name url checkout_path ref commit <<<"${line}"
    in_want "${name}" || continue

    dest="${root}/${checkout_path}"
    target="${commit:-${ref}}"
    echo ">>> ${name}: ${url} @ ${ref:-${commit}}"

    if [[ ! -d "${dest}/.git" ]]; then
        mkdir -p "$(dirname "${dest}")"
        git clone "${url}" "${dest}"
    fi

    git -C "${dest}" fetch --tags origin
    git -C "${dest}" checkout --quiet --detach "${target}"

    echo "    -> $(git -C "${dest}" describe --tags --always) ($(git -C "${dest}" rev-parse --short HEAD))"
done

echo ">>> done. (bootstrap is complete; \`cargo build\` and \`pytest\` should now work)"
