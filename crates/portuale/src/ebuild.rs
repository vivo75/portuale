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
use crate::ebuild_phases;
use crate::ebuild_unmerge;
use std::process::ExitCode;

#[cfg(test)]
mod tests {
    use super::*;

    fn args(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| s.to_string()).collect()
    }
}
