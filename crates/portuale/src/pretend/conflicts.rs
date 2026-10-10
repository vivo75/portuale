//! Slot-conflict text helpers.

use crate::pretend::*;

/// One classified reason a slot-conflicted parent atom rejects the
/// *other* instance's version, mirroring real
/// `_slot_conflict_handler._prepare_conflict_msg_and_check_for_specificity`'s
/// `collision_reasons` keys -- `version` with a `ge`/`eq`/`le` sub-type,
/// `slot` with the `:slot[/sub]` text (for the caret span), and `use`
/// with the violated flag. Documented cuts vs. real: the `soname` reason
/// key (no soname atom can reach this renderer -- `portage-dep` cannot
/// parse soname atoms and dep flattening drops unparseable tokens, so
/// the key is unreachable, not merely unimplemented), an `=*` operator's
/// `("version", None)` key (portuale files it under `"eq"`),
/// operator/USE colorization, and the `need_rebuild` "cannot be rebuilt"
/// trailer (wired below, not cut).
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum SlotConflictReason {
    Version(&'static str), // "ge" | "eq" | "le"
    Slot(String),          // the ":slot[/sub]" text, for the caret span
    Use(String),           // the violated flag, for the caret span
    /// Real `slot_collision.py`'s `("AtomArg", None)` key: a bare
    /// command-line parent whose atom is satisfied by every other
    /// instance, shown when some other instance is installed.
    Argument,
}

/// One slot-conflict parent that contributed at least one specific
/// collision reason -- the working row of `get_conflict()`'s per-instance
/// `pulled in by` rendering.
pub(crate) struct ClassifiedParent<'a> {
    pub(crate) parent_cpv: &'a str,
    pub(crate) atom_str: &'a str,
    pub(crate) atom: Atom,
    pub(crate) reasons: Vec<SlotConflictReason>,
    /// Real `unconditional_use_deps`: at least one `("use", flag)` reason
    /// came from a flag missing from the other instance's IUSE (rather
    /// than contradicted by its USE) -- such parents render before the
    /// rest of the selected set.
    pub(crate) use_unconditional: bool,
    /// Real `pkg_use_display(parent, ...)` pairs for this parent package.
    pub(crate) use_display: &'a [(String, String)],
}

/// `atom` rendered back to `{op}{cat}/{pkg}-{ver}[-rN][*]`, optionally with
/// its `:slot[/sub]` -- the two forms real portage builds as
/// `atom.without_use.without_slot` and `atom.without_use` for its
/// `findAtomForPackage` collision-reason probes.
pub(crate) fn slot_conflict_atom_string(atom: &Atom, include_slot: bool) -> String {
    let mut s = String::new();
    s.push_str(match &atom.operator {
        Operator::EqGlob => "=",
        o => o.as_str(),
    });
    s.push_str(&atom.category);
    s.push('/');
    s.push_str(&atom.package);
    if let Some(v) = &atom.version {
        s.push('-');
        s.push_str(v);
        if let Some(r) = &atom.revision {
            s.push_str("-r");
            s.push_str(r);
        }
        if atom.operator == Operator::EqGlob {
            s.push('*');
        }
    }
    if include_slot && let Some(slot) = &atom.slot {
        s.push(':');
        s.push_str(slot);
        if let Some(sub) = &atom.sub_slot {
            s.push('/');
            s.push_str(sub);
        }
    }
    s
}

/// One conflicting instance as seen by the collision-reason probes: the
/// `match_from_list` candidate string plus the flag sets real matches
/// USE-deps against (`other_pkg.iuse`, `_pkg_use_enabled(other_pkg)`).
pub(crate) struct ConflictOther {
    pub(crate) cpv: String,
    pub(crate) iuse: HashSet<String>,
    pub(crate) use_flags: HashSet<String>,
    pub(crate) installed: bool,
}

