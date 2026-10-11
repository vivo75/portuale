# crates/portage-vdb/src/scratch.rs
Scratch copies of an entry for code that needs a filesystem path.

## module
- Callers that need paths: `PORTAGE_UPDATE_ENV` (the `pkg_postinst` rewrite of `environment.bz2`), the saved-environment phases of an installed instance (`pkg_prerm`, `pkg_postrm`, `pkg_config`), and `quickpkg`.
