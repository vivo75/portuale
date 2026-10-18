//! Masked-dependency reports.

use crate::*;

// NOTE: the long `resolve_pretend` design notes that lived here (the single-atom
// decision, `update`/`selective`/`is_top_level`/`excluded` semantics) now live in
// the notes file; `resolve_pretend` itself lives in `resolve/`.
/// Whether every atom in `atoms` currently has a satisfying candidate -- one input to the
/// `AltPreference` rank for picking a `"||"` group's alternative (the `Available` half).
/// A blocker atom is always satisfiable here, vacuously.
/// Deliberately the *early* half of `resolve_pretend`'s logic only: the `--update`/`--newuse`/
/// `--exclude`/reinstall refinements only matter once an alternative is chosen.
// Portage: dep_check.py, dep_zapdeps all_available
pub(crate) fn atom_currently_satisfiable(
    repos: &[RepoConfig],
    atom_str: &str,
    config: &portage_profile::Config,
    // The `'backtrack` loop's accumulated per-`cat/pkg` constraints for this atom's
    // package (a `!`-prefixed entry is a negative, a bare entry positive).
    // Empty `&[]` at every non-disjunctive call site -- a strict no-op.
    // Portage: backtracking's runtime_pkg_mask seen by dep_zapdeps.
    extra_constraints: &[String],
) -> bool {
    let Some(atom) = portage_dep::parse_atom(atom_str) else {
        return false;
    };
    if atom.blocker != portage_dep::Blocker::None {
        return true;
    }
    let Ok(candidates) = list_candidates(repos, &atom.category, &atom.package) else {
        return false;
    };
    let visible: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| is_visible(c, &atom.category, &atom.package, config))
        .collect();
    if visible.is_empty() {
        return false;
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
    let candidate_str_refs: Vec<&str> = candidate_strs.iter().map(String::as_str).collect();
    let Some(matched) = portage_dep::match_from_list(atom_str, &candidate_str_refs) else {
        return false;
    };
    // Apply the accumulated `runtime_pkg_mask` negatives/positives the
    // same way `resolve_pretend` filters its own `matched` list
    // (lib.rs ~7437) -- an alternative whose every satisfying candidate is
    // masked out counts as unsatisfiable, so the next `||` alternative
    // wins on the retry.
    let matched: Vec<&str> = if extra_constraints.is_empty() {
        matched
    } else {
        matched
            .into_iter()
            .filter(|m| {
                extra_constraints.iter().all(|c| {
                    // Per-instance negatives hide no tree candidate.
                    if is_instance_only_mask(c) {
                        return true;
                    }
                    if let Some(neg) = c.strip_prefix('!') {
                        !portage_dep::match_from_list(neg, std::slice::from_ref(m))
                            .is_some_and(|r| !r.is_empty())
                    } else {
                        portage_dep::match_from_list(c, std::slice::from_ref(m))
                            .is_some_and(|r| !r.is_empty())
                    }
                })
            })
            .collect()
    };

    let Some(use_deps) = atom.use_deps.as_ref().filter(|d| !d.is_empty()) else {
        return !matched.is_empty();
    };
    let mut by_str: HashMap<&str, &Candidate> = HashMap::new();
    for (s, c) in candidate_str_refs.iter().zip(visible.iter()) {
        by_str.insert(*s, *c);
    }
    matched.into_iter().any(|m| {
        let Some(candidate) = by_str.get(m) else {
            return false;
        };
        let Some((iuse, use_flags)) =
            candidate_iuse_and_use(candidate, &atom.category, &atom.package, config)
        else {
            return false;
        };
        portage_dep::use_deps_satisfied(use_deps, &valid_iuse(&iuse, config), &use_flags)
    })
}

