# crates/portuale/src/ebuild_phases/tests/environment.rs
Environment-setup unit tests (md5-cache reads, FILESDIR/distdir links, builddir layout). Context moved out of the code comments (#336 Phase 7); the code keeps only the short why.

## fn compute_environment_reads_inherited_from_the_md5_cache
- `compute_environment` reads `INHERITED` from the ebuild's `metadata/md5-cache` entry (Portage's `porttree.py:872`) so `phase_env_vars` can export it and `bin/ebuild.sh` can snapshot it into `__INHERITED_QA_CACHE`, suppressing the spurious `Eclass '…' inherited illegally` QA notice on a phase re-source.
- `dev-libs/eclasspkg`'s fixture cache carries `INHERITED=pilotcheck`. A standalone ebuild outside any repo has no md5-cache, so the value is `None`.

## fn compute_environment_links_builddir_files_to_the_repo_filesdir
- Ports Portage's `_prepare_fake_filesdir`: FILESDIR is builddir/files, a symlink to the ebuild's repo `files/`. Without it every ebuild patch via `eapply` dies (L2 S5).
- Re-running is idempotent (Portage unlinks a stale target).

## fn compute_environment_links_builddir_files_even_without_a_repo_filesdir
- Backlog #94: Portage's `_prepare_fake_filesdir` links unconditionally -- a missing repo `files/` must yield a dangling symlink, never a plain directory (whose `rm -f` in `__dyn_clean` fails with "Is a directory" and aborts the rest of the cleanup under `set -e`, leaving `/var/tmp` residue -- seen on the host with `emerge -1 sys-libs/timezone-data`).
- `create_directories` leaves the dangling link alone instead of failing on it (or materialising the target).

## fn compute_environment_replaces_a_stale_plain_filesdir_with_the_link
- Migration path for pre-fix residue: a plain directory left at builddir/files (the shape `__dyn_clean` choked on) becomes the symlink again instead of erroring.

## fn prepare_fake_distdir_prunes_and_relinks_like_portage
- Expected value from Portage's `_prepare_fake_distdir` (lib/portage/prepare_build_dirs.py:518-549): entries that are not a symlink named in `A` (a stale symlink, a stray directory, a plain file) are removed, a symlink with the wrong target is recreated, a correct one is kept (pinned by comparing inode numbers before/after).

## fn compute_environment_builds_the_builddir_under_the_given_tmpdir
- `compute_environment` derives `PORTAGE_BUILDDIR` as `<tmpdir>/portage/<cat>/<pf>`, so a matched per-package tmpdir puts the whole build tree under it (S0 cell D's `PORTAGE_BUILDDIR` half).
