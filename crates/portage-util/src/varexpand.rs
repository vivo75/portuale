//! Portage's `varexpand` (`portage/util/__init__.py:885-1015`).

/// Expands `${NAME}` and `$NAME` in `template` the way Portage's
/// `varexpand` does. `lookup` answers each name; `None` expands to
/// nothing.
///
/// Escapes follow Portage. `\$` is a literal `$`, an escaped newline
/// disappears, and any other `\x` keeps both characters. `\\` is a
/// literal `\`, plus Portage's bug-compatible copy of a following quote
/// or `$`. Inside single quotes nothing expands. A malformed reference
/// (`${` without a name or closing brace) empties the whole result.
pub fn varexpand(template: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let chars: Vec<char> = template.chars().collect();
    let mut out = String::new();
    let mut pos = 0;
    let mut in_single = false;
    let mut in_double = false;
    while pos < chars.len() {
        let current = chars[pos];
        match current {
            '\'' => {
                out.push('\'');
                if !in_double {
                    in_single = !in_single;
                }
                pos += 1;
            }
            '"' => {
                out.push('"');
                if !in_single {
                    in_double = !in_double;
                }
                pos += 1;
            }
            '\\' if !in_single => {
                if pos + 1 >= chars.len() {
                    out.push('\\');
                    break;
                }
                let next = chars[pos + 1];
                pos += 2;
                match next {
                    '$' => out.push('$'),
                    '\\' => {
                        out.push('\\');
                        if pos < chars.len() && matches!(chars[pos], '\'' | '"' | '$') {
                            out.push(chars[pos]);
                            pos += 1;
                        }
                    }
                    '\n' => {}
                    other => {
                        out.push('\\');
                        out.push(other);
                    }
                }
            }
            '$' if !in_single => {
                pos += 1;
                if pos == chars.len() {
                    out.push('$');
                    continue;
                }
                let braced = chars[pos] == '{';
                if braced {
                    pos += 1;
                    if pos == chars.len() {
                        return String::new();
                    }
                }
                let start = pos;
                while pos < chars.len() && (chars[pos].is_ascii_alphanumeric() || chars[pos] == '_')
                {
                    pos += 1;
                }
                let name: String = chars[start..pos].iter().collect();
                if braced {
                    if pos == chars.len() || chars[pos] != '}' {
                        return String::new();
                    }
                    pos += 1;
                }
                if name.is_empty() {
                    return String::new();
                }
                if let Some(value) = lookup(&name) {
                    out.push_str(&value);
                }
            }
            _ => {
                out.push(current);
                pos += 1;
            }
        }
    }
    out
}
