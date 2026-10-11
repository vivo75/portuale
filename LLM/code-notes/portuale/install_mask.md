# crates/portuale/src/install_mask.rs
`INSTALL_MASK` filtering applied to the install image before anything is copied to `${ROOT}`.

## module
- Ports `InstallMask` + `install_mask_dir` (lib/portage/util/install_mask.py) plus the `no{man,info,doc}` `FEATURES` fold that Portage's `preinst_mask()` (bin/misc-functions.sh) does before writing `build-info/INSTALL_MASK`.
- Portage's `dblink.treewalk()` (vartree.py:4581-4610) runs the `preinst_mask` misc-function, reads the `INSTALL_MASK` it wrote into `build-info`, and -- *before* collision-protect and before a single file is copied to `${ROOT}` -- deletes every image path the mask matches, then `rmdir`s a now-empty `ED/usr/share` when any of `nodoc`/`noman`/`noinfo` is in `FEATURES`. Portuale ports that logic directly to Rust (the same "port the shell/Python step, don't spawn a shell for it" approach the vdb-merge code already takes) and calls it from both `merge_after_install` (source) and `merge_binpkg` (binary).
- Live-execution code, covered by Rust unit tests and the L1/L2 merge beds.

## fn resolve
- Portage `preinst_mask()`: fold the `no{man,info,doc}` `FEATURES` tokens into the configured `INSTALL_MASK` (each becomes `${EPREFIX}/usr/share/<man|info|doc>`; portuale assumes an empty `EPREFIX`, like every other real-execution path here). The loop order is Portage's own `for f in man info doc`. Returns the resolved mask string plus whether a `no*` token was folded in -- the latter gates Portage `dblink`'s own `rmdir(ED/usr/share)`.

## struct Pattern
- `pattern` keeps a leading `/` when `leading_slash`; The pattern has any leading `-` stripped (still keeps a leading `/` when `leading_slash`), exactly Portage's `_pattern.pattern`.

## struct InstallMask
- Portage `InstallMask`.

## impl InstallMask::new
- Portage `InstallMask.__init__`: `install_mask.split()`, each token a `_pattern` (a leading `-` flips it non-inclusive).

## impl InstallMask::matches
- Portage `InstallMask.match(path)` -- `path` is relative to `${ED}`, and ends with `/` when testing a directory.
- Portage iterates "relevant" patterns (an fnmatch-call optimisation) but the final `ret` is just "the last matching pattern's inclusiveness" in `orig_index` order, so iterating every pattern in order is equivalent. `self.patterns` is built in `orig_index` order.

## fn install_mask_dir
- Portage `install_mask_dir(base_dir, install_mask)`: delete every masked file under `base_dir`, then remove any directory the mask matches (with a trailing `/`) that ended up empty.
- Per-path errors are swallowed the way Portage's default `onerror`/`except OSError` path does for the common cases.
- The file walk matches Portage's `todo` stack; the dir walk pops parents in visit order, i.e. child-before-parent.
- `rmdir` silently failing on a non-empty dir is what Portage relies on for exclusion (`-`) patterns.

## fn fnmatch
- Python `fnmatch.fnmatch` semantics (case-sensitive, `/` is *not* special, `*` spans everything): translate to an anchored regex. Supports `*`, `?`, and `[...]`/`[!...]` character classes. Also reused by `needed_elf`'s `PROVIDES_EXCLUDE`/`REQUIRES_EXCLUDE` matcher.
- Covers the whole of what Portage `INSTALL_MASK` patterns ever use. Same `fnmatch.translate` semantics as Portage's `SonameDepsProcessor._exclude_pattern`.

## tests: exclusion_pattern_re_includes_a_path_a_later_lower_index_rule_masked
- Portage orders by orig_index; a `-` after a broad mask rescues.
