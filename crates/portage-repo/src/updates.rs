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
    /// `slotmove <pkg> <origslot> <newslot>` -- `pkg` is any non-blocker
    /// atom; `atom` keeps the versioned text (`None` for the bare form).
    SlotMove {
        cp: (String, String),
        old_slot: String,
        new_slot: String,
        atom: Option<String>,
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
    parse_updates_with_errors(text).cmds
}

/// One updates file's parse: the valid commands, the valid-directive
/// count (drives the per-file dots and the force) and whether any line
/// was malformed (an errored file applies but is never recorded).
/// A versioned `slotmove` atom counts as valid and yields a command
/// carrying its text: only the `move_slot_ent` passes match on it.
pub struct ParsedUpdates {
    /// The representable commands, in file order.
    pub cmds: Vec<UpdateCmd>,
    /// Valid directives, including versioned `slotmove` lines.
    pub valid: usize,
    /// True when any non-blank line was malformed.
    pub has_errors: bool,
}

/// Split parse of one updates file's text: blank lines skipped, every
/// other line either one valid directive or one error.
/// Portage: portage.update.parse_updates
pub fn parse_updates_with_errors(text: &str) -> ParsedUpdates {
    let mut cmds = Vec::new();
    let mut valid = 0usize;
    let mut has_errors = false;
    for line in text.lines() {
        let toks: Vec<&str> = line.split_whitespace().collect();
        match toks.as_slice() {
            [] => {}
            ["move", a, b] => {
                if let (Some(old), Some(new)) = (bare_cp(a), bare_cp(b)) {
                    cmds.push(UpdateCmd::Move { old, new });
                    valid += 1;
                } else {
                    has_errors = true;
                }
            }
            // A versioned `slotmove` atom is valid and yields a command
            // carrying its text; only the `move_slot_ent` passes match on
            // it, dep-string rewrites skip it like Portage does.
            ["slotmove", pkg, os, ns] => {
                if let Some(parsed) = slotmove_pkg(pkg)
                    && valid_slot_name(os)
                    && valid_slot_name(ns)
                {
                    valid += 1;
                    let versioned =
                        parsed.operator != portage_dep::Operator::None || parsed.version.is_some();
                    cmds.push(UpdateCmd::SlotMove {
                        cp: (parsed.category, parsed.package),
                        old_slot: os.to_string(),
                        new_slot: ns.to_string(),
                        atom: versioned.then(|| pkg.to_string()),
                    });
                } else {
                    has_errors = true;
                }
            }
            _ => {
                has_errors = true;
            }
        }
    }
    ParsedUpdates {
        cmds,
        valid,
        has_errors,
    }
}

/// A `slotmove` package: parseable and not a blocker. Versioned atoms
/// pass here; the caller keeps their text for the `move_slot_ent` match.
/// Portage: portage.update.parse_updates
fn slotmove_pkg(s: &str) -> Option<portage_dep::Atom> {
    let atom = portage_dep::parse_atom(s)?;
    if atom.blocker != portage_dep::Blocker::None {
        return None;
    }
    Some(atom)
}

/// Which of `cpvs` (`cat/pkg-ver` strings) a versioned `slotmove`
/// atom keeps: the `match_from_list` verdict Portage's `move_slot_ent`
/// reaches through `atom.with_slot(origslot)` (the slot itself is matched
/// by the caller's `SLOT` read, whose candidates carry no slot).
/// An unparseable atom matches nothing.
/// Portage: lib/portage/dbapi/__init__.py, move_slot_ent
pub fn slotmove_version_matches(atom: &str, cpvs: &[String]) -> Vec<String> {
    let refs: Vec<&str> = cpvs.iter().map(String::as_str).collect();
    match portage_dep::match_from_list(atom, &refs) {
        Some(hits) => hits.into_iter().map(|s| s.to_string()).collect(),
        None => Vec::new(),
    }
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

/// Every directive from one repo's `profiles/updates/`, quarter files
/// in `(year, quarter)` order. Also feeds the global-updates driver.
pub fn grab_updates_for_repo(repo_location: &Path) -> Vec<UpdateCmd> {
    let updpath = repo_location.join("profiles").join("updates");
    let Ok(rd) = portage_util::read_dir_entries(&updpath) else {
        return Vec::new();
    };
    let mut files: Vec<(PathBuf, (i32, i32, String))> = rd
        .into_iter()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                return None;
            }
            Some((e.path(), quarter_sort_key(&name)))
        })
        .collect();
    files.sort_by(|a, b| a.1.cmp(&b.1));
    let mut cmds = Vec::new();
    for (path, _) in files {
        if let Ok(text) = fs::read_to_string(&path) {
            cmds.extend(parse_updates_content(&text));
        }
    }
    cmds
}