/// Real `_prepare_conflict_msg_and_check_for_specificity`'s per-parent
/// `collision_reasons` classification: why `atom` (with USE and slot
/// stripped in turn) fails to match each `other` conflicting instance.
/// Version mismatch wins over slot mismatch wins over USE mismatch,
/// matching real's `elif` ordering. Returns the reasons plus whether any
/// USE reason was unconditional (missing from the other's IUSE).
pub(crate) fn slot_conflict_reasons(
    atom: &Atom,
    others: &[ConflictOther],
) -> (Vec<SlotConflictReason>, bool) {
    let bare = slot_conflict_atom_string(atom, false);
    let no_use = slot_conflict_atom_string(atom, true);
    let mut reasons: Vec<SlotConflictReason> = Vec::new();
    let mut use_unconditional = false;
    for other in others {
        let matches_bare =
            match_from_list(&bare, &[other.cpv.as_str()]).is_some_and(|v| !v.is_empty());
        if !matches_bare {
            let sub = match atom.operator {
                Operator::Ge | Operator::Gt => Some("ge"),
                Operator::Eq | Operator::EqGlob | Operator::Tilde => Some("eq"),
                Operator::Le | Operator::Lt => Some("le"),
                Operator::None => None,
            };
            if let Some(sub) = sub {
                let r = SlotConflictReason::Version(sub);
                if !reasons.contains(&r) {
                    reasons.push(r);
                }
            }
            // Real's `elif` chain (`slot_collision.py:307-397`): a
            // version-mismatched pair skips the slot and USE checks
            // below -- even when the operator is `None` and nothing is
            // recorded for this pair.
            continue;
        } else if atom.slot.is_some() {
            let matches_no_use =
                match_from_list(&no_use, &[other.cpv.as_str()]).is_some_and(|v| !v.is_empty());
            if !matches_no_use {
                let mut s = String::new();
                if let Some(slot) = &atom.slot {
                    s.push(':');
                    s.push_str(slot);
                }
                if let Some(sub) = &atom.sub_slot {
                    s.push('/');
                    s.push_str(sub);
                }
                let r = SlotConflictReason::Slot(s);
                if !reasons.contains(&r) {
                    reasons.push(r);
                }
                // Real's `elif` chain (`slot_collision.py`): the USE
                // check below runs only when the slot half matched --
                // otherwise the slot reason already explains this
                // parent against this other instance.
                continue;
            }
        }
        if let Some(use_deps) = &atom.use_deps {
            // `match_from_list` skips USE evaluation against plain-string
            // candidates (real's own `hasattr(x, "use")` guard), so a
            // version+slot match always "matches" here -- the USE verdict
            // comes from `use_mismatch_flags` alone, exactly partitioning
            // the cases where real's full-atom `findAtomForPackage` fails.
            // (Soname atoms never reach this renderer -- see the
            // `SlotConflictReason` doc comment.)
            let (missing, violated) = use_mismatch_flags(use_deps, &other.use_flags, &other.iuse);
            if !missing.is_empty() {
                use_unconditional = true;
            }
            let mut flags: Vec<String> = missing.into_iter().chain(violated).collect();
            flags.sort();
            for f in flags {
                let r = SlotConflictReason::Use(f);
                if !reasons.contains(&r) {
                    reasons.push(r);
                }
            }
        }
    }
    (reasons, use_unconditional)
}

