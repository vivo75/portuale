# crates/portuale/src/color.rs
ANSI colour for `emerge --pretend` output.

## module
- Increment 2 of the `-pv` Portage `output.py` layout plus colour buildout (increment 1 landed the bracket layout; see `pretend.rs::attr_display_field`).
- Ports the slice of `lib/portage/output.py` the pretend renderer needs: the RGB-name to ANSI-code table (`output.py:30-92`), `colorize()` (`output.py:383-392`), the handful of `_styles` entries (`output.py:126-154`), and `nc_len()` (`output.py:249-251`), plus `/etc/portage/color.map` parsing (`output.py:158-230`, `_parse_color_map`; see `color_map_overrides`) and the `PORTAGE_COLORMAP` phase-env export (`output.py:365`, `colormap()`; see `ebuild_phases::phase_colormap_export`).

## fn code
- Ports Portage's `esc_seq` plus the `rgb_ansi_colors[i]` to `ansi_codes[i]` mapping (`output.py:68-92`): `ansi_codes` is `[30m, 30;01m, 31m, 31;01m, …]`, paired positionally with the 16 `0xRRGGBB` names.

## fn style
- Ports Portage's `_styles` map (`output.py:126-154`), narrowed to the keys the pretend renderer's `colorize()` calls reach; each maps to exactly one colour-name (Portage's `_styles` values are 1-tuples for all of these). Covers the `PORTAGE_COLORMAP` phase-env export (`colormap()`'s 10 `PORTAGE_COLOR_*` keys), the `EOutput` `e*` prefixes (`output.py:128-135`, the elog `echo` module in `elog::echo_summary`), and `UserQuery.query`'s default `[Yes/No]` colours (`output.py:154-155`, used by `pretend.rs`'s `ask_confirm`).

## const STYLE_KEYS
- Every `_styles` key name, so `color_map_overrides` can validate a `color.map` `KEY=` (Portage's `_parse_color_map`: `if k not in _styles and k not in codes: ParseError`).

## fn color_map_overrides
- Ports Portage's `output.py::_parse_color_map` (`output.py:158-230`): parses `<config_root>/etc/portage/color.map` (`$PORTAGE_CONFIGROOT` here, read the same env-first way `resolve_havecolor` reads `$NO_COLOR`). A line is `KEY = VALUE` (`#` starts a comment); `KEY` must be a known `_styles`/`codes` name; `VALUE` is either a raw ANSI code (`^[0-9;]*m$` becomes `\x1b[` plus it) or a space-separated list of `codes` names (concatenated). A malformed line is skipped with a warning (portuale's `onerror`), never fatal. Cached once per process (Portage's own `_color_map_loaded` one-shot).

## fn phase_colormap_export
- Ports Portage's `output.py::colormap()` (`output.py:365-379`): the `PORTAGE_COLORMAP` phase-env value, bash source that `bin/isolated-functions.sh` `eval`s so an ebuild's `elog`/`einfo`/`ewarn` use the same (possibly `color.map`-overridden) colours `emerge` itself does. One `PORTAGE_COLOR_<KEY>=$'<ansi>'` line per Portage `colormap()` key. `\x1b` becomes `\E` for the bash `$'...'` literal (Portage emits `$'\e[...'`-style sequences).

## fn nc_len
- Ports Portage's `output.py:249-251` `nc_len`: the visible length with ANSI SGR sequences (`\x1b` … `m`) removed first. Portage's pattern is `re.sub(esc_seq + "^m]+m", "", mystr)`, i.e. `\x1b[^m]+m`; this is the same, sans regex.

## fn resolve_havecolor
- Ports Portage's gate from `lib/_emerge/actions.py:2816-2828` plus `lib/portage/util/no_color`.

## struct Colorizer
- Portage's module-global `havecolor` plus its `colorize()` function, together.

## impl Colorizer::pkgprint
- Ports Portage's `Display.pkgprint` (`output.py:265-292`), the merge-list case only (`pkg_info.merge` is always true for every bracket entry portuale prints).
