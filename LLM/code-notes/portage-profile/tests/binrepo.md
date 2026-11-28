# crates/portage-profile/src/tests/binrepo.rs
`parse_binrepos` pins: frozen defaults and `[DEFAULT]` inheritance.

## fn parse_binrepos_sections_inherit_default_keys
- Every `[section]` inherits the `[DEFAULT]` keys, like Portage's configparser (`BinRepoConfigLoader` reads `dict(parser[section].items())`, which includes defaults) (#539). A keyless section under `[DEFAULT] verify-signature = false` opts out of `gpkg_only`; without the default the stock-global `true` applies.

## fn parse_binrepos_reads_getbinpkg_exclude_and_include_keys
- Backlog numbers removed from the comment (original text): Backlog #347 S9: the per-binrepo `getbinpkg-exclude/include` keys (Portage `binrepo/config.py:42-75`, Q7 default yes). Grounded in S0 cell `g04-repokey` (a `getbinpkg-exclude = app-misc/s346-gbin-a` key filters the remote index like the CLI flag).

## fn parse_binrepos_drops_bad_getbinpkg_atoms_with_portages_warning
- Backlog numbers removed from the comment (original text): Backlog #347 S9: only package names and slot atoms are allowed (Portage `repository/config.py:91` `_find_bad_atoms`). Grounded in S0 cell `g05-repobadatom` (`=bad/atom-1` warns and is dropped).

## fn parse_binrepos_conflicting_repo_keys_warn_and_cancel
- Backlog numbers removed from the comment (original text): Backlog #347 S9: atoms in both of one binrepo's own lists warn and are dropped from both (Portage `binrepo/config.py:60-75`). The text is Portage's own, byte for byte (note the missing "and").

## fn parse_binrepos_reads_frozen_defaulting_to_false
- Backlog numbers removed from the comment (original text): Real `binrepo/config.py` `_bool_opts` (`"frozen": "false"` default, backlog #192).
