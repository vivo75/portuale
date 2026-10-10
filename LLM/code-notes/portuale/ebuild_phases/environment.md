# crates/portuale/src/ebuild_phases/environment.rs
One package's build environment: paths, directories, fake DISTDIR.

## struct Environment
- Matches `doebuild.py`'s layout exactly. `D`/`ED` carry a trailing separator by Portage's convention (every helper script's path-joining assumes it); the EAPI<=6 rule now lives in `eapi_path_var` (`phase_env.rs`).
- `inherited`: space-joined eclass names (`" ".join(mydata["_eclasses_"])`). Exported into every phase env so `bin/ebuild.sh`'s `__INHERITED_QA_CACHE=${INHERITED}` snapshot (then `unset INHERITED`, then re-source) suppresses the spurious `Eclass '...' inherited illegally` QA notice when a non-`depend` phase re-sources the ebuild.

## fn compute_environment
- The `files` symlink: the phase env's `FILESDIR` is builddir/files, so without the link every `eapply`/`FILESDIR` reference dies on a missing path (surfaced by app-misc/jq's `jq-1.6-r3-never-bundle-oniguruma.patch`).
- History (backlog #94, host `emerge -1 sys-libs/timezone-data` residue): the link used to be created only when the repo `files/` was a real dir, otherwise `create_directories` made a plain directory whose `rm -f` in `__dyn_clean` failed with "Is a directory" and, under `set -e`, aborted the rest of the cleanup, leaving `.../files` (+ parents) in `/var/tmp/portage`. Now unconditional; `create_directories` skips the symlink (it must not `create_dir_all` through a dangling link).

## fn ensure_fake_filesdir_link
- `bin/*.sh` needs every directory it writes into to exist: `__dyn_unpack` creates `WORKDIR` itself, but `T`/`D`/`HOME` must pre-exist (`doebuild()` creates them via `prepare_build_dirs()` before spawning).
- `S` is never pre-created: it exists only because `src_unpack`'s `unpack ${A}` extracts a fetched `SRC_URI` archive, or the ebuild sets `S` itself (e.g. `S="${WORKDIR}"`). `__dyn_prepare`/`__dyn_configure`/`__dyn_compile`/`__dyn_test`/`__dyn_install` die with `The source directory '${S}' doesn't exist` when `S` is not a directory; default `S=${WORKDIR}/${P}` (the `${WORKDIR}` fallback is EAPI 0-3 only; the empty-`A` fallback fires only when no phase up to the current one is defined). Pre-creating `S` would make `[[ -d ${S} ]]` always pass, so a source-less ebuild would build here while Portage dies.
- Removal narrowing: `symlink_metadata` never follows the link. A real directory at the link path is the pre-fix leftover shape (or something a phase dropped).

## fn prepare_fake_distdir
- Portage also chowns the directory to the portage gid when privileged; portuale models that for no builddir directory.
- A non-directory squatter fails Portage's `ensure_dirs`; removed first here.

## fn create_directories
- `bin/ebuild.sh`'s top-level `cd`s into `${PORTAGE_BUILDDIR}/empty` if it exists (EAPI 8's "requires us to use an empty directory here"), falling back to `${PORTAGE_PYM_PATH}` (always set, see `pym_path_value`), `die`-ing if neither works.
- `create_dir_all` would follow a dangling `files` link and fail (or materialise the target); `symlink_metadata` does not follow. Everything else listed is always a real dir.

## struct Environment accessors
- `portage_builddir` is also the scratch root for `ebuild_merge`'s replace-loop extracted-from-vdb ebuilds.
- `build_info/${PF}.ebuild`: real `phase-functions.sh`'s `__dyn_install` case branch unconditionally `mkdir`s build-info and copies the ebuild in, so by the time `run_package`'s `install` chain completes it pre-exists.
- `.installed`: real `__dyn_install` creates it unconditionally on success (no `FEATURES` gate); confirmed empirically that a portuale-run `ebuild <file> install` leaves it behind. Checked by `doebuild()`'s `mydo == "qmerge"` branch before skipping install.
