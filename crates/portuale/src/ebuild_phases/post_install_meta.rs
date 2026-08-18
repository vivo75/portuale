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

pub(crate) fn write_post_install_metadata(
    env: &Environment,
    root: &Path,
    build_env: &[(String, String)],
) -> Result<(), String> {
    let build_info = env.build_info();

    // `BUILD_TIME` first, unconditionally: the vdb copy and consolidated metadata
    // both carry it, and omitting it skews every entry against Portage's.
    let build_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("system clock before epoch: {e}"))?
        .as_secs();
    std::fs::write(build_info.join("BUILD_TIME"), format!("{build_time}\n"))
        .map_err(|e| format!("{}: {e}", build_info.join("BUILD_TIME").display()))?;

    let use_flags = build_phase_use(build_env);
    let iuse_effective = build_env
        .iter()
        .find(|(k, _)| k == "IUSE_EFFECTIVE")
        .map(|(_, v)| v.trim())
        .unwrap_or("");
    let Some(repo_root) = repo_root_for(&env.pkg_dir) else {
        return Ok(());
    };
    let Ok(metadata) = portage_repo::repo_aux_metadata(&repo_root, &env.category, &env.split.pf)
    else {
        return Ok(());
    };

    // Portage's conditional-key set: dep keys plus LICENSE/PROPERTIES/RESTRICT.
    for key in [
        "DEPEND",
        "RDEPEND",
        "BDEPEND",
        "PDEPEND",
        "IDEPEND",
        "LICENSE",
        "PROPERTIES",
        "RESTRICT",
    ] {
        let Some(raw) = metadata
            .get(key)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        else {
            // Empty: skip (nothing to unlink; bash never wrote these keys).
            continue;
        };
        let tokens: Vec<String> = raw.split_whitespace().map(String::from).collect();
        let reduced = portage_use_reduce::use_reduce_structured(
            &tokens,
            &use_flags,
            portage_use_reduce::MatchMode::Normal,
        )
        .map_err(|e| format!("{}: build-info/{key}: {e}", env.pkg_dir.display()))?;
        // Dependency atoms evaluate their own USE deps against the package USE
        // (non-atoms pass through); `*DEPEND` then binds `:=` to the installed slot.
        let value = if key.ends_with("DEPEND") {
            reduced
                .into_iter()
                .map(|tok| {
                    let tok =
                        portage_dep::evaluate_atom_conditionals(&tok, &use_flags).unwrap_or(tok);
                    bind_slot_operator(&tok, root)
                })
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            reduced.join(" ")
        };
        if value.is_empty() {
            continue;
        }
        std::fs::write(build_info.join(key), format!("{value}\n"))
            .map_err(|e| format!("{}: {e}", build_info.join(key).display()))?;
    }

    // `IUSE` verbatim (always written, even empty); `IUSE_EFFECTIVE` when the
    // config supplied one (absent for standalone runs, as in Portage).
    let iuse = metadata.get("IUSE").map(|s| s.trim()).unwrap_or("");
    std::fs::write(build_info.join("IUSE"), format!("{iuse}\n"))
        .map_err(|e| format!("{}: {e}", build_info.join("IUSE").display()))?;
    if !iuse_effective.is_empty() {
        std::fs::write(
            build_info.join("IUSE_EFFECTIVE"),
            format!("{iuse_effective}\n"),
        )
        .map_err(|e| format!("{}: {e}", build_info.join("IUSE_EFFECTIVE").display()))?;
    }

    // `SIZE`: regular files under `${D}`, one inode once. Both the archive
    // metadata and the vdb copy read it.
    let size = dir_size_bytes(&env.d())?;
    std::fs::write(build_info.join("SIZE"), format!("{size}\n"))
        .map_err(|e| format!("{}: {e}", build_info.join("SIZE").display()))?;
    Ok(())
}

/// Rewrites `build-info/NEEDED.ELF.2` with the trailing multilib category, generates
/// `REQUIRES`/`PROVIDES` (each only when non-empty), reports missing soname symlinks,
/// and appends the implicit libc dep. Packages with no ELF return early with neither.
/// Portage: doebuild.py, _post_src_install_soname_symlinks
pub(crate) fn write_post_install_soname_deps(env: &Environment, root: &Path) -> Result<(), String> {
    let build_info = env.build_info();
    let needed_path = build_info.join("NEEDED.ELF.2");
    let Ok(text) = std::fs::read_to_string(&needed_path) else {
        return Ok(());
    };

    let mut rewritten = String::new();
    let mut recognized: Vec<crate::needed_elf::NeededEntry> = Vec::new();
    let mut missing_symlinks: Vec<(String, String)> = Vec::new();
    let libpaths =
        crate::needed_elf::getlibpaths(root, std::env::var("LD_LIBRARY_PATH").ok().as_deref());
    for mut entry in crate::needed_elf::NeededEntry::parse_file(&text) {
        let obj_path = env.d().join(entry.filename.trim_start_matches('/'));
        if let Some(cat) = crate::needed_elf::compute_multilib_category(&obj_path) {
            entry.multilib_category = Some(cat);
            recognized.push(entry.clone());
        }
        // Report-only QA, for soname entries in an actual libdir; never created here.
        if !entry.soname.is_empty() {
            let obj_dir = obj_path.parent();
            let parent_rel = std::path::Path::new(&entry.filename)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let in_libdir = libpaths
                .iter()
                .any(|l| l.trim_end_matches('/') == parent_rel.trim_end_matches('/'));
            if in_libdir
                && let Some(dir) = obj_dir
                && !dir.join(&entry.soname).exists()
            {
                missing_symlinks.push((entry.filename.clone(), entry.soname.clone()));
            }
        }
        rewritten.push_str(&entry.to_needed_line());
    }
    std::fs::write(&needed_path, rewritten)
        .map_err(|e| format!("{}: {e}", needed_path.display()))?;

    let read_trim = |name: &str| {
        std::fs::read_to_string(build_info.join(name))
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };
    let provides_exclude = read_trim("PROVIDES_EXCLUDE");
    let requires_exclude = read_trim("REQUIRES_EXCLUDE");
    let (provides, requires) =
        crate::needed_elf::generate_soname_deps(&recognized, &provides_exclude, &requires_exclude);
    if let Some(requires) = requires {
        let path = build_info.join("REQUIRES");
        std::fs::write(&path, requires).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    if let Some(provides) = provides {
        let path = build_info.join("PROVIDES");
        std::fs::write(&path, provides).map_err(|e| format!("{}: {e}", path.display()))?;
    }

    if !missing_symlinks.is_empty() {
        eprintln!("QA Notice: Missing soname symlink(s):");
        eprintln!();
        for (obj, soname) in &missing_symlinks {
            let dir = std::path::Path::new(obj)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            eprintln!(
                "\t{} -> {}",
                std::path::Path::new(&dir).join(soname).display(),
                std::path::Path::new(obj)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            );
        }
        eprintln!();
    }

    inject_libc_dep(env, root)
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