pub fn global_package_updates() -> &'static [UpdateCmd] {
    if !PACKAGE_MOVES_ENABLED.load(AtomicOrdering::Relaxed) {
        return &[];
    }
    static CACHE: OnceLock<Vec<UpdateCmd>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let config_root = config_root_from_env();
        let Ok(repos) = find_repos(&config_root) else {
            return Vec::new();
        };
        let mut cmds = Vec::new();
        let mut seen: HashSet<PathBuf> = HashSet::new();
        for repo in &repos {
            if seen.insert(repo.location.clone()) {
                cmds.extend(grab_updates_for_repo(&repo.location));
            }
        }
        cmds
    })
}

/// Apply one `move` to one atom token: if its `cp` is `old`, rewrite
/// just the `cat/pkg` part, preserving the rest. Chained moves are
/// handled by the caller applying every command in order.
/// Portage: portage.update, update_dbentry
pub fn apply_move_to_token(token: &str, old: &(String, String), new: &(String, String)) -> String {
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
/// `:slot` is `old_slot` gets it rewritten to `new_slot`. The splice
/// keeps the blocker, the operator/version, a differing sub-slot, the
/// slot operator, and any `[usedeps]`; a sub-slot equalling the old
/// slot is rewritten too. A token carrying `::repo` is left alone:
/// Portage rejects it under every EAPI in use, so none of its metadata
/// paths ever rewrites one.
/// Portage: portage.update, update_dbentry
pub fn apply_slotmove_to_token(
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
    // `repo_deps` is false for every EAPI in use (Portage `eapi.py`), so a
    // `::repo` token is `InvalidAtom` on each metadata path: skipped.
    if atom.repo.is_some() {
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
    // Portage `with_slot`: `remove_slot` cuts at the first `:`, then the
    // new slot, `::repo`, and `[use]` are appended. The prefix keeps any
    // blockers; the tail past the old slot expression (`::repo` cannot
    // occur here, `[usedeps]` can) carries over verbatim.
    let Some(colon) = token.find(':') else {
        return token.to_string();
    };
    let prefix = &token[..colon];
    let mut rest = &token[colon + 1..];
    let Some(after_slot) = rest.strip_prefix(old_slot) else {
        return token.to_string();
    };
    rest = after_slot;
    if let Some(ss) = atom.sub_slot.as_deref() {
        let Some(after_sub) = rest.strip_prefix("/").and_then(|r| r.strip_prefix(ss)) else {
            return token.to_string();
        };
        rest = after_sub;
    }
    match atom.slot_operator {
        Some(portage_dep::SlotOperator::Star) => {
            let Some(after_op) = rest.strip_prefix('*') else {
                return token.to_string();
            };
            rest = after_op;
        }
        Some(portage_dep::SlotOperator::Equals) => {
            let Some(after_op) = rest.strip_prefix('=') else {
                return token.to_string();
            };
            rest = after_op;
        }
        None => {}
    }
    format!("{prefix}:{new_slotpart}{rest}")
}

/// Apply explicit `move`/`slotmove` commands (in order, so chains
/// resolve) to a `*DEPEND` string, for callers that already hold one
/// repo's commands instead of the global cache. `Some(rewritten)` only
/// when a token changed, else `None`.
// Portage: lib/portage/update.py, update_dbentry
pub fn apply_cmds_to_dep_string(dep: &str, cmds: &[UpdateCmd]) -> Option<String> {
    if cmds.is_empty() || dep.trim().is_empty() {
        return None;
    }
    let mut targets: HashMap<String, HashSet<String>> = HashMap::new();
    for cmd in cmds {
        let (cat, pkg) = match cmd {
            UpdateCmd::Move { old, .. } => old,
            UpdateCmd::SlotMove { cp, .. } => cp,
        };
        targets.entry(cat.clone()).or_default().insert(pkg.clone());
    }
    apply_updates_to_dep_string_with(dep, cmds, &targets)
}

/// Every `cat/pkg` that could be the first command to touch an atom:
/// the union of every `move`'s `old` and every `slotmove`'s `cp`,
/// grouped `category -> package names` for borrow-only lookups.
pub(crate) fn update_move_targets() -> &'static HashMap<String, HashSet<String>> {
    static TARGETS: OnceLock<HashMap<String, HashSet<String>>> = OnceLock::new();
    TARGETS.get_or_init(|| {
        let mut map: HashMap<String, HashSet<String>> = HashMap::new();
        for cmd in global_package_updates() {
            let (cat, pkg) = match cmd {
                UpdateCmd::Move { old, .. } => old,
                UpdateCmd::SlotMove { cp, .. } => cp,
            };
            map.entry(cat.clone()).or_default().insert(pkg.clone());
        }
        map
    })
}

/// Apply every `move`/`slotmove` (in order, so chains resolve) to one
/// atom string -- for a command-line or world atom.
pub fn apply_updates_to_atom(atom: &str) -> String {
    apply_updates_to_atom_with(atom, global_package_updates(), update_move_targets())
        .unwrap_or_else(|| atom.to_string())
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
            // A versioned `slotmove` never rewrites dep strings: Portage
            // cannot tell whether the version constraints match the token.
            UpdateCmd::SlotMove {
                cp,
                old_slot,
                new_slot,
                atom: None,
            } => apply_slotmove_to_token(&cur, cp, old_slot, new_slot),
            UpdateCmd::SlotMove { atom: Some(_), .. } => cur,
        };
    }
    // A cp-matching but token-identical command still reports unchanged.
    if cur == atom { None } else { Some(cur) }
}

