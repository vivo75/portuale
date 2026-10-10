# crates/portage-repo/src/info_pkgs.rs
Package rows for `emerge --info`.

## struct InfoPkgRow
- From Portage's `action_info`'s second `myvars` loop.

## fn expand_new_virt
- One level of Portage's `expand_new_virt` (`portage.dbapi._expand_new_virt`). Portage recurses into a second virtual reached this way; portuale does not (the standard `info` list only ever hits `virtual/os-headers`, which resolves in one hop).

## fn info_pkgs_table
- Mirrors Portage's `action_info` (`actions.py:2109-2165`): `<ver>::<repo>` uses Portage's `_unknown_repo` -> `<unknown repository>`; ` (virtual/foo)` marks a virtual-provided row.
- The atom is parsed only for `atom.cp` (`vardb.match` in Portage); the operator/version/slot/use-dep is stripped first.

## fn info_pkgs_table (dup-provider preference)
- Prefers a duplicate match carrying provider info (Portage's `if prev_match.provide_suffix: continue`).

## fn resolve_info_candidate
- What `emerge --info <atom>` shows in Portage's `action_info` per-package "`<cpv>::<repo> would be built with the following:`" block (`actions.py:2321` + `pkg_use_display`). Portage skips an atom with no visible ebuild candidate with a `- <atom>` "no matches" note (rendered by the caller).
- The USE display is built exactly the way a `New` `GraphEntry`'s `use_expand_display` is (all flags, Portage's `pkg_use_display` for a non-installed package).
- Dedup: Portage renders `sorted(pkg.iuse.all)`, a frozenset (see `refresh_entry_use_display`).
- The `mypkgs` gate (`actions.py:1896-1900`); the ebuild path is Portage's `portdb.findname(pkg.cpv, myrepo=pkg.repo)` for the `pkg_info()` run (`actions.py:2360`).

## struct InfoCandidate
- `use_expand_display` is Portage's `_display_use` output: each value already enabled-first/disabled-last ordered.

## struct InfoBinaryCandidate
- `use_expand_display` is Portage's `pkg_use_display` for a `built=True` package. The archive lookup is Portage's `bindb.bintree.getname(cpv)` + `xpak.tbz2(...).getfile` / `gpkg (...).get_metadata` (`actions.py:2362-2384`); the scratch layout lets the phase driver derive `CATEGORY`/`PN` from the path the way Portage's `doebuild(tree="bintree")` derives them from the binpkg metadata.

## fn has_local_binary_candidate
- Portage's `action_info` `bindb.match(x.cp)` `cp_exists` probe (`actions.py:1883`). Takes the run's already-built index (backlog #123) instead of re-deriving it from `&Config`.

## fn resolve_info_binary_candidate
- Portage's `(portdb, "ebuild"), (bindb, "binary")` search loop (`actions.py:1876-1900`). Portage never lists a binary candidate it couldn't run `pkg_info()` for, so unlike the ebuild path there is no separate "list but don't run" state. Takes the run's already-built index (backlog #123).
- Highest version first, then the first passing `pkg_info()`: Portage's `matches.reverse()` + `break`.

## const INFO_INSTALLED_VARS
- Portage's `mydesiredvars` (`actions.py:2333-2344`).

## fn read_vdb_env_vars
- Portage decompresses by spawning `bzip2 -d -c` (or `PORTAGE_BUNZIP2_COMMAND -c`); this crate is subprocess-free, so it decompresses in-process with the pure-Rust `bzip2` backend instead (see this crate's `Cargo.toml` -- zero C linkage, musl story untouched). Portage's own `if not os.path.isfile(env_file): return {}` covers missing files; a corrupt archive breaks Portage's pipe the same effective way (empty map).

## fn parse_env_assignments
- Scanner regexps: `var_assign_re = (^|^declare\s+-\S+\s+|^declare\s+|^export\s+)([^=\s]+)=("|\')?(.*)$` with `close_quote_re = (\\"|"|\')\s*$` (`vartree.py:1082-1123`). A quoted value whose closer is not on the same line consumes following lines verbatim until the closer (or EOF); the value is right-stripped with the closing quote dropped -- even at EOF without a closer, Portage still drops the last char. Portage alternation order, longest first (regex backtracking reaches the same): `<ws>` is any whitespace run (Portage `\s+`), and a failed dash-form backtracks to the plain `declare<ws>` form. Portage builds the value before its `if key in variables` check.

## fn resolve_installed_info
- Portage's `None` vs `split()`-equal branches (`actions.py:2336-2342`); the `USE` line's `( )` force/mask wrap is Portage's `pkg_use_display`, via the same `forced_or_masked_flags` the ebuild-candidate side uses.

## struct InstalledInfo::defines_pkg_info
- Unlike the ebuild-candidate path, an installed package is listed in `mypkgs` regardless; the phase step is gated on Portage's `if metadata["DEFINED_PHASES"]: if "info" not in ...split(): continue` (`actions.py:2350-2356`). Fires on empty because the check is falsy-based.
- The ebuild lookup is Portage's `vardb.findname(pkg.cpv)` locating `<pf>.ebuild`; portuale runs it via `ebuild_merge::run_vdb_saved_env_phase`.
