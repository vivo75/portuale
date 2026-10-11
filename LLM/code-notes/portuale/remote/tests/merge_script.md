# crates/portuale/src/remote/tests/merge_script.rs
Merge-script render and dispatch tests.

## tests: generated_scripts_use_absolute_portuale_bin
- #326 S8.3: also pins `PORTAGE_PYM_PATH=/`, the unset `PORTAGE_IPC_DAEMON`, and `PORTAGE_PYTHON` through the bundle's own `$UNITBIN/portuale-python`.

## tests: phase_exports_gates_merge_type_on_eapi
- Backlog #172: Portage `_emerge/Binpkg.py:92` sets `MERGE_TYPE=binary` for a binpkg merge, and Portage `config.environ()` (`config.py:3335-3336`) exports it only when `eapi_exports_merge_type` holds (EAPI >= 4, `eapi.py:300`). Bed `l31-20260926T061556Z` shows Portage appending `merge_type=binary` x4 (322 B) where mrg wrote `?` x4 (302 B). `mrg` only merges binpkgs, so `binary` is the only value.

## tests: phase_script_runs_pkg_pretend_from_saved_env
- Positive pretend dispatch through the Portage stack. Proves template + `ebuild.sh` + DEFINED_PHASES-agnostic dispatch.

## tests: collect_server_locale_forwards_only_set_vars
- Backlog #171b task 3: `LANG` set + empty-but-set `LANGUAGE` forwarded in `REGEN_LOCALE_VARS` order; unset `LC_ALL` skipped; a non-locale variable never consulted.

## tests: postinst_regen_script_matches_portage_shell_shape
- Backlog #171b tasks 2-3. EAPI 8 is posixish; the script unsets `O`, unsets `SHELL`, unsets `LC_ALL`, and re-exports the given pairs with values quoted like any other export.

## tests: merge_flow_applies_chown_before_chmod
- Backlog #169, site (a): `chown(2)` on a regular file clears `S_ISUID`/`S_ISGID` even when owner and group are unchanged. Portage `lib/portage/util/movefile.py::_apply_stat` does `os.chown` then `os.chmod` for exactly this reason. That needs root to observe behaviourally (no sudo on this host), so this pins the order structurally in `MERGE_FLOW` instead: the `chown --reference` line must precede the `chmod --reference` line in both the `obj` (`$tmp`) and `dir` (`$dest`) branches. Fails while chmod runs first.
