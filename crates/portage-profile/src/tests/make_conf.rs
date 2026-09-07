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
