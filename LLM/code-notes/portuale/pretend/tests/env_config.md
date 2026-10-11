# crates/portuale/src/pretend/tests/env_config.rs
Config-derived pretend gates: buildpkg, pkgdir trust, shell splitting, default opts, compressor, root-deps.

## fn buildpkg_comes_from_the_cli_then_the_resolved_features
- #37 S3: Portage's `create_depgraph_params.py` sets buildpkg='y' from `settings.features`; an explicit CLI `--buildpkg[=n]` wins.
- No CLI flag: resolved `FEATURES=buildpkg` enables it; without the token it stays off.
- An explicit `--buildpkg=n` wins over the feature.

## fn pkgdir_index_trusted_folds_the_resolved_features_chain
- Backlog #199 fix round 1: `pkgdir_index_trusted` is Portage's `bintree.populate`'s own `reindex = "pkgdir-index-trusted" not in features` (`bintree.py:936-938`) over the folded `FEATURES` chain, defaulting to trusted when nothing configures `FEATURES` at all (Portage's host `make.globals` default, `cnf/make.globals:81`).
- A later layer negates it (the `make.conf`-layer case); `-*` clears the chain; the `other_vars` fallback (the case `config_features_list` falls back for) still decides.

## fn shell_split_handles_quotes_and_escapes
- Unterminated quote: consume the rest.

## fn emerge_default_opts_are_prepended_to_argv
- Portage's `main.py:1352-1360`: `EMERGE_DEFAULT_OPTS` tokens come first, argv after -- so a command-line option wins.
- The splitter is the same shlex-shaped one `PORTAGE_IONICE_COMMAND` uses, so quoting in the variable is honoured.
- Unset variable: argv unchanged.
- `--ignore-default-opts` on the command line skips the variable entirely (Portage's `main.py:1352-1353`).

## fn config_bzip2_command_prefers_the_placed_config
- Backlog #171 review: a client `make.conf` compressor (e.g. `lbzip2`) wins over the `make.globals` default.
- Unset (or empty): the `cnf/make.globals:105` default.

## fn root_deps_running_root_is_none_only_when_flag_off_and_roots_coincide
- Ordinary ROOT=/ invocation, no flag: strict no-op.
- Cross-root build (ROOT != running root), no flag: routes to the running root -- Portage's unconditional BDEPEND/IDEPEND behavior, now reachable without --root-deps.
- --root-deps forces it even when the roots coincide.