/// Whether an atom whose `[use]` deps no candidate satisfies could be satisfied
/// by unmasking/unforcing a flag: which flags would have to change, checked
/// against the best candidate's `use.mask`/`use.force`. Evaluated per single
/// use-dep so default-marker/missing-IUSE semantics match exactly.
// Portage: Atom.violated_conditionals; dep_zapdeps all_use_unmasked
pub(crate) fn atom_all_use_unmasked(
    repos: &[RepoConfig],
    atom_str: &str,
    config: &portage_profile::Config,
    extra_constraints: &[String],
) -> bool {
    let Some(atom) = portage_dep::parse_atom(atom_str) else {
        return true;
    };
    if atom.blocker != portage_dep::Blocker::None {
        return true;
    }
    let Some(use_deps) = atom.use_deps.as_deref().filter(|d| !d.is_empty()) else {
        return true;
    };
    let Some((candidate_str, keywords, iuse, enabled, _slot)) =
        highest_available_candidate_ignoring_use(HighestAvailableCandidateIgnoringUseArgs {
            repos,
            atom_str,
            category: &atom.category,
            package: &atom.package,
            config,
            extra_constraints,
        })
    else {
        // Nothing to evaluate once the whole choice is `Unsatisfiable`.
        return true;
    };
    let valid_iuse = valid_iuse(&iuse, config);
    let eff_keywords = effective_keywords(
        config,
        &candidate_str,
        &atom.category,
        &atom.package,
        &keywords,
    );
    let stable = is_stable(IsStableArgs {
        keywords: &eff_keywords,
        candidate_str: &candidate_str,
        build_id: None,
        category: &atom.category,
        package: &atom.package,
        accept_keywords: &config.accept_keywords,
        package_accept_keywords: &config.package_accept_keywords,
    });
    let mask = resolved_use_mask_or_force(ResolvedUseMaskOrForceArgs {
        which: MaskOrForce::Mask,
        config,
        candidate_str: &candidate_str,
        build_id: None,
        category: &atom.category,
        package: &atom.package,
        stable,
    });
    let force = resolved_use_mask_or_force(ResolvedUseMaskOrForceArgs {
        which: MaskOrForce::Force,
        config,
        candidate_str: &candidate_str,
        build_id: None,
        category: &atom.category,
        package: &atom.package,
        stable,
    });
    let mut violated_enabled = Vec::new();
    let mut violated_disabled = Vec::new();
    for ud in use_deps {
        // "Would *this* flag have to change" per-dep classification.
        if !portage_dep::use_deps_satisfied(std::slice::from_ref(ud), &valid_iuse, &enabled) {
            match ud.op {
                portage_dep::UseDepOp::Enabled => violated_enabled.push(ud.flag.clone()),
                portage_dep::UseDepOp::Disabled => violated_disabled.push(ud.flag.clone()),
                // Conditional forms impose no candidate constraint in matching,
                // so they can never be "violated" here either.
                _ => {}
            }
        }
    }
    if violated_enabled.iter().any(|f| mask.contains(f)) {
        return false;
    }
    if violated_disabled
        .iter()
        .any(|f| force.contains(f) && !mask.contains(f))
    {
        return false;
    }
    true
}

/// The highest visible candidate matching an atom's `[...]`-stripped form, plus
/// its candidate string, IUSE, effective USE and slot -- the seeds of the
/// `use.mask`/`use.force` probe and the vdb slot check.
// Portage: dep_check.py, mydbapi_match_pkgs(atom.without_use)[-1]
pub(crate) type CandidateUseProbe = (
    String,
    Vec<String>,
    HashSet<String>,
    Rc<HashSet<String>>,
    String,
);

/// Arguments of [`highest_available_candidate_ignoring_use`].
pub(crate) struct HighestAvailableCandidateIgnoringUseArgs<'a> {
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) atom_str: &'a str,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) extra_constraints: &'a [String],
}

