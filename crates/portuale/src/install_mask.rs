//! `INSTALL_MASK` filtering: fold the `no{man,info,doc}` `FEATURES` tokens
//! into the mask, then delete every masked path from the install image
//! before anything is copied to `${ROOT}`.
// Portage: install_mask.py, InstallMask; misc-functions.sh, preinst_mask; vartree.py, dblink.treewalk

use std::path::Path;

/// Python `fnmatch.fnmatch` semantics (case-sensitive, `/` not special, `*`
/// spans everything), as an anchored regex. Supports `*`, `?` and
/// `[...]`/`[!...]` classes. Also reused by `needed_elf`'s
/// `PROVIDES_EXCLUDE`/`REQUIRES_EXCLUDE` matcher.
pub(crate) fn fnmatch(name: &str, pattern: &str) -> bool {
    thread_local! {
        static CACHE: std::cell::RefCell<std::collections::HashMap<String, regex::Regex>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let re = cache
            .entry(pattern.to_string())
            .or_insert_with(|| regex::Regex::new(&fnmatch_to_regex(pattern)).unwrap());
        re.is_match(name)
    })
}

fn fnmatch_to_regex(pattern: &str) -> String {
    let mut out = String::from("(?s)^");
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        match c {
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            '[' => {
                let mut j = i;
                if j < chars.len() && (chars[j] == '!' || chars[j] == '^') {
                    j += 1;
                }
                if j < chars.len() && chars[j] == ']' {
                    j += 1;
                }
                while j < chars.len() && chars[j] != ']' {
                    j += 1;
                }
                if j >= chars.len() {
                    // No closing bracket -- a literal `[`.
                    out.push_str("\\[");
                } else {
                    let mut class: String = chars[i..j].iter().collect();
                    i = j + 1;
                    if let Some(rest) = class.strip_prefix('!') {
                        class = format!("^{rest}");
                    }
                    // A `]` can't appear unescaped inside; `\` is escaped.
                    let class = class.replace('\\', "\\\\");
                    out.push('[');
                    out.push_str(&class);
                    out.push(']');
                }
            }
            _ => {
                if "\\.+()|{}^$".contains(c) {
                    out.push('\\');
                }
                out.push(c);
            }
        }
    }
    out.push('$');
    out
}
