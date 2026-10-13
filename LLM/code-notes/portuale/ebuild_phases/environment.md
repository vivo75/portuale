# crates/portuale/src/ebuild_phases/environment.rs
One package's build environment: paths, directories, fake DISTDIR.

## struct Environment
- The real directory layout `doebuild_environment()` computes (all paths, `PORTAGE_BUILDDIR`-relative, matching Portage `lib/portage/package/ebuild/doebuild.py:499-524` exactly): `D`/`ED` get a trailing separator, matching Portage's own convention (every real helper script's own path-joining assumes it).; the EAPI<=6 rule now lives in `eapi_path_var` (`phase_env.rs`).
- Space-joined eclass names for `INHERITED` (Portage `porttree.py:872`'s `" ".join(mydata["_eclasses_"])`), from the ebuild's own repo `metadata/md5-cache` entry. `None` for a standalone ebuild outside any repo, or one that inherits nothing. Exported into every phase env so `bin/ebuild.sh`'s own `__INHERITED_QA_CACHE=${INHERITED}` snapshot (then `unset INHERITED`, then re-source) suppresses the spurious `Eclass '…' inherited illegally` QA notice when a non-`depend` phase re-sources the ebuild -- see `phase_env_vars`.

## fn compute_environment
- Portage `porttree.py:872`: `INHERITED = " ".join(_eclasses_)` -- the eclass names from this ebuild's own repo `metadata/md5-cache` entry, in order. Modern md5-cache stores `_eclasses_=<name>\t<md5> \t<name>\t<md5>…`; older/fixture caches store a plain `INHERITED= <space list>`. Absent for a standalone ebuild outside any repo.
- Portage `prepare_build_dirs._prepare_fake_filesdir` (`lib/portage/package/ebuild/prepare_build_dirs.py:504-515`): `${PORTAGE_BUILDDIR}/files` is always a symlink to the ebuild's own `O/files` (the repo package dir's `files/`), even when that target does not exist -- Portage links unconditionally (a dangling link is fine: `FILESDIR` consumers fail loudly on a missing path either way, and crucially the `clean` phase's `rm -f .../files` only works on a link). The phase env's `FILESDIR` is `Environment::filesdir()` = builddir/files (matching Portage `doebuild.py:527`), so without this link every `eapply`/ `FILESDIR` reference in a real ebuild (app-misc/jq's `jq-1.6-r3-never-bundle-oniguruma.patch` is what surfaced it) dies on a missing path -- L2 S5 finding `l2-filesdir-symlink-missing`. A binary merge's ebuild is copied *into* the builddir (pkg_dir == portage_builddir), where the link would be a self-loop. Backlog #94 (host `emerge -1 sys-libs/timezone-data` residue): portuale used to link only when the repo `files/` was a real dir and otherwise let `create_directories` make a plain directory -- whose `rm -f` in `__dyn_clean` then fails with "Is a directory" and, under the phase's `set -e`, aborts the rest of the cleanup, leaving `.../files` (+ parents) behind in `/var/tmp/portage`. Link unconditionally now; `create_directories` skips the symlink (it must not `create_dir_all` through a dangling link).

## fn ensure_fake_filesdir_link
- Portage `bin/*.sh` needs every directory it writes into (via helpers, or bare bash redirection) to already exist -- `__dyn_unpack` creates `WORKDIR` itself, but `T`/`D`/`HOME` are relied on existing already (Portage `doebuild()` creates them via `prepare_build_dirs()` before spawning the phase at all). `S` is deliberately NOT created here (backlog #190): Portage `S` only ever exists because `src_unpack`'s own `unpack ${A}` call creates it as a side effect of extracting a real, fetched `SRC_URI` archive, or because the ebuild sets `S` itself (e.g. `S="${WORKDIR}"`). Portage `bin/phase-functions.sh`'s `__dyn_prepare`/`__dyn_configure`/ `__dyn_compile`/`__dyn_test`/`__dyn_install` die with `The source directory '${S}' doesn't exist` when `S` is not a directory (`phase-functions.sh:444,480,512,546,638`; default `S=${WORKDIR}/${P}` from `bin/ebuild.sh:500`; the `${WORKDIR}` fallback is EAPI 0-3 only, `bin/eapi.sh:8`; the empty-`A` fallback fires only when no phase up to the current one is defined). Pre-creating `${S}` would make that `[[ -d ${S} ]]` check always pass, so a source-less ebuild with no `S` would build under portuale while Portage dies -- exactly the #190 divergence. Portage `_prepare_fake_filesdir` (`prepare_build_dirs.py:504-515`) as a standalone step: `link_path` (`${PORTAGE_BUILDDIR}/files`) becomes a symlink to `target` (the repo package dir's `files/`), unconditionally -- dangling included, exactly like Portage. Removal safety (the link target may live anywhere, including the repo itself): `symlink_metadata` never follows the link, so only the link entry (or a plain file) is ever `remove_file`d; a real directory -- the pre-fix leftover shape, or anything a phase dropped -- is removed with `remove_dir_all`, but strictly bounded to this one known inside-the-builddir path (never a user path, never followed). The parent builddir is `create_dir_all`ed first so a first-ever build has somewhere to put the link.

## fn prepare_fake_distdir
- Portage also chowns the directory to the portage gid when privileged; portuale models that for no builddir directory.
- A non-directory squatter fails Portage's `ensure_dirs`; removed first here.
- Portage `prepare_build_dirs._prepare_fake_distdir` (`lib/portage/package/ebuild/prepare_build_dirs.py:518-549`): `${PORTAGE_BUILDDIR}/distdir` holds exactly one symlink per file of the package's own `A`, each pointing at `<actual>/<file>` -- so a phase sees only the distfiles it declared. Anything else in it (a stale symlink, a stray file, a directory) is removed; a symlink with the wrong target is recreated. Portage also chowns the directory to the portage gid when privileged; portuale does not model that for any builddir directory (see `create_directories`), so it is left out here too.

## fn create_directories
- `create_dir_all` would follow a dangling `files` link and fail (or materialise the target); `symlink_metadata` does not follow. Everything else listed is always a real dir.
- Portage `bin/ebuild.sh`'s own top-level code (run unconditionally as soon as it's sourced, EAPI 8's own comment: "requires us to use an empty directory here"): `cd`s into `${PORTAGE_BUILDDIR}/empty` if it exists, falling back to `${PORTAGE_PYM_PATH}` (always set in portuale, see `pym_path_value`) otherwise, `die`-ing if neither works. Always pre-created here so that fallback path is never reached at all.
- Portage `prepare_build_dirs` creates `${T}/logging`; `bin/isolated-functions.sh::__elog_base` silently drops every `elog`/`ewarn`/`eerror` message if the dir doesn't exist.

## struct Environment accessors
- `portage_builddir` is also the scratch root for `ebuild_merge`'s replace-loop extracted-from-vdb ebuilds.
- `build_info/${PF}.ebuild`: Portage `phase-functions.sh`'s `__dyn_install` case branch unconditionally `mkdir`s build-info and copies the ebuild in, so by the time `run_package`'s `install` chain completes it pre-exists.

## fn portage_tmpdir
- Portage `${PORTAGE_TMPDIR}` -- the root `bin/ebuild.sh`'s own `addwrite "${PORTAGE_TMPDIR}/portage"` opens to the sandbox, so it must be a real env var whenever `FEATURES=sandbox` is active.

## fn t
- Portage `${T}` -- also read by `ebuild_merge::process_merge_elog` (the per-package `elog` flush before the post-merge clean, #42).

## fn fake_distdir
- Portage `${PORTAGE_BUILDDIR}/distdir` (`config.py:3408`): the per-package fake `DISTDIR`.

## fn build_info
- Portage `${PORTAGE_BUILDDIR}/build-info`: created as a side effect of every Portage `unpack|prepare|configure|compile|test|clean|install` phase already run by the time `ebuild_package::run_package`'s own `install` chain completes (`bin/phase-functions.sh`'s own unconditional `mkdir build-info` in that case branch) -- so by the time packaging needs it, it already exists and already has a real copy of the ebuild file in it (`build-info/${PF}.ebuild`).

## fn installed_marker
- Portage `${PORTAGE_BUILDDIR}/.installed`: real, unmodified `bin/phase-functions.sh`'s own `__dyn_install` already creates this unconditionally on a successful `src_install` (`phase-functions.sh:653`, no `FEATURES` gate at all) -- this portuale writes nothing new for it, real phase execution already leaves it behind as a side effect (confirmed empirically: a Portage `ebuild <file> install` run via portuale's own binary leaves `.installed` in place). `ebuild_merge::run_qmerge` is the one caller: Portage `doebuild()`'s own `mydo == "qmerge"` branch checks for exactly this marker before skipping the install phase.

## fn compute_environment
- #348 S5: the phase `EAPI` prefers the normalized md5-cache value over the head parse (same `repo_aux_metadata` read that feeds `INHERITED`, so no extra I/O): Portage's phase EAPI comes from the cache (`setcpv` → `aux_get`), and the metadata writer word-splits every value — so `EAPI=" 8"` builds as `8`, while the head parse reads `"0"` and used to export `EAPI=0` (helpers gated off → `eapply_user: command not found`, S0 cells `d-eapisp-*`). Outside a repo (or with no `EAPI` key) the head value stands. Portage: `doebuild.py:doebuild_environment` → `setcpv`.
