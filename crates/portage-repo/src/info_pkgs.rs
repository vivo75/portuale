//! Package rows for `emerge --info`.

use crate::*;

/// The best visible candidate for `atom_str` plus its `-pv`-style USE display. `None`
/// when the atom resolves to no visible ebuild candidate (the caller renders "no matches").
/// Deliberately narrow: ebuild candidates only (no installed `was built with` diff, no
/// binary line) and no `pkg_info()` execution. The USE display matches `emerge -pv` verbatim.
// Portage: action_info, actions.py, pkg_use_display
pub fn resolve_info_candidate(
    repos: &[RepoConfig],
    atom_str: &str,
    config: &portage_profile::Config,
) -> Result<Option<InfoCandidate>, Error> {
    let atom = portage_dep::parse_atom(atom_str).ok_or_else(|| Error::InvalidAtom {
        atom: atom_str.to_string(),
    })?;
    let candidates = list_candidates(repos, &atom.category, &atom.package)?;
    let visible: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| is_visible(c, &atom.category, &atom.package, config))
        .collect();
    if visible.is_empty() {
        return Ok(None);
    }
    let candidate_strs: Vec<String> = visible
        .iter()
        .map(|c| {
            format!(
                "{}/{}-{}:{}/{}::{}",
                atom.category, atom.package, c.version, c.slot, c.sub_slot, c.repo_name
            )
        })
        .collect();
    let refs: Vec<&str> = candidate_strs.iter().map(String::as_str).collect();
    let Some(matched) = portage_dep::match_from_list(atom_str, &refs) else {
        return Ok(None);
    };
    let by_str: HashMap<&str, &Candidate> =
        refs.iter().copied().zip(visible.iter().copied()).collect();
    // USE-dep post-filter, same as `resolve_pretend`.
    let matched: Vec<&&Candidate> = matched
        .iter()
        .filter_map(|m| by_str.get(m))
        .filter(|c| match &atom.use_deps {
            Some(use_deps) if !use_deps.is_empty() => candidate_iuse_and_use(
                c,
                &atom.category,
                &atom.package,
                config,
            )
            .is_some_and(|(iuse, use_flags)| {
                portage_dep::use_deps_satisfied(use_deps, &valid_iuse(&iuse, config), &use_flags)
            }),
            _ => true,
        })
        .collect();
    let Some(best) = matched.into_iter().copied().max_by(|a, b| {
        vercmp_ordering(&a.version, &b.version).then(a.repo_priority.cmp(&b.repo_priority))
    }) else {
        return Ok(None);
    };

    let Some((_iuse, use_flags)) =
        candidate_iuse_and_use(best, &atom.category, &atom.package, config)
    else {
        return Ok(None);
    };
    // Dedup a flag `IUSE` names twice (eclass + ebuild); Portage renders a set.
    let mut disp_seen: HashSet<String> = HashSet::new();
    let mut disp: Vec<(String, bool)> = best
        .iuse
        .split_whitespace()
        .map(|t| t.trim_start_matches(['+', '-']).to_string())
        .filter(|f| disp_seen.insert(f.clone()))
        .map(|f| {
            let on = use_flags.contains(&f);
            (f, on)
        })
        .collect();
    disp.sort_by_key(|p| alnum_sort_key(&p.0));
    let cpv = format!("{}/{}-{}", atom.category, atom.package, best.version);
    let candidate_str = format!(
        "{}/{}-{}:{}/{}::{}",
        atom.category, atom.package, best.version, best.slot, best.sub_slot, best.repo_name
    );
    let forced = forced_or_masked_flags(ForcedOrMaskedFlagsArgs {
        iuse: &best.iuse,
        keywords: &best.keywords,
        candidate_str: &candidate_str,
        category: &atom.category,
        package: &atom.package,
        config,
    });
    // Renders every flag, like `pkg_use_display` for a non-installed package.
    let use_expand_display = build_use_expand_display(BuildUseExpandDisplayArgs {
        use_flags_display: &disp,
        config,
        installed: None,
        forced: &forced,
        all_flags: true,
        reinst_flags: &HashSet::new(),
    });
    // Only listed when its EAPI is 4+ and `DEFINED_PHASES` names `info`, so
    // `pkg_info()` can run.
    let pf = format!("{}-{}", atom.package, best.version);
    let defines_pkg_info = repo_aux_metadata(&best.repo_location, &atom.category, &pf)
        .ok()
        .and_then(|md| md.get("DEFINED_PHASES").cloned())
        .is_some_and(|dp| dp.split_whitespace().any(|p| p == "info"));
    // `<repo>/<cat>/<pkg>/<pf>.ebuild`, for the `pkg_info()` phase run.
    let ebuild_path = best
        .repo_location
        .join(&atom.category)
        .join(&atom.package)
        .join(format!("{pf}.ebuild"));
    Ok(Some(InfoCandidate {
        cpv,
        repo_name: best.repo_name.clone(),
        use_expand_display,
        defines_pkg_info,
        ebuild_path,
    }))
}

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

