//! `dep_cp`: the conservative `category/package` extraction behind the
//! `dep_atom` prefilter index, shared by the sqlite and redb writers.
//! Moved out of `sqlite/write.rs` unchanged (S5.3).

/// The `category/package` of one whitespace-separated token of a `*DEPEND`
/// string, or `None` when the token is not clearly an atom.
///
/// `dep_atom` is a **prefilter index**: it holds a superset of the real
/// dependency atoms, found without `portage-dep` (module doc, item 8). The
/// caller of `reverse_dependents` still does the real `USE` reduction and
/// atom match. A false positive is harmless, a false negative loses a
/// dependent, so the routine skips a token only when it cannot be a
/// `category/package` (or, for the version-stripping, when unsure). The rule:
///
/// - skip the structure tokens `(`, `)`, `||`, `^^`, `??` and anything
///   ending in `?` (a `USE` conditional);
/// - strip a leading `!!` or `!`, then one version operator (`<=`, `>=`,
///   `<`, `>`, `=`, `~`);
/// - cut at the first `[` (`USE` dependency) or `:` (slot, `::repo`);
/// - the rest must be `category/name[-version]`: a category of
///   `[A-Za-z0-9+_.-]`, a name of `[A-Za-z0-9+_-]` that does not start with
///   `-`, exactly one `/`;
/// - with an operator, the name must end in `-<version>[-r<N>][*]`
///   (digits and dots, an optional lowercase letter, `_alpha|beta|pre|rc|p`
///   suffixes), which is stripped; without one, a name that looks like it
///   ends in a version is skipped.
pub(crate) fn dep_cp(tok: &str) -> Option<&str> {
    if matches!(tok, "(" | ")" | "||" | "^^" | "??") || tok.ends_with('?') {
        return None;
    }
    let mut s = tok;
    s = s
        .strip_prefix("!!")
        .or_else(|| s.strip_prefix('!'))
        .unwrap_or(s);
    let mut has_op = false;
    for op in ["<=", ">=", "<", ">", "=", "~"] {
        if let Some(rest) = s.strip_prefix(op) {
            s = rest;
            has_op = true;
            break;
        }
    }
    let s = s.split(['[', ':']).next().unwrap_or(s);
    let (cat, rest) = s.split_once('/')?;
    let cat_ok = !cat.is_empty()
        && cat
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'_' | b'.' | b'-'));
    if !cat_ok || rest.contains('/') {
        return None;
    }
    let name = if has_op {
        strip_version(rest)?
    } else {
        if rest.rfind('-').is_some_and(|i| is_version(&rest[i + 1..])) {
            return None;
        }
        rest
    };
    let name_ok = !name.is_empty()
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'_' | b'-'));
    name_ok.then(|| &s[..cat.len() + 1 + name.len()])
}

/// The marker `cp` of a `dep_atom` row for a token that [`dep_cp`] could not
/// classify. No real `category/package` is empty, so the marker never
/// collides with a lookup key; `reverse_dependents` always adds it to its
/// key set (S8.1).
pub(crate) const UNSURE_CP: &str = "";

/// The index key of one token of a `*DEPEND` string (S8.1): `None` for a
/// structure token that can never hold an atom (`(`, `)`, `||`, `^^`,
/// `??`, `flag?`), `Some(cp)` when [`dep_cp`] reads a package out of it and
/// `Some(`[`UNSURE_CP`]`)` for anything else, i.e. a token that might still
/// be an atom the matcher accepts (a wildcard, a bare versioned name, an
/// odd version). The marker keeps the prefilter a superset: an entry with
/// such a token is returned for every `cp`.
pub(crate) fn dep_index_key(tok: &str) -> Option<&str> {
    if matches!(tok, "(" | ")" | "||" | "^^" | "??") || tok.ends_with('?') {
        return None;
    }
    Some(dep_cp(tok).unwrap_or(UNSURE_CP))
}

