//! make.conf-style parsing: logical lines, shell tokens, variable substitution.

use crate::*;

/// Splits `text` into logical assignment lines, joining physical lines
/// continued either by a trailing unescaped `\` or by an unclosed
/// `'`/`"` quote -- real `getconfig`'s `shlex` reads across newlines
/// inside quotes, which `cnf/make.globals` relies on for its multi-line
/// `FEATURES=`. Quote characters are kept; `parse_kv_line` strips the
/// outer pair. Joined physical lines are separated by a single space.
pub(crate) fn logical_lines(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for raw in text.lines() {
        // A full-line comment outside any continuation is skipped whole
        // (real `getconfig`'s `shlex` sets `commenters = "#"`) -- without
        // this a `#` inside comment prose, e.g. `Don't`, would open a
        // bogus quote that swallows the rest of the file.
        if quote.is_none() && cur.is_empty() && raw.trim_start().starts_with('#') {
            continue;
        }
        let mut line = raw;
        let mut cont = false;
        if quote.is_none() && line.ends_with('\\') && !line.ends_with("\\\\") {
            line = &line[..line.len() - 1];
            cont = true;
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        let mut prev = ' ';
        let mut escaped = false;
        for (idx, c) in line.char_indices() {
            match quote {
                Some('"') if !escaped && c == '\\' => {
                    escaped = true;
                    prev = c;
                    continue;
                }
                Some('"') if escaped => {
                    escaped = false;
                    prev = c;
                    continue;
                }
                Some(q) => {
                    if c == q {
                        quote = None;
                    }
                }
                None => {
                    if c == '"' || c == '\'' {
                        quote = Some(c);
                    } else if c == '#' && prev.is_whitespace() {
                        // Inline comment: drop from here to end of line.
                        cur.push_str(line[..idx].trim_end());
                        line = "";
                        break;
                    }
                }
            }
            prev = c;
        }
        cur.push_str(line);
        if quote.is_none() && !cont {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Real `portage.util.getconfig` tokenizes a `KEY=VALUE` line with a
/// POSIX `shlex` lexer (`shlex.py:read_token`, `posix=True`,
/// `wordchars` extended by getconfig to `~!@#$%*_\:;?,./-+{}`,
/// `quotes = "'"`, `escape = \`); the value token is quote-removed and
/// backslash-processed *before* `varexpand`
/// (`portage/util/__init__.py:885`) ever sees it. Portuale used to strip
/// only the outer quote pair, so an escaped `\"` kept its backslash and
/// `\$` reached the `${VAR}` regex, which expanded it. On this host that
/// corrupted every `make.globals` fetch command: real resolves
/// `wget ... -O "${DISTDIR}/${FILE}" "${URI}"`, portuale's value was
/// `wget ... -O \"\/var/cache/distfiles/\\" \"\\"` (backlog #70).
///
/// Mirrors shlex's states:
///   - the token ends at unquoted whitespace (or `#`, shlex's commenter);
///   - inside `"` only `\"` -> `"` and `\\` -> `\` lose the backslash;
///     every other `\x` (notably `\$`) survives to [`substitute`];
///   - inside `'` every character is literal until the closing `'`;
///   - outside quotes `\x` -> `x`, and a quote adjacent to a token opens
///     its quoted state and continues the same token.
pub(crate) fn shlex_token(raw: &str) -> String {
    const WHITESPACE: [char; 4] = [' ', '\t', '\r', '\n'];
    let chars: Vec<char> = raw.chars().collect();
    let mut out = String::new();
    let mut pos = 0;
    while pos < chars.len() && WHITESPACE.contains(&chars[pos]) {
        pos += 1;
    }
    let mut quote: Option<char> = None;
    while pos < chars.len() {
        let current = chars[pos];
        match quote {
            Some(q) => {
                if current == q {
                    quote = None;
                    pos += 1;
                } else if q == '"' && current == '\\' && pos + 1 < chars.len() {
                    // shlex's escape state inside `"`: only the quote
                    // itself and the escape character are consumed.
                    let next = chars[pos + 1];
                    if next == '\\' || next == '"' {
                        out.push(next);
                    } else {
                        out.push('\\');
                        out.push(next);
                    }
                    pos += 2;
                } else {
                    out.push(current);
                    pos += 1;
                }
            }
            None => {
                if WHITESPACE.contains(&current) || current == '#' {
                    break;
                }
                if current == '"' || current == '\'' {
                    quote = Some(current);
                    pos += 1;
                } else if current == '\\' {
                    if pos + 1 < chars.len() {
                        out.push(chars[pos + 1]);
                        pos += 2;
                    } else {
                        out.push('\\');
                        pos += 1;
                    }
                } else {
                    out.push(current);
                    pos += 1;
                }
            }
        }
    }
    out
}

/// Real `varexpand` (`portage/util/__init__.py:885-1015`) over the
/// [`shlex_token`]-processed value: while not inside a surviving single
/// quote (e.g. the `'`s of a `"'${VAR}'"` value), an escaped `\$` is a
/// literal `$`, `\\` is a literal `\` (plus real's bug-compatible extra
/// character when the next one is a quote or `$`), an escaped newline
/// disappears, any other `\x` keeps both characters, and
/// `${VARNAME}`/`$VARNAME` expand (a name already set earlier in the
/// config wins, else the process environment -- as bash sees it when it
/// sources `make.conf` -- else empty; real warns and returns an empty
/// value for a malformed reference, which this mirrors without the
/// warning). The environment fallback is what lets a fixture write a
/// relocatable `PKGDIR="${PORTAGE_CONFIGROOT}/pkgdir"` instead of an
/// absolute path.
pub(crate) fn substitute(value: &str, scalars: &HashMap<String, String>) -> String {
    portage_util::varexpand(value, |name| {
        scalars
            .get(name)
            .cloned()
            .or_else(|| std::env::var(name).ok())
    })
}

/// Parses one `KEY="value"` / `KEY='value'` / `KEY=value` line, returning
/// the value as the `shlex`-tokenized text ([`shlex_token`]; real
/// `getconfig` runs this even with `expand=False`). `None` for comments,
/// blank lines, or anything that isn't a simple assignment (conditionals,
/// function defs, etc. -- out of scope; real make.defaults/make.conf
/// files don't use them).
pub(crate) fn parse_kv_line(line: &str) -> Option<(&str, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let eq = line.find('=')?;
    let key = line[..eq].trim();
    if key.is_empty()
        || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        || key.chars().next().unwrap().is_ascii_digit()
    {
        return None;
    }
    Some((key, shlex_token(&line[eq + 1..])))
}

/// Resolves `source <path>` directives against `config_root` as if it
/// were `/` (chroot-style), matching PORTAGE_CONFIGROOT/ROOT semantics
/// elsewhere in portuale -- an absolute `source /etc/make.local` reads
/// `<config_root>/etc/make.local`, not the real host path. A missing
/// sourced file is silently skipped (lenient default; real bash would
/// error, but no fixture or real usage in portuale relies on that).
pub(crate) fn process_make_conf_file(
    path: &Path,
    config_root: &Path,
    scalars: &mut HashMap<String, String>,
    config: &mut Config,
    visited_sources: &mut HashSet<PathBuf>,
) -> Result<(), Error> {
    let canon = match path.canonicalize() {
        Ok(c) => c,
        Err(_) => return Ok(()), // missing file: lenient no-op, see doc comment
    };
    if !visited_sources.insert(canon.clone()) {
        return Ok(());
    }
    let text = fs::read_to_string(&canon).map_err(|e| Error::ReadFile {
        path: canon.display().to_string(),
        source: e,
    })?;
    for line in logical_lines(&text) {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("source ") {
            let sourced = rest.trim();
            let sourced_path = Path::new(sourced);
            let resolved = if sourced_path.is_absolute() {
                config_root.join(sourced_path.strip_prefix("/").unwrap_or(sourced_path))
            } else {
                canon
                    .parent()
                    .map(|p| p.join(sourced_path))
                    .unwrap_or_else(|| sourced_path.to_path_buf())
            };
            process_make_conf_file(&resolved, config_root, scalars, config, visited_sources)?;
            continue;
        }
        let Some((key, raw_value)) = parse_kv_line(trimmed) else {
            continue;
        };
        let value = substitute(&raw_value, scalars);
        note_incremental(config, key, &value);
        note_higher(config, key);
        match key {
            "USE" => {
                apply_use_incremental(&value, &mut config.use_flags);
                config.conf_use_tokens.push(value.clone());
            }
            "ACCEPT_KEYWORDS" => apply_incremental(&value, &mut config.accept_keywords),
            "USE_EXPAND" => apply_incremental(&value, &mut config.use_expand),
            "USE_EXPAND_UNPREFIXED" => apply_incremental(&value, &mut config.use_expand_unprefixed),
            "USE_EXPAND_IMPLICIT" => apply_incremental(&value, &mut config.use_expand_implicit),
            "IUSE_IMPLICIT" => apply_incremental(&value, &mut config.iuse_implicit),
            "USE_EXPAND_HIDDEN" => apply_incremental(&value, &mut config.use_expand_hidden),
            _ => {}
        }
        scalars.insert(key.to_string(), value);
    }
    Ok(())
}

/// Reads one `/etc/portage/env/<name>` file as an ordered list of
/// `(KEY, value)` assignments -- real `getconfig(penvfile,
/// allow_sourcing=True)`. `source <path>` lines are expanded in place
/// (absolute against `config_root` chroot-style, relative against the
/// file's own directory), the same resolution `process_make_conf_file`
/// uses; a `${VAR}` in a value is left literal here and substituted by
/// the caller (`expand_package_env_files`) against the per-entry
/// expand map. A missing file yields an empty
/// list (real portage warns from `setcpv`; portuale follows its
/// standing "no warnings from deep in config resolution" precedent).
/// `visited` guards a `source` cycle.
pub(crate) fn read_env_file_kv(
    path: &Path,
    config_root: &Path,
    visited: &mut HashSet<PathBuf>,
) -> Vec<(String, String)> {
    let Ok(canon) = path.canonicalize() else {
        return Vec::new();
    };
    if !visited.insert(canon.clone()) {
        return Vec::new();
    }
    let Ok(text) = fs::read_to_string(&canon) else {
        return Vec::new();
    };
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("source ") {
            let sourced = Path::new(rest.trim());
            let resolved = if sourced.is_absolute() {
                config_root.join(sourced.strip_prefix("/").unwrap_or(sourced))
            } else {
                canon
                    .parent()
                    .map(|p| p.join(sourced))
                    .unwrap_or_else(|| sourced.to_path_buf())
            };
            out.extend(read_env_file_kv(&resolved, config_root, visited));
            continue;
        }
        if let Some((key, raw_value)) = parse_kv_line(trimmed) {
            out.push((key.to_string(), raw_value));
        }
    }
    out
}

/// One `package.env` entry's env files expanded in order: real
/// `_grab_pkg_env` copies the global expand map once per package
/// (`env.d` + `make.globals` + `make.defaults` + `make.conf` values --
/// portuale's own `scalars` at the `package.env` read site) and reads
/// each file with `getconfig(..., expand=that map)`, which also feeds
/// every assignment back into the map -- so a later line (or a later
/// file of the same entry) sees earlier ones, exactly like
/// `make.conf`'s own within-file chaining. Returns every pair in file
/// order, `${VAR}`-substituted; callers split the `USE=` half (tokens
/// split *after* substitution) from the scalar half. Per-package
/// values never flow back into `scalars` (real: "don't want
/// per-package settings to pollute the global expand_map").
pub(crate) fn expand_package_env_files(
    env_dir: &Path,
    files: &[String],
    config_root: &Path,
    scalars: &HashMap<String, String>,
) -> Vec<(String, String)> {
    let mut map = scalars.clone();
    let mut out = Vec::new();
    for name in files {
        let mut visited = HashSet::new();
        for (key, raw_value) in read_env_file_kv(&env_dir.join(name), config_root, &mut visited) {
            let value = substitute(&raw_value, &map);
            map.insert(key.clone(), value.clone());
            out.push((key, value));
        }
    }
    out
}
