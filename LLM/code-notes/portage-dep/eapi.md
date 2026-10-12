# crates/portage-dep/src/eapi.rs
Rust port of Portage's EAPI attribute table. Context moved out of the code comments (#341 S1); the code keeps only the short why.

## module
- Ports `lib/portage/eapi.py` `_get_eapi_attrs` (eapi.py:236-329), `Eapi.__init__` (eapi.py:224-229) and `lib/portage/__init__.py` `eapi_is_supported` (with `_testing_eapis`/`_deprecated_eapis`), Portage 3.0.82.2. Only the attrs some portuale code reads or S2 needs are modeled; more are added when a caller needs one.
- `None`/unsupported (`"10"`, `"9-foo"`) gives the permissive set (eapi.py:247-287). A *supported* spelling `Eapi.__init__` cannot parse (`3_pre1`, `" 8"`) raises `ValueError` there, so `eapi_attrs` returns `Err(EapiValueError)`; callers with no Portage behaviour to mirror treat it as a deliberate cut (`parse_atom_with` maps it to `AtomError`, S2). Never split on `_`: Portage reads `3_pre1` via `partition("-")[0]` = `"3_pre1"`, not 3 (review I2, proven live 2026-10-12).
- `md5_dict.rs`'s `eapi_*` helpers delegate here but keep their historical answers: trim, `""` → false, numeric-prefix reading for unsupported (`"10"` idepend/iuse_effective true, where Portage's permissive set says false) and for the deprecated underscore spellings. Changing those is a behaviour change, not done in S1 (plan task S1 Step 4; S0 matrix records whether any reachable path sees an unsupported EAPI). Proven identical by a temporary old-vs-new equivalence test over 31 inputs × 5 helpers (removed after passing).
- Live checks (host Portage 3.0.82.2, 2026-10-12): `_get_eapi_attrs` row-by-row (`None`, `0`–`9`, `10`, `9-foo`, `9-pre1` == `9`, `ValueError` for `3_pre1`/`7_pre1`/`" 8"`/`"8 "`) and `eapi_is_supported` spellings (`3_pre1`/`9-pre1`/`" 8 "` true; `""`/`"10"`/`"9-foo"`/`"8-pre1"` false). `portage.const.EAPI == 9`, so the supported digits are `0`–`9`.

## const SUPPORTED_EAPIS
- `Eapi.ALL_EAPIS` (eapi.py:209-221). The testing (`9-pre1`) and deprecated (`*_pre*`) spellings live in separate consts because `Eapi.__init__` rejects them while `eapi_is_supported` accepts them.

## fn eapi_is_supported
- `str(eapi).strip()` membership, exactly like Portage (`lib/portage/__init__.py`).

## struct EapiValueError
- Carries the input string; its `Display` matches Portage's `ValueError` text (`'<e>' not recognized as a valid EAPI`).

## fn eapi_attrs
- `partition("-")[0]` is `split_once('-')` (Rust's `str` has no `partition`); the `>=`/`<=` compares are plain `u8` compares against the digit key.

## tests
- `attrs_match_portage_eapi_table`: every row live-checked against `_get_eapi_attrs` (see module note); `is_supported_matches_portage`: live-checked against `portage.eapi_is_supported`.
