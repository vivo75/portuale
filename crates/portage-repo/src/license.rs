//! LICENSE parsing and ACCEPT_LICENSE acceptance.

use crate::*;

/// LICENSE's PMS 7.3.2 structure: plain tokens, `||` any-of groups, and the
/// bundle a conditional or plain sub-group contributes inside a `||` alternative
/// list (`AllOf`). A flat `use_reduce` can't be reused here: group boundaries
/// decide masking, unlike DEPEND/RDEPEND recursion.
// Portage: portage.dep.use_reduce with opconvert=True
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LicenseNode {
    License(String),
    AnyOf(Vec<LicenseNode>),
    AllOf(Vec<LicenseNode>),
}

/// What opened the bracket being collected; carried explicitly since the stack
/// holds `LicenseNode`s, which can't carry a pending marker like a flat token stack.
pub(crate) enum PendingBracket {
    None,
    Conditional(String),
    AnyOf,
}

/// Parses pre-split `LICENSE` tokens into their `||`/USE-conditional tree (see
/// `LicenseNode`). Bracket handling structurally mirrors `use_reduce_flat`,
/// building a tree instead of flattening.
// Portage: portage.dep.use_reduce with opconvert=True
pub(crate) fn parse_license_tree(
    tokens: &[String],
    use_flags: &HashSet<String>,
) -> Result<Vec<LicenseNode>, Error> {
    let mut stack: Vec<Vec<LicenseNode>> = vec![Vec::new()];
    let mut bracket_stack: Vec<PendingBracket> = Vec::new();
    let mut pending = PendingBracket::None;
    let mut need_bracket = false;

    for (pos, token) in tokens.iter().enumerate() {
        match token.as_str() {
            "(" => {
                if tokens.get(pos + 1).map(String::as_str) == Some(")") {
                    return Err(Error::LicenseExpectedDepString { token: pos + 2 });
                }
                need_bracket = false;
                stack.push(Vec::new());
                bracket_stack.push(std::mem::replace(&mut pending, PendingBracket::None));
            }
            ")" => {
                if need_bracket {
                    return Err(Error::LicenseExpectedOpen { token: pos + 1 });
                }
                if stack.len() <= 1 {
                    return Err(Error::LicenseNoMatchingOpen { token: pos + 1 });
                }
                let collected = stack.pop().unwrap();
                let opened_by = bracket_stack.pop().unwrap();
                // A group closed directly inside a `||` alternative list nests
                // (`AllOf`); anywhere else it flattens into its parent.
                let parent_is_any_of = matches!(bracket_stack.last(), Some(PendingBracket::AnyOf));
                match opened_by {
                    PendingBracket::AnyOf => {
                        stack
                            .last_mut()
                            .unwrap()
                            .push(LicenseNode::AnyOf(collected));
                    }
                    PendingBracket::Conditional(cond) => {
                        if portage_use_reduce::is_active(
                            &cond,
                            use_flags,
                            portage_use_reduce::MatchMode::Normal,
                        )? {
                            if parent_is_any_of {
                                stack
                                    .last_mut()
                                    .unwrap()
                                    .push(LicenseNode::AllOf(collected));
                            } else {
                                stack.last_mut().unwrap().extend(collected);
                            }
                        }
                    }
                    PendingBracket::None => {
                        if parent_is_any_of {
                            stack
                                .last_mut()
                                .unwrap()
                                .push(LicenseNode::AllOf(collected));
                        } else {
                            stack.last_mut().unwrap().extend(collected);
                        }
                    }
                }
            }
            "||" => {
                if need_bracket {
                    return Err(Error::LicenseExpectedOpenGotOr { token: pos + 1 });
                }
                need_bracket = true;
                pending = PendingBracket::AnyOf;
            }
            _ => {
                if need_bracket {
                    return Err(Error::LicenseExpectedOpenGotToken {
                        token: token.to_string(),
                        at: pos + 1,
                    });
                }
                if token.ends_with('?') {
                    need_bracket = true;
                    pending = PendingBracket::Conditional(token.clone());
                } else {
                    stack
                        .last_mut()
                        .unwrap()
                        .push(LicenseNode::License(token.clone()));
                }
            }
        }
    }

    if stack.len() != 1 {
        return Err(Error::LicenseMissingCloseParen);
    }
    if need_bracket {
        return Err(Error::LicenseMissingOpenParen);
    }
    Ok(stack.pop().unwrap())
}

