# crates/portuale/src/global_updates/output.rs
The `Performing Global Updates` stdout block (backlog #576): the header, the per-file dots and every progress character, driven by the halves' hooks.

## struct GlobalUpdatesPrinter
- Ports the output statements of `_do_global_updates` (`_global_updates.py:99-137` for the header/dots, `:190-206` for the move characters, `:235-274` for the config/`update_ents` characters, `:274` for the trailing blank lines; the writes are `writemsg_stdout`/`writemsg`, `util/__init__.py:103-131`, verbatim plus flush).
- `notice` carries the `update_notice_printed` flag (`:101-102`); the legend is the `"  ".join` over `""` plus the eight `bold(char)=...` entries, three with embedded newlines (`:110-125`). `colorize("GOOD", …)` / `bold(…)` go through the shared `Colorizer` (`output.py:397-404`), so the block is plain exactly when `havecolor` is off (the capture probe sets it to 0, like `emerge_main` before `adjust_configs`).
- `--quiet` suppresses the legend and the per-file lines (`:110, :128-130`); the vdb `#` hook is `None` under quiet (`:258-259`) while the binpkg `*` hook is rebound after that gate (`:265-269`), so `*` still prints. `p` is never gated (`update.py:396-397` writes and flushes unconditionally), nor are `@`/`%`/`s`/`S` (`if moves:` only, `:193-206`).
- Methods take `&self` (interior `RefCell`/`Cell`) so the halves' `Fn` progress hooks call them directly. A failed write panics, like `println!` on `EPIPE`.
- No `ERROR:` output: the shared parser (`portage-repo` `parse_updates_content`) drops malformed lines without reporting them, and the #551 ruling keeps portuale without that output until the updates re-read lands. The t5 oracle has no malformed lines (fixture deliberately not extended); the stderr shape for later is #513 shape 16 (`ERROR: Update type not recognized '…'` on stderr next to the file's dots on stdout).
- The driver (and the tests below) interleave per `(repo, command)` — vdb then binpkg per command — because Portage's move loop nests the binpkg move inside the per-command loop; batch totals alone would group as `@s%S` instead of the captured `@%sS`.

## tests
- `stdout_matches_the_portage_capture` / `quiet_stdout_matches_the_portage_capture`: every half runs over a scratch copy (`cp -a`, `TempDir` guard kept alive) in Portage's order — world decisions (silent), moves interleaved per command, config files, vdb then binpkg `update_ents` — with all progress reporting into the printer. Stdout equals the committed capture byte for byte after replacing the capture's scratch prefix; world/config settings mirror the Task 7/9 tests (`/etc` protect, multi-instance, `zstd`, `compress-index`).
- `notice_uses_the_colorizer_keys`: the header/legend go through `GOOD`/`bold` (expectation built through the same `Colorizer`, so absolute codes stay `color.rs`'s pin).

## fn drive_fixture (tests)
- Now drives the production driver (`run_global_updates`) over the scratch copy instead of repeating its order, with the same halves (`/etc` protect, `zstd`, multi-instance, `compress-index`, colour off, privilege from the scratch root); the two stdout pins are unchanged.

## fn drive_fixture (ordering)
- Per-file dots print in sorted file-name order; Portage's `scandir` order is arbitrary per filesystem (a single file here, so no ruling needed).
