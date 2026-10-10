# crates/portuale/src/self_exe.rs
The running binary's path, resolved once for `PORTUALE_BIN` re-execs.

## module
- Resolved once (#331) because Linux reports a replaced or unlinked running executable as `<path> (deleted)` (`/proc/self/exe`): asking `current_exe()` after a `cargo build`, or after `emerge` merged a new portuale over the running one, exports a path that does not exist and every later phase dies.
- Every phase and generated script reaches portuale again through `PORTUALE_BIN` (the `portageq-wrapper` shim, the `__helper` shims).
- Portage re-execs nothing of its own: its helpers are files under `PORTAGE_BIN_PATH`, which a portage merge replaces in place, so later phases simply run the new files. Resolving the path once at startup gives the same behaviour.
- `main` calls `self_exe` before any phase can run.
