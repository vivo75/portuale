# crates/portage-profile/src/tests/binrepo.rs
`parse_binrepos` pins: frozen defaults and `[DEFAULT]` inheritance.

## fn parse_binrepos_sections_inherit_default_keys
- Every `[section]` inherits the `[DEFAULT]` keys, like Portage's configparser (`BinRepoConfigLoader` reads `dict(parser[section].items())`, which includes defaults) (#539). A keyless section under `[DEFAULT] verify-signature = false` opts out of `gpkg_only`; without the default the stock-global `true` applies.
