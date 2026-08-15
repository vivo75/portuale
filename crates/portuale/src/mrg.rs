//! `mrg`: a portuale-only applet (there will never be a Portage counterpart)
//! offering Portage's `emerge` option surface -- every short and long option
//! keeps Portage's spelling -- over portuale's own emerge codepath. A clap
//! front end parses the options, translates the match into canonical
//! long-form argv (see `to_emerge_argv`), and hands it to `pretend::run`,
//! the exact function the `emerge` applet runs. Only the option *names* are
//! shared; semantics beyond the names may differ. Exit codes follow argparse
//! (0 for success/help, 2 for usage errors) via clap's own rendering.
// Portage: _emerge/main.py (options, shortmapping, longopt_aliases, argument_options, actions)

use clap::builder::PossibleValuesParser;
use clap::{Arg, ArgAction, ArgMatches, Command};
#[cfg(any(feature = "vdb-sqlite", feature = "vdb-redb"))]
use portage_vdb::InstalledDb;
use std::process::ExitCode;

/// Portage's `valid_integers` (`int(s) >= 0`).
fn valid_integer(s: &str) -> bool {
    let Ok(v) = s.parse::<i128>() else {
        return false;
    };
    v >= 0
}

/// Portage's `valid_floats` (`float(s) >= 0`). A non-finite result still
/// compares against 0 like Python's `float(s) >= 0` does.
fn valid_float(s: &str) -> bool {
    let Ok(v) = s.parse::<f64>() else {
        return false;
    };
    v >= 0.0
}

// ---------------------------------------------------------------------
// VDB backend selection
// ---------------------------------------------------------------------

use portage_vdb::BackendKind;
use std::path::{Path, PathBuf};