/// `extra_constraints` is the same backtrack-loop filter
/// [`atom_currently_satisfiable`] applies, so once backtracking masks the
/// highest version this probes the highest *unmasked* one instead.
pub(crate) fn highest_available_candidate_ignoring_use(
    args: HighestAvailableCandidateIgnoringUseArgs<'_>,
) -> Option<CandidateUseProbe> {
    let HighestAvailableCandidateIgnoringUseArgs {
        repos,
        atom_str,
        category,
        package,
        config,
        extra_constraints,
    } = args;
    let stripped = portage_dep::without_use(atom_str);
    let Ok(candidates) = list_candidates(repos, category, package) else {
        return None;
    };
    let visible: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| is_visible(c, category, package, config))
        .collect();
    if visible.is_empty() {
        return None;
    }
    let candidate_strs: Vec<String> = visible
        .iter()
        .map(|c| {
            format!(
                "{category}/{package}-{}:{}/{}::{}",
                c.version, c.slot, c.sub_slot, c.repo_name
            )
        })
        .collect();
    let candidate_str_refs: Vec<&str> = candidate_strs.iter().map(String::as_str).collect();
    let matched = portage_dep::match_from_list(stripped, &candidate_str_refs)?;
    // Same backtrack-loop filter as `atom_currently_satisfiable`.
    let matched: Vec<&str> = if extra_constraints.is_empty() {
        matched
    } else {
        matched
            .into_iter()
            .filter(|m| {
                extra_constraints.iter().all(|c| {
                    // Per-instance negatives hide no tree candidate.
                    if is_instance_only_mask(c) {
                        return true;
                    }
                    if let Some(neg) = c.strip_prefix('!') {
                        !portage_dep::match_from_list(neg, std::slice::from_ref(m))
                            .is_some_and(|r| !r.is_empty())
                    } else {
                        portage_dep::match_from_list(c, std::slice::from_ref(m))
                            .is_some_and(|r| !r.is_empty())
                    }
                })
            })
            .collect()
    };
    let mut by_str: HashMap<&str, &Candidate> = HashMap::new();
    for (s, c) in candidate_str_refs.iter().zip(visible.iter()) {
        by_str.insert(*s, *c);
    }
    // Matches arrive in repo/readdir order, so pick the highest version explicitly.
    let best = matched
        .iter()
        .filter_map(|m| by_str.get(m))
        .max_by(|a, b| vercmp_ordering(&a.version, &b.version))?;
    let (iuse, use_flags) = candidate_iuse_and_use(best, category, package, config)?;
    let best_str = format!(
        "{category}/{package}-{}:{}/{}::{}",
        best.version, best.slot, best.sub_slot, best.repo_name
    );
    Some((
        best_str,
        best.keywords.clone(),
        iuse,
        use_flags,
        best.slot.clone(),
    ))
}

/// Human reason strings for a candidate that is *not* [`is_visible`] -- the
/// `(masked by: …)` text after each `- <cpv>` line. `package.mask`, the
/// `KEYWORDS` string and the `LICENSE` string match Portage verbatim;
/// `PROPERTIES`/`RESTRICT` render as the bare key name.
// Portage: getmaskingstatus._getmaskingstatus, _get_masking_status
pub(crate) fn candidate_masking_reasons(
    candidate: &Candidate,
    category: &str,
    package: &str,
    config: &portage_profile::Config,
) -> Vec<String> {
    // Portage checks the EAPI gate before every other mask and reports it
    // alone (`EAPI <eapi>`); nothing else about the candidate is read.
    // Portage: getmaskingstatus.py
    if !md5_dict::eapi_is_supported(&candidate.eapi) {
        return vec![format!("EAPI {}", candidate.eapi)];
    }
    let candidate_str = format!(
        "{category}/{package}-{}:{}/{}::{}",
        candidate.version, candidate.slot, candidate.sub_slot, candidate.repo_name
    );
    let mut reasons = Vec::new();

    if any_config_entry_matches_with_build_id(
        &config.package_mask,
        &candidate_str,
        candidate.build_id.as_deref(),
        category,
        package,
    ) && !any_config_entry_matches_with_build_id(
        &config.package_unmask,
        &candidate_str,
        candidate.build_id.as_deref(),
        category,
        package,
    ) {
        reasons.push("package.mask".to_string());
    }

    if !license_accepted(candidate, category, package, &candidate_str, config) {
        let missing = missing_licenses(candidate, category, package, &candidate_str, config);
        reasons.push(if missing.is_empty() {
            "LICENSE".to_string()
        } else {
            format!("{} license(s)", missing.join(" "))
        });
    }

    if !metadata_key_accepted(MetadataKeyAcceptedArgs {
        value_str: &candidate.properties,
        candidate,
        category,
        package,
        candidate_str: &candidate_str,
        config,
        which: MetadataKey::Properties,
        global_accept: &config.accept_properties,
        package_accept: &config.package_properties,
    }) {
        reasons.push("PROPERTIES".to_string());
    }

    if !metadata_key_accepted(MetadataKeyAcceptedArgs {
        value_str: &candidate.restrict,
        candidate,
        category,
        package,
        candidate_str: &candidate_str,
        config,
        which: MetadataKey::Restrict,
        global_accept: &config.accept_restrict,
        package_accept: &config.package_accept_restrict,
    }) {
        reasons.push("RESTRICT".to_string());
    }

    // The reason reads the effective KEYWORDS (profile `package.keywords`
    // applied): a `-~amd64` removal reports `missing keyword`, not the
    // raw `~amd64 keyword`. Portage: getmaskingstatus.py
    let eff_keywords = effective_keywords(
        config,
        &candidate_str,
        category,
        package,
        &candidate.keywords,
    );
    if !keywords_accepted(KeywordsAcceptedArgs {
        keywords: &eff_keywords,
        candidate_str: &candidate_str,
        build_id: candidate.build_id.as_deref(),
        category,
        package,
        accept_keywords: &config.accept_keywords,
        package_accept_keywords: &config.package_accept_keywords,
    }) {
        let kmask = eff_keywords
            .iter()
            .find_map(|k| {
                k.strip_prefix('~')
                    .filter(|a| config.accept_keywords.contains(*a))
                    .map(|a| format!("~{a}"))
                    .or_else(|| {
                        k.strip_prefix('-')
                            .filter(|a| config.accept_keywords.contains(*a))
                            .map(|a| format!("-{a}"))
                    })
            })
            .unwrap_or_else(|| "missing".to_string());
        reasons.push(format!("{kmask} keyword"));
    }

    // `invalid` reasons go last, after the masking-status ones.
    reasons.extend(invalid_dep_reasons(candidate, category, package, config));

    if reasons.is_empty() {
        reasons.push("masked".to_string());
    }
    reasons
}