/// Whether `nodes` (implicit AND) has a license missing from `acceptable`.
/// A bool rather than the full masked-license list (there is no mask-reason
/// display to feed it).
// Portage: LicenseManager._getMaskedLicenses, non-|| branch
pub(crate) fn tree_has_masked_license(nodes: &[LicenseNode], acceptable: &HashSet<String>) -> bool {
    nodes.iter().any(|n| node_has_masked_license(n, acceptable))
}

pub(crate) fn node_has_masked_license(node: &LicenseNode, acceptable: &HashSet<String>) -> bool {
    match node {
        LicenseNode::License(name) => !acceptable.contains(name),
        LicenseNode::AllOf(members) => tree_has_masked_license(members, acceptable),
        // A `||` group is satisfied once one alternative is fully unmasked.
        LicenseNode::AnyOf(members) => !members
            .iter()
            .any(|m| !node_has_masked_license(m, acceptable)),
    }
}

/// The list form of `tree_has_masked_license`: every unaccepted license name.
/// For a `||` group, empty once one alternative is fully clean; otherwise every
/// masked name across every alternative. Order-preserving; the caller sorts.
// Portage: LicenseManager._getMaskedLicenses
pub(crate) fn tree_masked_license_names(
    nodes: &[LicenseNode],
    acceptable: &HashSet<String>,
) -> Vec<String> {
    nodes
        .iter()
        .flat_map(|n| node_masked_license_names(n, acceptable))
        .collect()
}

pub(crate) fn node_masked_license_names(
    node: &LicenseNode,
    acceptable: &HashSet<String>,
) -> Vec<String> {
    match node {
        LicenseNode::License(name) => {
            if acceptable.contains(name) {
                Vec::new()
            } else {
                vec![name.clone()]
            }
        }
        LicenseNode::AllOf(members) => tree_masked_license_names(members, acceptable),
        LicenseNode::AnyOf(members) => {
            if members
                .iter()
                .any(|m| !node_has_masked_license(m, acceptable))
            {
                Vec::new()
            } else {
                members
                    .iter()
                    .flat_map(|m| node_masked_license_names(m, acceptable))
                    .collect()
            }
        }
    }
}

/// Whether `license_str` (PMS 7.3.2) has a required-but-unaccepted license, given
/// this candidate's resolved USE and its concrete acceptable set (see
/// `license_accepted`). An empty string is never masked.
// Portage: LicenseManager.getMissingLicenses via _getMaskedLicenses
pub(crate) fn has_masked_license(
    license_str: &str,
    use_flags: &HashSet<String>,
    acceptable: &HashSet<String>,
) -> Result<bool, Error> {
    if license_str.trim().is_empty() {
        return Ok(false);
    }
    let tokens: Vec<String> = license_str.split_whitespace().map(String::from).collect();
    let tree = parse_license_tree(&tokens, use_flags)?;
    Ok(tree_has_masked_license(&tree, acceptable))
}

/// Every token `value_str` could mention, USE-conditionals included (every
/// conditional forced active). Needed to resolve a `"*"` accept-list token into
/// something concrete. Group boundaries don't matter for this flat question.
// Portage: matchall=1 semantics
pub(crate) fn all_mentioned_tokens(value_str: &str) -> Result<HashSet<String>, Error> {
    let tokens: Vec<String> = value_str.split_whitespace().map(String::from).collect();
    let flat = portage_use_reduce::use_reduce_flat(
        &tokens,
        &HashSet::new(),
        portage_use_reduce::MatchMode::All,
    )?;
    Ok(flat.into_iter().filter(|t| t != "||").collect())
}

/// Resolves symbolic accept tokens (`"*"`/`"-*"`/`"-token"`/`"token"`) into a
/// concrete acceptable set, given everything the value could mention.
/// Shared by `license_accepted`/`metadata_key_accepted`.
// Portage: getMissingLicenses, _getMissingProperties, _getMissingRestrict
pub(crate) fn resolve_acceptable_tokens(
    accept_tokens: &[String],
    all_mentioned: &HashSet<String>,
) -> HashSet<String> {
    let mut acceptable: HashSet<String> = HashSet::new();
    for token in accept_tokens {
        if token == "*" {
            acceptable.extend(all_mentioned.iter().cloned());
        } else if token == "-*" {
            acceptable.clear();
        } else if let Some(name) = token.strip_prefix('-') {
            acceptable.remove(name);
        } else {
            acceptable.insert(token.clone());
        }
    }
    acceptable
}
