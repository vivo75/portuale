//! Autounmask dependency chains and suggestions.

use crate::*;

/// The `#required by …` lines for an autounmask change on `owner`'s
/// dependency (or a top-level `current_atom`), narrowed to portuale's
/// one-level-parent tracking. A top-level atom yields a single
/// `required by <atom> (argument)` line.
/// Portage: depgraph.py, _get_dep_chain_as_comment.
pub(crate) fn autounmask_dep_chain(
    owner: &Option<(String, String)>,
    current_atom: &str,
    top_level: &TopLevel<'_>,
    entries: &[GraphEntry],
    root: &Path,
) -> Vec<String> {
    let Some(start) = owner else {
        // A top-level change prints the argument atom *as given*, not the
        // change's own `>=<cpv>` left-hand form. Several top-level atoms
        // can name the same package (e.g. two valid lines of one `@set`);
        // the sorted-first wins so the line is deterministic (iterating
        // the `HashSet` with `find` printed either atom from run to run).
        let mut args: Vec<&str> = top_level
            .iter()
            .copied()
            .filter(|t| {
                parse_dep_atom(t, "autounmask").is_some_and(|a| {
                    parse_dep_atom(current_atom, "autounmask")
                        .is_some_and(|c| a.category == c.category && a.package == c.package)
                })
            })
            .collect();
        args.sort_unstable();
        let arg = args.first().copied().unwrap_or(current_atom);
        return vec![format!("required by {} (argument)", top_level.label(arg))];
    };
    // Walks the whole parent chain to the argument, not just the immediate
    // owner. Each node renders `required by <cpv>::<repo>` (vdb repo for an
    // installed parent) or `required by <atom> (argument)`.
    //
    // Single-branch rule (deliberate): the owner is re-derived as the first
    // `required_by` entry and the ascent follows one branch.
    let mut chain: Vec<String> = Vec::new();
    let mut visited: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    let mut cur = start.clone();
    loop {
        if !visited.insert(cur.clone()) {
            break;
        }
        let mut args: Vec<String> = top_level
            .iter()
            .filter(|t| {
                parse_dep_atom(t, "autounmask").is_some_and(|a| {
                    a.category == cur.0
                        && a.package == cur.1
                        && a.blocker == portage_dep::Blocker::None
                })
            })
            .map(|t| format!("required by {} (argument)", top_level.label(t)))
            .collect();
        // Sorted: several top-level atoms can name one package (see
        // above); the `HashSet` order would print them at random.
        args.sort();
        let parent = entries.iter().find(|e| {
            e.category == cur.0
                && e.package == cur.1
                && !matches!(
                    e.outcome,
                    PretendOutcome::NoVisibleCandidate | PretendOutcome::Uninstall { .. }
                )
        });
        let mut advanced = false;
        if let Some(parent) = parent {
            let (version, installed) = match &parent.outcome {
                PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
                    (Some(version.clone()), false)
                }
                PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => {
                    (Some(to.clone()), false)
                }
                PretendOutcome::AlreadyInstalled { version } => (Some(version.clone()), true),
                _ => (None, false),
            };
            if let Some(v) = version {
                let repo = if installed {
                    installed_pkg_repo(root, &cur.0, &cur.1, &v)
                } else {
                    parent.repo_name.clone().unwrap_or_default()
                };
                if repo.is_empty() {
                    chain.push(format!("required by {}/{}-{v}", cur.0, cur.1));
                } else {
                    chain.push(format!("required by {}/{}-{v}::{repo}", cur.0, cur.1));
                }
            }
            // Ascend to the parent's own requirer, one hop per loop.
            match parent.required_by.first() {
                Some(next) if *next != cur => {
                    cur = next.clone();
                    advanced = true;
                }
                _ => {}
            }
        }
        if !args.is_empty() {
            chain.extend(args);
            break;
        }
        if !advanced {
            if chain.is_empty() {
                chain.push(format!("required by {}/{}", cur.0, cur.1));
            }
            break;
        }
    }
    chain
}

/// `USE="…"` then one ` VAR="…"` per non-hidden `USE_EXPAND` group, from
/// the `(name, body)` pairs [`pkg_use_display_for`] produces.
/// Portage: pkg_use_display.
pub(crate) fn render_use_display_pairs(disp: &[(String, String)]) -> String {
    let use_body = disp
        .iter()
        .find(|(name, _)| name == "USE")
        .map(|(_, body)| body.as_str())
        .unwrap_or("");
    let mut out = format!("USE=\"{use_body}\"");
    for (name, body) in disp {
        if name != "USE" {
            out.push_str(&format!(" {name}=\"{body}\""));
        }
    }
    out
}

/// The `(node, node_type)` pairs for `_show_unsatisfied_dep`'s trailing
/// `(dependency required by "<node>" [<node_type>])` lines, narrowed to
/// portuale's one-level-parent tracking. Empty when the failing package IS
/// the top-level atom (the atom is already the `xinfo` shown above).
/// Portage: depgraph.py, _show_unsatisfied_dep.
pub(crate) fn required_use_dep_chain(
    owner: &Option<(String, String)>,
    top_level: &TopLevel<'_>,
    entries: &[GraphEntry],
) -> Vec<(String, String)> {
    let Some((oc, op)) = owner else {
        return Vec::new();
    };
    let mut chain: Vec<(String, String)> = Vec::new();
    if let Some(parent) = entries
        .iter()
        .find(|e| &e.category == oc && &e.package == op)
    {
        let parent_version = match &parent.outcome {
            PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
                Some(version.as_str())
            }
            PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => {
                Some(to.as_str())
            }
            _ => None,
        };
        if let Some(v) = parent_version {
            let node = match &parent.repo_name {
                Some(repo) => format!("{oc}/{op}-{v}::{repo}"),
                None => format!("{oc}/{op}-{v}"),
            };
            let node_type = match parent.source {
                CandidateSource::Binary => "binary",
                CandidateSource::Ebuild => "ebuild",
            };
            chain.push((node, node_type.to_string()));
        }
    }
    // Sorted-first, like `autounmask_dep_chain` above: several top-level
    // atoms can name the same package and the `HashSet` order is random.
    let mut matches: Vec<&str> = top_level
        .iter()
        .copied()
        .filter(|t| {
            parse_dep_atom(t, "autounmask").is_some_and(|a| a.category == *oc && a.package == *op)
        })
        .collect();
    matches.sort_unstable();
    if let Some(arg) = matches.first() {
        chain.push((top_level.label(arg).to_string(), "argument".to_string()));
    }
    chain
}

