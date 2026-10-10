# crates/portage-profile/src/resolve/licenses.rs
ACCEPT_LICENSE, ACCEPT_PROPERTIES, ACCEPT_RESTRICT and their `package.*` files.

## fn fold_licenses_and_accepts
- `license_groups`: Portage `LicenseManager._read_license_groups` (`LicenseManager.py:47`) over `LocationsManager.profile_locations` (`LocationsManager.py:432`) -- the `profiles/` directory of the main repo and each overlay, NOT the individual profile-chain levels (Gentoo puts `license_groups` at `<repo>/profiles/license_groups`, never in a `profiles/<foo>/` profile dir -- verified live against a Portage tree). Then the user-level `/etc/portage/license_groups`. "Extend, don't stack/replace" semantics; see `parse_license_groups_lines`.
- ACCEPT_LICENSE: last-level-wins scalar; see `accept_license`'s doc comment for why, and the `* -@EULA` default portuale replicates when it is never set anywhere. `scalars` holds the profile chain + make.conf value via the same catch-all `scalars.insert` every other scalar variable goes through in `process_lines`/`process_make_conf_file`.
- package.license: user-level only; see `package_license`'s doc comment for the deliberately-not-replicated `profile-license` profile-format and `*/*`-global-extraction Portage quirks.
- ACCEPT_PROPERTIES/ACCEPT_RESTRICT: last-level-wins scalars, Portage `*` default (see `accept_properties`'s doc comment). No `@group` expansion for either, unlike ACCEPT_LICENSE/package.license.
- package.properties/package.accept_restrict reuse `parse_package_license_lines` directly (see its doc comment).