/// The **binary** half of the search: only with no installed match and no visible ebuild
/// candidate, and only under `--usepkg`/`--usepkgonly`. The highest-version build whose
/// EAPI is 4+ with `info` in `DEFINED_PHASES` wins; `None` when no such build exists.
// Portage: action_info, actions.py
pub fn resolve_info_binary_candidate(
    config: &portage_profile::Config,
    atom_str: &str,
    local_binpkg: &std::sync::Arc<BinaryIndex>,
) -> Result<Option<InfoBinaryCandidate>, Error> {
    let atom = portage_dep::parse_atom(atom_str).ok_or_else(|| Error::InvalidAtom {
        atom: atom_str.to_string(),
    })?;
    let index = local_binpkg;
    let candidates = list_binary_candidates(index, &atom.category, &atom.package);
    if candidates.is_empty() {
        return Ok(None);
    }
    let candidate_strs: Vec<String> = candidates
        .iter()
        .map(|c| {
            format!(
                "{}/{}-{}:{}/{}::{}",
                atom.category, atom.package, c.version, c.slot, c.sub_slot, c.repo_name
            )
        })
        .collect();
    let refs: Vec<&str> = candidate_strs.iter().map(String::as_str).collect();
    let Some(matched) = portage_dep::match_from_list(atom_str, &refs) else {
        return Ok(None);
    };
    let by_str: HashMap<&str, &Candidate> = refs.iter().copied().zip(candidates.iter()).collect();
    // Highest version first, then the first defining `pkg_info()`.
    let mut ranked: Vec<&Candidate> = matched
        .iter()
        .filter_map(|m| by_str.get(m).copied())
        .collect();
    ranked.sort_by(|a, b| vercmp_ordering(&b.version, &a.version));
    for best in ranked {
        let Some(metadata) = read_binary_metadata_instance(
            index,
            &atom.category,
            &atom.package,
            &best.version,
            best.build_id.as_deref(),
        ) else {
            continue;
        };
        let eapi = metadata.get("EAPI").map(String::as_str).unwrap_or("0");
        let defines_info = metadata
            .get("DEFINED_PHASES")
            .is_some_and(|dp| dp.split_whitespace().any(|p| p == "info"));
        if matches!(eapi, "0" | "1" | "2" | "3") || !defines_info {
            continue;
        }
        let Some(rel_path) = metadata.get("PATH").filter(|p| !p.is_empty()) else {
            continue;
        };
        let Some((_iuse, use_flags)) =
            candidate_iuse_and_use(best, &atom.category, &atom.package, config)
        else {
            continue;
        };
        let mut disp_seen: HashSet<String> = HashSet::new();
        let mut disp: Vec<(String, bool)> = best
            .iuse
            .split_whitespace()
            .map(|t| t.trim_start_matches(['+', '-']).to_string())
            .filter(|f| disp_seen.insert(f.clone()))
            .map(|f| {
                let on = use_flags.contains(&f);
                (f, on)
            })
            .collect();
        disp.sort_by_key(|p| alnum_sort_key(&p.0));
        let cpv = format!("{}/{}-{}", atom.category, atom.package, best.version);
        let candidate_str = format!(
            "{}/{}-{}:{}/{}::{}",
            atom.category, atom.package, best.version, best.slot, best.sub_slot, best.repo_name
        );
        let forced = forced_or_masked_flags(ForcedOrMaskedFlagsArgs {
            iuse: &best.iuse,
            keywords: &best.keywords,
            candidate_str: &candidate_str,
            category: &atom.category,
            package: &atom.package,
            config,
        });
        let use_expand_display = build_use_expand_display(BuildUseExpandDisplayArgs {
            use_flags_display: &disp,
            config,
            installed: None,
            forced: &forced,
            all_flags: true,
            reinst_flags: &HashSet::new(),
        });
        return Ok(Some(InfoBinaryCandidate {
            cpv,
            repo_name: best.repo_name.clone(),
            use_expand_display,
            archive_path: Path::new(&config.pkgdir).join(rel_path),
            category: atom.category.clone(),
            package: atom.package.clone(),
            pf: format!("{}-{}", atom.package, best.version),
        }));
    }
    Ok(None)
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