/// Arguments of [`render_required_use_block`].
pub(crate) struct RenderRequiredUseBlockArgs<'a> {
    pub(crate) xinfo: &'a str,
    pub(crate) cpv_repo: &'a str,
    pub(crate) use_display: &'a [(String, String)],
    pub(crate) reduced_human: &'a str,
    pub(crate) full_human: &'a str,
    pub(crate) dep_chain: &'a [(String, String)],
}

/// `_show_unsatisfied_dep`'s REQUIRED_USE block: the header, the
/// `- <cpv>::<repo> USE="…"` line, the minimal still-unsatisfied
/// sub-expression, the complete expression (only when it differs), then the
/// dep chain. Colorization is a documented no-op (matches `emerge -p`
/// without `--color=y`).
/// Portage: depgraph.py, _show_unsatisfied_dep.
pub(crate) fn render_required_use_block(args: RenderRequiredUseBlockArgs<'_>) -> String {
    let RenderRequiredUseBlockArgs {
        xinfo,
        cpv_repo,
        use_display,
        reduced_human,
        full_human,
        dep_chain,
    } = args;
    let mut out = format!(
        "\n!!! The ebuild selected to satisfy \"{xinfo}\"{} has unmet requirements.\n",
        crate::display_root_suffix()
    );
    out.push_str(&format!(
        "- {cpv_repo} {}\n",
        render_use_display_pairs(use_display)
    ));
    out.push_str("\n  The following REQUIRED_USE flag constraints are unsatisfied:\n");
    out.push_str(&format!("    {reduced_human}\n"));
    if reduced_human != full_human {
        out.push_str(
            "\n  The above constraints are a subset of the following complete expression:\n",
        );
        out.push_str(&format!("    {full_human}\n"));
    }
    out.push('\n');
    if !dep_chain.is_empty() {
        let lines: Vec<String> = dep_chain
            .iter()
            .map(|(node, ty)| format!("(dependency required by \"{node}\" [{ty}])"))
            .collect();
        out.push_str(&lines.join("\n"));
        out.push('\n');
    }
    out
}

/// Arguments of [`check_if_latest_atom_form`].
pub(crate) struct CheckIfLatestAtomFormArgs<'a> {
    pub(crate) resolved: &'a Candidate,
    pub(crate) all_candidates: &'a [Candidate],
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) check_visibility: bool,
}

/// An autounmask change's left-hand atom: `>=<cpv>` when `resolved` is the
/// highest candidate of its cp, `>=<cpv>:<slot>` when it's the highest *in
/// its own slot*, else `=<cpv>`. `>=` for USE and license changes (unlike
/// keywords, which always use `=`), so the change keeps applying to newer
/// builds.
/// Portage: _display_autounmask's check_if_latest.
///
/// `check_visibility`: `true` for USE changes (only genuinely-visible
/// higher builds widen the atom), `false` for license changes (every
/// higher build counts).
pub(crate) fn check_if_latest_atom_form(args: CheckIfLatestAtomFormArgs<'_>) -> String {
    let CheckIfLatestAtomFormArgs {
        resolved,
        all_candidates,
        category,
        package,
        config,
        check_visibility,
    } = args;
    let cpv = format!("{category}/{package}-{}", resolved.version);
    let higher = |same_slot: bool| {
        all_candidates.iter().any(|c| {
            (!same_slot || c.slot == resolved.slot)
                && vercmp_ordering(&c.version, &resolved.version) == std::cmp::Ordering::Greater
                && (!check_visibility || is_visible(c, category, package, config))
        })
    };
    if !higher(false) {
        format!(">={cpv}")
    } else if !higher(true) {
        format!(">={cpv}:{}", resolved.slot)
    } else {
        format!("={cpv}")
    }
}

/// The `check_visibility=True` form (USE changes).
pub(crate) fn autounmask_use_atom_form(
    resolved: &Candidate,
    all_candidates: &[Candidate],
    category: &str,
    package: &str,
    config: &portage_profile::Config,
) -> String {
    check_if_latest_atom_form(CheckIfLatestAtomFormArgs {
        resolved,
        all_candidates,
        category,
        package,
        config,
        check_visibility: true,
    })
}

/// The `'backtrack` loop's `autounmask_use_config` accumulator rendered
/// as `Config::autounmask_use` -- one `(cat/pkg, [±flag …])` entry per
/// package, tokens sorted for determinism.
pub(crate) fn autounmask_use_tier(
    acc: &HashMap<(String, String), HashMap<String, bool>>,
) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = acc
        .iter()
        .map(|((cat, pkg), flags)| {
            let mut toks: Vec<String> = flags
                .iter()
                .map(|(f, on)| if *on { f.clone() } else { format!("-{f}") })
                .collect();
            toks.sort();
            (format!("{cat}/{pkg}"), toks)
        })
        .collect();
    out.sort();
    out
}