/// `foo-1.2_p3-r1*` to `foo`; `None` when no version follows the last `-`.
fn strip_version(rest: &str) -> Option<&str> {
    let mut r = rest.strip_suffix('*').unwrap_or(rest);
    if let Some(i) = r.rfind("-r")
        && !r[i + 2..].is_empty()
        && r[i + 2..].bytes().all(|b| b.is_ascii_digit())
    {
        r = &r[..i];
    }
    let i = r.rfind('-')?;
    is_version(&r[i + 1..]).then(|| &r[..i])
}

/// `digits(.digits)*[a-z](_(alpha|beta|pre|rc|p)digits*)*`, no revision.
fn is_version(v: &str) -> bool {
    let b = v.as_bytes();
    let digits = |i: &mut usize| {
        let start = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        *i > start
    };
    let mut i = 0;
    if !digits(&mut i) {
        return false;
    }
    while i < b.len() && b[i] == b'.' {
        i += 1;
        if !digits(&mut i) {
            return false;
        }
    }
    if i < b.len() && b[i].is_ascii_lowercase() {
        i += 1;
    }
    while i < b.len() && b[i] == b'_' {
        i += 1;
        let Some(suffix) = ["alpha", "beta", "pre", "rc", "p"]
            .iter()
            .find(|s| v[i..].starts_with(**s))
        else {
            return false;
        };
        i += suffix.len();
        digits(&mut i);
    }
    i == b.len()
}

#[cfg(test)]
mod tests {
    use super::{dep_cp, dep_index_key};

    #[test]
    fn dep_index_key_marks_unclear_tokens_and_skips_structure() {
        for (tok, want) in [
            ("dev-libs/a", Some("dev-libs/a")),
            (">=dev-libs/a-1.2", Some("dev-libs/a")),
            ("(", None),
            ("||", None),
            ("ssl?", None),
            ("!ssl?", None),
            ("dev-libs/*", Some("")),
            ("dev-libs/a-1.2", Some("")),
            (">=dev-libs/a", Some("")),
            ("justaword", Some("")),
        ] {
            assert_eq!(dep_index_key(tok), want, "{tok}");
        }
    }

    #[test]
    fn dep_cp_extracts_the_package_of_atom_like_tokens() {
        for (tok, want) in [
            ("dev-libs/a", Some("dev-libs/a")),
            ("!dev-libs/a", Some("dev-libs/a")),
            ("!!dev-libs/a", Some("dev-libs/a")),
            (">=dev-libs/a-1.2.3", Some("dev-libs/a")),
            ("<dev-libs/a-1.2_p3-r2", Some("dev-libs/a")),
            ("~dev-libs/a-1.2b", Some("dev-libs/a")),
            ("=dev-libs/a-1*", Some("dev-libs/a")),
            ("=x11-libs/gtk+-3.24.1:3=[X,-y(+)]", Some("x11-libs/gtk+")),
            ("dev-lang/python:3.11", Some("dev-lang/python")),
            ("dev-lang/python:0/1.2=", Some("dev-lang/python")),
            ("dev-libs/a::gentoo", Some("dev-libs/a")),
            ("dev-libs/a[x,y?]", Some("dev-libs/a")),
            ("virtual/pkgconfig", Some("virtual/pkgconfig")),
            ("sys-libs/glibc-r1", Some("sys-libs/glibc-r1")),
            ("(", None),
            (")", None),
            ("||", None),
            ("ssl?", None),
            ("!ssl?", None),
            ("^^", None),
            ("a/b/c", None),
            ("justaword", None),
            ("dev-libs/a-1.2", None),
            (">=dev-libs/a", None),
            (">=dev-libs/a-x", None),
            ("dev-libs/a*", None),
            ("/a", None),
            ("dev-libs/", None),
        ] {
            assert_eq!(dep_cp(tok), want, "{tok}");
        }
    }
}
