# crates/portuale/src/privileges.rs
The privilege check for merging into a root.

## module
- Portage's `portage.data`'s own security-level model (`lib/portage/data.py:93-166`): install/uninstall operations that write into a root-owned filesystem need `uid == 0`. The one documented exception is Portage "unprivileged mode" (`_unprivileged_mode`, `data.py:111-114`): a non-root user who *owns* the target root -- can write to it, and it is not merely world-writable -- may still merge into it (Portage `secpass` becomes 2, using the root dir's own uid/gid for installed files). That covers a prefix / chroot / staging tree the user set up themselves.
- Portage additionally has a middle tier (`secpass == 1`, membership in the `portage` group) that only grants `--fetchonly` / `--buildpkgonly`; portuale has no `portage`-group concept, so this collapses to a plain "privileged or not" check -- matching Portage's own behaviour for every real merge/unmerge action, which always requires `secpass == 2`.

## fn is_privileged
- Portage `uid == 0 or _unprivileged_mode(eroot, eroot_st)` (Portage `secpass == 2`): may this process merge/unmerge into `root`? `true` when the effective uid is 0, or when a non-root caller owns `root` (writable, and not writable only because of the world-writable bit -- Portage `not eroot_st.st_mode & 0o0002`). When `root` does not exist yet, the nearest existing ancestor is checked, matching Portage `first_existing`.
- Writable only because it is world-writable -- Portage does not treat this as ownership.

## fn deny_superuser
- Portage `actions.py:3965-4012`'s own error path, narrowed to portuale's single privilege tier: print `<prog>: superuser access is required` and the hint that `--pretend` previews without it. `prog` is `"emerge"` or `"ebuild"`.

## fn path_writable
- The one `os.access(path, W_OK)` copy. `fetch.rs::distdir_writable`, `info_files.rs::is_writable`, `pretend/news.rs`'s state-dir gate and the global-updates `pkgdir`/`path` gates delegate to it; the ancestor-walk wrappers (`is_privileged`'s ownership probe, the binpkg `pkgdir_writable` `first_existing` walk) stay local to their Portage call sites.

## tests
- `a_dir_the_caller_owns_is_unprivileged_mode`: a tmp dir this process just created -- owned, writable, not world-writable after the explicit chmod. A not-yet-created child resolves to the owned parent.
- `root_is_always_privileged` / `a_world_writable_dir_is_not_ownership` / `a_dir_the_caller_cannot_write_is_not_privileged`: return early for a root caller (root passes before the mode check); `/usr` exists everywhere, is root-owned and not world-writable.
