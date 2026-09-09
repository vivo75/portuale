# `TEST/` — real-tree validation for portuale

Scaffolding for running `portuale` (and the real `emerge`) against a real
Gentoo tree inside container images.

Two things live here:

1. **The legacy `/init` probes** (`scripts/`, `create-container.bash`) —
   ad-hoc reproducers from earlier slices, run in lexicographic order by
   the container's PID-1 `/init`.
2. **The differential test bed** (`run/`, `compare/`, `layers/`,
   `atomlists/`, `net/`, `images/`) — the structured Portuale-vs-Portage
   comparison described in
   [`docs/history/real-world-testing.md`](../docs/history/real-world-testing.md). Built
   slice by slice; **slice 1 = infra + L0**.

---

## The differential test bed

### Prerequisites

- `podman` + `buildah`; root containers are fine (and preferred where
  they give a cleaner test — see the doc §1.10).
- The `localhost/test-portuale:latest` image:
  `sudo TEST/create-container.bash`. It bakes the pinned `gentoo` +
  `buildovl` repos, the `porttest` overlay skeleton
  (`images/overlay/porttest/`), and stable portage.
- Host: `python3` + `PyYAML`, and `dev-util/diffoscope` for L2 drill-down.
- `portuale` is built automatically by the `run/` orchestrators
  (`cargo build --release -p portuale`, plus the `emerge`/`ebuild`/`mrg`
  multicall symlinks).

### L0 — resolver parity at real-tree scale

```sh
TEST/run/l0-resolver.sh                       # full atom list
TEST/run/l0-resolver.sh TEST/atomlists/foo.txt
```

Runs `emerge -pv` for every entry in `atomlists/l0-resolve.txt` under
both real portage (`/usr/sbin/emerge`) and portuale
(`/usr/local/bin/emerge`) inside a throwaway container, then
`compare/resolve-compare.py` diffs the merge lists / USE / order /
errors / exit codes. Findings that match `compare/known-divergences.yaml`
are "explained"; the run is green iff none are unexplained.

Output: `TEST/logs/l0-<timestamp>/` (raw `emerge` outputs, `meta.tsv`,
`fingerprint.tsv`, `l0-report.txt`, `l0-report.json`);
`TEST/logs/l0-report.txt` symlinks the latest.

Env: `L0_SKIP_PORTAGE_UPGRADE=1` (skip the `=sys-apps/portage-3.0.82.2`
step), `L0_SKIP_MULTI=1` (skip the `@system`/`@world` whole-graph runs),
`L0_EMERGE_OPTS`, `PORTTEST_IMAGE`, `PORTTEST_PODMAN`.

### L1 — merge parity from an identical prebuilt binpkg set

```sh
TEST/run/l1-merge-from-binpkg.sh                       # the default set
TEST/run/l1-merge-from-binpkg.sh TEST/atomlists/l1-porttest.txt   # synthetic edge cases
```

`atomlists/l1-porttest.txt` is the `porttest` synthetic set (§7 of
`docs/history/real-world-testing.md`) — nine fixtures each isolating one
merge-path behaviour (setuid/caps, hardlinks, symlink farm, `keepdir`,
`dodoc`, `INSTALL_MASK`, `pkg_*` phase markers, `splitdebug`, unicode
names). Live-mounted from `images/overlay/porttest/`, staged only when
the atom list has `porttest/` atoms — no image rebuild.

Portage builds `atomlists/l1-merge.txt` (+ deps) from source **once**,
into a persistent `TEST/logs/_l1-pkgcache/` `$PKGDIR`. Then Portage and
portuale each `emerge -k --getbinpkg --oneshot` that same `$PKGDIR` into their own
fresh container's `/`; `compare/snapshot.sh` captures exactly the merged
files + `/etc` + the VDB; `compare/normalize.py` strips the legitimately-
volatile bits (`BUILD_TIME`/`COUNTER`, `env_update` output, `.pyc`,
regenerated caches — see `normalize.md`); `compare/diff.py` emits typed
findings (`MISSING`/`MODE`/`OWNER`/`XATTR`/`SIZE`/`CONTENT`/`SYMLINK`/
`VDB:<file>`/`CONTENTS`, plus a non-fatal `MTIME` count). Green iff every
hard finding matches `known-divergences.yaml`.

Output: `TEST/logs/l1-<timestamp>/` (`portage.*` / `portuale.*` snapshots
+ merge logs, `l1-report.txt`, `l1-report.json`);
`TEST/logs/l1-report.txt` symlinks the latest.

Env: `L1_REBUILD=1` (wipe the pkgcache), `L1_SKIP_BUILD=1` (reuse it),
`L1_JOBS`, `L1_SKIP_PORTAGE_UPGRADE`, `PORTTEST_IMAGE`, `PORTTEST_PODMAN`.

> **Do not pass `L1_SKIP_PORTAGE_UPGRADE=1` for L1.** The image's base
> portage (`3.0.81.3`) predates the VDB **consolidated `metadata` file**
> (`_consolidate_to_metadata_file`, `vartree.py`), which portuale mirrors
> because it targets `3.0.82.2` (the version the run upgrades to). Skip
> the upgrade and every merged package shows a spurious
> `[VDB] …/metadata  present for portuale, absent for portage` finding —
> a reference-version artefact, not a portuale bug. (L0 is `--pretend`
> only, so `L0_SKIP_PORTAGE_UPGRADE=1` there is fine and faster.)

The reinstall + upgrade sub-cases are a slice-2 follow-up.

### L2+ (not yet implemented)

`net/up.sh` / `net/down.sh` (shared network + volumes) for the HTTP
binhost / `mrg` client; `compare/gpkg-{structure,diff}.sh`. See §14.

### Layout

```
run/          host orchestrators (l0-resolver.sh, l1-merge-from-binpkg.sh, lib.sh)
layers/l0/    in-container.sh — the per-atom probe driver
layers/l1/    build.sh (Portage, from source) + consume.sh (one PM, merge + snapshot)
atomlists/    curated atom / package lists
compare/      resolve-compare.py (L0), snapshot.sh + normalize.py + diff.py (L1),
              normalize.md, known-divergences.yaml
net/          up.sh / down.sh
images/       Containerfile material + overlay/porttest/
logs/         run output (git-ignored)  — incl. _l1-pkgcache/ (the binpkg cache)
```

---

## The legacy `/init` probes

From the repo root:

```sh
podman run --rm --cgroups=enabled --cgroupns=private \
  --security-opt seccomp=unconfined \
  -v ./TEST/scripts:/TEST/scripts -v ./TEST/logs:/TEST/logs \
  -v "$PWD/rust/target/release:/usr/local/bin" \
  localhost/test-portuale
```

- `/TEST/scripts` — executables run in lexicographic order by `/init`.
- `00-install-portage.sh` — upgrades the image's stable portage to
  `~amd64 =sys-apps/portage-3.0.82.2` (the version portuale mirrors).
  Must stay lexicographically first.
- `10-config-dump.sh`, `20-real-compare.sh`, `31-…`, `40-…`, `42-…`,
  `43-…` — earlier-slice reproducers.

Inside the container you are `root` (user namespaces; uid 1000 outside).
