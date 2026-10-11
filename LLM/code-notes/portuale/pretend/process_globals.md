# crates/portuale/src/pretend/process_globals.rs
Set the resolver's process-wide options from the parsed command line.

## fn apply_process_globals
- `--package-moves` (Portage's `y_or_n`, default "y"): see `portage_repo::set_package_moves_enabled`.
- `--useoldpkg-atoms ATOMS`: a process-global read during candidate selection (see `portage_repo::set_useoldpkg_atoms`). `opts.useoldpkg_atoms` stays for the slot-collision `need_rebuild` trailer, which re-checks the same atoms at render time (Portage reads `frozen_config.useoldpkg_atoms` there too).
- `--binpkg-changed-deps=y|n` / `--use-ebuild-visibility=y|n`: resolver-time overrides of automatic defaults, applied as process-globals (see those setters' own doc comments).
- `--debug`/`-d` under `--pretend`: besides `PORTAGE_DEBUG=1` in the phase env (a no-op when nothing is built), Portage also turns on the full resolver trace (`initialize_logger(logging.DEBUG)`). Portuale emits the same shapes -- see `portage_repo::resolver_trace`. Portuale's build path doesn't route through the traced resolver the way Portage's does.
- `--useoldpkg-atoms ATOMS` is set once here, before any `resolve_pretend_graph` call; empty is a strict no-op.
- For `--binpkg-changed-deps` / `--use-ebuild-visibility`, `None` / `false` (the defaults when the flag is absent) are strict no-ops.