/// One dependency atom's masked-only disclosure: the "All ebuilds that could
/// satisfy … have been masked" block for a *dependency*. Rendered in place of
/// the bare `!!! no visible ebuild for dependency` line, with the
/// `(dependency required by …)` chain and the docs footer.
// Portage: _show_unsatisfied_dep, depgraph.py
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskedDepReport {
    pub category: String,
    pub package: String,
    /// The requesting atom text (unevaluated, as queued).
    pub atom: String,
    /// `(cpv::repo, reasons)` descending version, cpv-deduped: the
    /// `- <cpv> (masked by: …)` lines the renderer prints.
    pub masked: Vec<(String, Vec<String>)>,
    /// `(node, type)` pairs, innermost parent first, up to the top-level
    /// argument(s). An installed intermediate renders its vdb repo with type
    /// `"installed"`; a binary parent renders `"binary"`. One chain per direct
    /// parent (Portage shows only its first abandon; portuale discloses all).
    // Portage: _get_dep_chain
    pub chain: Vec<(String, String)>,
}

/// The separate "no ebuilds built with USE flags to satisfy" disclosure for a
/// *dependency*'s `[use]`-unsatisfied atom: `Missing IUSE: …` rows (IUSE lacks
/// a required flag) or `Change USE: +… -…` rows (effective USE can't satisfy
/// the unconditional use-deps). Rendered instead of the bare no-visible line.
// Portage: _show_unsatisfied_dep
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UseUnsatDepReport {
    pub category: String,
    pub package: String,
    /// The requesting atom as queued, unevaluated, so a `[flag]` dep shows as written.
    pub atom: String,
    /// The `- <cpv>::<repo> (<reason>)` rows: the latest `Change USE:`-adjustable
    /// candidate only, else every `Missing IUSE:` candidate in descending order.
    /// Never empty (the report exists only when a row does).
    pub rows: Vec<(String, Vec<String>)>,
    /// Walked post-pass exactly like [`MaskedDepReport::chain`].
    pub chain: Vec<(String, String)>,
}

