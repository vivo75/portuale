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
   [`docs/real-world-testing.md`](../docs/real-world-testing.md). Built
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

### L1+ (not yet implemented)

`net/up.sh` / `net/down.sh` (shared network + volumes), `compare/snapshot.sh`
(filesystem/VDB manifest — done), `compare/normalize.py` + `compare/diff.py`
(skeletons, slice 2). See the doc's slice plan (§14).

### Layout

```
run/          host orchestrators (l0-resolver.sh, lib.sh)
layers/l0/    in-container.sh — the per-atom probe driver
atomlists/    curated atom lists
compare/      resolve-compare.py, snapshot.sh, normalize.{py,md}, diff.py,
              known-divergences.yaml
net/          up.sh / down.sh
images/       Containerfile material + overlay/porttest/
logs/         run output (git-ignored)
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
