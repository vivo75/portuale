//! `profiles/updates` package and slot moves, applied when metadata is read.

use crate::*;

// `profiles/updates/` package moves, applied at read time.
// Portage: portage.update, _global_updates._do_global_updates

/// One validated `profiles/updates/` directive.
/// Portage: portage.update.parse_updates
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCmd {
    /// `move <old cat/pkg> <new cat/pkg>` -- both bare `cat/pkg`.
    Move {
        old: (String, String),
        new: (String, String),
    },
    /// `slotmove <cat/pkg> <origslot> <newslot>` -- unversioned form only.
    SlotMove {
        cp: (String, String),
        old_slot: String,
        new_slot: String,
    },
}

/// A bare `cat/pkg` (no version/slot/operator/blocker/use/repo), else `None`.
/// Portage: portage.update.parse_updates
pub(crate) fn bare_cp(s: &str) -> Option<(String, String)> {
    let a = portage_dep::parse_atom(s)?;
    if a.blocker != portage_dep::Blocker::None
        || a.operator != portage_dep::Operator::None
        || a.version.is_some()
        || a.slot.is_some()
        || a.slot_operator.is_some()
        || a.use_deps.is_some()
        || a.repo.is_some()
    {
        return None;
    }
    Some((a.category, a.package))
}

/// A slot name is `[A-Za-z0-9_][A-Za-z0-9+_.-]*`.
/// Portage: portage.update, _get_slot_re
pub(crate) fn valid_slot_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphanumeric() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '_' | '.' | '-'))
}

/// One directive per non-blank line, `move`/`slotmove` only; a
/// malformed line is skipped, never fatal.
/// Portage: portage.update.parse_updates
pub fn parse_updates_content(text: &str) -> Vec<UpdateCmd> {
    let mut out = Vec::new();
    for line in text.lines() {
        let toks: Vec<&str> = line.split_whitespace().collect();
        match toks.as_slice() {
            ["move", a, b] => {
                if let (Some(old), Some(new)) = (bare_cp(a), bare_cp(b)) {
                    out.push(UpdateCmd::Move { old, new });
                }
            }
            // A versioned slotmove atom is a no-op; `bare_cp` drops it.
            ["slotmove", pkg, os, ns]
                if valid_slot_name(os) && valid_slot_name(ns) && bare_cp(pkg).is_some() =>
            {
                out.push(UpdateCmd::SlotMove {
                    cp: bare_cp(pkg).unwrap(),
                    old_slot: os.to_string(),
                    new_slot: ns.to_string(),
                });
            }
            _ => {}
        }
    }
    out
}

/// Sort key for `NQ-YYYY` names; a non-matching name sorts last, by name.
pub(crate) fn quarter_sort_key(name: &str) -> (i32, i32, String) {
    if let Some((q, y)) = name.split_once("Q-")
        && let (Ok(q), Ok(y)) = (q.parse::<i32>(), y.parse::<i32>())
        && (1..=4).contains(&q)
    {
        return (y, q, name.to_string());
    }
    (i32::MAX, 0, name.to_string())
}

/// Apply one `move` to one atom token: if its `cp` is `old`, rewrite
/// just the `cat/pkg` part, preserving the rest. Chained moves are
/// handled by the caller applying every command in order.
/// Portage: portage.update, update_dbentry
pub(crate) fn apply_move_to_token(
    token: &str,
    old: &(String, String),
    new: &(String, String),
) -> String {
    let old_cp = format!("{}/{}", old.0, old.1);
    if !token.contains(&old_cp) {
        return token.to_string();
    }
    let Some(atom) = portage_dep::parse_atom(token.trim_start_matches('!')) else {
        return token.to_string();
    };
    if (atom.category.as_str(), atom.package.as_str()) != (old.0.as_str(), old.1.as_str()) {
        return token.to_string();
    }
    let new_cp = format!("{}/{}", new.0, new.1);
    token.replacen(&old_cp, &new_cp, 1)
}

