# crates/portuale/src/remote/client_binary.rs
Finding, checking and installing portuale on the client.

## module
- Slice 8, backlog #326 D5/Q1-Q3/Q5: the server installs and verifies its own binary on the client. The file it ships is `current_exe()`, or `--remote-portuale-binary=<path>` (e.g. a static musl build when the server itself runs a dynamic dev build).
- The client must hold a file with the same full SHA-256; whatever file is found, the full digest is always what gets compared. `<hash>` in `portuale-<hash>` is the first 16 hex digits of that digest (Q5).
- Different server builds get different names, so they never overwrite each other; a plain `portuale` whose digest differs is never touched (it may be an operator's own install).

## fn bin_search_dirs
- Only the `--remote-portuale-dir=<abs>` pair when that option is given (Q1), else the default pair.

## fn bin_candidates
- D5 order: per directory, `portuale` then `portuale-<hash>`.

## fn server_bin_info
- Reads the shipped binary's hash and ELF machine (Q3).

## fn arch_matches
- Whether the shipped binary runs on the client (Q3). An unknown pair is a mismatch only when both sides are known.

## struct BinPreflight
- `None` for local transport without `--remote-portuale-dir`, which keeps `current_exe()` and installs nothing (D5).

## fn bin_preflight
- Skips the gates by design on local transport without the override (D5).

## fn dir_writable_for_install
- A world-writable directory without the sticky bit is refused (D5).

## fn evaluate_bin_gates
- The S8.1 gates over the parsed preflight values.

## fn ensure_client_binary
- Runs the install stage when the plan says so (S8.2).

## fn remove_client_path
- Best-effort removal of a client temp path (the S8.2 install's failure cleanup).

## fn ping_client_binary
- Runs `"<abs>" __helper ping` on the client and requires the D8 token on stdout: the installed binary really executes there.

## fn install_client_binary
- The S8.2 install: `mktemp`, stream, `chmod 0755`, digest re-check, `mv`, then the `__helper ping` (D8 token). The idempotent fast path (a matching target needs only its ping, with no status line) makes the second run report no `install-bin` line.
