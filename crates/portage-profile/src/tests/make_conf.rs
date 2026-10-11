use super::*;

#[test]
fn logical_lines_joins_multiline_quotes_and_skips_apostrophe_comments() {
    // A `#` comment containing a lone `'` must not open a quote that
    // swallows the rest of the file (the real cnf/make.globals bug).
    let text = "# Don't edit this\n\
                FEATURES=\"a b\n\
                c d\"\n\
                # it's fine\n\
                X=\"y\" # trailing note\n\
                Z=1\n";
    let lines = logical_lines(text);
    assert_eq!(lines, vec!["FEATURES=\"a b c d\"", "X=\"y\"", "Z=1"]);
}

/// The escape matrix of real `getconfig` + `varexpand`, captured with
/// `portage.util.getconfig` on portage 3.0.82.2 (2026-09-16); the
/// `expand` map held `B=HELLO`. `\"`/`\\` lose their backslash in
/// the shlex stage, `\$` survives it and becomes a literal `$`, and
/// a quote that survives shlex (here from the opposite quote kind)
/// suspends expansion.
#[test]
fn shlex_and_varexpand_escape_handling_matches_portage_getconfig() {
    let scalars: HashMap<String, String> = [("B".to_string(), "HELLO".to_string())]
        .into_iter()
        .collect();
    let expand = |line: &str| {
        let (_, raw) = parse_kv_line(line).expect("kv line");
        substitute(&raw, &scalars)
    };
    assert_eq!(expand(r##"A="x \$y""##), "x $y");
    assert_eq!(expand(r##"A="x \"y\"""##), "x \"y\"");
    assert_eq!(expand(r##"A="x \\y""##), "x \\y");
    assert_eq!(expand(r##"A="x \\${B} z""##), "x ${B} z");
    assert_eq!(expand(r##"A='x \$y'"##), "x $y");
    assert_eq!(expand(r##"A='${B}'"##), "HELLO");
    assert_eq!(expand(r##"A="'${B}'""##), "'${B}'");
    assert_eq!(expand(r##"A=${B}"##), "HELLO");
    assert_eq!(expand(r##"A=$B"##), "HELLO");
    assert_eq!(expand(r##"A="$B""##), "HELLO");
    assert_eq!(expand(r##"A="\$""##), "$");
    assert_eq!(expand(r##"A="\\""##), "\\");
    assert_eq!(expand(r##"A="\$B""##), "$B");
    assert_eq!(expand(r##"A="\\$B""##), "$B");
    assert_eq!(expand(r##"A="a\nb""##), "a\\nb");
}
