# crates/portuale/src/pretend/tests/attrs.rs
Root suffixes, the mask column, USE suffixes, and nomerge rows.

## fn root_suffix_annotates_each_entry_with_its_own_root
- Backlog #242 Slice D: Portage's `output.py:841-862` appends unquoted `to <EROOT>` on every merge row whose OWN root is not `/` (S0 map `LLM/evidence/2026-10-02-242-display-sites.md` §1a) — the #206 cut reopened per display site.
- Single-root `/`: everything bare (L0). Staged target: target rows carry the target root (reopened cut). Dual roots: each side annotates its own root; and a `/`-rooted row stays bare even under a staged target (Portage's blk0x/cyc0y rows, S0 §1a). Defensive: a running entry with no running root in hand stays bare.

## fn attr_display_field_carries_the_seventh_mask_column_unless_quiet
- Portage's `set_pkg_info` fills the mask column `if self.include_mask_str()` (`verbosity > 1`), and Portage's default `emerge -p` verbosity is 2 -- so the field is 7 columns even without `-v`, dropping to 6 only under `--quiet` (verbosity 1, `include_mask` false). A plain reinstall with no keyword/hard mask -> `I N S f U D` = `  R   ` then a bare space.
- Same entry under `--quiet`: 6 columns, no mask slot. A `~arch`-only-visible New -> the 7th column is `~` (`gen_mask_str` "unstable"), not a space.

## fn use_suffix_picks_the_p_or_pv_use_rendering_by_verbosity
- A `New` entry: the resolver renders the same full list into both fields (`is_new` renders every flag regardless of `all_flags`), so `-p` and `-pv` show the same USE line.
- `--quiet` (verbosity 1): no USE line at all unless `-v` too. `-pvq`: `print_use_string` true again, `all_flags` true -> the full list (like `-pv`).
- A `Reinstall` with one flipped flag: `-pv` shows the full list, plain `-p` shows only the change. `-pvq`: full list, same as `-pv`.
- A `Reinstall` with nothing changed: no USE line at `-p`. `-pvq`: `all_flags` true -> the full list shows even though nothing changed.

## fn nomerge_row_carries_the_use_suffix_when_the_package_has_one
- #154 (`--tree dev-libs/gpcyclec`): the ancestor occurrence of a merge node renders through the nomerge arm, which appends the same USE suffix as a merge row. Portage's `resolver/output.py`: `Display.__call__` runs `_display_use` for every package (no ordered/merge gate) and `print_messages` appends `" " + verboseadd`; `verbose_size` is merge-gated so no size suffix ever follows on this arm.
- The gpcyclec cell shape: a `New` entry whose `-p` USE display is `x` (gpcyclea-1.0, IUSE=x, enabled).
- A USE-less package (empty display, e.g. `dev-libs/diamond`): the tail is empty and the row is byte-identical to the pre-#154 rendering, so no existing pin moves.
- `--quiet` keeps the established narrow pad.

## fn use_suffix_renders_portages_trailing_space_after_each_use_group
- Backlog #272: Portage's `_create_use_string` (`output_helpers.py:336-338`) returns `f'{name}="{ret}" '` -- one trailing space per non-empty group -- and `print_messages` (`output.py:542-544`) joins it with a single leading space, so every painted merge-list row ends with `" `, at `-p` and `-v` alike. Grounded on host Portage 3.0.82.2 (`/usr/sbin/emerge`) staged-fixture probes (`--pretend --autounmask "dev-libs/useflagpkg[missingflag]"`, New and installed-USE reinstall shapes): Portage's changed row reads `... USE="missingflag*" ` where portuale printed `... USE="missingflag*"`.
- Plain `-p`: the changed flag paints, with Portage's trailing space. `-pv`: the full list, same trailing space. Bare rows stay bare: an empty display still renders nothing.
