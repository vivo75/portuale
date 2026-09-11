//! The unsatisfied slot-operator dependency probe.

use crate::*;

/// Arguments of [`slot_operator_unsatisfied_probe`].
#[cfg(test)]
pub(crate) struct SlotOperatorUnsatisfiedProbeArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) slot_want: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) pullers: &'a SlotPullers,
    pub(crate) masked: &'a HashMap<(String, String), Vec<MaskEntry>>,
    pub(crate) with_bdeps: bool,
    pub(crate) excluded: &'a [String],
    pub(crate) config: &'a portage_profile::Config,
}

/// [`slot_operator_unsatisfied_probe_full`]'s result: the installed
/// parents to reinstall and the `(category, package, version)` binary
/// parents to mask.
pub(crate) type SlotOpUnsatProbe = (
    BTreeSet<(String, String)>,
    BTreeSet<(String, String, String)>,
);
