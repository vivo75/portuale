//! VDB metadata written after src_install.

use crate::ebuild_phases::*;

/// Writes the USE-evaluated `*DEPEND`/`LICENSE`/`PROPERTIES`/`RESTRICT`/`IUSE`
/// (+ `IUSE_EFFECTIVE`) metadata into `${PORTAGE_BUILDDIR}/build-info`, right after
/// a successful `src_install`. Without it the vdb entry and later binpkgs carry no
/// dependency metadata. Source is the md5-cache entry; conditionals reduce against
/// the resolved `USE` (empty for standalone runs). `*DEPEND` atoms also bind `:=`.
/// Portage: doebuild.py, _post_src_install_write_metadata, _slot_operator.py
/// Binds one `:=` atom to `<slot>/<sub-slot>=` of the highest installed version
/// satisfying it. Non-atoms, non-`:=` atoms, and unresolvable `:=` pass through.
/// String surgery on the slot-dep substring (`replacen` once: no version carries
/// `:`, and neither `::repo` nor `[usedep]` can collide).
/// Portage: _slot_operator.py, _eval_deps
pub(crate) fn bind_slot_operator(token: &str, root: &Path) -> String {
    let Some(atom) = portage_dep::parse_atom(token) else {
        return token.to_string();
    };
    if atom.slot_operator != Some(portage_dep::SlotOperator::Equals) {
        return token.to_string();
    }
    let best = portage_repo::installed_candidates(root, &atom.category, &atom.package)
        .into_iter()
        .filter(|(version, slot, sub_slot)| {
            let cpv_slot = format!(
                "{}/{}-{version}:{slot}/{sub_slot}",
                atom.category, atom.package
            );
            portage_dep::match_from_list(token, &[cpv_slot.as_str()]).is_some_and(|m| !m.is_empty())
        })
        .max_by(|(a, _, _), (b, _, _)| {
            portage_versions::vercmp(a, b)
                .map(|c| c.cmp(&0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    let Some((_, slot, sub_slot)) = best else {
        return token.to_string();
    };
    let old_slotdep = match (&atom.slot, &atom.sub_slot) {
        (None, _) => ":=".to_string(),
        (Some(s), None) => format!(":{s}="),
        (Some(s), Some(ss)) => format!(":{s}/{ss}="),
    };
    token.replacen(&old_slotdep, &format!(":{slot}/{sub_slot}="), 1)
}

/// The resolved `USE` for this package, from the `build_env` slice (empty for
/// standalone runs).
pub(crate) fn build_phase_use(build_env: &[(String, String)]) -> std::collections::HashSet<String> {
    build_env
        .iter()
        .find(|(k, _)| k == "USE")
        .map(|(_, v)| v.split_whitespace().map(String::from).collect())
        .unwrap_or_default()
}

/// Appends an implicit `>=<provider>` dep on the installed libc to `RDEPEND`
/// (lowest installed provider version; skipped when this package is itself one).
/// Portage: doebuild.py, _inject_libc_dep
pub(crate) fn inject_libc_dep(env: &Environment, root: &Path) -> Result<(), String> {
    let current_cp = (env.category.clone(), env.split.pn.clone());
    let mut providers: Vec<(String, String)> =
        portage_repo::libc_provider_cps(root).into_iter().collect();
    providers.sort();
    let mut injected: Vec<String> = Vec::new();
    for (category, package) in providers {
        if (category.clone(), package.clone()) == current_cp {
            return Ok(());
        }
        let lowest = portage_repo::installed_candidates(root, &category, &package)
            .into_iter()
            .min_by(|a, b| portage_versions::vercmp(&a.0, &b.0).unwrap_or(0).cmp(&0));
        if let Some((version, _slot, _sub_slot)) = lowest {
            injected.push(format!(">={category}/{package}-{version}"));
        }
    }
    if injected.is_empty() {
        return Ok(());
    }
    let rdepend_path = env.build_info().join("RDEPEND");
    let existing = std::fs::read_to_string(&rdepend_path)
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let value = if existing.is_empty() {
        injected.join(" ")
    } else {
        format!("{existing} {}", injected.join(" "))
    };
    std::fs::write(&rdepend_path, format!("{value}\n"))
        .map_err(|e| format!("{}: {e}", rdepend_path.display()))?;
    Ok(())
}

/// Sums regular files under `dir` by size, one inode once; dirs/symlinks
/// contribute nothing, unreadable entries error.
/// Portage: doebuild.py, _post_src_install_uid_fix
pub(crate) fn dir_size_bytes(dir: &Path) -> Result<u64, String> {
    use std::os::unix::fs::MetadataExt;
    fn walk(dir: &Path, seen: &mut std::collections::HashSet<u64>) -> Result<u64, String> {
        let mut total = 0;
        let entries =
            portage_util::read_dir_entries(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for entry in entries {
            let path = entry.path();
            let md =
                std::fs::symlink_metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            if md.is_dir() {
                total += walk(&path, seen)?;
            } else if md.is_file() && seen.insert(md.ino()) {
                total += md.len();
            }
        }
        Ok(total)
    }
    walk(dir, &mut std::collections::HashSet::new())
}
