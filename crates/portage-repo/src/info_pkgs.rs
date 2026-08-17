//! Package rows for `emerge --info`.

use crate::*;

/// One `emerge --info <atom>` per-package result -- see `resolve_info_candidate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoCandidate {
    /// `category/package-version` of the best visible ebuild candidate.
    pub cpv: String,
    pub repo_name: String,
    /// `USE="…"` + one `VAR="…"` per non-hidden `USE_EXPAND` group, same pairs `pretend.rs`
    /// renders for a `New` entry under `-pv`.
    pub use_expand_display: Vec<(String, String)>,
    /// Whether this candidate defines `pkg_info()` (EAPI 4+ + `info` in
    /// `DEFINED_PHASES`); only then is the per-package block printed.
    pub defines_pkg_info: bool,
    /// `<repo>/<cat>/<pkg>/<pf>.ebuild`, for the `pkg_info()` phase run.
    pub ebuild_path: PathBuf,
}

/// One `emerge --usepkg --info <atom>` result for a non-installed binary
/// package -- see `resolve_info_binary_candidate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoBinaryCandidate {
    /// `category/package-version` of the chosen binary build.
    pub cpv: String,
    pub repo_name: String,
    /// The binary's recorded `USE`/`IUSE`, `( )`-wrapped for the profile's
    /// forced/masked flags (same shape as `InstalledInfo`).
    pub use_expand_display: Vec<(String, String)>,
    /// `$PKGDIR/<PATH>` -- the archive itself, for extracting its embedded
    /// `<pf>.ebuild` to run `pkg_info()`.
    pub archive_path: PathBuf,
    /// `category`, `package`, `pf` of the chosen build -- the caller lays the
    /// extracted ebuild out as `<scratch>/<category>/<package>/<pf>.ebuild` so
    /// the phase driver derives `CATEGORY`/`PN` from the path.
    pub category: String,
    pub package: String,
    pub pf: String,
}

/// Whether `atom_str`'s `cat/pkg` has *any* local binary build. Unlike
/// `resolve_info_binary_candidate` this does not gate on `pkg_info()`: a
/// binary-only package still isn't a "no ebuilds to satisfy" error.
// Portage: action_info, bindb.match(x.cp)
pub fn has_local_binary_candidate(
    local_binpkg: &std::sync::Arc<BinaryIndex>,
    atom_str: &str,
) -> bool {
    let Some(atom) = portage_dep::parse_atom(atom_str) else {
        return false;
    };
    !list_binary_candidates(local_binpkg, &atom.category, &atom.package).is_empty()
}

/// One `emerge --info <atom>` result for an installed package -- see
/// `resolve_installed_info`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledInfo {
    pub cpv: String,
    pub repo_name: String,
    /// The vdb-recorded `USE="…"` display, same shape as
    /// `InfoCandidate::use_expand_display`.
    pub use_expand_display: Vec<(String, String)>,
    /// `INFO_INSTALLED_VARS` whose vdb value differs from the current
    /// config, as `(name, stored value)` -- rendered `NAME="value"`.
    pub differing_vars: Vec<(String, String)>,
    /// `INFO_INSTALLED_VARS` with no vdb value -- the `Unset: …` line.
    pub unset_vars: Vec<String>,
    /// Whether `pkg_info()` is attempted for this match: fires when `DEFINED_PHASES` names
    /// `info`, **and also** when it is entirely empty (falsy-check quirk). `"-"` does *not* fire.
    // Portage: action_info, actions.py
    pub defines_pkg_info: bool,
    /// `category`, `package`, `version` of this vdb entry -- the `pkg_info()`
    /// phase runs from the vdb-saved `environment.bz2` + `<pf>.ebuild`, the
    /// same path `--config` uses.
    pub category: String,
    pub package: String,
    pub version: String,
}
