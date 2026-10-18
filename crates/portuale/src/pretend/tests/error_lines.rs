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

fn parsed(args: &[&str]) -> Result<EmergeOpts<'static>, std::process::ExitCode> {
    let owned: &'static [String] = Box::leak(
        args.iter()
            .map(|a| a.to_string())
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );
    parse_emerge_args(owned)
}

/// Backlog #405: a bundled `-X` takes the rest of the bundle or the next argument
/// as its value; `-j`/`-l` followed by letters is the bare option plus more short
/// flags. Oracle: Portage 3.0.82.2, `LLM/evidence/2026-10-18-405-s0/compare.txt`
/// (`-pX` errors, `-jabc` is `-j -a -b -c`).
#[test]
fn bundled_x_and_j_l_clusters_parse_like_portage() {
    let opts = parsed(&["-pXfoo", "bar"]).expect("-pXfoo parses");
    assert!(opts.pretend);
    assert_eq!(opts.excluded, vec!["foo".to_string()]);
    let opts = parsed(&["-pX", "foo bar"]).expect("-pX foo parses");
    assert_eq!(opts.excluded, vec!["foo".to_string(), "bar".to_string()]);
    assert!(parsed(&["-pX"]).is_err());
    let opts = parsed(&["-jpv"]).expect("-jpv is -j -p -v");
    assert!(opts.pretend && opts.verbose);
    assert_eq!(opts.jobs, usize::MAX);
    assert!(parsed(&["-jx"]).is_err());
    assert!(parsed(&["-lx2"]).is_err());
    assert!(parsed(&["-zp"]).is_err());
    let opts = parsed(&["--load-average", "--pretend"]).expect("a bare --load-average parses");
    assert!(opts.pretend && opts.load_average.is_none());
}

/// Backlog #401: `-f`/`-F` parse as options and bundle letters; `-F` implies `-f`.
/// Portage: `_emerge/actions.py` (`fetchonly`, `fetch_all_uri`).
#[test]
fn fetchonly_and_fetch_all_uri_parse_alone_and_in_bundles() {
    let opts = parsed(&["-pf", "dev-libs/a"]).expect("-pf parses");
    assert!(opts.pretend && opts.fetchonly && !opts.fetch_all_uri);
    let opts = parsed(&["--fetchonly", "dev-libs/a"]).expect("--fetchonly parses");
    assert!(opts.fetchonly);
    let opts = parsed(&["-F"]).expect("-F parses");
    assert!(opts.fetchonly && opts.fetch_all_uri);
    let opts = parsed(&["-aF", "dev-libs/a"]).expect("-aF parses");
    assert!(opts.ask && opts.fetchonly && opts.fetch_all_uri);
    let opts = parsed(&["--fetch-all-uri"]).expect("--fetch-all-uri parses");
    assert!(opts.fetchonly && opts.fetch_all_uri);
}