/// Apply one `slotmove` to one atom token: an atom on `cp` whose
/// `:slot` is `old_slot` gets it rewritten to `new_slot`. Versioned
/// atoms are left alone.
/// Portage: portage.update, update_dbentry
pub(crate) fn apply_slotmove_to_token(
    token: &str,
    cp: &(String, String),
    old_slot: &str,
    new_slot: &str,
) -> String {
    let base = token.trim_start_matches('!');
    let Some(atom) = portage_dep::parse_atom(base) else {
        return token.to_string();
    };
    if (atom.category.as_str(), atom.package.as_str()) != (cp.0.as_str(), cp.1.as_str()) {
        return token.to_string();
    }
    if atom.version.is_some() {
        return token.to_string();
    }
    // A slotmove target carrying `::repo` or `[usedeps]` is left alone
    // rather than risk a corrupt splice (no fixture covers it).
    if atom.repo.is_some() || atom.use_deps.is_some() {
        return token.to_string();
    }
    match &atom.slot {
        Some(s) if s == old_slot => {}
        _ => return token.to_string(),
    }
    // Rebuild the `:slot` part, keeping a differing sub-slot but
    // rewriting one that equalled the old slot.
    let sub = match &atom.sub_slot {
        Some(ss) if ss == old_slot => new_slot.to_string(),
        Some(ss) => ss.clone(),
        None => String::new(),
    };
    let op = match atom.slot_operator {
        Some(portage_dep::SlotOperator::Equals) => "=",
        Some(portage_dep::SlotOperator::Star) => "*",
        None => "",
    };
    let new_slotpart = if sub.is_empty() {
        format!("{new_slot}{op}")
    } else {
        format!("{new_slot}/{sub}{op}")
    };
    let prefix = token.split(':').next().unwrap_or(token);
    format!("{prefix}:{new_slotpart}")
}

/// `Some(rewritten)` only when a command actually changed the token,
/// `None` when byte-identical (no `String` allocated).
pub(crate) fn apply_updates_to_atom_with(
    atom: &str,
    cmds: &[UpdateCmd],
    targets: &HashMap<String, HashSet<String>>,
) -> Option<String> {
    if targets.is_empty() {
        return None;
    }
    // Fast path: if the atom's own `cp` is no command's target,
    // nothing below can rewrite it (borrowed lookups, no allocation).
    if let Some(parsed) = portage_dep::parse_atom(atom.trim_start_matches('!'))
        && !targets
            .get(parsed.category.as_str())
            .is_some_and(|pkgs| pkgs.contains(parsed.package.as_str()))
    {
        return None;
    }
    let mut cur = atom.to_string();
    for cmd in cmds {
        cur = match cmd {
            UpdateCmd::Move { old, new } => apply_move_to_token(&cur, old, new),
            UpdateCmd::SlotMove {
                cp,
                old_slot,
                new_slot,
            } => apply_slotmove_to_token(&cur, cp, old_slot, new_slot),
        };
    }
    // A cp-matching but token-identical command still reports unchanged.
    if cur == atom { None } else { Some(cur) }
}

pub(crate) fn apply_updates_to_dep_string_with(
    dep: &str,
    cmds: &[UpdateCmd],
    targets: &HashMap<String, HashSet<String>>,
) -> Option<String> {
    let mut out: Option<String> = None;
    let mut consumed = 0usize;
    let mut copied = 0usize;
    for chunk in dep.split_inclusive(char::is_whitespace) {
        let offset = consumed;
        consumed += chunk.len();
        let trimmed_len = chunk.trim_end_matches(char::is_whitespace).len();
        let (tok, ws) = chunk.split_at(trimmed_len);
        if tok.is_empty() || tok.ends_with('?') || matches!(tok, "||" | "(" | ")") {
            continue;
        }
        if let Some(updated) = apply_updates_to_atom_with(tok, cmds, targets) {
            let buf = out.get_or_insert_with(|| String::with_capacity(dep.len()));
            buf.push_str(&dep[copied..offset]);
            buf.push_str(&updated);
            buf.push_str(ws);
            copied = offset + chunk.len();
        }
    }
    let mut buf = out?;
    buf.push_str(&dep[copied..]);
    Some(buf)
}

/// Every `move` chain fully resolved once: `old cat/pkg` -> final
/// `cat/pkg` (a chain resolves as the old per-cp loop did).
pub(crate) fn move_chain_map_with(
    cmds: &[UpdateCmd],
) -> HashMap<(String, String), (String, String)> {
    let olds: HashSet<&(String, String)> = cmds
        .iter()
        .filter_map(|c| match c {
            UpdateCmd::Move { old, .. } => Some(old),
            UpdateCmd::SlotMove { .. } => None,
        })
        .collect();
    let mut map = HashMap::new();
    for old in olds {
        let mut cur = old.clone();
        for cmd in cmds {
            if let UpdateCmd::Move { old: cmd_old, new } = cmd
                && cur == *cmd_old
            {
                cur = new.clone();
            }
        }
        if cur != *old {
            map.insert(old.clone(), cur);
        }
    }
    map
}
