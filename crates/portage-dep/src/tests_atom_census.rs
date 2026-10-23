//! Census of the `Atom` fields read by the candidate matcher.
//!
//! `candidate_hits` destructures `Atom` with no `..`, binding every field to
//! `_`, so adding a field to `Atom` breaks the build there and forces a
//! decision about whether the matcher must read it. This test scans the
//! source text of `lib.rs` to check the shape of that destructure: it is
//! present, has no `..`, names each of the 13 fields exactly once, and every
//! field carries a `// read:` or `// field-ok:` marker on the same line.

const SOURCE: &str = include_str!("lib.rs");

const ATOM_FIELDS: [&str; 13] = [
    "blocker",
    "operator",
    "category",
    "package",
    "version",
    "revision",
    "slot",
    "sub_slot",
    "slot_operator",
    "use_deps",
    "repo",
    "build_id",
    "extended",
];

/// The text of `pub struct Atom { ... }`, body only.
fn atom_struct_body() -> &'static str {
    let start = SOURCE
        .find("pub struct Atom {")
        .expect("pub struct Atom in lib.rs");
    let body = &SOURCE[start + "pub struct Atom {".len()..];
    &body[..body.find("\n}").expect("end of struct Atom")]
}

/// The text between `fn candidate_hits(` and the closing `} = atom;` of its
/// destructure. Panics with a readable message when the destructure is absent.
fn candidate_hits_prefix_and_destructure() -> (&'static str, &'static str) {
    let fn_start = SOURCE
        .find("fn candidate_hits(")
        .expect("fn candidate_hits in lib.rs");
    let after_fn = &SOURCE[fn_start..];
    let open = after_fn
        .find("let Atom {")
        .expect("candidate_hits must destructure Atom with `let Atom {`");
    let head = &after_fn[..open];
    let rest = &after_fn[open + "let Atom {".len()..];
    let close = rest
        .find("} = atom;")
        .expect("candidate_hits destructure must end with `} = atom;`");
    (head, &rest[..close])
}

#[test]
fn atom_struct_has_the_13_census_fields() {
    let body = atom_struct_body();
    let mut declared: Vec<&str> = body
        .lines()
        .filter_map(|line| {
            let line = line.trim_start().strip_prefix("pub ")?;
            Some(line.split(':').next()?.trim())
        })
        .collect();
    declared.sort_unstable();
    let mut expected = ATOM_FIELDS.to_vec();
    expected.sort_unstable();
    assert_eq!(declared, expected, "Atom fields changed: update the census");
}

#[test]
fn candidate_hits_destructures_atom_without_rest_pattern() {
    let (_, destructure) = candidate_hits_prefix_and_destructure();
    assert!(
        !destructure.contains(".."),
        "candidate_hits must destructure Atom exhaustively (no `..`)"
    );
}

#[test]
fn candidate_hits_destructure_is_the_first_thing_in_the_fn() {
    let (head, _) = candidate_hits_prefix_and_destructure();
    // Only the signature, whitespace and `//` comments may come before it.
    let between = head
        .split_once('{')
        .map(|(_, after)| after)
        .expect("fn body opening brace");
    let statements_before: Vec<&str> = between
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//"))
        .collect();
    assert!(
        statements_before.is_empty(),
        "the Atom destructure must be the first statement of candidate_hits, found {statements_before:?}"
    );
}

#[test]
fn each_field_is_bound_once_with_a_marker() {
    let (_, destructure) = candidate_hits_prefix_and_destructure();
    for field in ATOM_FIELDS {
        let lines: Vec<&str> = destructure
            .lines()
            .filter(|line| {
                line.trim_start()
                    .strip_prefix(field)
                    .is_some_and(|rest| rest.trim_start().starts_with(": _,"))
            })
            .collect();
        assert_eq!(
            lines.len(),
            1,
            "field `{field}` must be bound exactly once in the destructure, found {}",
            lines.len()
        );
        let line = lines[0];
        let marked = marker_ok(line);
        assert!(
            marked,
            "field `{field}` needs `// read: <name>` or `// field-ok: <reason>` on its line: {line:?}"
        );
    }
}

/// True for `// read: <non-empty>` or `// field-ok: <non-empty>` on `line`.
fn marker_ok(line: &str) -> bool {
    let Some((_, comment)) = line.split_once("//") else {
        return false;
    };
    ["read:", "field-ok:"].iter().any(|key| {
        comment
            .trim_start()
            .strip_prefix(key)
            .is_some_and(|reason| !reason.trim().is_empty())
    })
}
