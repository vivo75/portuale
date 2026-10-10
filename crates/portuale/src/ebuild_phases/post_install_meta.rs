//! VDB metadata written after src_install.

use crate::ebuild_phases::*;

/// Real `doebuild.py::_post_src_install_write_metadata`
/// (`lib/portage/package/ebuild/doebuild.py:2700-2782`), run right after
/// a successful `src_install` (real portage calls it from
/// `doebuild(mydo="install")`): write the USE-conditional-evaluated
/// dependency / `LICENSE` / `PROPERTIES` / `RESTRICT` / `IUSE`
/// (+ `IUSE_EFFECTIVE`) metadata files into `${PORTAGE_BUILDDIR}/build-
/// info`. `bin/phase-functions.sh __dyn_install`'s own build-info loop
/// (run unmodified, before this) writes `CATEGORY`/`SLOT`/`KEYWORDS`/
/// `IUSE`/`USE`/`EAPI`/`DEFINED_PHASES`/… but *not* these keys -- real
/// portage's Python side fills them in. Without this the merged vdb
/// entry and the `xpak`/`gpkg` a `$PKGDIR` scan later reads carry no
/// dependency metadata at all (found via the `binpkg.rs` scan buildout).
///
/// Source is the ebuild's own `metadata/md5-cache` entry (the same
/// already-trusted source `fetch_sources` reads `SRC_URI` from) --
/// `settings.configdict["pkg"]` in real portage. USE-conditionals are
/// evaluated against `use_flags` -- the resolved `USE` for this package
/// (`build_phase_use` pulls it out of the `emerge <atom>` build path's
/// own `build_env`; empty for a standalone `ebuild <file>` run, which
/// resolves no graph) -- via `use_reduce_structured` (real
/// `paren_enclose(use_reduce(v, uselist=use))`, the bracket/`||`-
/// preserving normalized token stream).
///
/// Real `_slot_operator._eval_deps`'s own per-atom step: an atom with a
/// `:=` slot operator (`slot_operator == "="`) is rewritten to
/// `:<slot>/<sub-slot>=` taken from the highest installed version in
/// `<root>/var/db/pkg` that satisfies it (`vardb.match(x)[-1]`). A
/// non-atom token, a non-`:=` atom, or a `:=` dep with nothing installed
/// is returned unchanged (real "just leave it as-is for now ... keeping
/// the information in vdb").
///
/// The rewrite is string surgery on the atom's own slot-dep substring
/// (`:=` / `:2=` / `:2/3=`, all reconstructable from the parsed
/// `slot`/`sub_slot`) rather than reserialising the whole atom -- that
/// substring is distinctive enough to appear exactly once in a
/// well-formed atom, so `replacen(.., 1)` is safe (a version can't
/// contain `:`, `::repo` carries no `=`, a `[usedep]` carries no `:`).
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

