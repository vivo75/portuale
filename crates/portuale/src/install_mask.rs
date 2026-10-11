//! `INSTALL_MASK` filtering: fold the `no{man,info,doc}` `FEATURES` tokens
//! into the mask, then delete every masked path from the install image
//! before anything is copied to `${ROOT}`.
// Portage: install_mask.py, InstallMask; misc-functions.sh, preinst_mask; vartree.py, dblink.treewalk

use std::path::Path;

/// Fold the `no{man,info,doc}` `FEATURES` tokens into the configured mask
/// (each becomes `/usr/share/<man|info|doc>`; empty `EPREFIX` assumed).
/// Returns the mask plus whether a `no*` token was folded in, which gates
/// the caller's `rmdir(ED/usr/share)`.
pub(crate) fn resolve<S: AsRef<str>>(configured: &str, features: &[S]) -> (String, bool) {
    let mut tokens: Vec<String> = configured.split_whitespace().map(String::from).collect();
    let mut prunes_usr_share = false;
    for (feature, subdir) in [("noman", "man"), ("noinfo", "info"), ("nodoc", "doc")] {
        if features.iter().any(|f| f.as_ref() == feature) {
            tokens.push(format!("/usr/share/{subdir}"));
            prunes_usr_share = true;
        }
    }
    (tokens.join(" "), prunes_usr_share)
}

struct Pattern {
    is_inclusive: bool,
    leading_slash: bool,
    /// The pattern with any leading `-` stripped.
    pattern: String,
}

/// Mask pattern list: `install_mask.split()`, a leading `-` flips a token
/// non-inclusive.
pub(crate) struct InstallMask {
    patterns: Vec<Pattern>,
}

impl InstallMask {
    /// One `install_mask.split()` token: a leading `-` flips it non-inclusive.
    pub(crate) fn new(install_mask: &str) -> Self {
        let patterns = install_mask
            .split_whitespace()
            .map(|tok| {
                let is_inclusive = !tok.starts_with('-');
                let pattern = if is_inclusive { tok } else { &tok[1..] }.to_string();
                let leading_slash = pattern.starts_with('/');
                Pattern {
                    is_inclusive,
                    leading_slash,
                    pattern,
                }
            })
            .collect();
        Self { patterns }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// `path` is relative to `${ED}`, ending with `/` for a directory. The
    /// result is the last matching pattern's inclusiveness, in order.
    pub(crate) fn matches(&self, path: &str) -> bool {
        let mut ret = false;
        for p in &self.patterns {
            let hit = if p.leading_slash {
                let pattern = if path.ends_with('/') {
                    format!("{}/", p.pattern.trim_end_matches('/'))
                } else {
                    p.pattern.clone()
                };
                let body = &pattern[1..];
                fnmatch(path, body) || fnmatch(path, &format!("{}/*", body.trim_end_matches('/')))
            } else {
                let base = path
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or(path);
                fnmatch(base, &p.pattern)
            };
            if hit {
                ret = p.is_inclusive;
            }
        }
        ret
    }
}

/// Delete every masked file under `base_dir`, then remove masked directories
/// left empty. Per-path `unlink`/`rmdir` errors are swallowed.
pub(crate) fn install_mask_dir(base_dir: &Path, mask: &InstallMask) -> std::io::Result<()> {
    let rel_of = |p: &Path| -> String {
        p.strip_prefix(base_dir)
            .map(|r| r.to_string_lossy().into_owned())
            .unwrap_or_default()
    };

    // Remove masked files (iterative DFS).
    let mut dir_stack: Vec<std::path::PathBuf> = Vec::new();
    let mut todo: Vec<std::path::PathBuf> = vec![base_dir.to_path_buf()];
    while let Some(parent) = todo.pop() {
        dir_stack.push(parent.clone());
        let entries = match portage_util::read_dir_entries(&parent) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries {
            let abs_path = entry.path();
            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            if file_type.is_dir() {
                todo.push(abs_path);
            } else if mask.matches(&rel_of(&abs_path)) {
                let _ = std::fs::remove_file(&abs_path);
            }
        }
    }

    // Remove masked dirs, deepest first. `rmdir` silently fails on a
    // non-empty dir, which is what makes exclusion (`-`) patterns work.
    while let Some(dir_path) = dir_stack.pop() {
        if mask.matches(&format!("{}/", rel_of(&dir_path))) {
            let _ = std::fs::remove_dir(&dir_path);
        }
    }
    Ok(())
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use portage_util::TempDir;

    #[test]
    fn resolve_folds_no_star_features_in_portage_order() {
        let (mask, prune) = resolve("", &["noinfo", "noman"]);
        assert_eq!(mask, "/usr/share/man /usr/share/info");
        assert!(prune);

        let (mask, prune) = resolve("/opt/foo", &["nodoc"]);
        assert_eq!(mask, "/opt/foo /usr/share/doc");
        assert!(prune);

        let (mask, prune) = resolve("", &[] as &[&str]);
        assert_eq!(mask, "");
        assert!(!prune);
    }

    #[test]
    fn anchored_pattern_matches_the_dir_and_everything_under_it() {
        let mask = InstallMask::new("/usr/share/info");
        assert!(mask.matches("usr/share/info"));
        assert!(mask.matches("usr/share/info/"));
        assert!(mask.matches("usr/share/info/dir"));
        assert!(mask.matches("usr/share/info/foo/bar.info"));
        assert!(!mask.matches("usr/share/information"));
        assert!(!mask.matches("usr/share/doc/x"));
        assert!(!mask.matches("usr/bin/info"));
    }

    #[test]
    fn unanchored_pattern_matches_basename_anywhere() {
        let mask = InstallMask::new("*.la");
        assert!(mask.matches("usr/lib64/libfoo.la"));
        assert!(mask.matches("libfoo.la"));
        assert!(!mask.matches("usr/lib64/libfoo.so"));
    }

    #[test]
    fn exclusion_pattern_re_includes_a_path_a_later_lower_index_rule_masked() {
        // A `-` after a broad mask rescues the path.
        let mask = InstallMask::new("/usr/share/doc -/usr/share/doc/keepme");
        assert!(mask.matches("usr/share/doc/gone/readme"));
        assert!(!mask.matches("usr/share/doc/keepme"));
        assert!(!mask.matches("usr/share/doc/keepme/NEWS"));
    }

    #[test]
    fn install_mask_dir_removes_matched_files_and_empty_dirs_only() {
        let tmp = TempDir::new("im-test").keep();
        let ed = tmp.join("image");
        std::fs::create_dir_all(ed.join("usr/share/info")).unwrap();
        std::fs::create_dir_all(ed.join("usr/share/fonts/noto")).unwrap();
        std::fs::write(ed.join("usr/share/info/foo.info"), b"x").unwrap();
        std::fs::write(ed.join("usr/share/info/dir"), b"x").unwrap();
        std::fs::write(ed.join("usr/share/fonts/noto/a.ttf"), b"x").unwrap();

        let mask = InstallMask::new("/usr/share/info");
        install_mask_dir(&ed, &mask).unwrap();

        assert!(!ed.join("usr/share/info").exists(), "empty masked dir gone");
        assert!(
            ed.join("usr/share/fonts/noto/a.ttf").exists(),
            "unmasked content untouched"
        );
        assert!(ed.join("usr/share").exists(), "still non-empty, kept");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
