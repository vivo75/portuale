# crates/portuale/src/helpers/mod.rs
The `portuale __helper` dispatcher and its helper table.

## module
- Native phase helpers (#326 S2, decisions D1/D2/D3/D6/D7/D8). `portuale __helper <name> ...` is dispatched first in `main()`, from `args_os()` (never `args()`: helpers receive raw filenames that may not be UTF-8), before anything else. It never calls `bin_dir()` (no extraction, no atexit), loads no config, and writes only its own outputs (D7).
- Names: `python` (the D1 dispatcher: the native `filter-bash-environment` (S3) plus the transition table holding every not-yet-ported script of 0.3), `chmod-lite` (D2/D3), `ebuild-ipc` (always 127, D6), `ping` (a fixed token, D8). Anything unknown exits 127 with `portuale: no native helper for: <argv>`, so an upstream re-sync that adds a Python call fails loudly (D1).

## mod chmod_lite/doins/filter_env/gpkg/locale/python/xattr/xpak
- #329: the merge copies xattrs through the same `_copyxattr` / `_cmpxattr` port the helpers use (re-exported here for the merge: `xattr_set` for tests, `copy_xattr`/`xattr_excluded`/`xattr_get`/`xattr_list` for callers).

## const PING_TOKEN
- The fixed answer of the `ping` helper (D8: proves `$PORTUALE_BIN` reaches a phase as a working binary, never the libtest harness).

## fn no_native_helper
- The D1 unknown-helper message: `argv` joined by spaces (lossy), exit 127. For the `python` dispatcher the caller passes the full `["python", ...]` argv so the message reads `portuale: no native helper for: python <argv>`.

## tests: dispatcher_tests
- Child processes, never in-process: the native helpers run across a real process boundary. `PORTUALE_PORTAGE_CHECKOUT=/nonexistent` in the unknown-helper probes.
- `an_unknown_helper_name_exits_127_with_the_message`: S2 dispatcher table: an unknown helper name exits 127 with the `no native helper` message naming the full argv.
- `ebuild_ipc_always_exits_127`: S2/D6: `ebuild-ipc` always exits 127 (never 1: exit 1 would make `has_version` silently answer "not installed").
- `ping_prints_the_fixed_token`: S2/D8: `ping` prints the fixed token and exits 0.
- `python_with_an_unknown_program_exits_127`: S2 dispatcher table: `python` with anything but the probe program or a transition script exits 127 with the `python <argv>` message.
- `python_with_a_non_native_script_exits_127`: S7 dispatcher table: the transition table is empty (every 0.3 script is natively ported), so a never-in-scope script like `dohtml.py` exits 127 with the `python <argv>` message -- the same as any other unknown name. (`dohtml` dies first for EAPI >= 7, the ebuild floor, so it has no native port and never will.)
- `python_xattr_helper_routes_natively_without_interpreter_or_checkout`: S7 dispatcher table: the last native rows route without any interpreter or checkout (`xattr-helper.py --help` answers natively even with both disabled).
- `python_c_with_the_exact_probe_program_answers_natively`: S2 dispatcher table: `-c` with the exact QA probe program answers natively (the full locale table is `locale::table_tests`; env is cleared, `PATH=/usr/bin:/bin`, answer `UTF-8`).
