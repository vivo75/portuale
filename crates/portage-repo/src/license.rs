//! LICENSE parsing and ACCEPT_LICENSE acceptance.

use crate::*;

/// LICENSE's own PMS 7.3.2 grammar structure -- plain license tokens,
/// `||` any-of groups, and (once a `flag?` USE-conditional has already
/// been resolved) the *bundle* a conditional or plain sub-group
/// contributes when it sits directly inside a `||` group's own
/// alternative list (`AllOf`) -- verified directly against real
/// `portage.dep.use_reduce(..., opconvert=True)`: a `||` group's own
/// members are flat (`['||', 'MIT', 'BSD']`, not double-nested), but a
/// *plain* sub-group (or a conditional's own resolved contents) sitting
/// directly inside that same `||`'s member list stays a genuine nested
/// unit (`['||', ['GPL-2', 'MIT'], 'BSD']` for `|| ( ( GPL-2 MIT ) BSD
/// )` -- "GPL-2 AND MIT" is one whole alternative, not two independent
/// ones) -- while the identical sub-group anywhere *else* (top level, or
/// inside another plain/conditional group) flattens directly into its
/// parent instead (opconvert's own "AND of AND is just AND" collapse).
/// This distinction is why `use_reduce_flat` (which discards *all* group
/// boundaries, a deliberate, already-documented simplification real
/// DEPEND/RDEPEND recursion in portuale relies on) can't be reused for
/// LICENSE at all -- the same reasoning that already made
/// `portage_required_use` its own separate algorithm rather than a mode
/// of this same function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LicenseNode {
    License(String),
    AnyOf(Vec<LicenseNode>),
    AllOf(Vec<LicenseNode>),
}

/// What opened the bracket currently being collected -- mirrors
/// `use_reduce_flat`'s own `need_bracket`/conditional-pop dance, just
/// carried explicitly here since this parser's own stack holds
/// `LicenseNode`s (which can't carry a pending "flag?" or "||" marker
/// the way a flat `Vec<String>` stack can).
pub(crate) enum PendingBracket {
    None,
    Conditional(String),
    AnyOf,
}

/// Parses `tokens` (a `LICENSE` string, pre-split on whitespace) into
/// its own real `||`/USE-conditional tree structure -- see
/// `LicenseNode`'s own doc comment for the exact real-structure ground
/// truth this mirrors, and why. Bracket/`need_bracket` handling is
/// otherwise a direct structural port of `use_reduce_flat`'s own
/// (`portage-use-reduce`), just building a tree instead of flattening.
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
                // Whether the group we just closed sits directly inside
                // a `||`'s own alternative list -- decides flatten
                // (`extend`) vs. nest (`AllOf`) for a plain/conditional
                // group; see `LicenseNode`'s own doc comment.
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

/// Whether `nodes` (implicit AND -- the top level, or an `AllOf` bundle)
/// has at least one license that isn't in `acceptable`. Mirrors real
/// `LicenseManager._getMaskedLicenses`'s own non-`||` branch, as a bool
/// rather than the full "list every masked license" diagnostic (this
/// portuale has no mask-reason display to feed it -- same simplification
/// `check_required_use` already makes for REQUIRED_USE).
pub(crate) fn tree_has_masked_license(nodes: &[LicenseNode], acceptable: &HashSet<String>) -> bool {
    nodes.iter().any(|n| node_has_masked_license(n, acceptable))
}

pub(crate) fn node_has_masked_license(node: &LicenseNode, acceptable: &HashSet<String>) -> bool {
    match node {
        LicenseNode::License(name) => !acceptable.contains(name),
        LicenseNode::AllOf(members) => tree_has_masked_license(members, acceptable),
        // Satisfied (not masked) once at least one alternative is fully
        // unmasked -- mirrors real _getMaskedLicenses's own "||" branch:
        // "if not tmp: return []" the moment one alternative comes back
        // clean.
        LicenseNode::AnyOf(members) => !members
            .iter()
            .any(|m| !node_has_masked_license(m, acceptable)),
    }
}

/// The list form of `tree_has_masked_license` -- real
/// `LicenseManager._getMaskedLicenses`'s own return value: every license
/// name that isn't in `acceptable`. For a `||` group, `[]` the moment
/// one alternative is fully clean; otherwise every masked name across
/// every alternative ("we don't know which combination the user will
/// unmask"). Order-preserving (real appends as it walks); the caller
/// sorts + dedups.
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

/// Whether `license_str` (a candidate's own real `LICENSE` metadata,
/// PMS 7.3.2) has at least one required-but-unaccepted license, given
/// `use_flags` (this candidate's own resolved USE, only ever consulted
/// if `license_str` actually contains a `?`) and `acceptable` (the
/// fully-resolved, concrete set of accepted license names for this
/// specific candidate -- see `license_accepted`). Mirrors real
/// `LicenseManager.getMissingLicenses` (via `_getMaskedLicenses`), ported
/// as a bool. An empty `license_str` is never masked, matching real
/// `use_reduce("")`'s own empty result.
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

