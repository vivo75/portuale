# crates/portuale/src/pretend/standalone_actions.rs
The standalone actions: --resume, --config, --search, --check-news, --info, --clean, --rage-clean, --unmerge, --depclean, --prune.

## fn dispatch_standalone_actions
- `--resume` / `-r`: ports Portage's `_emerge/actions.py` resume handling.

## fn dispatch_queries
- `--config <atom>`: ports Portage's `action_config`.
- `--search`/`-s`: ports Portage's `action_search`, which passes `search`'s `verbose` as `"--quiet" not in myopts`.
- `--check-news`: ports Portage's `actions.py` `count_unread_news` block.
- `--info`: ports Portage's `action_info`, narrowed to its deterministic config block plus the per-atom "Package Settings" block.
- `--search`: Portage's `action_search` passes `search`'s `verbose` as `"--quiet" not in myopts` -- so the full `Latest version` / `Homepage` / `Description` block shows by default and `-q` makes it terse; `-v` is irrelevant to search.
- `--info`: Portage's `action_info` consults `bindb` (the local `$PKGDIR` pool) for its `(bindb, "binary")` search half whenever `--usepkg` is on -- so scan `$PKGDIR` here too, exactly as the ordinary `--usepkg` resolve path does further down (that scan runs after this standalone-action dispatch, so `--info` needs its own).

## fn dispatch_maintenance
- `--clean`: a standalone removal action -- keep only the newest version per slot.
- `--rage-clean`: a fast `--unmerge` (identical `--pretend` display).
