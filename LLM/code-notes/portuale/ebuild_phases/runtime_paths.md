# crates/portuale/src/ebuild_phases/runtime_paths.rs
Where the phase runtime (`bin/`, `pym/`) and the repository live; the embedded runtime.

## fn repo_root
- Every consumer only joins a path onto this and checks existence, so a missing tree degrades like a missing `3rdparty/portage`.

## fn portage_checkout
- Native mode: every helper is native (`helpers/`, via `bin/portuale-python`); `has_version`/`best_version` go through the vendored `portageq-wrapper` shim (native `portuale portageq`).
- Callers degrade like a missing binary when nobody cloned the checkout.

## fn pym_path_value
- Portage's value is the realpath of its own `lib/`, exported as a constant config key. Shells die on a missing value: `__dyn_clean` `cd`s unconditionally (a cwd outside the build dir, since the dir is removed), and `bin/ebuild.sh` has the same `die`. No checkout means no helper needs the path (all native) — the variable only preserves Portage's never-empty invariant.

## fn pythonpath_value
- `PYTHONPATH=${PORTAGE_PYTHONPATH:-${PORTAGE_PYM_PATH}}` for the `gpkg-helper.py`/`xpak-helper.py` invocation, so with a checkout the helper imports `portage` from it.

## fn bin_dir
- Vendored `bin/` contents: all `.sh` (`ebuild.sh` and its source closure), every `ebuild-helpers/` script, `estrip`/`ecompress`/`ecompress-file`, the `*-qa-check.d/` sets, stdlib-only `filter-bash-environment.py` — so `emerge` runs with no Portage installed. Portuale-owned shims beside them: `portuale-python` (`PORTAGE_PYTHON` in native mode), `chmod-lite` (native helper, or the upstream script in oracle mode), `ebuild-ipc` (always 127: no IPC daemon), `portageq-wrapper` (native `portuale portageq`).
- The `.py` helpers importing `portage` (`doins.py`, `xpak-helper.py`, `gpkg-helper.py`, `dohtml.py`, `xattr-helper.py`, `install.py`) are not vendored. In native mode `portuale-python` answers them all natively (`helpers/`) except `dohtml.py`, never in scope (`dohtml` dies first for EAPI 7+) and exiting 127 like any unknown helper.
- Native mode (unset or `native` `PORTUALE_PYTHON_HELPERS`, the default): `PORTAGE_BIN_PATH` is the vendored `bin/` directly, no overlay. Oracle mode (`PORTUALE_PYTHON_HELPERS=real`, the differential-tests handle needing installed Portage): symlink overlay, vendored entries win, non-vendored `.py` helpers fall through to `<checkout>/bin/`.
- `portageq-wrapper` is deliberately vendored: the shim execs native `portuale portageq` via `$PORTUALE_BIN`, and vendored entries link over the checkout's, so the shim wins even with `3rdparty/portage` present — `has_version`/`best_version` never need Python or the checkout.
- A symlinked entry resolves to the vendored file because `ebuild.sh` never `realpath`s the prefix.

## fn exec_scratch_base
- Helpers run by name from `PATH`; hardened hosts mount `/tmp` `noexec`. Candidates: `$TMPDIR`, `/var/tmp`, `/dev/shm`, then `XDG_RUNTIME_DIR`.

## fn register_cleanup
- `bin_dir()` has call sites in three files with independent exit paths; per-site removal would sprawl and rot. One `atexit` registration on the first directory created covers every command (and the test harness's subprocess runs) on all normal exits. A failed registration keeps the old leak rather than breaking the run.

## fn require_phase_runtime
- Called at the top of a run so a missing runtime reports up front, never half-way through a plan.