/// The plain-miss disclosure for a dependency `NoVisibleCandidate` that is
/// neither masked-only nor `[use]`-unsatisfied. Rendered instead of the bare
/// `!!! no visible ebuild for dependency "<cp>"` line.
// Portage: _show_unsatisfied_dep, "there are no ebuilds to satisfy"
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlainMissDepReport {
    pub category: String,
    pub package: String,
    /// The requesting atom as queued, unevaluated, so a `[flag=]` dep shows as written.
    pub atom: String,
    /// Walked post-pass exactly like [`MaskedDepReport::chain`].
    pub chain: Vec<(String, String)>,
    /// `--usepkgonly`: the miss reads `no binary packages to satisfy`.
    // Portage: _show_unsatisfied_dep, myopts --usepkgonly
    pub binary_only: bool,
}

/// The candidates an atom names by version, visibility and USE ignored. `None`
/// when the atom matches no ebuild at all by version, or when some matching
/// ebuild is actually visible (the `[use]`-mismatch path handles that instead).
// Portage: _show_unsatisfied_dep via db.match(atom.without_use)
pub(crate) fn masked_candidates_for_atom(
    repos: &[RepoConfig],
    atom_str: &str,
    config: &portage_profile::Config,
) -> Option<Vec<(String, Vec<String>)>> {
    let atom = portage_dep::parse_atom(atom_str)?;
    let candidates = list_candidates(repos, &atom.category, &atom.package).ok()?;
    masked_rows_for_pool(&candidates, atom_str, &atom, config)
}

/// The version-matching-but-masked rows for an explicit candidate pool
/// (ebuilds above; the `--usepkgonly` binary pool below): `None` when the
/// pool has no match at all, or when some match is actually visible.
fn masked_rows_for_pool(
    candidates: &[Candidate],
    atom_str: &str,
    atom: &portage_dep::Atom,
    config: &portage_profile::Config,
) -> Option<Vec<(String, Vec<String>)>> {
    // Version/slot filter, USE and visibility ignored. The match runs
    // per candidate (not per string): binpkg-multi-instance builds share
    // one string, and only the instance with the equal build-id may report
    // an `=`+build-id atom (dep:2994-3008, S9).
    let match_atom = portage_dep::parse_match_atom(atom_str)?;
    let mut masked: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| {
            let s = format!(
                "{}/{}-{}:{}/{}::{}",
                atom.category, atom.package, c.version, c.slot, c.sub_slot, c.repo_name
            );
            portage_dep::match_atom_matches_candidate(&match_atom, &s, c.build_id.as_deref())
        })
        .collect();
    if masked.is_empty()
        || masked
            .iter()
            .any(|c| is_visible(c, &atom.category, &atom.package, config))
    {
        return None;
    }
    // Descending version.
    masked.sort_by(|a, b| vercmp_ordering(&b.version, &a.version));

    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for c in masked {
        let reasons = candidate_masking_reasons(c, &atom.category, &atom.package, config);
        // Portage prints an EAPI-masked binary row bare (verified live on
        // 3.0.82.2: `- app-misc/t348-d-good-1 (masked by: EAPI 99)` even
        // with `REPO: t348` in the stanza, while a keyword-masked twin
        // prints `::t348`), and omits `::repo` whenever the row's repo is
        // unknown (`show_masked_packages` appends it only `if repo:`).
        let repo_unknown = c.repo_name.is_empty() || c.repo_name == "__unknown__";
        let eapi_masked_binary =
            c.source == CandidateSource::Binary && reasons.iter().any(|r| r.starts_with("EAPI "));
        let output_cpv = if eapi_masked_binary || repo_unknown {
            format!("{}/{}-{}", atom.category, atom.package, c.version)
        } else {
            format!(
                "{}/{}-{}::{}",
                atom.category, atom.package, c.version, c.repo_name
            )
        };
        if !seen.insert(output_cpv.clone()) {
            continue;
        }
        out.push((output_cpv, reasons));
    }
    Some(out)
}

