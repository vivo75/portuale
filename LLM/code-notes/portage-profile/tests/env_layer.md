# crates/portage-profile/src/tests/env_layer.rs
Context moved out of the code comments.

## fn env_over_config_scalar_follows_the_calling_env_over_make_conf_chain
- Backlog numbers removed from the comment (original text): Backlog #173: `BINPKG_FORMAT`'s full chain is calling env over `make.conf` over profile `make.defaults` over `make.globals` (real `config.regenerate()`'s `env` `USE_ORDER` layer on top of the file stack). The scratch root carries no `make.globals` (the fixture shape), so the file side is `make.conf` here.

## fn the_embedded_make_globals_seeds_the_unmerge_defaults_under_the_stack
- Backlog numbers removed from the comment (original text): Backlog #356 S4 (Q3), reworked by #384: the embedded `make.globals` (the base of every root, hermetic ones included) carries Portage's two unmerge defaults at the bottom of the stack before `make.conf` expansion, so later layers fold as on a real system. Expected behaviour from Portage 3.0.82.2 (`cnf/make.globals:77-84` `unmerge-orphans`, `:93` `UNINSTALL_IGNORE=/lib/modules/*`).
