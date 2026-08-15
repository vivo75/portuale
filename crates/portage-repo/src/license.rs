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
