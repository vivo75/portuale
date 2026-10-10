# crates/portage-repo/src/env_roots.rs
ROOT, PORTAGE_CONFIGROOT and PORTAGE_TMPDIR from the environment.

## const PORTAGE_TMPDIR_DEFAULT
- Portage `cnf/make.globals:35`'s own `PORTAGE_TMPDIR="/var/tmp"`. Portage `doebuild_environment()` builds the per-package `${PORTAGE_TMPDIR}/portage/${CATEGORY}/${PF}` on top of this value (`doebuild.py:504-524`) -- the double `portage/portage/` builddir bug in `TEST/findings/l2.md` "#38 recon" (`l2-gpkg-dostrip-splitdebug` S0) traced to this boundary defaulting to `/var/tmp/portage` instead.

## fn portage_tmpdir_from_env
- Serves CLI boundaries with no resolved config: `ebuild <file>`, `execute_unmerge`, `emerge --regen`.

## fn portage_tmpdir_from_config
- Reads the `make.globals` + profile chain + `make.conf` fold in `Config::other_vars`.

## fn running_root_from_env
- `--root-deps`'s own Portage running-root default: Portage `ESYSROOT` resolves to the Portage *build machine's* own `/` whenever `SYSROOT` is left unset (see `running_root_satisfies_atom`'s doc comment for the full grounding) -- `/` is the correct Portage default here too, for the same reason `config_root_from_env`/`root_from_env` default to `/`. `PORTAGE_RUNNING_ROOT` itself is NOT a Portage portage environment variable (Portage portage has no way to override this at all -- it's always genuinely `/`, full stop, outside of Prefix's own `BROOT` concept) -- a portuale-specific override purely so a test can point this at a fixture's own fake vdb tree instead of the real host, the same "explicit override for tests, Portage default at the CLI boundary" pattern `MergeOptions::config_root` already established.
