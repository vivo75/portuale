// `ebuild <file> <command> [command...]`: CLI-surface recognition
// plus real execution for the phase chain (`pretend`/`setup`/`unpack`/
// `prepare`/`configure`/`compile`/`test`/`install`), `merge`/`qmerge`/
// `unmerge`/`package`/`config`/`info`/`prerm`/`postrm`. Every other
// Portage command is recognized but still a dry-run stub (`instprep`
// runs inside every `merge`/`qmerge`; only the standalone command is
// stubbed).
//
// Exit codes mirror Portage's `ebuild`: 2 for missing args, 1 for
// everything else invalid.
//
// Known deviation: `bin/ebuild` uses `parse_known_args`, which swallows
// an unrecognized flag into the positional leftovers (surfacing later
// as a confusing error); portuale rejects it immediately instead.
//
// `-h`/`--help` wins unconditionally, like argparse's own check during
// parsing. The help text is a portuale-specific summary, not a port of
// the argparse usage block; `--version` is deliberately not implemented.
// Portage: bin/ebuild, doebuild.py

use crate::ebuild_merge;
use crate::ebuild_options::{self, Kind};
use crate::ebuild_package;
use crate::ebuild_phases;
use crate::ebuild_unmerge;
use std::process::ExitCode;

/// Whether `--help`/`-h` appears anywhere in `args`. No short-flag
/// bundle scanning: `bin/ebuild` declares no short aliases for its own
/// options, so `-h` is never part of a bundle.
fn wants_help(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--help" || arg == "-h")
}

/// A short, portuale-specific summary (not a port of the argparse
/// usage block; see the module doc).
fn print_help() {
    println!("ebuild: command-line interface to the Portuale package manager");
    println!();
    println!("Usage:");
    println!("   ebuild <ebuild file> <command> [command ...]");
    println!("   ebuild --help");
    println!();
    println!("Commands run for real: the actionmap phase chain (unpack, prepare,");
    println!("configure, compile, test, install), the standalone phases (pretend,");
    println!("setup, ...), and merge / qmerge / unmerge / package / config / info /");
    println!("prerm / postrm. Any other Portage command (clean, digest, manifest, ...)");
    println!("is recognized and accepted but not yet implemented -- a no-op dry run.");
    println!();
    println!("Options:");
    println!("   --force              regenerate digests (with the digest/manifest commands)");
    println!("   --color y|n          enable or disable color output");
    println!(
        "   --debug              show debug output (as Portage: sets PORTAGE_DEBUG, so bin/ebuild.sh runs set -x)"
    );
    println!("   --ignore-default-opts  do not use the EBUILD_DEFAULT_OPTS environment variable");
    println!("   --skip-manifest      skip all manifest checks");
    println!(
        "   --shell bash|brush   real shell backend for phase/hook execution (default: bash; portuale extension)"
    );
    println!("   -h, --help           show this message and exit");
    println!();
    println!(
        "Every other Portage ebuild option is recognized by name (see bin/ebuild) but \
         not yet implemented -- using one reports which option it is, instead of a \
         generic error. --shell is a portuale extension, not a Portage bin/ebuild \
         option."
    );
    println!("See README.md and ebuild(1) for the full picture.");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| s.to_string()).collect()
    }
}
