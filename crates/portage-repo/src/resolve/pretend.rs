//! Selecting one atom's candidate (`resolve_pretend`).

use crate::*;

// Reads one `slot_operator_mask_built` negative in binary-marked form
// (`!=cat/pkg-ver[binary]`). Portage keys the mask by package object so only the
// binary instance hides; portuale's string readers match both, so the marker keeps
// them apart: tree readers skip marked entries, only the binary-pool filter honours
// them (via this function).
// Portage: depgraph.py, _slot_change_backtrack; MaskReason::SlotOperatorBuilt
pub(crate) fn slot_operator_binary_masked_version(
    neg: &str,
    category: &str,
    package: &str,
) -> Option<String> {
    let rest = neg.strip_prefix("!=")?.strip_suffix("[binary]")?;
    let (c, p, v) = split_cpv(rest)?;
    (c == category && p == package).then_some(v)
}

// Whether a string constraint is a binary-only `slot_operator_mask_built` negative:
// every `extra_constraints` filter and the missing-dep probe skip these (the
// same-version ebuild stays visible); only the binary-pool filter honours them.
pub(crate) fn is_binary_only_mask(constraint: &str) -> bool {
    constraint
        .strip_prefix('!')
        .is_some_and(|neg| neg.ends_with("[binary]"))
}

// The installed-only half of the same per-instance distinction
// (`!=cat/pkg-ver[installed]`, filed by the missing-dep trigger for an installed
// parent with no merge-bound version): tree readers skip marked entries, only the
// installed-instance selection honours them. A marked negative never parses as a
// version, so the missed-update collection skips it too.
// Portage: depgraph.py, _add_dep missing-dependency mask
pub(crate) fn is_installed_only_mask(constraint: &str) -> bool {
    constraint
        .strip_prefix('!')
        .is_some_and(|neg| neg.ends_with("[installed]"))
}

// Reads one installed-only missing-dependency negative. Mirror of
// [`slot_operator_binary_masked_version`] for the other instance kind.
pub(crate) fn slot_operator_installed_masked_version(
    neg: &str,
    category: &str,
    package: &str,
) -> Option<String> {
    let rest = neg.strip_prefix("!=")?.strip_suffix("[installed]")?;
    let (c, p, v) = split_cpv(rest)?;
    (c == category && p == package).then_some(v)
}

// Every per-instance string negative (`[binary]` and `[installed]` marks):
// tree-candidate readers skip them all; instance pools keep honouring their own mark.
// Portage keys every such mask by package object.
pub(crate) fn is_instance_only_mask(constraint: &str) -> bool {
    is_binary_only_mask(constraint) || is_installed_only_mask(constraint)
}

// A bundled options struct would touch every call site for each single-flag
// addition; not worth it (same reasoning as `resolve_pretend_graph`'s allow).
/// Where [`resolve_pretend`] selects from: the repos, the target root,
/// its config and the local binary-package index.
#[derive(Clone, Copy)]
pub struct SelectSource<'a> {
    pub repos: &'a [RepoConfig],
    pub root: &'a Path,
    pub config: &'a portage_profile::Config,
    /// The run's already-built local `$PKGDIR` binary index.
    pub local_binpkg: &'a std::sync::Arc<BinaryIndex>,
}

/// The emerge options that shape one [`resolve_pretend`] selection.
/// `Default` is emerge with no option given (`--with-bdeps` and
/// `--selective` on, everything else off) for a dependency atom.
#[derive(Debug, Clone, Copy)]
pub struct SelectOptions<'a> {
    pub newuse: bool,
    pub changed_use: bool,
    pub update: bool,
    pub excluded: &'a [String],
    pub changed_deps: bool,
    pub with_bdeps: bool,
    pub changed_slot: bool,
    pub selective: bool,
    pub is_top_level: bool,
    pub usepkg: bool,
    pub usepkgonly: bool,
    pub binpkg_respect_use: bool,
    pub usepkg_exclude: &'a [String],
    pub usepkg_include: &'a [String],
    pub rebuilt_binaries: bool,
    pub rebuilt_binaries_timestamp: Option<u64>,
    pub newrepo: bool,
    pub empty: bool,
    pub getbinpkg: bool,
    pub autounmask_keywords: bool,
    pub autounmask_use: bool,
    pub autounmask_license: bool,
    pub autounmask_masks: bool,
    pub extra_constraints: &'a [String],
}

impl Default for SelectOptions<'_> {
    fn default() -> Self {
        Self {
            newuse: false,
            changed_use: false,
            update: false,
            excluded: &[],
            changed_deps: false,
            with_bdeps: true,
            changed_slot: false,
            selective: true,
            is_top_level: false,
            usepkg: false,
            usepkgonly: false,
            binpkg_respect_use: false,
            usepkg_exclude: &[],
            usepkg_include: &[],
            rebuilt_binaries: false,
            rebuilt_binaries_timestamp: None,
            newrepo: false,
            empty: false,
            getbinpkg: false,
            autounmask_keywords: false,
            autounmask_use: false,
            autounmask_license: false,
            autounmask_masks: false,
            extra_constraints: &[],
        }
    }
}