/// Real `highlight_violations`' `colored_idx`: the character positions in
/// `atom_str` under the violated operator, version, `:slot`, and/or USE
/// tokens -- the `^` marker line printed beneath a displayed parent atom.
/// The render site wraps these spans in `BAD` red (`colorize_marked_spans`)
/// and shifts the markers onto the displayed string; real marks the
/// pre-color indices and drifts (upstream bug -- and real's own red
/// varies by terminal colormap, so byte-parity with the drift is
/// unachievable anyway).
pub(crate) fn slot_conflict_caret_idx(
    atom_str: &str,
    atom: &Atom,
    version_violated: bool,
    slot_violated: bool,
    use_flags: &[String],
) -> std::collections::HashSet<usize> {
    let chars: Vec<char> = atom_str.chars().collect();
    let mut idx: std::collections::HashSet<usize> = std::collections::HashSet::new();

    let mut slot_str = String::new();
    if let Some(slot) = &atom.slot {
        slot_str.push(':');
        slot_str.push_str(slot);
    }
    if let Some(sub) = &atom.sub_slot {
        slot_str.push('/');
        slot_str.push_str(sub);
    }
    match atom.slot_operator {
        Some(SlotOperator::Equals) => slot_str.push('='),
        Some(SlotOperator::Star) => slot_str.push('*'),
        None => {}
    }

    let span = |needle: &str, from_end: bool| -> Option<usize> {
        if needle.is_empty() {
            return None;
        }
        let n: Vec<char> = needle.chars().collect();
        let positions = 0..=chars.len().saturating_sub(n.len());
        let hit = |i: &usize| chars[*i..*i + n.len()] == n[..];
        if from_end {
            positions.rev().find(hit)
        } else {
            positions.into_iter().find(hit)
        }
    };
    let mut mark = |start: usize, len: usize| {
        for i in start..start + len {
            idx.insert(i);
        }
    };

    if version_violated {
        let op_len = match &atom.operator {
            Operator::EqGlob => 1, // "=" (the "*" rides with the version)
            o => o.as_str().chars().count(),
        };
        if op_len > 0 {
            mark(0, op_len);
        }
        let ver_str = atom.version.as_deref().map(|v| {
            let mut s = v.to_string();
            if let Some(r) = &atom.revision {
                s.push_str("-r");
                s.push_str(r);
            }
            if atom.operator == Operator::EqGlob {
                s.push('*');
            }
            s
        });
        if let Some(ver) = &ver_str
            && let Some(start) = span(ver, true)
        {
            mark(start, ver.chars().count());
        }
        if let Some(start) = span(&slot_str, false) {
            mark(start, slot_str.chars().count());
        }
    } else if slot_violated && let Some(start) = span(&slot_str, false) {
        mark(start, slot_str.chars().count());
    }
    if !use_flags.is_empty()
        && let Some(bracket) = chars.iter().position(|&c| c == '[')
        && let Some(close) = chars.iter().position(|&c| c == ']')
        && close > bracket
    {
        // Real `highlight_violations`' USE-token branch: mark every
        // `[...]` token whose flag is violated. Real walks its own
        // re-rendered tokens with a running cursor (no spaces assumed);
        // portuale locates each comma-separated piece from the cursor
        // instead, so spaced atoms (`[x, y]`) still mark correctly. The
        // strip rule is real's own (`lstrip("-!")` / `rstrip("=?")`) --
        // including its quirk that `(+)`/`(-)`-suffixed tokens never
        // match.
        let inner: String = chars[bracket + 1..close].iter().collect();
        let mut cursor = bracket + 1;
        for piece in inner.split(',') {
            let piece_chars: Vec<char> = piece.chars().collect();
            let start = chars[cursor..]
                .windows(piece_chars.len().max(1))
                .position(|w| w == &piece_chars[..])
                .map(|p| p + cursor);
            if let Some(start) = start {
                let flag = piece
                    .trim()
                    .trim_start_matches(['-', '!'])
                    .trim_end_matches(['=', '?']);
                if use_flags.iter().any(|f| f == flag) {
                    mark(start, piece_chars.len());
                }
                cursor = start + piece_chars.len();
            }
        }
    }
    idx
}

/// Wrap every `idx`-marked char of `text` in `BAD` red (a no-op when
/// color is off) and return the wrapped string plus the marked
/// positions shifted onto it. The marked char's displayed position is
/// recorded after the ANSI open codes are pushed, so carets land on
/// visible text, never inside an escape. See the render site for why
/// markers track the displayed string rather than real's pre-color
/// indices.
pub(crate) fn colorize_marked_spans(
    text: &str,
    idx: &std::collections::HashSet<usize>,
    color: &Colorizer,
) -> (String, std::collections::HashSet<usize>) {
    let (open, close) = color.wrap_codes("BAD");
    let mut out = String::new();
    let mut shifted = std::collections::HashSet::new();
    for (i, c) in text.chars().enumerate() {
        if idx.contains(&i) {
            out.push_str(&open);
            shifted.insert(out.chars().count());
            out.push(c);
            out.push_str(&close);
        } else {
            out.push(c);
        }
    }
    (out, shifted)
}

