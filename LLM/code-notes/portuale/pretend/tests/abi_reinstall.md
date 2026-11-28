# crates/portuale/src/pretend/tests/abi_reinstall.rs
Context moved out of the code comments.

## fn a_merge_bound_upgrade_provider_is_not_force_reinstalled
- Backlog numbers removed from the comment (original text): A merge-bound upgrade provider is not force-reinstalled by the ABI edge alone (backlog #623): Portage tags `r` only through a `force_reinstall` arg (`output.py`, `PkgAttrDisplay.force_reinstall`), and the only such args are the rebuild/reinstall/replace sets (`_gen_reinstall_sets`, `depgraph.py:5479-5480`) -- there is no provider-side ABI rule. Verified live against host Portage 3.0.83: `dev-libs/boost-1.53.0` (upgraded over installed 1.52.0), `virtual/libmysqlclient-21`, `dev-libs/slotbindtarget-2.0` and `dev-libs/casctarget-2.0` all print a plain `U` with no rebuild of their consumer. Since #819 the replace set no longer tags an upgrade either (`pretend::run`), so no upgrade carries an `r`.
