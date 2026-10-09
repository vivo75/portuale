# `bin/` — vendored Portage phase runtime

This directory is a **vendored copy of upstream Portage's `bin/`** — the
bash that `portuale` sources and executes for real ebuild phase
execution. It ships with the pilot so `emerge` runs on a host with **no
Portage installed and no Portage checkout**.

Copied verbatim from the ref recorded in `3rdparty/repos.toml`'s
`[portage]` entry, *except* `phase-functions.sh`, which carries one local
change (see its own header comment — brush strategy #2).

## What's here

| | |
|---|---|
| `*.sh` | `ebuild.sh` and its whole `source` closure: `isolated-functions.sh` → `eapi.sh`, `version-functions.sh`; `phase-functions.sh`, `phase-helpers.sh`, `save-ebuild-env.sh`, `bashrc-functions.sh`; `misc-functions.sh` (→ `ebuild.sh`); `helper-functions.sh` |
| `ebuild-helpers/` | every `dobin`/`doins`/`emake`/`prepstrip`/… helper (all bash; the `elog`/`newins`/`prepall`/`chown` symlinks preserved) |
| `estrip`, `ecompress` | referenced by `misc-functions.sh`'s `install_qa_check` |
| `*-qa-check.d/` | the `install`/`preinst`/`postinst` QA-check script sets `misc-functions.sh` sources |
| `filter-bash-environment.py` | stdlib-only (no `import portage`); `__filter_readonly_variables` runs it on every phase's env save |
| `portuale-python` | **portuale-owned** (not upstream): `PORTAGE_PYTHON` points here in native mode; execs `$PORTUALE_BIN __helper python` (#326 D1) |
| `chmod-lite` | **portuale-owned** (not upstream): execs `$PORTUALE_BIN __helper chmod-lite`, or the checkout's script with `PORTUALE_PYTHON_HELPERS=real` (#326 D1/D2) |
| `ebuild-ipc` | **portuale-owned** (not upstream): always exits 127, there is no IPC daemon (#326 D6) |
| `portageq-wrapper` | **portuale-owned** (not upstream): execs `$PORTUALE_BIN portageq` (feat#157 S6); no `PATH` fallback (#326 D5) |
| `ecompress-file` | upstream's bash helper, vendored verbatim (it is not in the `3rdparty` checkout overlay path; a missing copy is fatal in `ecompress`) |

## What's *not* here (and needs no checkout)

The `.py` helpers that `import portage` — `doins.py` (so `doins` /
`newins` / `dodoc` / `newbin` / …), `dohtml.py`, `install.py`,
`xpak-helper.py`, `gpkg-helper.py`, `xattr-helper.py` (and the old
`chmod-lite.py`) — are **not vendored because they are all answered
natively** (`rust/portuale/src/helpers/`, #326 S2–S7): in native mode
`PORTAGE_PYTHON` points at `portuale-python`, which execs
`$PORTUALE_BIN __helper python`. The D1 transition table is gone, so
anything unrecognised — `dohtml.py` included, which is never in scope
(`dohtml` dies first for EAPI ≥ 7) — exits 127 like any unknown
helper. `chmod-lite`/`ebuild-ipc` above are shims, not the upstream
scripts. With no checkout, phases run exactly as with one; only
`PORTUALE_PYTHON_HELPERS=real` (the D2 oracle handle for differential
tests) builds the symlink overlay onto `<checkout>/bin/` and needs
`3rdparty/portage` present.

## Re-syncing from upstream

When `3rdparty/repos.toml`'s `[portage] commit` is bumped: copy
each file here over from the new upstream tree, then re-apply the local
change noted in `phase-functions.sh`'s header. A plain
`diff -r 3rdparty/portage/bin bin` (ignoring `phase-functions.sh`,
`portageq-wrapper`, the other portuale-owned files listed below, and
this README) should otherwise be empty. A re-sync that adds or changes
a Python call site needs its helper ported first — unknown names exit
127 by design (#326 D1), so a new upstream helper would fail loudly
until `rust/portuale/src/helpers/` answers it.

## Portuale-owned files (a re-sync must not overwrite)

These have no upstream counterpart; `diff -r` ignores them and a re-sync
never copies over them:

- `portuale-python`, `chmod-lite`, `ebuild-ipc` (the `__helper` shims);
- `portageq-wrapper` (the native `portuale portageq` shim);
- `ecompress-file` is upstream's file vendored verbatim (it *is*
  overwritten by a re-sync, byte-identical).

## Embedded in the binary (backlog #322)

`rust/portuale/build.rs` compiles this whole directory, and the sibling
`cnf/sets/portage.conf` (a vendored copy of upstream's package-set
definitions, same ref), into the `portuale` binary. Resolution order for the
runtime (`ebuild_phases::resolve_bin_dir`): `$PORTUALE_BIN_DIR` (a set but wrong
value is an error), then this directory in the tree the binary was built in,
then the embedded copy extracted to `$TMPDIR/portuale-rt.<pid>/bin` and
removed at exit. So a lone binary in a container or on a minimal host runs
phases and remote merges. `cargo` reruns the script when anything under `bin/`
or `cnf/` changes; a container build needs `COPY bin/ bin/` and
`COPY cnf/ cnf/` next to `rust/` (see `musl/Containerfile`).
