//! Masked-dependency reports.

use crate::*;

/// The IUSE-missing-conditional mask: every `flag? ( … )` group and every conditional
/// use-dep in the five dep keys must reference a flag valid for the atom-owning package's
/// own effective IUSE (see [`implicit_iuse_set`]), or the candidate is masked `invalid`.
/// Validated with `matchall=True` semantics; first failure per key wins, every key still runs.
/// Only ever called for repo/binary candidates, never vdb ones.
// Portage: Package._validate_deps, use_reduce with matchall=True
pub(crate) fn invalid_use_conditional_reasons(
    candidate: &Candidate,
    category: &str,
    package: &str,
    config: &portage_profile::Config,
) -> Vec<String> {
    let ebuild_map;
    let deps: &HashMap<String, String> = match candidate.source {
        CandidateSource::Ebuild => {
            let stem = format!("{package}-{}", candidate.version);
            let Ok(map) = repo_aux_metadata(&candidate.repo_location, category, &stem) else {
                return Vec::new();
            };
            ebuild_map = map;
            &ebuild_map
        }
        // A `Packages` index carries the binpkg's own dep keys; no fixture oracle
        // pins the message text for this arm.
        CandidateSource::Binary => &candidate.binary_deps,
    };
    let valid = implicit_iuse_set(&candidate.iuse, config);
    let mut out = Vec::new();
    for key in ["BDEPEND", "DEPEND", "IDEPEND", "PDEPEND", "RDEPEND"] {
        let Some(dep) = deps.get(key) else { continue };
        if dep.trim().is_empty() {
            continue;
        }
        if let Some(msg) = first_invalid_conditional(dep, &valid) {
            out.push(format!("invalid: {key}: {msg}"));
        }
    }
    out
}

/// First IUSE-missing conditional in one dep string, in encounter order -- see
/// [`invalid_use_conditional_reasons`].
pub(crate) fn first_invalid_conditional(dep: &str, valid: &HashSet<String>) -> Option<String> {
    /// The current level's last token; only a `?`-suffixed token can open a group.
    enum Last {
        None,
        Cond(String),
        Other,
    }
    let mut stack: Vec<Last> = vec![Last::None];
    for tok in dep.split_whitespace() {
        match tok {
            "(" => stack.push(Last::None),
            ")" => {
                let _closed = stack.pop();
                let Some(top) = stack.last_mut() else {
                    continue;
                };
                let raw = match top {
                    Last::Cond(raw) => std::mem::take(raw),
                    _ => continue,
                };
                *top = Last::Other;
                let inner = raw.strip_suffix('?').unwrap_or(raw.as_str());
                let flag = inner.strip_prefix('!').unwrap_or(inner);
                if !valid.contains(flag) {
                    return Some(format!(
                        "USE flag '{flag}' referenced in conditional '{raw}' is not in IUSE"
                    ));
                }
            }
            "||" => {
                if let Some(top) = stack.last_mut() {
                    *top = Last::Other;
                }
            }
            t if t.ends_with('?') => {
                if let Some(top) = stack.last_mut() {
                    *top = Last::Cond(t.to_string());
                }
            }
            t => {
                if let Some(top) = stack.last_mut() {
                    *top = Last::Other;
                }
                let Some(atom) = portage_dep::parse_atom(t) else {
                    continue;
                };
                let Some(use_deps) = atom.use_deps else {
                    continue;
                };
                // `_conditionals_class` slot order: disabled, enabled, equal, not_equal.
                for op in [
                    portage_dep::UseDepOp::IfParentDisabled,
                    portage_dep::UseDepOp::IfParentEnabled,
                    portage_dep::UseDepOp::EqualParent,
                    portage_dep::UseDepOp::OppositeParent,
                ] {
                    let (prefix, suffix) = match op {
                        portage_dep::UseDepOp::IfParentDisabled => ("!", "?"),
                        portage_dep::UseDepOp::IfParentEnabled => ("", "?"),
                        portage_dep::UseDepOp::EqualParent => ("", "="),
                        portage_dep::UseDepOp::OppositeParent => ("!", "="),
                        portage_dep::UseDepOp::Enabled | portage_dep::UseDepOp::Disabled => {
                            continue;
                        }
                    };
                    for ud in &use_deps {
                        if ud.op == op && !valid.contains(&ud.flag) {
                            return Some(format!(
                                "USE flag '{f}' referenced in conditional '{prefix}{f}{suffix}' in atom '{t}' is not in IUSE",
                                f = ud.flag
                            ));
                        }
                    }
                }
            }
        }
    }
    None
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
}
