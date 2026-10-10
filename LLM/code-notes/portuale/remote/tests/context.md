# crates/portuale/src/remote/tests/context.rs
Small context unit tests: exec handoff, locale split, option parsing.

## tests: split_server_locale_fans_lc_all_out_like_portage
- Backlog #171c task 1, unit level: the `split_LC_ALL` port fans a set `LC_ALL` out over all twelve Portage `locale_categories` and drops it (Portage `portage/util/locale.py:22-36,160` + `config.py:3374-3385`). Fills an unset `LANG`, never clobbers an explicit one.
- The brief's case: server `LC_ALL=C.UTF-8` alone. Portage overwrites unconditionally; Portage never splits on non-posixish EAPIs.
