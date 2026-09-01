//! Package rows for `emerge --info`.

use crate::*;

/// One row of `emerge --info`'s `info_pkgs` version table: a resolved `cat/pkg` and its
/// installed versions, each `<ver>::<repo>` (` (virtual/foo)` suffix when virtual-expanded).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoPkgRow {
    pub cp: String,
    pub versions: Vec<String>,
}

/// One level of `expand_new_virt`: a non-`virtual/*` yields itself; a `virtual/*` yields the
/// installed `(cat, pkg)` of every atom in its highest installed version's vdb `RDEPEND`,
/// tagged with the originating `virtual/foo`. Uninstalled/empty yields itself; no recursion.
// Portage: portage.dbapi._expand_new_virt
pub(crate) fn expand_new_virt(
    root: &Path,
    category: &str,
    package: &str,
) -> Vec<(String, String, Option<String>)> {
    if category != "virtual" {
        return vec![(category.to_string(), package.to_string(), None)];
    }
    let orig = format!("{category}/{package}");
    let Some(version) = installed_candidates(root, category, package)
        .into_iter()
        .map(|(v, _, _)| v)
        .max_by(|a, b| vercmp_ordering(a, b))
    else {
        return vec![(category.to_string(), package.to_string(), None)];
    };
    let use_flags = read_vdb_flag_set(root, category, package, &version, "USE");
    let rdepend = read_vdb_string(root, category, package, &version, "RDEPEND");
    let mut out: Vec<(String, String, Option<String>)> = Vec::new();
    if let Some(atoms) = flat_dep_atoms(&rdepend, &use_flags) {
        let mut sorted: Vec<String> = atoms.into_iter().collect();
        sorted.sort();
        for atom_str in sorted {
            if let Some(atom) = portage_dep::parse_atom(&atom_str)
                && atom.category != "virtual"
                && !installed_candidates(root, &atom.category, &atom.package).is_empty()
            {
                out.push((atom.category, atom.package, Some(orig.clone())));
            }
        }
    }
    if out.is_empty() {
        vec![(category.to_string(), package.to_string(), None)]
    } else {
        out
    }
}

/// The `info_pkgs` version table `emerge --info` prints: the six hardcoded atoms
/// plus `<main_repo>/profiles/info_pkgs`, each `expand_new_virt`'d, then one
/// `cat/pkg`-sorted row per resolved `cat/pkg` with an installed version --
/// versions `vercmp`-sorted, `<ver>::<repo>`. The caller pads `cp` to the widest.
// Portage: action_info, actions.py
pub fn info_pkgs_table(root: &Path, main_repo_location: &Path) -> Vec<InfoPkgRow> {
    let mut atoms: Vec<String> = [
        "dev-build/autoconf",
        "dev-build/automake",
        "virtual/os-headers",
        "sys-devel/binutils",
        "dev-build/libtool",
        "dev-lang/python",
    ]
    .map(String::from)
    .to_vec();
    let extra =
        fs::read_to_string(main_repo_location.join("profiles/info_pkgs")).unwrap_or_default();
    for line in extra.lines() {
        let l = line.trim();
        if !l.is_empty() && !l.starts_with('#') {
            atoms.push(l.to_string());
        }
    }

    // (orig cp string, resolved cat, resolved pkg, provide_suffix)
    let mut resolved: Vec<(String, String, String, Option<String>)> = Vec::new();
    for a in &atoms {
        // Only `atom.cp` is used, so strip any operator/version/slot/use-dep first.
        let Some(atom) = portage_dep::parse_atom(a) else {
            continue;
        };
        let orig_cp = format!("{}/{}", atom.category, atom.package);
        for (cat, pkg, provide) in expand_new_virt(root, &atom.category, &atom.package) {
            resolved.push((orig_cp.clone(), cat, pkg, provide));
        }
    }
    resolved.sort();
    resolved.dedup();

    use std::collections::BTreeMap;
    // matched_cp -> ver -> "ver::repo (suffix)"
    let mut cp_map: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for (orig_cp, cat, pkg, _provide) in &resolved {
        let matched_cp = format!("{cat}/{pkg}");
        for r in installed_refs(root, cat, pkg) {
            let repo = if r.repo == "__unknown__" {
                "<unknown repository>".to_string()
            } else {
                r.repo.clone()
            };
            let provide_suffix = if &matched_cp != orig_cp {
                format!(" ({orig_cp})")
            } else {
                String::new()
            };
            let entry = cp_map.entry(matched_cp.clone()).or_default();
            // Prefer a duplicate match carrying provider info.
            if let Some(existing) = entry.get(&r.version)
                && existing.contains(" (")
            {
                continue;
            }
            entry.insert(
                r.version.clone(),
                format!("{}::{}{}", r.version, repo, provide_suffix),
            );
        }
    }

    cp_map
        .into_iter()
        .map(|(cp, ver_map)| {
            let mut versions: Vec<(String, String)> = ver_map.into_iter().collect();
            versions.sort_by(|a, b| vercmp_ordering(&a.0, &b.0));
            InfoPkgRow {
                cp,
                versions: versions.into_iter().map(|(_, s)| s).collect(),
            }
        })
        .collect()
}

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

