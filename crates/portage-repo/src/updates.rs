//! `profiles/updates` package and slot moves, applied when metadata is read.

use crate::*;

// ===========================================================================
// profiles/updates/ package moves  (SCOPE_BACKLOG Part 2 Section H item 2)
// ===========================================================================
//
// Real `portage.update` + `portage._global_updates._do_global_updates`: each
// repo's `profiles/updates/<quarter>` files hold `move cat/old cat/new` and
// `slotmove cat/pkg <oldslot> <newslot>` directives that a sync applies, once
// and permanently, to the vdb, world file, binary packages and config files,
// and that ebuild metadata regeneration bakes into `metadata/md5-cache`.
//
// Portuale never syncs and never mutates the vdb, so it applies the
// directives at *read* time instead: `apply_updates_to_atom` (command-line and
// world atoms), `apply_updates_to_dep_string` (each `*DEPEND` string read from
// md5-cache -- real `update_dbentry`), and `apply_updates_to_cp` /
// `apply_updates_to_slot` (an installed package's identity as the resolver
// sees it). `installed_cp_sources` runs the `move` map *backwards* so a query
// for the new name still finds the not-yet-renamed vdb directory.
//
// DELIBERATE CUTS (all no-ops for a `--pretend` that never writes):
//   - the actual on-disk vdb category rename / `*DEPEND` rewrite, the world /
//     world_sets file rewrite, binary-package (`%`/`S`) updates, and the
//     `/etc/portage/package.*` (`p`) rewrite -- `--pretend` mutates none of
//     these, so applying the moves in memory on read is equivalent.
//   - real `grab_updates`' `os.scandir` file order (+ `prev_mtimes`
//     incremental logic): portuale sorts the quarter files by `(year,
//     quarter)` parsed from the `NQ-YYYY` name (non-matching names last, by
//     name) and re-reads them every run. Deterministic, and strictly better
//     than real portage's arbitrary order for a chained move that spans two
//     quarter files.
//   - a `slotmove` whose atom carries a version/operator: real
//     `update_dbentry` acts only on the unversioned case
//     (`orig_atom.version is None`), so portuale just drops the qualified
//     form at parse time (same net effect).

/// One validated `profiles/updates/` directive (real
/// `portage.update.parse_updates`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCmd {
    /// `move <old cat/pkg> <new cat/pkg>` -- both bare `cat/pkg` (real
    /// `parse_updates` rejects a blocker or any `atom != atom.cp`).
    Move {
        old: (String, String),
        new: (String, String),
    },
    /// `slotmove <cat/pkg> <origslot> <newslot>` -- only the unversioned
    /// `cat/pkg` form is kept (see the module cut note).
    SlotMove {
        cp: (String, String),
        old_slot: String,
        new_slot: String,
    },
}

/// A bare `cat/pkg` (no version/slot/operator/blocker/use/repo), else `None`
/// -- real `parse_updates`' `Atom(x)` + `atom.blocker or atom != atom.cp`
/// rejection.
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

/// Real `_get_slot_re`: a slot name is `[A-Za-z0-9_][A-Za-z0-9+_.-]*`.
pub(crate) fn valid_slot_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphanumeric() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '_' | '.' | '-'))
}

/// Real `portage.update.parse_updates`: one directive per non-blank line,
/// `move`/`slotmove` only, wrong arity or a malformed atom/slot skips that
/// line (real appends to `errors` -- never fatal).
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
            // A version/operator-qualified slotmove atom is a no-op in real
            // `update_dbentry`; `bare_cp` already drops it.
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

/// `(year, quarter)` from a `NQ-YYYY` updates-file name, for the quarter
/// sort. A name that doesn't match sorts last (`year = i32::MAX`), by name.
pub(crate) fn quarter_sort_key(name: &str) -> (i32, i32, String) {
    if let Some((q, y)) = name.split_once("Q-")
        && let (Ok(q), Ok(y)) = (q.parse::<i32>(), y.parse::<i32>())
        && (1..=4).contains(&q)
    {
        return (y, q, name.to_string());
    }
    (i32::MAX, 0, name.to_string())
}

