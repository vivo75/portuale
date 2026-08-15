//! Ebuild candidates for a package.

use crate::*;

/// Which kind of package this `Candidate` is -- Portage's `pkg.type_name`.
/// An ebuild's USE is computed dynamically; a binary's was baked at build
/// time (`binary_use`). `--pretend` prints `[ebuild` vs. `[binary`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateSource {
    Ebuild,
    Binary,
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub version: String,
    pub keywords: Vec<String>,
    pub slot: String,
    /// The sub-slot half of `SLOT="main/sub"` (defaults to `slot`); folded into
    /// candidate strings so sub-slot atoms can match. Portage: PMS 8.3.3
    pub sub_slot: String,
    /// Which repo the ebuild/metadata lives in: re-reads DEPEND later and
    // breaks same-version cross-repo ties toward higher priority.
    pub repo_location: PathBuf,
    pub repo_priority: i32,
    /// The repo's `repos.conf` section name, appended as `::name` so `::repo`
    /// atoms filter (Portage also cross-checks `profiles/repo_name`).
    pub repo_name: String,
    /// Raw `LICENSE` (PMS 7.3.2), unreduced; empty when absent. Read with the
    /// same md5-cache dict as keywords/slot at zero extra I/O.
    pub license: String,
    /// Raw `IUSE`: consulted only when `license` holds a `?` conditional
    /// (Portage `use_reduce`'s `if '?' in license_str` optimization).
    pub iuse: String,
    /// Raw `PROPERTIES` / `RESTRICT`, same zero-extra-I/O reasoning.
    pub properties: String,
    pub restrict: String,
    /// Ebuild (md5-cache) or binary (`Packages` index entry).
    pub source: CandidateSource,
    /// The baked build-time USE (`Packages` `USE` field; presence = enabled).
    /// `None` for ebuilds, whose USE is computed (`effective_use_flags`).
    pub binary_use: Option<HashSet<String>>,
    /// From a remote binhost index, not yet in the local `$PKGDIR`
    /// (Portage `bintree.isremote`); flows to the `g` bracket column.
    pub remote: bool,
    /// The `BUILD_ID` / `BUILD_TIME` fields (`None` for ebuilds): display
    /// suffix and newest-instance ordering.
    pub build_id: Option<String>,
    pub build_time: Option<i64>,
    /// This binary instance's own recorded `*DEPEND` strings, so
    /// `--binpkg-changed-deps` compares the exact instance. Empty for ebuilds.
    pub binary_deps: HashMap<String, String>,
}

/// Accepts `<package>-<version>` only when the rest starts with a digit, so
/// "foo" never absorbs a sibling "foo-bar-2.0" directory.
pub(crate) fn strip_version_prefix<'a>(dir_name: &'a str, package: &str) -> Option<&'a str> {
    let rest = dir_name.strip_prefix(package)?.strip_prefix('-')?;
    if rest.starts_with(|c: char| c.is_ascii_digit()) {
        Some(rest)
    } else {
        None
    }
}

/// Splits `package-version` into `(PN, PVR)` like Portage `_pkgsplit`: longest
/// trailing `-`-suffix that `ververify` accepts. Inverse of
/// [`strip_version_prefix`]; the single copy.
pub fn split_pf(pf: &str) -> Option<(String, String)> {
    let words: Vec<&str> = pf.split('-').collect();
    for i in 1..words.len() {
        let candidate = words[i..].join("-");
        // A `-r<digits>` revision belongs to the version -- but only when the
        // rest still verifies (Portage `_pkgsplit` has the same shape).
        if portage_versions::ververify(&candidate) {
            return Some((words[..i].join("-"), candidate));
        }
    }
    None
}