/// Whitespace-preserving pass over a `*DEPEND` string: rewrite every
/// atom token, leave separators and non-atom tokens untouched.
/// `Some(rewritten)` only when a token changed, else `None`.
/// Portage: portage.update, update_dbentry
pub fn apply_updates_to_dep_string(dep: &str) -> Option<String> {
    if global_package_updates().is_empty() || dep.trim().is_empty() {
        return None;
    }
    apply_updates_to_dep_string_with(dep, global_package_updates(), update_move_targets())
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

pub(crate) fn move_chain_map() -> &'static HashMap<(String, String), (String, String)> {
    static MAP: OnceLock<HashMap<(String, String), (String, String)>> = OnceLock::new();
    MAP.get_or_init(|| move_chain_map_with(global_package_updates()))
}

/// The forward `move` destination for a `cat/pkg` (chains applied):
/// one [`move_chain_map`] lookup, identity when nothing moved it.
pub fn apply_updates_to_cp(category: &str, package: &str) -> (String, String) {
    match move_chain_map().get(&(category.to_string(), package.to_string())) {
        Some(dest) => dest.clone(),
        None => (category.to_string(), package.to_string()),
    }
}

pub(crate) type InstalledCpSourcesMap = HashMap<(String, String), Vec<(String, String)>>;

/// Inverted [`move_chain_map`]: destination -> the `Move` commands'
/// `old` cps landing on it, in command order. Built once, so the
/// lookup is a single `HashMap` get.
pub(crate) fn installed_cp_sources_map_with(cmds: &[UpdateCmd]) -> InstalledCpSourcesMap {
    let chain = move_chain_map_with(cmds);
    let mut map: InstalledCpSourcesMap = HashMap::new();
    for cmd in cmds {
        if let UpdateCmd::Move { old, .. } = cmd {
            // Group by resolved destination, in command order, deduped
            // (a cycle's target is already `out[0]`).
            let dest = chain.get(old).cloned().unwrap_or_else(|| old.clone());
            if dest == *old {
                continue;
            }
            let bucket = map.entry(dest).or_default();
            if !bucket.contains(old) {
                bucket.push(old.clone());
            }
        }
    }
    map
}

pub(crate) fn installed_cp_sources_map() -> &'static InstalledCpSourcesMap {
    static MAP: OnceLock<InstalledCpSourcesMap> = OnceLock::new();
    MAP.get_or_init(|| installed_cp_sources_map_with(global_package_updates()))
}

