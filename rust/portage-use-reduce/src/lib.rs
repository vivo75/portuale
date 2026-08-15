// Rust port of `portage.dep.use_reduce` (lib/portage/dep/__init__.py),
// restricted to `flat=True` mode -- the depgraph/config-resolution slice
// after atom matching (see PORTING/PROMPT.md).
//
// Unlike atom.rs, this is NOT a narrowed grammar: flat mode's tokenizer
// and bracket/use-conditional handling (the part of use_reduce this ports)
// is fully self-contained in the real implementation and is ported as-is,
// error behavior included. What's out of scope is a set of *optional
// parameters* the harness simply never exercises: `masklist`, `excludeall`
// (rare flag-override sets), `is_src_uri`/the "->" SRC_URI arrow token
// (fetch-restriction syntax), `opconvert` and non-flat structured output
// (`flat=False`'s nested-list bracket-optimization logic is considerably
// more involved and not needed for a flat token list), `subset`, and
// `token_class`/`is_valid_flag` (tokens stay opaque strings here, matching
// how config.py itself calls use_reduce for RESTRICT/PROPERTIES/IUSE-like
// values -- see the grep in PORTING/README.md).
//
// `flat=True` is a real, heavily-used invocation mode (not a convenience
// fiction): lib/portage/package/ebuild/config.py, _emerge/resolver/, and
// _emirrordist all call use_reduce(..., flat=True) for exactly this "give
// me a flat token list" need.

use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    /// Conditionals are active iff the flag is in `uselist` (negated for `!flag?`).
    Normal,
    /// Every conditional is active, regardless of `uselist` (real use_reduce's `matchall=True`).
    All,
    /// Every conditional is inactive, regardless of `uselist` (real use_reduce's `matchnone=True`).
    None,
}

fn useflag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9+_@-]*$").unwrap())
}

fn is_active(
    conditional: &str,
    uselist: &HashSet<String>,
    mode: MatchMode,
) -> Result<bool, String> {
    let (flag, negated) = match conditional.strip_prefix('!') {
        Some(rest) => (&rest[..rest.len() - 1], true),
        None => (&conditional[..conditional.len() - 1], false),
    };
    if !useflag_re().is_match(flag) {
        return Err(format!(
            "invalid use flag '{flag}' in conditional '{conditional}'"
        ));
    }
    Ok(match mode {
        MatchMode::All => true,
        MatchMode::None => false,
        MatchMode::Normal => {
            (uselist.contains(flag) && !negated) || (!uselist.contains(flag) && negated)
        }
    })
}

/// `tokens` is the dep string pre-split on whitespace (equivalent to
/// Python's `depstr.split()`, which is the real function's own first
/// step). Returns the flattened token list, or an error message mirroring
/// `InvalidDependString`.
pub fn use_reduce_flat(
    tokens: &[String],
    uselist: &HashSet<String>,
    mode: MatchMode,
) -> Result<Vec<String>, String> {
    let mut stack: Vec<Vec<String>> = vec![Vec::new()];
    let mut need_bracket = false;

    for (pos, token) in tokens.iter().enumerate() {
        match token.as_str() {
            "(" => {
                if tokens.get(pos + 1).map(String::as_str) == Some(")") {
                    return Err(format!(
                        "expected: dependency string, got: ')', token {}",
                        pos + 2
                    ));
                }
                // "(" always satisfies a pending "||"/"flag?" requirement,
                // regardless of need_bracket's prior state.
                need_bracket = false;
                stack.push(Vec::new());
            }
            ")" => {
                if need_bracket {
                    return Err(format!("expected: '(', got: ')', token {}", pos + 1));
                }
                if stack.len() <= 1 {
                    return Err(format!("no matching '(' for ')', token {}", pos + 1));
                }
                let l = stack.pop().unwrap();
                let top = stack.last_mut().unwrap();
                let conditional = match top.last() {
                    Some(last) if last.ends_with('?') => Some(top.pop().unwrap()),
                    _ => None,
                };
                match conditional {
                    Some(cond) => {
                        if is_active(&cond, uselist, mode)? {
                            stack.last_mut().unwrap().extend(l);
                        }
                    }
                    None => stack.last_mut().unwrap().extend(l),
                }
            }
            "||" => {
                if need_bracket {
                    return Err(format!("expected: '(', got: '||', token {}", pos + 1));
                }
                need_bracket = true;
                stack.last_mut().unwrap().push("||".to_string());
            }
            "->" => {
                // is_src_uri is always false for this harness (SRC_URI
                // arrows are out of v1 scope), matching real use_reduce's
                // behavior when is_src_uri=False.
                return Err(format!(
                    "SRC_URI arrow are only allowed in SRC_URI: token {}",
                    pos + 1
                ));
            }
            _ => {
                if need_bracket {
                    return Err(format!("expected: '(', got: '{token}', token {}", pos + 1));
                }
                if token.ends_with('?') {
                    need_bracket = true;
                }
                stack.last_mut().unwrap().push(token.clone());
            }
        }
    }

    if stack.len() != 1 {
        return Err("Missing ')' at end of string".to_string());
    }
    if need_bracket {
        return Err("Missing '(' at end of string".to_string());
    }

    Ok(stack.pop().unwrap())
}