/// The `environment.bz2` assignment scanner: `declare`/`export`-prefixed or plain `KEY=value`
/// lines, with `"`/`'` quoting and multi-line quoted values (later assignments win). Only
/// `wanted` keys are returned, but continuation lines are consumed regardless.
// Portage: vartree.py, _aux_env_search (borrowed from filter-bash-environment.py)
pub(crate) fn parse_env_assignments(text: &str, wanted: &[&str]) -> HashMap<String, String> {
    fn strip_prefix(line: &str) -> &str {
        // Longest-first: `declare -FLAGS<ws>`, `declare<ws>`, `export<ws>`, plain.
        for word in ["declare", "export"] {
            if let Some(rest) = line.strip_prefix(word)
                && rest.starts_with(|c: char| c.is_whitespace())
            {
                let after_ws = rest.trim_start();
                if word == "declare"
                    && let Some(flags) = after_ws.strip_prefix('-')
                {
                    // `-\S+\s+`: dash, non-empty non-whitespace token,
                    // then whitespace.
                    let token_len = flags
                        .bytes()
                        .take_while(|b| !b.is_ascii_whitespace())
                        .count();
                    if token_len > 0 && flags[token_len..].starts_with(|c: char| c.is_whitespace())
                    {
                        return after_ws[1 + token_len..].trim_start();
                    }
                }
                return after_ws;
            }
        }
        line
    }
    fn closer_at_end(s: &str, quote: char) -> bool {
        // A bare trailing quote closes; an escaped `\"` at the very end does not.
        let t = s.trim_end();
        let mut chars = t.chars().rev();
        if chars.next() != Some(quote) {
            return false;
        }
        chars.next() != Some('\\')
    }
    let mut out = HashMap::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        // Key: non-empty run of non-`=`/non-whitespace, immediately followed by `=`.
        let body = strip_prefix(line);
        let eq = body
            .bytes()
            .take_while(|b| *b != b'=' && !b.is_ascii_whitespace())
            .count();
        if eq == 0 || body.as_bytes().get(eq) != Some(&b'=') {
            continue;
        }
        let (key, after_eq) = (&body[..eq], &body[eq + 1..]);
        let (quote, rest) = match after_eq.chars().next() {
            Some(q @ ('"' | '\'')) => (Some(q), &after_eq[q.len_utf8()..]),
            _ => (None, after_eq),
        };
        let value = match quote {
            None => after_eq.trim_end().to_string(),
            Some(q) => {
                if closer_at_end(rest, q) {
                    let t = rest.trim_end();
                    t[..t.len() - 1].to_string()
                } else {
                    // `lines()` stripped newlines; restore the non-closing ones.
                    let mut joined = rest.to_string();
                    for next in lines.by_ref() {
                        joined.push_str(next);
                        if closer_at_end(next, q) {
                            break;
                        }
                        joined.push('\n');
                    }
                    let t = joined.trim_end();
                    t[..t.len().saturating_sub(1)].to_string()
                }
            }
        };
        if wanted.contains(&key) {
            out.insert(key.to_string(), value);
        }
    }
    out
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
