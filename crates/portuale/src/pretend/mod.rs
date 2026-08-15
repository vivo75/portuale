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

mod news;
pub use news::*;

use crate::ebuild_merge;

use crate::ebuild_phases;

use portage_dep::{Atom, Blocker, Operator, SlotOperator, match_from_list, parse_atom};

use std::collections::{BTreeMap, HashMap, HashSet};

use std::path::Path;

use std::process::ExitCode;
