# crates/portage-profile/src/virtuals.rs
Deprecated profile and user `virtuals` files: the grabdict gate and the config-load warnings (backlog #513).

## fn grabdict_nonempty
- Port of Portage's `grabdict` gate for a `virtuals` file (`lib/portage/util/__init__.py`, `grabdict`): a line not starting with `#` whose tokens, cut at the first `#` token, number at least two (key plus one provider). A missing file is empty. Portage's `readlines` universal-newline behaviour is mimicked (`\r\n` and lone `\r` end lines).
- Oracle shapes 05 (comment only, silent), 18 (`virtual/editor` alone, silent), 01/02/03 (`virtual/editor app-editors/vim`, warns) in `LLM/evidence/2026-10-24-513/`. Pins: `src/tests/virtuals.rs::grabdict_nonempty_needs_a_key_and_a_value`.
- Not mirrored: Portage's `PermissionDenied` raise for an unreadable file. Here an unreadable file counts as empty (backlog #513 residue).

## fn profile_virtuals_warnings
- One `ProfileWarning` per non-empty `<profile dir>/virtuals`, in the order given. `resolve_config` passes the profile chain (parents first) and then the user profile node (`/etc/portage/profile`), which Portage appends to `self.profiles` last (`_config/LocationsManager.py:169-176`).
- Text: the Python `warnings.warn` rendering Portage prints, `file:line: UserWarning: <dir>/virtuals is deprecated, support for PROVIDE virtuals was removed in portage-2.3.25, use GLEP 37 virtual packages instead` then `  warnings.warn(`. The location is `portage/package/ebuild/_config/VirtualsManager.py:54`, not the host's interpreter prefix (`/usr/lib/python3.14/site-packages/`, Portage's own output): portuale cannot know that prefix, and the pins must not depend on it. Source: `_config/VirtualsManager.py:45-60` (line 54 is the `warnings.warn(` call).
- Rank `WARN_RANK_VIRTUALS_PROFILE` (6, after `package.mask`'s 5): the oracle order is `package.mask` invalid atom, then the virtuals warning (shape 16).
- Printed once: the config is loaded once per run and the warning is pushed once per file; Portage's cloned configs (`config.py:362`, `copy.deepcopy`) do not repeat it either.

## struct Config (field user_virtuals_deprecated)
- `/etc/portage/virtuals` exists (any content). Portage's `config.validate` (`config.py:1474-1479`) prints the block for each run that calls `validate_ebuild_environment`, which the CLI decides per action (`pretend/early_actions.rs`, `prints_user_virtuals_notice`). The text is `USER_VIRTUALS_NOTICE` (`src/virtuals.rs`), printed by the CLI, not in `config_warnings`, because it is not config-load output.
- Backlog #513; oracle shapes 01, 04, 17, 20-24, 27-29 (`LLM/evidence/2026-10-24-513/`).

## unreadable profile `virtuals` (#549)
- `grabdict_nonempty` returns `Result<bool, io::Error>`: `NotFound` maps to `Ok(false)` (Portage's `grablines` ignores `ENOENT`); every other errno is reported.
- `profile_virtuals_warnings` returns `Result<_, Error>`: `PermissionDenied` becomes `Error::VirtualsPermissionDenied` (`Display`: `Permission denied: '<path>'`, verbatim at the CLI with exit 13 via `bin/emerge:63-65`); any other read error becomes `Error::ReadFile` (Portage re-raises those; the CLI renders them `emerge: reading ...`, no traceback parity claimed).
- Scope note: the user `/etc/portage/virtuals` check stays `.exists()`-only -- Portage's `os.path.exists` gate is equally readability-blind (verified live: mode-000 user file still warns).
