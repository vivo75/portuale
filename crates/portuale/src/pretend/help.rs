//! `emerge --help`.

/// Whether `--help`/`-h` appears anywhere in `args`, including as one
/// character of a short-flag bundle -- see the module doc comment on why
/// this wins unconditionally, checked before anything else.
pub(crate) fn wants_help(args: &[String]) -> bool {
    args.iter().any(|arg| {
        arg == "--help"
            || arg == "-h"
            || (arg.starts_with('-') && !arg.starts_with("--") && arg[1..].contains('h'))
    })
}

/// The `emerge --help` / `-h` text: a grouped tour of every action and
/// option portuale actually implements. NOT a port of real emerge's
/// own `_emerge/help.py` (157 lines of colorized syntax for its full
/// ~130-flag surface -- see the module doc comment). The contract suite
/// pins it in full.
pub(crate) fn print_help() {
    print!("{HELP_TEXT}");
}

pub(crate) const HELP_TEXT: &str = include_str!("help.txt");