/// This candidate's own effective `ACCEPT_LICENSE`/`ACCEPT_PROPERTIES`/
/// `ACCEPT_RESTRICT`-style symbolic token list: `global_accept`, with
/// every matching `package_accept` entry's own tokens layered on top,
/// in atom-specificity order -- real `_getPkgAcceptLicense`'s own
/// `accept_license.extend(x)` loop over `ordered_by_atom_specificity`
/// matches (and its `_getMissingProperties`/`_getMissingRestrict`
/// siblings, which do the identical thing for their own accept lists).
/// These accept lists only ever *extend* (no `-token` cancellation
/// across sources), so a global atom-specificity sort here is harmless --
/// unlike `package.use.mask`/`.force` (see `specificity_ordered_flags`),
/// which must stay in profile-chain order so a child `-flag` can cancel
/// a parent mask.
pub(crate) fn resolve_accept_tokens(
    global_accept: &[String],
    package_accept: &[(String, Vec<String>)],
    candidate_str: &str,
    category: &str,
    package: &str,
) -> Vec<String> {
    let mut matching = config_entries_matching(package_accept, candidate_str, category, package);
    matching.sort_by_key(|(atom, _)| atom_specificity(atom));

    let mut accept_tokens = global_accept.to_vec();
    for (_, tokens) in matching {
        accept_tokens.extend(tokens.iter().cloned());
    }
    accept_tokens
}

/// Every token `value_str` (a `LICENSE`/`PROPERTIES`/`RESTRICT` string)
/// could possibly mention, USE-conditionals included -- real
/// `matchall=1` semantics (every conditional forced active), needed to
/// resolve a `"*"` token in an accept-list into something concrete.
/// Reuses `portage_use_reduce::use_reduce_flat` with `MatchMode::All`
/// directly: group boundaries (`||`) don't matter for this flat "what
/// token names exist at all" question, unlike the real masking check
/// itself for `LICENSE` (see `has_masked_license`'s own doc comment).
pub(crate) fn all_mentioned_tokens(value_str: &str) -> Result<HashSet<String>, Error> {
    let tokens: Vec<String> = value_str.split_whitespace().map(String::from).collect();
    let flat = portage_use_reduce::use_reduce_flat(
        &tokens,
        &HashSet::new(),
        portage_use_reduce::MatchMode::All,
    )?;
    Ok(flat.into_iter().filter(|t| t != "||").collect())
}

/// Resolves `accept_tokens` (symbolic -- `"*"`/`"-*"`/`"-token"`/
/// `"token"`) into a concrete acceptable-token set, given
/// `all_mentioned` (see `all_mentioned_tokens`). Shared by
/// `license_accepted`/`metadata_key_accepted` -- real
/// `getMissingLicenses`/`_getMissingProperties`/`_getMissingRestrict`
/// all use this identical algorithm, just for a different metadata key.
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

pub(crate) fn license_accepted(
    candidate: &Candidate,
    category: &str,
    package: &str,
    candidate_str: &str,
    config: &portage_profile::Config,
) -> bool {
    if candidate.license.trim().is_empty() {
        return true;
    }
    let use_flags = use_flags_if_conditional(UseFlagsIfConditionalArgs {
        value_str: &candidate.license,
        candidate,
        category,
        package,
        candidate_str,
        config,
    });
    let accept_tokens = resolve_accept_tokens(
        &config.accept_license,
        &config.package_license,
        candidate_str,
        category,
        package,
    );
    let Ok(all_mentioned) = all_mentioned_tokens(&candidate.license) else {
        return false;
    };
    let acceptable = resolve_acceptable_tokens(&accept_tokens, &all_mentioned);

    match has_masked_license(&candidate.license, &use_flags, &acceptable) {
        Ok(masked) => !masked,
        Err(_) => false,
    }
}

/// The exact license names `candidate`'s own `LICENSE` needs accepting --
/// real `LicenseManager.getMissingLicenses` in its list form (via
/// `_getMaskedLicenses`). Empty when the LICENSE is fully accepted (or
/// empty, or unparseable). Sorted + deduped, the form
/// `_display_autounmask`'s `license_msg` prints (`' '.join(sorted(
/// missing_licenses))`). Shares every step with `license_accepted`.
pub(crate) fn missing_licenses(
    candidate: &Candidate,
    category: &str,
    package: &str,
    candidate_str: &str,
    config: &portage_profile::Config,
) -> Vec<String> {
    if candidate.license.trim().is_empty() {
        return Vec::new();
    }
    let use_flags = use_flags_if_conditional(UseFlagsIfConditionalArgs {
        value_str: &candidate.license,
        candidate,
        category,
        package,
        candidate_str,
        config,
    });
    let accept_tokens = resolve_accept_tokens(
        &config.accept_license,
        &config.package_license,
        candidate_str,
        category,
        package,
    );
    let Ok(all_mentioned) = all_mentioned_tokens(&candidate.license) else {
        return Vec::new();
    };
    let acceptable = resolve_acceptable_tokens(&accept_tokens, &all_mentioned);
    let tokens: Vec<String> = candidate
        .license
        .split_whitespace()
        .map(String::from)
        .collect();
    let Ok(tree) = parse_license_tree(&tokens, &use_flags) else {
        return Vec::new();
    };
    let mut names = tree_masked_license_names(&tree, &acceptable);
    names.sort();
    names.dedup();
    names
}