/// Every `cat/pkg` whose `move` chain lands here, so a vdb query for the
/// new name also scans the old directory. The pre-move name itself is
/// excluded when a `move` renamed it away: its entry no longer exists
/// under that name (Portage renames the directory; see the code note).
/// Portage: vartree.py, move_ent
pub fn installed_cp_sources(category: &str, package: &str) -> Vec<(String, String)> {
    let target = (category.to_string(), package.to_string());
    let moved_away = move_chain_map().contains_key(&target);
    installed_cp_sources_core(&target, installed_cp_sources_map().get(&target), moved_away)
}

/// Shared core of [`installed_cp_sources`] and its test seam: `extra` is the
/// inverted bucket for `target`; `moved_away` drops `target` itself, whose
/// directory a `move` renamed away.
fn installed_cp_sources_core(
    target: &(String, String),
    extra: Option<&Vec<(String, String)>>,
    moved_away: bool,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if !moved_away {
        out.push(target.clone());
    }
    if let Some(extra) = extra {
        out.extend(extra.iter().cloned());
    }
    out
}

/// Test seam for [`installed_cp_sources`]: the same lookup against
/// an explicit command list.
#[cfg(test)]
pub(crate) fn installed_cp_sources_with(
    category: &str,
    package: &str,
    cmds: &[UpdateCmd],
) -> Vec<(String, String)> {
    let target = (category.to_string(), package.to_string());
    let moved_away = move_chain_map_with(cmds).contains_key(&target);
    installed_cp_sources_core(
        &target,
        installed_cp_sources_map_with(cmds).get(&target),
        moved_away,
    )
}

pub(crate) type SlotMoveMap = HashMap<(String, String), Vec<(String, String)>>;

/// Every bare `slotmove` grouped by its `cat/pkg`, in command order, so
/// `apply_updates_to_slot` walks only that package's own moves. Versioned
/// atoms stay out: the read path sees no version to match them against.
/// Portage: portage.update, update_dbentry
pub(crate) fn slot_move_map() -> &'static SlotMoveMap {
    static MAP: OnceLock<SlotMoveMap> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut map: SlotMoveMap = HashMap::new();
        for cmd in global_package_updates() {
            if let UpdateCmd::SlotMove {
                cp,
                old_slot,
                new_slot,
                atom: None,
            } = cmd
            {
                map.entry(cp.clone())
                    .or_default()
                    .push((old_slot.clone(), new_slot.clone()));
            }
        }
        map
    })
}

/// Apply `slotmove` to an installed package's `(slot, sub_slot)` given its
/// (already forward-moved) `cat/pkg`.
pub fn apply_updates_to_slot(
    category: &str,
    package: &str,
    slot: &str,
    sub_slot: &str,
) -> (String, String) {
    let Some(moves) = slot_move_map().get(&(category.to_string(), package.to_string())) else {
        return (slot.to_string(), sub_slot.to_string());
    };
    let mut s = slot.to_string();
    let mut ss = sub_slot.to_string();
    for (old_slot, new_slot) in moves {
        if s == *old_slot {
            if ss == *old_slot {
                ss = new_slot.clone();
            }
            s = new_slot.clone();
        }
    }
    (s, ss)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Explicit commands rewrite a dep string in order, without the
    /// global cache behind it.
    #[test]
    fn cmds_rewrite_a_dep_string_in_order() {
        let cmds = vec![
            UpdateCmd::Move {
                old: ("dev-libs".to_string(), "oldpkg".to_string()),
                new: ("dev-libs".to_string(), "newpkg".to_string()),
            },
            UpdateCmd::SlotMove {
                cp: ("dev-libs".to_string(), "slotpkg".to_string()),
                old_slot: "0".to_string(),
                new_slot: "1".to_string(),
                atom: None,
            },
        ];
        assert_eq!(
            apply_cmds_to_dep_string("dev-libs/oldpkg dev-libs/slotpkg:0\n", &cmds).as_deref(),
            Some("dev-libs/newpkg dev-libs/slotpkg:1\n")
        );
        assert_eq!(apply_cmds_to_dep_string("sys-apps/other\n", &cmds), None);
        assert_eq!(apply_cmds_to_dep_string("", &cmds), None);
        assert_eq!(apply_cmds_to_dep_string("dev-libs/oldpkg\n", &[]), None);
    }
}
