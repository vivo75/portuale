//! Checking the binary plan and running the binpkg flow.

use crate::remote::*;

// --- Vdb shadow + pre-ship collision pre-check ---------------------------------
///
/// The server reasons about installed-file ownership *before* shipping a
/// unit's tarball: a unit doomed by a foreign-owned collision fails here,
/// with zero client writes. The shadow is the placed vdb read directly
/// (`server:`) or pulled once at plan start (`client:`).
///
/// Deliberately lenient: only ownership by a *different* package fails
/// the pre-check. Same-package ownership (any version/slot) passes --
/// the client driver refines those against the live vdb, or fail-closed
/// without one.
///
// Installed-file ownership: absolute path -> owning `(category, pf)`.
pub(crate) struct VdbShadow {
    pub(crate) owners: std::collections::HashMap<String, (String, String)>,
}

/// `obj`/`sym` paths out of one vdb `CONTENTS` file (Portage
/// `dblink`/`vartree` shape: `obj <path> <md5> <mtime>`,
/// `sym <path> -> <target> <mtime>`). `dir` lines merge freely
/// client-side, so they never gate a pre-check.
pub(crate) fn contents_owned_paths(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            match (words.next(), words.next()) {
                (Some("obj"), Some(path)) | (Some("sym"), Some(path)) => Some(path.to_string()),
                _ => None,
            }
        })
        .collect()
}

impl VdbShadow {
    /// Read `<dir>/<cat>/<pf>/CONTENTS` (two-level walk). Missing/unreadable
    /// files are skipped, not fatal: an empty shadow pre-checks nothing,
    /// and the client driver still gates every collision.
    pub(crate) fn load(dir: &std::path::Path) -> Self {
        Self::from_db(&portage_vdb::FilesDb::open_vdb_dir(dir))
    }

    /// The shadow of any installed-package database: entries in listing
    /// order (in-progress `-MERGING-<pf>` entries are never listed), the
    /// first owner of a path wins. An unlistable database or an
    /// unreadable / non-UTF-8 `CONTENTS` contributes nothing.
    pub(crate) fn from_db(db: &dyn portage_vdb::InstalledDb) -> Self {
        let mut owners = std::collections::HashMap::new();
        let Ok(keys) = db.entries() else {
            return Self { owners };
        };
        for key in keys {
            let Ok(Some(bytes)) = db.read_file(&key, "CONTENTS") else {
                continue;
            };
            let Ok(text) = String::from_utf8(bytes) else {
                continue;
            };
            for path in contents_owned_paths(&text) {
                owners
                    .entry(path)
                    .or_insert_with(|| (key.category.clone(), key.pf.clone()));
            }
        }
        Self { owners }
    }

    /// Owner `(category, pf)` of an absolute image path, if recorded.
    pub(crate) fn owner(&self, abspath: &str) -> Option<&(String, String)> {
        self.owners.get(abspath)
    }

    /// True when `pf` belongs to package `pn` (Portage `_pkgsplit`
    /// longest-trailing-version rule via `portage_repo::split_pf`).
    pub(crate) fn same_package(pf: &str, pn: &str) -> bool {
        portage_repo::split_pf(pf).is_some_and(|(name, _)| name == pn)
    }
}

/// `(kind, rel)` image paths out of a staged `filemeta` file (the exact
/// bytes the client will gate on: `<kind> <md5> <mtime> <rel>
/// [<target>]`, whitespace-free by bundle-build construction).
pub(crate) fn parse_filemeta_paths(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let kind = words.next()?.to_string();
            if !matches!(kind.as_str(), "obj" | "sym" | "dir") {
                return None;
            }
            words.next()?;
            words.next()?;
            let rel = words.next()?.to_string();
            Some((kind, rel))
        })
        .collect()
}

/// Pre-ship gate for one unit: every `obj`/`sym` image path owned in the
/// shadow by a *different* package fails with a `not shipping` error
/// (zero client writes so far). Same-package ownership passes for the
/// client driver to refine.
pub(crate) fn shadow_precheck(
    shadow: &VdbShadow,
    cpv: &str,
    category: &str,
    pn: &str,
    staged_filemeta: &str,
) -> Result<(), String> {
    for (kind, rel) in parse_filemeta_paths(staged_filemeta) {
        if kind == "dir" {
            continue;
        }
        let abspath = format!("/{rel}");
        if let Some((cat, pf)) = shadow.owner(&abspath)
            && (cat != category || !VdbShadow::same_package(pf, pn))
        {
            return Err(format!(
                "mrg: not shipping {cpv}: {abspath} owned by {cat}/{pf} (vdb shadow)"
            ));
        }
    }
    Ok(())
}

/// Load the shadow for this run: read directly for `server:` placement,
/// pull once for `client:` placement.
pub(crate) fn load_vdb_shadow(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
) -> Result<VdbShadow, String> {
    match &ctx.vdb {
        ConfigPlacement::Server(path) => Ok(VdbShadow::load(std::path::Path::new(path))),
        ConfigPlacement::Client(path) => {
            let tmp = TempDir::new("portuale-remote-vdb").keep();
            // Like the etc-portage pull, the tmp dir is left for
            // forensics; it carries no secrets beyond file lists.
            pull_dir(ctx, control, path, &tmp)?;
            Ok(VdbShadow::load(&tmp))
        }
    }
}

/// Every merge-bound entry must be `Binary` in a remote plan (the
/// `--getbinpkgonly` resolve guarantees it); anything else is a usage
/// error naming the offenders (exit 2 at the call site).
pub(crate) fn check_binary_plan(entries: &[portage_repo::GraphEntry]) -> Result<(), String> {
    use portage_repo::{CandidateSource, PretendOutcome};
    let mut offenders = Vec::new();
    for entry in entries {
        let merge_bound = !matches!(
            entry.outcome,
            PretendOutcome::AlreadyInstalled { .. } | PretendOutcome::NoVisibleCandidate
        );
        if merge_bound && entry.source != CandidateSource::Binary {
            let version = match &entry.outcome {
                PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
                    version.clone()
                }
                PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => {
                    to.clone()
                }
                _ => String::new(),
            };
            offenders.push(format!("{}/{}-{version}", entry.category, entry.package));
        }
    }
    if offenders.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "remote plan is not binary-only (needs --getbinpkgonly): {}",
            offenders.join(" ")
        ))
    }
}