/// The top-level "all masked" block. `set_member_of` names the set
/// argument this atom was expanded from, if any: Portage discloses set
/// members with a `(dependency required by …)` line, while a direct
/// top-level atom reports no chain.
pub(crate) fn all_masked_report(
    repos: &[RepoConfig],
    atom_str: &str,
    config: &portage_profile::Config,
    xinfo: &str,
    set_member_of: Option<&str>,
) -> Option<String> {
    // The header/footer live here; the masked list is shared with the
    // dependency disclosure.
    let masked = masked_candidates_for_atom(repos, atom_str, config)?;

    render_all_masked_block(xinfo, masked, set_member_of)
}

/// Binary-only twin of [`all_masked_report`] for `--usepkgonly`, where
/// ebuilds are ineligible rather than masked: when every version-matching
/// binary is masked, the block lists the binary rows (Portage's
/// `_show_unsatisfied_dep` over the binary pool). `None` when no binary
/// matches by version, or when some match is actually visible (then the
/// resolve proceeds, or the ebuild-side paths below report).
// Portage: S0 cell `F-binary-rewritten` (`emerge -pv --usepkgonly=y
// app-misc/t341-good` against the rewritten `RDEPEND`).
pub(crate) fn all_binary_masked_report(
    binaries: &[Candidate],
    atom_str: &str,
    config: &portage_profile::Config,
    xinfo: &str,
) -> Option<String> {
    let atom = portage_dep::parse_atom(atom_str)?;
    let masked = masked_rows_for_pool(binaries, atom_str, &atom, config)?;
    render_all_masked_block(xinfo, masked, None)
}

fn render_all_masked_block(
    xinfo: &str,
    masked: Vec<(String, Vec<String>)>,
    set_member_of: Option<&str>,
) -> Option<String> {
    // Portage prints the upgrade paragraph whenever any listed row is
    // EAPI-masked (`show_masked_packages` returns `have_eapi_mask`, and
    // the caller wraps `portage.const.EAPI` — `9` here — at 75 columns;
    // the two lines below are that wrap, byte for byte).
    // Portage: _emerge/depgraph.py, show_masked_packages
    let have_eapi_mask = masked
        .iter()
        .any(|(_, reasons)| reasons.iter().any(|r| r.starts_with("EAPI ")));
    let mut out = format!(
        "\n!!! All ebuilds that could satisfy {xinfo}{} have been masked.\n\
         !!! One of the following masked packages is required to complete your request:\n",
        crate::display_root_suffix()
    );
    for (output_cpv, reasons) in masked {
        out.push_str(&format!(
            "- {output_cpv} (masked by: {})\n",
            reasons.join(", ")
        ));
    }
    if have_eapi_mask {
        out.push_str(eapi_mask_upgrade_notice());
    }
    // A set member discloses the set that pulled it in; a direct
    // top-level atom reports no chain. Portage: depgraph.py,
    // _show_unsatisfied_dep (the `atom_arg` dep-chain walk ends at the
    // set for `SetArg` members).
    if let Some(set) = set_member_of {
        out.push_str(&format!(
            "\n(dependency required by \"{set}\" [argument])\n"
        ));
    } else {
        out.push('\n');
    }
    out.push_str(
        "For more information, see the MASKED PACKAGES section in the emerge\n\
         man page or refer to the Gentoo Handbook.\n",
    );
    Some(out)
}

/// Whether `msg` is the top-level "all masked" abort block built by
/// [`all_masked_report`]/[`all_binary_masked_report`]: the only resolver
/// `Err` raised after resolution started, so the only one whose exit path
/// prints the resolution notice (351-ABORT). The dependency-path masked
/// disclosures share the sentence but never travel as `Err`.
pub fn is_all_masked_abort_report(msg: &str) -> bool {
    msg.trim_start()
        .starts_with("!!! All ebuilds that could satisfy")
}

/// Portage's EAPI-masked upgrade paragraph (the leading blank line
/// included): printed after the masked rows whenever any of them is
/// EAPI-masked, before the `MASKED PACKAGES` footer. Shared by the
/// top-level block above and the dependency-path masked disclosures.
/// Portage: _emerge/depgraph.py, _show_unsatisfied_dep (`have_eapi_mask`)
pub fn eapi_mask_upgrade_notice() -> &'static str {
    "\nThe current version of portage supports EAPI '9'. You must upgrade to a\n\
     newer version of portage before EAPI masked packages can be installed.\n"
}