/// Every directive from one repo's `profiles/updates/`, quarter files in
/// `(year, quarter)` order (real `grab_updates` + `parse_updates`, minus the
/// scandir order / `prev_mtimes` logic -- see the module cut note).
pub(crate) fn grab_updates_for_repo(repo_location: &Path) -> Vec<UpdateCmd> {
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

/// Real `update_dbentry` for a single `move`, applied to one atom token:
/// if the token parses as an atom whose `cp` is `old`, rewrite just the
/// `cat/pkg` part (first occurrence, real `token.replace(old, new, 1)`),
/// preserving every version/slot/use/blocker part. Chained moves are
/// handled by the caller applying every command in order.
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

/// Real `update_dbentry` for a single `slotmove` (`orig_atom.version is
/// None` branch), applied to one atom token: an atom on `cp` whose
/// `:slot` (ignoring sub-slot / operator) is `old_slot` gets it rewritten
/// to `new_slot`. Versioned atoms are left alone (real's own
/// restriction).
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
    // Documented micro-cut: a slotmove target token that also carries a
    // `::repo` or `[usedeps]` part is left alone rather than risk a
    // corrupt splice (real `Atom.with_slot` rebuilds every part -- no
    // fixture exercises this combination).
    if atom.repo.is_some() || atom.use_deps.is_some() {
        return token.to_string();
    }
    match &atom.slot {
        Some(s) if s == old_slot => {}
        _ => return token.to_string(),
    }
    // Rebuild the `:slot` part. Real `with_slot` keeps a differing
    // sub-slot but rewrites a sub-slot that equalled the old slot.
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
    // Splice the new slot expression in place of the old `:...` run.
    let prefix = token.split(':').next().unwrap_or(token);
    format!("{prefix}:{new_slotpart}")
}

/// Apply every `move`/`slotmove` (in order, so chains resolve) to one atom
/// string -- for a command-line or world atom.
/// Every `cat/pkg` that *could* be the first command in the loop to
/// touch an atom -- the union of every `move` command's `old` and every
/// `slotmove`'s `cp`, grouped `category -> package names`. Grouping keeps
/// the hot per-token check two **borrowed** lookups (`#119`: the flat
/// `HashSet<(String, String)>`'s `contains` demanded a
/// `(category.clone(), package.clone())` tuple, two `String` allocations
/// per token of every rewritten dep string). On a real tree
/// `profiles/updates/` holds thousands of accumulated commands, and this
/// walk runs per token per dep string.
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

pub fn apply_updates_to_atom(atom: &str) -> String {
    apply_updates_to_atom_with(atom, global_package_updates(), update_move_targets())
        .unwrap_or_else(|| atom.to_string())
}

/// Backlog #119: `Some(rewritten)` only when a command actually changed
/// the token, `None` when it is byte-identical to the input -- the
/// unchanged case costs no `String`.
pub(crate) fn apply_updates_to_atom_with(
    atom: &str,
    cmds: &[UpdateCmd],
    targets: &HashMap<String, HashSet<String>>,
) -> Option<String> {
    if targets.is_empty() {
        return None;
    }
    // Fast path: if the atom's own `cp` isn't the `old`/`cp` of any
    // command, nothing in the loop below can ever rewrite it. Both
    // lookups borrow (`HashMap<String,_>::get(&str)`,
    // `HashSet<String>::contains(&str)`), so this costs no allocation.
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
    // A command that matched the cp but left the token unchanged (a
    // slotmove on another slot, a versioned atom) is reported as
    // unchanged, exactly like the old `updated != *dep` comparison.
    if cur == atom { None } else { Some(cur) }
}

/// Real `update_dbentry`'s whitespace-preserving `re.split(r"(\s+)")` pass
/// over a `*DEPEND` string: rewrite every atom token, leave separators and
/// non-atom tokens (`||`, `(`, `)`, `use?`) untouched.
///
/// Backlog #119: `Some(rewritten)` only when at least one token changed;
/// `None` means the input is returned as-is, so the caller can skip its
/// insert and the unchanged case allocates no output at all. The old
/// shape `format!`ed every chunk and `collect`ed unconditionally, even
/// when no `profiles/updates/` command matched -- 7.5 % of the reference
/// workload across the md5-cache misses' dep strings.
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
/// `cat/pkg`. Real portage bakes package moves into the vdb + metadata
/// cache at sync time; portuale applies them at read time instead, but a
/// linear replay of all ~500 `profiles/updates/` `move` commands *per
/// lookup* -- and `apply_updates_to_cp` runs once per installed package at
/// ~10 resolver call sites -- was ~15% of a deep `emerge -pu`. Built by
/// replaying the command list once per distinct `old` cp, so a chain
/// (`a/x -> b/x -> c/x`) resolves exactly as the old per-cp loop did.
/// Same `OnceLock`-over-`global_package_updates()` shape as
/// `update_move_targets` (so `--package-moves=n` -> empty list -> empty
/// map -> identity, matching the old loop's no-op).
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

