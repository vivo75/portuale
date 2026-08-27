//! `--unmerge`: listing and removing installed packages.

use crate::pretend::*;

/// Whether a `FEATURES` token is on in the raw process env. This is the
/// fallback for the CLI boundaries with no resolved config; every
/// build/merge path reads the resolved list instead.
pub(crate) fn feature_enabled(token: &str) -> bool {
    std::env::var("FEATURES")
        .map(|f| f.split_whitespace().any(|t| t == token))
        .unwrap_or(false)
}

/// Drop every world atom for the removed package unless some installed
/// version still satisfies it; all other atoms are kept untouched. The file
/// is rewritten sorted, with comment and `@set` lines not carried forward.
/// Portage: _sets/files.py, WorldSelectedPackagesSet.cleanPackage
pub(crate) fn deselect_from_world(
    root: &Path,
    category: &str,
    package: &str,
) -> Result<(), String> {
    let mykey = format!("{category}/{package}");
    let mut current = read_world_atoms(root)?;
    let before = current.len();
    current.retain(|raw| {
        let Some(atom) = parse_atom(raw) else {
            return true;
        };
        if format!("{}/{}", atom.category, atom.package) != mykey {
            return true;
        }
        portage_repo::installed_candidates(root, &atom.category, &atom.package)
            .into_iter()
            .any(|(v, slot, sub_slot)| {
                let cs = format!("{}/{}-{v}:{slot}/{sub_slot}", atom.category, atom.package);
                match_from_list(raw, &[cs.as_str()]).is_some_and(|m| !m.is_empty())
            })
    });
    if current.len() == before {
        return Ok(());
    }
    current.sort();
    current.dedup();
    let db = portage_vdb::for_root(root);
    let mut txn = db.begin_write().map_err(|e| e.to_string())?;
    txn.set_world(&portage_vdb::World { atoms: current })
        .map_err(|e| e.to_string())?;
    txn.commit().map_err(|e| e.to_string())?;
    Ok(())
}

/// One `selected:` / `protected:` / `omitted:` row of the per-package block:
/// the label right-justified into 14 columns, then each version followed by
/// a trailing space, or the literal `none ` when empty, trailing spaces
/// included. `selected` versions render red, `protected`/`omitted` green.
/// Portage: unmerge.py, _unmerge_display
pub(crate) fn print_unmerge_row(label: &str, versions: &[String], color: &Colorizer) {
    let head = format!("{label}: ");
    let padded = format!("{head:>14}");
    if versions.is_empty() {
        println!("{padded}none ");
    } else {
        let key = if label == "selected" {
            "UNMERGE_WARN"
        } else {
            "GOOD"
        };
        let mut line = padded;
        for v in versions {
            line.push_str(&color.c(key, &format!("{v} ")));
        }
        println!("{line}");
    }
}

/// Every installed `(category, package, version, slot)` in the vdb: used only
/// for the bare-name ("null category") resolution.
/// Portage: vartree.py, dbapi.cpv_all
pub(crate) fn installed_cp_versions(root: &Path) -> Vec<(String, String, String, String)> {
    let mut out = Vec::new();
    // `-MERGING-<pf>` entries are never a resolution candidate, so skipped.
    let Ok(entries) = portage_vdb::for_root(root).entries() else {
        return out;
    };
    for key in entries {
        let (category, dirname) = (key.category, key.pf);
        if let Some((name, version)) = split_pf(&dirname) {
            // Through the vdb seam; the main slot only (an absent `SLOT`
            // reads as `""`, like the old default).
            let slot = portage_repo::vdb_entry_slot(root, &category, &dirname)
                .split('/')
                .next()
                .unwrap_or("0")
                .to_string();
            out.push((category, name, version, slot));
        }
    }
    out
}
