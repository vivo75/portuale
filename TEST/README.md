# `TEST/` — real-tree validation for portuale

Scaffolding for running `portuale` (and the real `emerge`) against a real
Gentoo tree, inside the `localhost/test-portuale:latest` container image.

## Running

From the repo root:

```sh
podman run \
  --rm \
  --cgroups=enabled \
  --cgroupns=private \
  --security-opt seccomp=unconfined \
  -v ./TEST/scripts:/TEST/scripts \
  -v ./TEST/logs:/TEST/logs \
  -v "$PWD/rust/target/release:/usr/local/bin" \
  localhost/test-portuale
```

The container's own mount points stay `/TEST/scripts` and `/TEST/logs`;
only the host side lives in this repo.

- `/TEST/scripts` — executables run in **lexicographic order** by the
  container's `/init`. Add probes here as `NN-name.sh` (must be `chmod +x`).
- `/TEST/logs` — initially empty; scripts may write logfiles here. Log
  output is git-ignored (`.gitignore` in this dir).
- `/usr/local/bin` — on `PATH`; mount point for the built `portuale`/`ebuild`
  binaries (`cargo build --release -p portuale` first, and make sure the
  `ebuild` symlink next to `portuale` exists).

Inside the container you are `root` (user namespaces; uid 1000 outside).

## Scripts

- `00-install-portage.sh` — the image ships **stable** portage, but portuale
  is ported against `~amd64 =sys-apps/portage-3.0.82.2`. This installs that
  first so every later script's real `emerge` matches the version whose
  source `portuale` mirrors. Must stay lexicographically first.
- `10-config-dump.sh` — dumps the container's `emerge --info` /
  `make.conf` / `EMERGE_DEFAULT_OPTS` and a plain-vs-`-v` `emerge --pretend`
  sample, to `/TEST/logs/10-config-dump.log`.
- `20-real-compare.sh` — resolves a handful of real packages with both
  `portuale --pretend` and the real `emerge --pretend` and diffs the
  `[ebuild ...]` lines (to `/TEST/logs/20-real-compare.log`).
