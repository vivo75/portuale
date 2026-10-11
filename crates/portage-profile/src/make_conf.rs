//! make.conf-style parsing: logical lines, shell tokens, variable substitution.

use crate::*;

/// Split `text` into logical lines, joining continuations (trailing `\`)
/// and lines inside an unclosed quote. Quote characters are kept for
/// `parse_kv_line` to strip.
pub(crate) fn logical_lines(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for raw in text.lines() {
        // A full-line `#` comment outside any continuation is skipped whole;
        // without this a `#` in prose (e.g. `Don't`) would open a bogus
        // quote swallowing the rest of the file.
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

/// Tokenize a `KEY=VALUE` right-hand side with POSIX-shlex semantics
/// (quote removal, backslash handling); the value token is processed
/// *before* [`substitute`] ever sees it.
// Portage: portage.util.getconfig (shlex lexer)
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
                    // Inside `"`, only the quote and the escape character
                    // itself consume the backslash.
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

/// Expand `${VAR}`/`$VAR` in a [`shlex_token`]-processed value: config
/// names set earlier win, else the process environment, else empty.
/// Malformed references expand to empty without warning.
// Portage: portage/util/__init__.py, varexpand
pub(crate) fn substitute(value: &str, scalars: &HashMap<String, String>) -> String {
    portage_util::varexpand(value, |name| {
        scalars
            .get(name)
            .cloned()
            .or_else(|| std::env::var(name).ok())
    })
}

/// Parse one `KEY=value` line, returning the shlex-tokenized value.
/// `None` for comments, blanks, or non-assignments (conditionals,
/// function defs, …).
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

/// Process one make.conf-style file, following `source <path>` directives
/// (absolute paths resolve against `config_root` chroot-style). A missing
/// sourced file is silently skipped; cycles are visited once.
pub(crate) fn process_make_conf_file(
    path: &Path,
    config_root: &Path,
    scalars: &mut HashMap<String, String>,
    config: &mut Config,
    visited_sources: &mut HashSet<PathBuf>,
) -> Result<(), Error> {
    let canon = match path.canonicalize() {
        Ok(c) => c,
        Err(_) => return Ok(()), // missing file: lenient no-op
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

/// Read one `/etc/portage/env/<name>` file as ordered `(KEY, value)`
/// pairs, expanding `source` lines in place. `${VAR}` stays literal for
/// the caller to substitute. A missing file yields an empty list.
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

/// Expand one `package.env` entry's env files in order, threading each
/// assignment into the expand map for later lines. Per-package values
/// never flow back into the global map.
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