/// The forward `move` destination for a `cat/pkg` (chains applied), for an
/// installed package's identity as the resolver should see it -- one
/// [`move_chain_map`] lookup, identity when nothing moved it.
pub fn apply_updates_to_cp(category: &str, package: &str) -> (String, String) {
    match move_chain_map().get(&(category.to_string(), package.to_string())) {
        Some(dest) => dest.clone(),
        None => (category.to_string(), package.to_string()),
    }
}

/// Inverted `move` map: fully-resolved destination -> the `Move`
/// commands' `old` cps that land on it, in command order.
pub(crate) type InstalledCpSourcesMap = HashMap<(String, String), Vec<(String, String)>>;

/// Inverted [`move_chain_map`]: `installed_cp_sources` used to replay the
/// whole `profiles/updates/` list (one `apply_updates_to_cp` per command)
/// on every call -- 6 M calls on a deep `emerge -pu`; this is built once,
/// so the lookup is a single `HashMap` get. Same
/// `OnceLock`-over-`global_package_updates()` shape as `move_chain_map`
/// (so `--package-moves=n` -> empty list -> empty map).
pub(crate) fn installed_cp_sources_map_with(cmds: &[UpdateCmd]) -> InstalledCpSourcesMap {
    let chain = move_chain_map_with(cmds);
    let mut map: InstalledCpSourcesMap = HashMap::new();
    for cmd in cmds {
        if let UpdateCmd::Move { old, .. } = cmd {
            // `installed_cp_sources` pushes `old` exactly when its
            // fully-resolved chain lands on the queried cp; group by
            // that destination up front, in command order, deduped.
            // A `move` cycle replays back to its start (`dest == old`);
            // the target is already `out[0]`, so skipping keeps the old
            // per-call loop's `!out.contains(old)` invariant.
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

/// Every `cat/pkg` whose forward-`move` chain lands on `(category,
/// package)` -- including `(category, package)` itself. Lets a vdb query
/// for the *new* name also scan the *old*, not-yet-renamed directory
/// (real global-updates renames it on disk; portuale doesn't).
pub fn installed_cp_sources(category: &str, package: &str) -> Vec<(String, String)> {
    let target = (category.to_string(), package.to_string());
    let mut out = vec![target.clone()];
    if let Some(extra) = installed_cp_sources_map().get(&target) {
        out.extend(extra.iter().cloned());
    }
    out
}

/// Test seam for [`installed_cp_sources`] (the
/// `apply_updates_to_dep_string_with` precedent): the same lookup against
/// an explicit command list, so a synthetic `move` cycle can be tested
/// without global state.
#[cfg(test)]
pub(crate) fn installed_cp_sources_with(
    category: &str,
    package: &str,
    cmds: &[UpdateCmd],
) -> Vec<(String, String)> {
    let target = (category.to_string(), package.to_string());
    let mut out = vec![target.clone()];
    if let Some(extra) = installed_cp_sources_map_with(cmds).get(&target) {
        out.extend(extra.iter().cloned());
    }
    out
}

/// `cat/pkg` -> its `slotmove`s (`old_slot`, `new_slot`), in command order.
pub(crate) type SlotMoveMap = HashMap<(String, String), Vec<(String, String)>>;

/// Every `slotmove` grouped by its `cat/pkg`, in command order -- the
/// `slotmove` counterpart to [`move_chain_map`], so `apply_updates_to_slot`
/// is a single `HashMap` lookup plus a walk of just that package's own
/// handful of slot moves rather than the whole `profiles/updates/` list.
pub(crate) fn slot_move_map() -> &'static SlotMoveMap {
    static MAP: OnceLock<SlotMoveMap> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut map: SlotMoveMap = HashMap::new();
        for cmd in global_package_updates() {
            if let UpdateCmd::SlotMove {
                cp,
                old_slot,
                new_slot,
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
