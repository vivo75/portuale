//! Autounmask dependency chains and suggestions.

use crate::*;

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
    top_level: &std::collections::HashSet<&str>,
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
    if let Some(arg) = top_level
        .iter()
        .find(|t| portage_dep::parse_atom(t).is_some_and(|a| a.category == *oc && a.package == *op))
    {
        chain.push(((*arg).to_string(), "argument".to_string()));
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
    let mut out =
        format!("\n!!! The ebuild selected to satisfy \"{xinfo}\" has unmet requirements.\n");
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