/// Arguments of [`slot_conflict_need_rebuild`].
pub(crate) struct SlotConflictNeedRebuildArgs<'a> {
    pub(crate) parent_cpv: &'a str,
    pub(crate) atom: &'a Atom,
    pub(crate) excluded: &'a [String],
    pub(crate) useoldpkg_atoms: &'a [String],
    pub(crate) usepkgonly: bool,
    pub(crate) repos: &'a [portage_repo::RepoConfig],
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) installed: &'a std::collections::HashSet<(String, String, String)>,
}

/// Real `slot_collision.py`'s `need_rebuild` scan (lines 407-458) for one
/// shown parent in a `slot` group: when the parent package is installed
/// and its atom is a *built* slot-operator atom (`cat/pkg:S/SS=` -- real
/// `Atom.slot_operator_built`; soname atoms can't reach this renderer,
/// see the `SlotConflictReason` doc comment), report why it cannot be
/// rebuilt -- `--exclude` / `--useoldpkg-atoms` match, or its ebuild
/// masked/unavailable (real `_equiv_ebuild_visible`). Under
/// `--usepkgonly` real stays quiet to avoid false positives. Returns the
/// reason, if any. `installed` holds every vdb `(cat, pkg, ver)`.
pub(crate) fn slot_conflict_need_rebuild(args: SlotConflictNeedRebuildArgs<'_>) -> Option<String> {
    let SlotConflictNeedRebuildArgs {
        parent_cpv,
        atom,
        excluded,
        useoldpkg_atoms,
        usepkgonly,
        repos,
        config,
        installed,
    } = args;
    if atom.slot_operator != Some(SlotOperator::Equals) || atom.sub_slot.is_none() {
        return None;
    }
    let (cp, slot_part) = parent_cpv.split_once(':')?;
    let (ccat, cpkg, cver) = portage_repo::split_cpv(cp)?;
    if !installed.contains(&(ccat.clone(), cpkg.clone(), cver.clone())) {
        return None;
    }
    let slot = slot_part.split('/').next().unwrap_or("");
    let against = if slot.is_empty() {
        format!("{ccat}/{cpkg}-{cver}")
    } else {
        format!("{ccat}/{cpkg}-{cver}:{slot}")
    };
    if excluded
        .iter()
        .any(|e| match_from_list(e, &[against.as_str()]).is_some_and(|m| !m.is_empty()))
    {
        return Some("matched by --exclude argument".to_string());
    }
    if useoldpkg_atoms
        .iter()
        .any(|e| match_from_list(e, &[against.as_str()]).is_some_and(|m| !m.is_empty()))
    {
        return Some("matched by --useoldpkg-atoms argument".to_string());
    }
    if usepkgonly {
        return None;
    }
    if !ebuild_visible_at(repos, &ccat, &cpkg, &cver, config) {
        return Some("ebuild is masked or unavailable".to_string());
    }
    None
}

