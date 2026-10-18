use super::*;

/// Plan #347 S8 (347-ERR): option-parsing failures print Portage's
/// `emerge: error:` line without the `usage:` block (347-USG cut).
/// The trailing-newline rule (D4) is byte-exact: messages Portage
/// passes to `parser.error` carry their own trailing `\n` and gain
/// argparse's, ending in a blank line; argparse-native errors end in
/// one `\n`. Oracle: Portage 3.0.82.2 C cells
/// `LLM/evidence/2026-10-15-346-s0/c/` (`c01`, `c06`, `c12` below).
#[test]
fn parser_error_keeps_portages_trailing_newline_rule() {
    // C cell c01 (`--jobs=x`): `parser.error` message with trailing
    // `\n`, so the rendered line ends in a blank line.
    assert_eq!(
        parser_error_text("Invalid --jobs parameter: 'x'\n"),
        "emerge: error: Invalid --jobs parameter: 'x'\n\n"
    );
    // C cell c06 (`--color=maybe`): argparse-native invalid choice,
    // single `\n`.
    assert_eq!(
        parser_error_text("argument --color: invalid choice: 'maybe' (choose from 'y', 'n')"),
        "emerge: error: argument --color: invalid choice: 'maybe' (choose from 'y', 'n')\n"
    );
    // C cell c12 (`--frobnicate`): argparse-native unrecognized
    // arguments, single `\n`.
    assert_eq!(
        parser_error_text("unrecognized arguments: --frobnicate"),
        "emerge: error: unrecognized arguments: --frobnicate\n"
    );
}
