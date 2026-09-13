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

### L2 — portuale as builder (structure + cross-install)

```sh
TEST/run/l2-portuale-builder.sh TEST/atomlists/l1-porttest.txt   # fixture track, strict
L2_MODE=payload-tolerant L2_BUILD_MODE=deep \
  TEST/run/l2-portuale-builder.sh                               # real L1 set
```

Both package managers build the atom list from source into separate
fresh `$PKGDIR`s with a shared `DISTDIR`
(`layers/l2/build-portage.sh`, `layers/l2/build-portuale.sh`), then:
`gpkg-structure.sh --dir … --packages` validates every archive
(member set, inner roots, the stable metadata-key set, Manifest,
filename↔`BUILD_ID`, `Packages` stanza);
`gpkg-diff.sh --mode strict|payload-tolerant` diffs each
portage-built/portuale-built pair (outer layout, normalised metadata,
image paths/attrs, payload; strict = payload hard, for the
deterministic `porttest` fixtures); finally real Portage merges the
portuale-built **candidate** and the portage-built **reference** into
two identical fresh containers (`layers/l1/consume.sh`), portuale
merges the portage-built set as the L1 **control**, and
`diff.py --layer l2 [--tolerate-payload]` compares the normalised
snapshots. `L2_BUILD_MODE=bpkgonly` (default) is archive-only
`--buildpkgonly`; `deep` builds+merges the dependency closure first
(real `-B` refuses unmerged deps).

Output: `TEST/logs/l2-<timestamp>/` (`structure-*.txt`,
`archive-<cat>-<pn>.txt`, `cross-install.txt`, `control.txt`,
`classification.txt`, `l2-report.txt`, `l2-report.json`);
`TEST/logs/l2-report.txt` symlinks the latest. Env: `L2_MODE`,
`L2_BUILD_MODE`, `L2_REBUILD`, `L2_SKIP_BUILD`, `L2_JOBS`,
`L2_SKIP_PORTAGE_UPGRADE`, `PORTTEST_*`.

Status (2026-09-13): the **fixture track is green modulo filed
producer gaps** — 0 unexplained structural/archive findings, 0
unexplained in the cross-install diff, control 0/0. The real set is
**blocked** on a systemic build-env gap (see
[`findings/l2.md`](findings/l2.md) `l2-bpkgonly-env`); all open
findings are filed there and adjudicated temporarily via
`known-divergences.yaml` (`layer: l2`, `owner: portuale-bug`).

Host-only self-tests (no container): `compare/test-gpkg-structure.sh`,
`compare/test-gpkg-diff.sh`, `compare/test-diff-tolerance.py`.

### L3+ (not yet implemented)

`net/up.sh` / `net/down.sh` (shared network + volumes) for the HTTP
binhost / `mrg` client. Designs, controls, deferred fixtures, risks,
and metrics: `docs/real-world-testing.md` §§2–8 (extracted from
`docs/history/real-world-testing.md`, whose §14 slice history and §1
methodology critique stay there).

### Layout

```
run/          host orchestrators (l0-resolver.sh, l1-merge-from-binpkg.sh,
              l2-portuale-builder.sh, lib.sh)
layers/l0/    in-container.sh — the per-atom probe driver
layers/l1/    build.sh (Portage, from source) + consume.sh (one PM, merge + snapshot)
layers/l2/    build-portage.sh + build-portuale.sh (archive-only / deep)
atomlists/    curated atom / package lists
compare/      resolve-compare.py (L0), snapshot.sh + normalize.py + diff.py (L1/L2),
              gpkg-structure.sh + gpkg-diff.sh (L2), test-*.sh, normalize.md,
              known-divergences.yaml
net/          up.sh / down.sh
images/       Containerfile material + overlay/porttest/ (incl. metadata/md5-cache)
logs/         run output (git-ignored)  — incl. _l1-pkgcache/, _l2-*
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