/// Real portage, for an EAPI with slot operators (every EAPI 5+), skips
/// the `*DEPEND` keys in this loop and writes them from
/// `evaluate_slot_operator_equal_deps` (`portage/dep/_slot_operator.py`)
/// instead: every `:=` slot-operator atom is bound to the actual
/// `<slot>/<sub-slot>=` of the highest installed version that satisfies
/// it (`vardb.match(x)[-1]`), leaving an unresolvable one bare. This
/// portuale now does the same, `bind_slot_operator` per `*DEPEND` token
/// (`_eval_deps`'s own per-atom loop) -- so a package portuale merges
/// records `dev-libs/foo:2/3=` in its vdb/binpkg build-info, the data a
/// later sub-slot rebuild check needs. An ebuild with no `:=` operator,
/// or one whose `:=` dep isn't installed, is byte-identical to before.
///
/// v1 cut still: real `_eval_deps` walks `RDEPEND`/`PDEPEND` against the
/// target `ROOT` vdb and `DEPEND`/`BDEPEND` against the target/running
/// vdb respectively -- portuale's own single-root world binds every
/// `*DEPEND` key against the one `<root>/var/db/pkg` (same simplification
/// `--root-deps` documents); and the real `|| ( A:= B:= )` "record
/// sub-slot on A only" TODO (bug #455904) is moot without disjunctive
/// `:=` handling anywhere.
/// The `USE=` value from a `run_commands_async` `build_env` slice as a
/// flag set -- what the `emerge <atom>` build path resolved for this
/// package. Empty for a standalone `ebuild <file>` run (no `build_env`).
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

    // Real `_post_src_install_write_metadata` (`doebuild.py:2727-2732`)
    // writes `int(time.time())` into `build-info/BUILD_TIME` before any
    // metadata key, unconditionally (backlog #47). The vdb entry copies
    // it (`populate_vdb_tmp`) and `_consolidate_to_metadata_file`
    // folds it into the consolidated `metadata` body -- real's own
    // reader (`versions.py:413`) and binpkg multi-instance logic compare
    // against it, and omitting it made every portuale vdb entry differ
    // from real's by the `BUILD_TIME` file *and* the `metadata` field.
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

    // real `_vdb_use_conditional_keys` = `Package._dep_keys` + LICENSE /
    // PROPERTIES / RESTRICT.
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
            // real portage unlinks a stale build-info/<k> when the value
            // is empty; `bin/phase-functions.sh` never wrote these, so
            // there is nothing to unlink here.
            continue;
        };
        let tokens: Vec<String> = raw.split_whitespace().map(String::from).collect();
        let reduced = portage_use_reduce::use_reduce_structured(
            &tokens,
            &use_flags,
            portage_use_reduce::MatchMode::Normal,
        )
        .map_err(|e| format!("{}: build-info/{key}: {e}", env.pkg_dir.display()))?;
        // Real `_post_src_install_write_metadata` with `token_class=Atom`
        // (`doebuild.py:2749`): every dependency-atom token's own USE
        // deps are evaluated against this package's effective USE before
        // the result is stored -- `Atom.evaluate_conditionals`,
        // `lib/portage/dep/__init__.py:1387`; an unknown/disabled
        // `flag?` use-dep is *dropped* (bug #386829's canonicalisation),
        // while a `flag?` group conditional was already handled by
        // `use_reduce_structured` above. `evaluate_atom_conditionals` is
        // the port; a token that isn't an atom (a `||` marker, a bare
        // paren) passes through unchanged.
        //
        // Then the `*DEPEND` keys go through real
        // `evaluate_slot_operator_equal_deps`: bind every `:=` atom to
        // the installed dependency's `<slot>/<sub-slot>=`.
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

    // Real `_post_src_install_write_metadata` (`doebuild.py:2720-2744`):
    // `IUSE` is written verbatim from the package config ("in case it's
    // corrupted due to local environment settings", bug #386829) and is
    // *always* present -- a no-IUSE ebuild gets an empty
    // `metadata/IUSE`, which is exactly what real archives carry.
    // `IUSE_EFFECTIVE` (EAPI 5+) is the profile's own
    // `_calc_iuse_effective` result, threaded here as an ordinary phase
    // env key by #37 S1 (`portage_profile::phase_environ`); absent for a
    // standalone `ebuild <file>` run with no resolved config, in which
    // case real's `configdict` has no such key either.
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

    // Real `_post_src_install_uid_fix` (`doebuild.py:2997-3004`): the
    // package's own installed size, summed over `${D}` -- regular files
    // only, and hardlinked inodes counted exactly once (the same
    // `counted_inodes` dedup real uses). Written into `build-info/SIZE`,
    // which both the archive metadata (`metadata/SIZE`, #39) and the vdb
    // copy.
    let size = dir_size_bytes(&env.d())?;
    std::fs::write(build_info.join("SIZE"), format!("{size}\n"))
        .map_err(|e| format!("{}: {e}", build_info.join("SIZE").display()))?;
    Ok(())
}

/// Real `_post_src_install_soname_symlinks` (`doebuild.py:3069-3300`),
/// run where real runs it: after the `install_qa_check
/// install_symlink_html_docs install_hooks` misc-function sequence, in
/// `_emerge/EbuildPhase._commands_exit`. Four things, all #39:
///
/// 1. Rewrite `build-info/NEEDED.ELF.2` with the 6th, trailing multilib
///    category field -- real reads each object's own ELF header
///    (`ELFHeader.read`) and writes `NeededEntry.__str__`'s
///    `arch;obj;soname;rpaths;needed;category` line back. A line whose
///    object no longer reads as ELF keeps an empty category (real's
///    `multilib_category = None`).
/// 2. Generate `build-info/REQUIRES`/`PROVIDES` from the recognized
///    entries (`SonameDepsProcessor`, `PROVIDES_EXCLUDE`/
///    `REQUIRES_EXCLUDE` aware); real writes each file only when the
///    corresponding map is non-empty.
/// 3. Report a `QA Notice: Missing soname symlink(s):` block when a
///    library's own soname symlink is missing (real only *reports*;
///    it never creates the link here).
/// 4. Append `_inject_libc_dep`'s implicit `>=<installed libc>` to
///    `build-info/RDEPEND` (bug #753500). Real reaches this function
///    only when `NEEDED.ELF.2` exists, so a package with no ELF gets
///    neither `PROVIDES`/`REQUIRES` nor the libc dep -- matched by the
///    early return below.
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
        // Real's own symlink QA: only for an entry with an soname in a
        // real libdir, and never created -- reported only.
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

/// Real `_inject_libc_dep` (`doebuild.py:3026-3067`, bug #753500): every
/// ELF-bearing package records an implicit `>=<provider>` runtime dep on
/// the installed libc, so a binpkg merge cannot silently downgrade the
/// libc it was built against. `find_libc_deps(..., realized=True)`:
/// expand `virtual/libc`'s installed `RDEPEND` to its provider cps
/// (`portage_repo::libc_provider_cps`), take each provider's lowest
/// installed version (real `portdb.match(atom)[0]`), skip entirely when
/// this package *is* a libc provider (real's own `pkgcmp` self-check),
/// then append the `>=` atoms to the existing `build-info/RDEPEND`.
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

/// Real `_post_src_install_uid_fix`'s own size accumulation: every
/// regular file under `dir`, once per inode (`counted_inodes`), summed
/// by `st_size`. Directory and symlink entries contribute nothing;
/// unreadable entries are an error (real would raise the same way).
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
