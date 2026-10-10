# crates/portage-repo/src/md5_dict.rs
The `md5-dict` aux-cache validator shared by `--regen` and the depend-phase fallback.

## module
- Portage walks three auxdb rungs for a cpv -- the repo's pregen `metadata/md5-cache`, the writable depcachedir, then `doebuild(mydo="depend")` -- and runs the same validator on the first two. The pregen cache is `flat_hash.md5_database` (`store_eclass_paths = False`), the writable depcachedir is `mtime_md5_database` via `portdbapi.auxdbmodule` (`config.py:524`), which writes `_eclasses_` as `name\tpath\tmd5` triples (`cache/template.py:23`); Portage's `validate_and_rewrite_cache` compares the checksum field and ignores the path (`eclass_cache.py:150-170`).

## fn eapi_is_supported
- `const.EAPI = 9` (`portage/__init__.py:391`); sets `_testing_eapis` and `_deprecated_eapis` as listed in the code.

## fn eapi_has_slot_operator
- Rule is `Eapi(eapi) >= Eapi("5")` (`eapi.py` else-branch), where `Eapi` compares only the integer before any `-` suffix (`"9-pre1"` reads as 9). Portage evaluates the attribute strictly after the support gate in `FakeVartree._apply_dynamic_deps` (`lib/_emerge/FakeVartree.py:158-169`); the only reachable inputs here are `"0"`–`"9"`.

## fn eapi_empty_groups_always_true
- `empty_groups_always_true = eapi <= Eapi("6")` (`eapi.py:295`), same dash-suffix-insensitive integer comparison. Portage's defaults struct for `None`/unsupported EAPIs (`eapi.py:247-253`) is also what Portage's `dep_check` feeds `use_reduce` for installed packages (`dep_check.py:865-877`).

## fn eapi_has_iuse_effective
- Backlog #230. PMS defines `IUSE_EFFECTIVE` beginning with EAPI 5, instead of the pre-EAPI-5 `_get_implicit_iuse` rule; governs the `PORTAGE_USE` masking (`config.py` setcpv). Both attributes share the `Eapi(eapi) >= Eapi("5")` gate.

## fn eapi_has_idepend
- `idepend = eapi >= Eapi("8")` (`eapi.py:304`). Backlog #194: the `--onlydeps-with-rdeps=y` oracle cell on an EAPI-7 root merges `[B, C, D]` with no `E` for exactly this reason. `portdb.aux_get` serves `""` and `depgraph.py` never walks it.

## fn cache_entry_is_valid
- Validation mirrors `cache/template.py::validate_entry`: the entry parses as `KEY=value` lines (a corrupt line invalidates, Portage's `flat_hash._parse_data` raising `CacheCorruption`); `_md5_` equals the md5 of the current ebuild file bytes; `EAPI` defaults to `"0"` when missing/empty (Portage's `porttree.py:644-648`) and unsupported-EAPI entries are disregarded so `depend` re-runs; every `_eclasses_` pair/triple still matches the eclass file that wins across the `masters`-then-self chain (Portage's `eclass_cache.py::validate_and_rewrite_cache`; the last tree in the chain holding the file wins, the same rule `portuale::regen::eclasses_field` documents). A missing/empty `_eclasses_` validates (nothing inherited, Portage's empty-dict success); a missing eclass file, a differing md5, or a malformed field length invalidates.
- Falling through to `EbuildMetadataPhase` when no auxdb hits is Portage's behavior.

## fn entry_is_valid
- Shared by `--regen` (the repo's pregen `metadata/md5-cache`, pairs), the depcachedir rung (`ebuild_phases::depend_phase_metadata`, triples) and `repo_aux_metadata`'s read-path validation.

## fn eclass_md5
- Portage's hot path: the resolver's per-candidate validation reads the same eclass many times.

## tests: triple_eclass_format
- S0 cell (a): the writable depcachedir is `mtime_md5_database` (`store_eclass_paths = True`); validation reads the `(dir, checksum)` tuple's `itemgetter(1)`, i.e. checksum only.