/// Real `pkg_use_display(pkg, ...)`'s own return string -- `USE="…"`
/// followed by one ` VAR="…"` per non-hidden `USE_EXPAND` group (the
/// pairs `portage_repo::pkg_use_display_for` produced). Real always
/// renders a `USE=` field even when the package declares no flags
/// (`var_order.insert(0, "USE")` + the unconditional
/// `flag_displays.append`), so an empty slice still yields `USE=""`.
/// Colorization of the flag tokens (real `UseFlagDisplay.__str__`'s
/// `red`/`blue`) is a documented cut here -- it is a no-op under
/// `emerge -p` without `--color=y` anyway.
pub(crate) fn render_pkg_use_display(disp: &[(String, String)]) -> String {
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

/// Backlog #242 Slice D: real `Package.__str__`'s own quoted destination
/// (`Package.py:568-608`) for a scheduled-for-merge skipped-block node
/// whose OWN root is not `/` — ` to '<ROOT>'`, inside the paren, or empty
/// when that root is `/` (real's bare `/`-rooted missed line). `running`
/// is the row's `missed_targets_running_root` /
/// `consumer_targets_running_root`; the path comes from the display
/// roots (target `root`, `running_root` when the row rides it).
pub(crate) fn skipped_node_suffix(
    running: bool,
    root: &Path,
    running_root: Option<&Path>,
) -> String {
    let own: Option<&Path> = if running { running_root } else { Some(root) };
    match own {
        Some(r) if r != Path::new("/") => format!(" to '{}'", r.display()),
        _ => String::new(),
    }
}

/// Backlog #90 (S2), extended by #230: the `^` marker line under a
/// skip-conflict pin -- real `format_unmatched_atom`
/// (`_emerge/resolver/output.py:892`) marks the leading operator chars
/// plus the version token (`~` + `1.0` in `~dev-libs/whblocker-1.0`,
/// staged oracle), plus a `:slot[/sub-slot][op]` span when the atom
/// carries one that mismatches the missed package (`:967-975`). The
/// spans are computed on the raw atom text (no color realignment --
/// same deliberate divergence as the slot block's own markers).
/// USE-token spans stay a documented cut (no grounded case carries
/// USE-deps on a skipped row). `highlight_version` is real's
/// `highlight_version` (#270): the operator and version spans are only
/// marked when the missed package fails the atom's version part.
pub(crate) fn skip_conflict_caret_line(
    atom: &str,
    pkg_slot: &str,
    pkg_sub_slot: &str,
    highlight_version: bool,
) -> String {
    // Collect real's highlight spans first, then render once: spans may
    // abut (a version end meets its `:slot` start), so incremental
    // push-`^`-or-pad rendering would misplace the later one.
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let op_len: usize = atom
        .chars()
        .take_while(|c| matches!(c, '=' | '<' | '>' | '~' | '!'))
        .map(|c| c.len_utf8())
        .sum();
    if op_len > 0 && highlight_version {
        spans.push((0, op_len));
    }
    let parsed = portage_dep::parse_atom(atom);
    if highlight_version && let Some(ver) = parsed.as_ref().and_then(|a| a.version.clone()) {
        // Last occurrence: a package name itself may contain digits
        // (`foo-2-bar-1.0`), so anchoring on the first would mark the
        // wrong span. A trailing `-rN` revision is left out of the
        // span (no oracle covers it).
        if let Some(start) = atom.rfind(ver.as_str()) {
            spans.push((start, start + ver.len()));
        }
    }
    // Real's `highlight_slot`: the atom names a slot (or sub-slot) the
    // missed package does not have. The span covers `:slot`, the
    // `/sub-slot` half when present, and the trailing slot operator
    // (`=`/`*`), anchored on its occurrence in the raw atom text.
    if let Some(a) = parsed.as_ref() {
        let slot_mismatch = a.slot.as_deref().is_some_and(|s| s != pkg_slot);
        let sub_mismatch = a.sub_slot.as_deref().is_some_and(|s| s != pkg_sub_slot);
        if (slot_mismatch || sub_mismatch)
            && let Some(slot) = a.slot.as_deref()
        {
            let mut slot_str = format!(":{slot}");
            if let Some(sub) = a.sub_slot.as_deref() {
                slot_str.push('/');
                slot_str.push_str(sub);
            }
            match a.slot_operator {
                Some(portage_dep::SlotOperator::Star) => slot_str.push('*'),
                Some(portage_dep::SlotOperator::Equals) => slot_str.push('='),
                None => {}
            }
            if let Some(start) = atom.find(slot_str.as_str()) {
                spans.push((start, start + slot_str.len()));
            }
        }
    }
    let end = spans.iter().map(|(_, e)| *e).max().unwrap_or(0);
    let mut line = String::new();
    for i in 0..end {
        if spans.iter().any(|(s, e)| *s <= i && i < *e) {
            line.push('^');
        } else {
            line.push(' ');
        }
    }
    line
}
