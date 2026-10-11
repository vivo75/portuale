# crates/portuale/src/remote/ssh.rs
The ssh transport: argv, control sockets, running scripts, copying files.

## fn control_dir
- Unix-domain socket paths cap at ~108 bytes, and `%C` alone still needs ~60 of those, so candidates are tried shortest-first: `$XDG_RUNTIME_DIR` (usually `/run/user/<uid>`, the shortest stable per-user dir), then `$HOME/.portuale/cp`.

## fn ssh_argv
- Rc 255 is the transport-error signal, per the Ansible convention (a wrapper around the system `ssh` binary, multiplexed).

## fn local_client_bash
- Two things must not ride along or be missing (backlog #313): the server's own VDB selection (`PORTUALE_VDB_*`, set by `mrg` for a `--remote-vdb=server:` copy) -- a client phase's `has_version` / `best_version` must read the *client's* VDB, which stays a files tree (`mrg.rs` `setup_vdb`) -- and `PORTUALE_BIN`, which the vendored `portageq-wrapper` shim execs (#326 S8.3: every generated script exports the resolved absolute path, and local transport keeps `current_exe()` with no install).

## fn run_raw_command
- Over ssh the trailing words are joined with spaces for the remote shell, so each word is `sh_quote`d; a client path with spaces or metacharacters (e.g. a `--remote-portuale-dir`) stays one word (#326 S8 review).

## fn send_bytes
- Carries bytes the server never wrote to disk: old-hook envs staged for the merge driver (backlog #171).
