//! `emerge --pretend`: resolve against every repos.conf repo and display, never
//! build. USE/ACCEPT_KEYWORDS/mask data come from the profile chain + make.conf
//! (see portage-profile), not a stand-in. DEPEND+RDEPEND recurse (diamond-safe;
//! multiple slots coexist); blockers and slot conflicts are reported, not enforced.
//! Output is a simplified subset of Portage's `--pretend` output, not byte-identical.
//! Every Portage emerge option/action is recognized by name; unimplemented ones fail
//! with an accurate message, unknown ones with "unrecognized option".
//!
//! Flag notes below; full Portage groundings live in the code-notes file.
//
//! `--update` keeps an installed version that still satisfies the atom;
//! `--deep` (`-D`, optional value) cuts recursion into installed packages'
//! dependencies; `--exclude` (`-X`, repeatable, space-separated lists) pins
//! matching packages as-is. `--deselect` (`-W`, optional value) is its own
//! standalone action, previewed under `--pretend`.
//! `--json` is portuale-only (Portage has no such mode): the whole graph as one
//! JSON line. `--verbose` (`-v`, optional value) and short-flag bundling (`-pv`)
//! follow Portage's value-taking rules; `--help` wins wherever it appears.

use crate::color::{self, Colorizer};

mod args;
mod config_action;
mod conflicts;
mod display;
mod env_config;
mod help;
mod info;
mod json;
mod news;
mod prompt;
mod removal;
mod world;

#[cfg(test)]
mod tests;

pub(crate) use args::*;
pub(crate) use config_action::*;
pub(crate) use conflicts::*;
pub(crate) use display::*;
pub(crate) use env_config::*;
pub(crate) use help::*;
pub(crate) use info::*;
pub(crate) use json::*;
pub use news::*;
pub(crate) use prompt::*;
pub(crate) use removal::*;
pub(crate) use world::*;

use crate::ebuild_merge;

use crate::ebuild_package;

use crate::ebuild_phases;

use crate::emerge_build;

use crate::emerge_getbinpkg;

use crate::emerge_options;

use crate::needed_elf;

use portage_dep::{Atom, Blocker, Operator, SlotOperator, match_from_list, parse_atom};

use portage_repo::{
    BlockerConflict, BlockerSatisfiedBy, ChangedDepsReportEntry, GraphEntry, PretendOutcome,
    SlotConflict, all_installed_packages, config_root_from_env, ebuild_visible_at, root_from_env,
    slot_conflict_flag_sets, split_pf,
};

use std::collections::{BTreeMap, HashMap, HashSet};

use std::path::Path;

use std::process::ExitCode;

/// Reports and returns the exit code for one option/action token that is not
/// otherwise handled -- shared by standalone tokens and short-flag bundle
/// characters, so both produce identical messages.
fn report_option(token: &str) -> ExitCode {
    if let Some(found) = emerge_options::lookup(token) {
        // Reports and exits immediately, so this option's own value token needs
        // no skipping: nothing after this point is ever looked at.
        let kind = if found.category == emerge_options::Category::Action {
            "action"
        } else {
            "option"
        };
        eprintln!(
            "emerge: {kind} {:?} is a Portage emerge {kind}, but is not yet \
             implemented in portuale -- run \"emerge --help\" for the options \
             and actions that are.",
            found.canonical
        );
    } else {
        eprintln!("emerge: unrecognized option {token:?}");
    }
    ExitCode::from(2)
}

/// The `--backtrack` budget when the flag is absent: Portage's own default
/// (`0` only under `--nodeps`). Single source for the initializer and `--help`.
// Portage: _emerge/depgraph.py, _backtrack_depgraph
const DEFAULT_BACKTRACK_MAX: u32 = 20;
