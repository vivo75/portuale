# crates/portage-repo/src/use_display.rs
USE flag display columns for `emerge --pretend -v`.

## struct InstalledUseState
- Portage `output.py::_display_use` + `map_to_use_expand`: the `old_use` / `old_iuse` inputs. `None` at the call site for a `New` entry (no installed side) -> every flag shown plain.

## fn build_use_expand_display
- Portage `output_helpers.py::_create_use_string`, fed by `output.py::_display_use` + `map_to_use_expand`.
- Input contract: `use_flags_display` is already IUSE-declared, enabled-resolved, and `alnum_sort_key`-ordered -- Portage's own `any_iuse.sort(key=_alnum_sort_key)`, a natural sort so `python3_9` precedes `python3_12`.
- Grouping: one group per `config.use_expand` variable whose `lowercase(name)_` prefixes the flag; the prefix is stripped (Portage `map_to_use_expand`: `val[len(exp) + 1:]`). `config.use_expand_hidden` groups are dropped (Portage `remove_hidden`). Within each group enabled render first, then disabled, each in natural order -- Portage's `" ".join(enabled + disabled)`.
- `emerge --alphabetical` collapses the two back into one interleaved list (still `alnum_sort_key`); applied at render time (`pretend.rs::use_suffix`), not here, since it needs no resolver state.
- Installed diff: `installed` is `Some` for an `Upgrade`/`Downgrade`/`Reinstall` entry -- i.e. Portage `pkg_info.previous_pkg is not None`, `is_new` false. Portage `_DisplayConfig` sets `verbosity = 3` whenever `--verbose` is given (`output_helpers.py:180-187`), so `all_flags = (verbosity == 3)` is always true for `emerge -pv`: the diff shows every flag, not just changed ones. Full marker table: enabled + not in old IUSE -> `flag%*`; enabled + in old IUSE + not in old USE -> `flag*`; enabled + unchanged -> `flag` (plain); disabled + not in old IUSE -> `-flag%`; disabled + was in old USE -> `-flag*`; disabled + unchanged -> `-flag` (plain); in old IUSE but gone from current IUSE -> `(-flag%)`, or `(-flag%*)` if enabled -- Portage's `removed` list.
- `reinst_flags` (per-flag "this flag triggered the reinstall" force) is still not modelled -- it only widens what `all_flags` already shows here.
- `forced` is full flag names -- Portage `_display_use`'s `self.forced_flags` = `pkg.use.force | pkg.use.mask` (see `forced_or_masked_flags`). Portage wraps the rendered token in `( … )`, and skips the trailing `%` on a `-flag%` (Portage: `if flag not in iuse_forced: flag_str += "%"`).
- Deliberately not ported (separate cut): Portage ANSI colorization.
- Returns `[(VAR_NAME, "rendered")]` with `USE` first, then the `USE_EXPAND` vars in sorted order (Portage `sorted(self.use_expand)` with `"USE"` inserted at position 0). An empty group produces no entry (Portage's `if ret:` guard); a package with no displayable flags returns `[]`.

## fn alnum_sort_key
- Portage `output_helpers.py::_alnum_sort_key`: split a string on runs of digits (`re.split(r"(\d+)", x)` -- alternates non-digit / digit / ...), compare digit runs as numbers. Used for the `USE="…"` flag list -- Portage's `any_iuse.sort(key=_alnum_sort_key)` (and the `--alphabetical` combined list, and the removed-from-IUSE list).

## enum AlnumKey
- `Str` is ordered before `Num` so a shorter key sorts before a longer one that extends it with a number (the two variants never meet at the same position between two well-formed keys -- the split always alternates `Str`/`Num`).

## fn refresh_entry_use_display
- The `--autounmask-backtrack` off path uses this to reflect an accumulated `package.use` change on a package the loop did not re-walk (Portage `_pkg_use_enabled` consulting `_needed_use_config_changes` for the display only). Mirrors the main walk's own entry-display block; a no-op for an entry with no on-disk IUSE / a non-merge outcome.

## fn repaint_running_root_entry_displays
- Backlog #242 Slice D. The CLI layer calls this once per resolve when the run is genuinely cross-root (running root differs from target); single-root runs never reach here (no entry is running-rooted there). Candidate lookups reuse the shared `repos` (both roots resolve the same trees -- Track X Slice B/C); old-USE diffs read the running root's own vdb, like Portage's per-root `vardb`. Same no-op tolerance as `refresh_entry_use_display`.
- Portage `_display_use` reads the package's own `root_config` (`output.py:822`).

## fn refresh_one_entry_display
- Rebuilds one merge entry's `use_flags_display` / `use_expand_display[_p]` from `config`: effective USE, force/mask wraps, expand grouping and the installed old-USE diff all read that config and `root`'s vdb.
- Dedup detail: Portage builds the USE display from `sorted(pkg.iuse.all)`, where `iuse.all` is a `frozenset` (`Package.py`) -- an eclass and the ebuild both declaring a flag (e.g. gnome-extra/gnome-color-manager's `IUSE="test test"`) still renders once. Dedup by bare name, keeping the first spelling.

## fn build_use_expand_display::render_flag
- Portage `_create_use_string` leaves `flag_str = None` (flag omitted) for an unchanged flag -- and for a removed-from-IUSE flag -- unless `conf.all_flags` (`verbosity == 3`, i.e. `emerge -pv`) or `reinst_flag` (the flag is in the reinstall's own `_reinstall_for_flags` trigger set -- Portage `reinst_flags_map`).
- The one case this changes at plain `-p`: a flag the new ebuild dropped from IUSE that nonetheless triggered a `--newuse`/`--changed-use` reinstall now shows in the `(-flag%)` removed list (its enabled-state / IUSE-presence change markers already carried the still-present trigger flags).
- Removed-flag render: Portage `yellow("-" + flag) + "%"`, `+ "*"` if it was on, always `"(" + ... + ")"`.

## fn build_use_expand_display::removed routing
- Portage `removed_iuse = set(old_iuse).difference(cur_iuse)`, shown inside `_create_use_string`'s `if conf.all_flags or reinst_flag` branch.
- Portage sorts `cur union old` as one list; portuale keeps its own cur-then-removed routing, a pre-existing structural difference -- only the sort key changed here.

## fn build_use_expand_display::render loop
- Portage `_create_use_string`: `" ".join(enabled + disabled + removed)` in that polarity order, each group already in the incoming bare-name order.
