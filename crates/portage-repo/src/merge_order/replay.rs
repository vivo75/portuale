//! Replaying `_create_graph` for fail-fast passes.

use crate::merge_order::*;

/// A `(category, package)` key.
pub(crate) type Cp = (String, String);

/// The USE state `replay_create_graph` needs for one entry: declared `IUSE`, the
/// enabled flags before any autounmask change, and the flags no change can
/// touch. `None` for entries whose USE cannot change (installed, binary, missing).
#[derive(Debug, Clone, Default)]
pub(crate) struct ReplayUse {
    pub iuse: HashSet<String>,
    pub enabled: HashSet<String>,
    pub fixed: HashSet<String>,
}

/// The first dependency the replayed walk fails on, as `(parent entry, dep index)`.
/// `contradiction` marks the autounmask shape (a flip Portage rejects).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReplayFailure {
    pub parent: usize,
    pub edge: usize,
    pub contradiction: bool,
}

/// What `replay_create_graph` saw: admission order, the edge admitting each entry,
/// the first failing dependency, and the needed USE changes per `cat/pkg`.
#[derive(Debug, Clone, Default)]
pub(crate) struct WalkReplay {
    pub admitted: Vec<usize>,
    pub admitted_by: Vec<Option<(usize, usize)>>,
    /// For an argument entry: the index into `top_level_atoms` of the adding atom.
    pub admitted_arg: Vec<Option<usize>>,
    /// Every resolved `(parent, dep index, target)` edge, in walk order.
    pub edges: Vec<(usize, usize, usize)>,
    /// Per `cat/pkg`, the `(parent, dep index)` first needing each USE change.
    pub flip_origin: HashMap<(String, String), (usize, usize)>,
    pub failure: Option<ReplayFailure>,
    pub needed: HashMap<(String, String), HashMap<String, bool>>,
}

/// Arguments of [`replay_create_graph`].
pub(crate) struct ReplayCreateGraphArgs<'a> {
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) top_level_atoms: &'a [String],
    pub(crate) root: &'a Path,
    pub(crate) allow_flips: bool,
    pub(crate) needed_init: &'a HashMap<(String, String), HashMap<String, bool>>,
    pub(crate) use_of: &'a dyn Fn(usize) -> Option<ReplayUse>,
}

/// Arguments of [`replay_key`].
pub(crate) struct ReplayKeyArgs<'a> {
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) pre: &'a DigraphPrelude<'a>,
    pub(crate) alt_suppressed: &'a [HashSet<usize>],
    pub(crate) i: usize,
    pub(crate) disjunctive: bool,
    pub(crate) key: u8,
    pub(crate) allow_flips: bool,
    pub(crate) use_of: &'a dyn Fn(usize) -> Option<ReplayUse>,
    pub(crate) discovered: &'a mut [bool],
    pub(crate) stack: &'a mut Vec<usize>,
    pub(crate) out: &'a mut WalkReplay,
}
