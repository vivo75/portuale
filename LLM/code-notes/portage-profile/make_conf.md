# crates/portage-profile/src/make_conf.rs
make.conf-style parsing: logical lines, shell tokens, `${VAR}` substitution, and `source` handling.

## fn logical_lines
- Joined physical lines are separated by a single space. Ports Portage's `getconfig`'s `shlex` reading across newlines inside quotes, which `cnf/make.globals` relies on for its multi-line `FEATURES=`.

## fn shlex_token
- Portage's `portage.util.getconfig` tokenizes a `KEY=VALUE` line with a POSIX `shlex` lexer (`shlex.py:read_token`, `posix=True`, `wordchars` extended by getconfig to `~!@#$%*_\:;?,./-+{}`, `quotes = "'"`, `escape = \`); the value token is quote-removed and backslash-processed *before* `varexpand` (`portage/util/__init__.py:885`) ever sees it. Portuale used to strip only the outer quote pair, so an escaped `\"` kept its backslash and `\$` reached the `${VAR}` regex, which expanded it. On this host that corrupted every `make.globals` fetch command: Portage resolves `wget ... -O "${DISTDIR}/${FILE}" "${URI}"`, portuale's value was `wget ... -O \"\/var/cache/distfiles/\\" \"\\"` (backlog #70).
- Mirror states: the token ends at unquoted whitespace (or `#`, shlex's commenter); inside `"` only `\"` -> `"` and `\\` -> `\` lose the backslash, every other `\x` (notably `\$`) survives to `substitute`; inside `'` every character is literal until the closing `'`; outside quotes `\x` -> `x`, and a quote adjacent to a token opens its quoted state and continues the same token.

## fn substitute
- Portage's `varexpand` (`portage/util/__init__.py:885-1015`) over the `shlex_token`-processed value: while not inside a surviving single quote (e.g. the `'`s of a `"'${VAR}'"` value), an escaped `\$` is a literal `$`, `\\` is a literal `\` (plus Portage's bug-compatible extra character when the next one is a quote or `$`), an escaped newline disappears, any other `\x` keeps both characters, and `${VARNAME}`/`$VARNAME` expand (a name already set earlier in the config wins, else the process environment, as bash sees it when it sources `make.conf`, else empty; Portage warns and returns an empty value for a malformed reference, which this mirrors without the warning). The environment fallback is what lets a fixture write a relocatable `PKGDIR="${PORTAGE_CONFIGROOT}/pkgdir"` instead of an absolute path.

## fn parse_kv_line
- Runs the `shlex` tokenization even with `expand=False`, like Portage's `getconfig`. Out of scope: conditionals, function defs, etc.; Portage's make.defaults/make.conf files don't use them.

## fn process_make_conf_file
- Resolves `source <path>` directives against `config_root` as if it were `/` (chroot-style), matching PORTAGE_CONFIGROOT/ROOT semantics elsewhere in portuale: an absolute `source /etc/make.local` reads `<config_root>/etc/make.local`, not the host path. A missing sourced file is silently skipped (lenient default; bash would error, but no fixture or usage in portuale relies on that).

## fn read_env_file_kv
- Reads one `/etc/portage/env/<name>` file: Portage's `getconfig(penvfile, allow_sourcing=True)`. A `${VAR}` in a value is left literal here and substituted by the caller (`expand_package_env_files`) against the per-entry expand map. A missing file yields an empty list (Portage warns from `setcpv`; portuale follows its standing "no warnings from deep in config resolution" precedent). `visited` guards a `source` cycle.

## fn expand_package_env_files
- Portage's `_grab_pkg_env` copies the global expand map once per package (`env.d` + `make.globals` + `make.defaults` + `make.conf` values, portuale's own `scalars` at the `package.env` read site) and reads each file with `getconfig(..., expand=that map)`, which also feeds every assignment back into the map, so a later line (or a later file of the same entry) sees earlier ones, exactly like `make.conf`'s own within-file chaining. Returns every pair in file order, `${VAR}`-substituted; callers split the `USE=` half (tokens split *after* substitution) from the scalar half. Per-package values never flow back into `scalars` (Portage: "don't want per-package settings to pollute the global expand_map").
