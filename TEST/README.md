# `TEST/` — real-tree validation for the PORTING pilot

Scaffolding for running `portuale` (and the real `emerge`) against a real
Gentoo tree, inside the `localhost/test-portuale:latest` container image.

## Running

```sh
podman run --rm \
  -v ./TEST/scripts:/TEST/scripts \
  -v ./TEST/logs:/TEST/logs \
  -v "$PWD/PORTING/rust/target/release:/usr/local/bin" \
  localhost/test-portuale
```

- `/TEST/scripts` — executables run in **lexicographic order** by the
  container's `/init`. Add probes here as `NN-name.sh`.
- `/TEST/logs` — initially empty; scripts may write logfiles here.
- `/usr/local/bin` — on `PATH`; mount point for the built `portuale`/`ebuild`
  binaries (`cargo build --release -p portuale` first, and make sure the
  `ebuild` symlink next to `portuale` exists).

Inside the container you are `root` (user namespaces; uid 1000 outside).

## Scripts

- `00-install-portage.sh` — the image ships **stable** portage, but `PORTING/`
  is ported against `~amd64 =sys-apps/portage-3.0.82.2`. This installs that
  first so every later script's real `emerge` matches the version whose
  source `portuale` mirrors. Must stay lexicographically first.
- `20-real-compare.sh` — resolves a handful of real packages with both
  `portuale --pretend` and the real `emerge --pretend` and diffs the
  `[ebuild ...]` lines.
