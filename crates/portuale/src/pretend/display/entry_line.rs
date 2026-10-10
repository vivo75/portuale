//! One merge-list row.

use crate::color::PkgprintArgs;
use crate::pretend::*;

/// Arguments of [`print_entry_line`].
pub(crate) struct PrintEntryLineArgs<'a> {
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) root: &'a Path,
    pub(crate) index: usize,
    pub(crate) indent: &'a str,
    /// #75 C1: real `set_pkg_info` (`output.py:612-614`): a merge node
    /// reached as a tree **ancestor** (`_ordered_tree_display`'s
    /// `add_parents` passes `ordered=False`) is re-labelled `nomerge` and
    /// rendered through `_set_no_columns`' non-merge arm, with no attr
    /// columns -- but WITH the USE display (`_display_use` has no
    /// ordered/merge gate; #154). Flat callers (and every occurrence the
    /// reversed display list reaches directly) pass `true`.
    pub(crate) ordered: bool,
    pub(crate) top_level_pkgs: &'a HashSet<(String, String)>,
    pub(crate) onlydeps: bool,
    pub(crate) oneshot: bool,
    pub(crate) verbose: bool,
    pub(crate) quiet: bool,
    pub(crate) alphabetical: bool,
    pub(crate) columns: bool,
    pub(crate) columnwidth: i64,
    pub(crate) running_root: Option<&'a Path>,
    pub(crate) color: &'a Colorizer,
    pub(crate) system_atoms: &'a [String],
    pub(crate) world_atoms: &'a [String],
    pub(crate) force_reinstall_cps: &'a HashSet<(String, String)>,
    pub(crate) blocker_lines: &'a mut Vec<String>,
    /// Masked-dependency disclosures for this resolve (real
    /// `_show_unsatisfied_dep`'s dependency block): the `NoVisibleCandidate`
    /// arm renders the recorded block in place of the bare
    /// `!!! no visible ebuild for dependency` line. Empty when nothing was
    /// masked-only (e.g. `--resume --pretend`, which never re-resolves).
    pub(crate) masked_deps: &'a [portage_repo::MaskedDepReport],
    /// `[use]`-unsatisfied dependency disclosures (backlog #20): real's
    /// precedence puts this block ahead of the masked one, and the
    /// `NoVisibleCandidate` arm below does the same.
    pub(crate) use_unsat_deps: &'a [portage_repo::UseUnsatDepReport],
    /// Plain-miss disclosures (#135 (d)): real's `emerge: there are no
    /// ebuilds to satisfy "<atom>"` + chain block, rendered last in the
    /// `NoVisibleCandidate` arm (after the two sibling blocks).
    pub(crate) plain_miss_deps: &'a [portage_repo::PlainMissDepReport],
    /// Backlog #206 S2: real `_show_circular_deps` forces `--verbose` for
    /// the stuck-remainder display, so every row carries its fetch size
    /// (`verbose_size`, `0 KiB` for an ebuild whose distfiles are already
    /// present) — while plain `-pv` rows keep portuale's established cut
    /// of omitting that bare `0 KiB` (see the `size_suffix` note below).
    /// True only for the circular re-display; every other caller passes
    /// false.
    pub(crate) force_sizes: bool,
    /// Backlog #221 (I5): the settling pass's recorded cycle edges,
    /// threaded into the trailing blocker disposition below (a wait chain
    /// through a demoted branch dispositions like the walk took it).
    pub(crate) circular: &'a HashMap<(bool, String, String), Vec<portage_repo::CircularDepChild>>,
}

/// One `GraphEntry`'s own display line, `indent` prepended right before
/// the category/package text (empty for flat mode, `print_tree`'s own
/// growing prefix for `--tree`) -- the exact same per-outcome
/// bracket/reason logic the flat loop always had, just factored out so
/// both display modes share one implementation rather than drifting
/// apart. `onlydeps`/`top_level_pkgs` decide suppression exactly as
/// before: a directly-requested top-level atom's own line (not its
/// dependencies) is hidden under `--onlydeps`, whatever its outcome.
/// `columns`/`columnwidth` switch the New/Upgrade/Downgrade/Reinstall
/// arms below to `columns_line`'s own layout instead of the default
/// inline `"...-version (upgrade from X)"` format -- see its own doc
/// comment. Never both `columns` and a non-empty `indent` at once: the
/// CLI layer refuses `--tree`+`--columns` together (the only source of a
/// non-empty `indent`), so `columns_line`'s own `indent` parameter is
/// always `""` in practice here, still threaded through for symmetry
/// with the non-columns arms.
///
/// `entries`/`index`/`root` are what the #68/#72 B2 blocker disposition
/// needs (see `blocker_row_disposition`): the entry's own trailing
/// blocker rows are filtered here, the inline ones are printed by the
/// caller right after this line.
pub(crate) fn print_entry_line(args: PrintEntryLineArgs<'_>) {
    let PrintEntryLineArgs {
        entries,
        root,
        index,
        indent,
        ordered,
        top_level_pkgs,
        onlydeps,
        oneshot,
        verbose,
        quiet,
        alphabetical,
        columns,
        columnwidth,
        running_root,
        color,
        system_atoms,
        world_atoms,
        force_reinstall_cps,
        blocker_lines,
        masked_deps,
        use_unsat_deps,
        plain_miss_deps,
        force_sizes,
        circular,
    } = args;
    let entry = &entries[index];
    // Real `_DisplayConfig` verbosity: `--quiet and 1 or --verbose and 3
    // or 2`. `--quiet` wins over `-v`. `v3` is "verbosity == 3" -- the
    // gate on `::repo`/`:slot` cpv decoration, the `Total:` line, and the
    // fetch-size suffix.
    let v3 = verbose && !quiet;
    let onlydeps_suppressed =
        onlydeps && top_level_pkgs.contains(&(entry.category.clone(), entry.package.clone()));
    // Real `Display.check_system_world` (`output.py`): `world` when this
    // package already matches an atom in `var/lib/portage/world`, OR
    // when it's a directly-requested target (a "favorite") that
    // `create_world_atom` would actually add -- i.e. NOT `--oneshot`/
    // `--onlydeps` (real `_DisplayConfig.oneshot`), and not an unslotted
    // `@system` member (real "unslotted system packages will not be
    // stored in world"). `system` = matches a `@system` atom (slot-
    // qualified `@system` atoms are matched version-only -- a cosmetic-
    // only miss, colour only). The full `create_world_atom` slot/repo/
    // virtual logic is a documented cut (see `update_world_file`).
    let binary = entry.source == portage_repo::CandidateSource::Binary;
    let is_favorite = top_level_pkgs.contains(&(entry.category.clone(), entry.package.clone()));
    let unslotted = entry.slot.as_deref().unwrap_or("0") == "0";
    let classify = |version: &str| -> (bool, bool) {
        let cpv = format!("{}/{}-{version}", entry.category, entry.package);
        let matches_any = |atoms: &[String]| {
            atoms
                .iter()
                .any(|a| match_from_list(a, &[cpv.as_str()]).is_some_and(|m| !m.is_empty()))
        };
        let system = matches_any(system_atoms);
        let would_add_to_world = is_favorite && !(oneshot || onlydeps) && !(system && unslotted);
        let world = would_add_to_world || matches_any(world_atoms);
        (system, world)
    };
    // Real `output.py:841-862`'s own `to <root>` annotation for a
    // non-`/`-rooted entry -- empty for every `/`-rooted one (see
    // `root_suffix`'s own doc comment). Placed right before `use_suffix`
    // in each arm below, matching real portage's own ordering
    // (`pkg_str + " " + oldbest + "to " + pkg.root`, with the USE display
    // coming later on the same line -- backlog #242 Slice D repaints a
    // running-rooted row's USE from the running profile, so the order
    // matters there too).
    let root_annotation = root_suffix(entry, root, running_root);
    // Real --pretend's own bracket word: literally `pkg.type_name`
    // (`lib/_emerge/RootConfig.py`'s own `pkg_tree_map`, the exact
    // two strings `"ebuild"`/`"binary"` portuale's own
    // `CandidateSource` mirrors) -- a binary merge prints
    // `"[binary"`, never `"[ebuild"`, regardless of outcome.
    let bracket = match entry.source {
        portage_repo::CandidateSource::Binary => "binary",
        portage_repo::CandidateSource::Ebuild => "ebuild",
    };
    // The fixed-width `attr_display` field flags this entry contributes,
    // shared by every merge outcome below (see `attr_display_field`).
    // `force_reinstall` (the red `r` column) is set when this `(cat, pkg)`
    // is on either side of a slot-operator ABI-rebuild edge -- real
    // `PkgAttrDisplay.force_reinstall`, the
    // `@__auto_slot_operator_replace_installed__` set yielded with
    // `force_reinstall=True`. `remote_binary` (the `g` column) is
    // `entry.remote_binary` -- real `attr_display.remote_binary =
    // pkg.remote` for a `--getbinpkg` binary not yet in `$PKGDIR`.
    let force_reinstall = force_reinstall_cps
        .contains(&(entry.category.clone(), entry.package.clone()))
        || matches!(
            entry.outcome,
            portage_repo::PretendOutcome::Reinstall {
                slot_operator_rebuild: true,
                ..
            }
        );
    let field = |new: bool, new_slot: bool, replace: bool, new_version: bool, downgrade: bool| {
        attr_display_field(AttrDisplayFieldArgs {
            interactive: entry.interactive,
            new,
            force_reinstall,
            new_slot,
            replace,
            fetch_restrict: entry.fetch_restrict && !entry.fetch_restrict_satisfied,
            fetch_restrict_satisfied: entry.fetch_restrict_satisfied,
            remote_binary: entry.remote_binary,
            new_version,
            downgrade,
            mask: entry.keyword_mask,
            include_mask: !quiet,
            color,
        })
    };
    // `emerge -pv` (verbosity 3) decorates the bracket cpv *and* every
    // `[old-ver]` with `:slot`/`::repo` (real `_append_slot` /
    // `_append_repository` / `convert_myoldbest`). `show_slot` is real
    // `_append_slot`'s own gate, computed once for the whole line.
    let entry_slot = entry.slot.as_deref().unwrap_or("0");
    let entry_sub = entry.sub_slot.as_deref().unwrap_or("0");
    let entry_repo = entry.repo_name.as_deref().unwrap_or("");
    let is_non00 = |s: &str, ss: &str| format!("{s}/{ss}") != "0/0";
    let show_slot = entry.new_slot
        || is_non00(entry_slot, entry_sub)
        || entry.oldbest.iter().any(|r| is_non00(&r.slot, &r.sub_slot));
    // The version as displayed in the bracket line: bare at `-p`,
    // `:slot::repo`-decorated at `-pv`. Real `output.py::_append_build_id`
    // runs first (before `_append_slot`/`_append_repository`): a
    // `type_name == "binary"` entry whose `Packages` index carried
    // `BUILD_ID` shows `<cpv>-<build_id>` (`GraphEntry::build_id`, `Some`
    // only for such a binary).
    let disp_version = |v: &str| -> String {
        let v = match &entry.build_id {
            Some(bid) => format!("{v}-{bid}"),
            None => v.to_string(),
        };
        if v3 {
            decorate_version(DecorateVersionArgs {
                version: &v,
                slot: entry_slot,
                sub_slot: entry_sub,
                repo: entry_repo,
                show_slot,
                force_sub_slot: false,
            })
        } else {
            v
        }
    };
    // Real `convert_myoldbest`: `blue("[" + ", ".join(versions) + "]")`,
    // each version `-r0`-stripped and (at `-pv`) `:slot::repo`-decorated
    // with *its own* slot/sub_slot/repo. Empty string when there's no
    // `oldbest` (a brand-new `New`, or a `Reinstall` whose version,
    // slot, sub-slot and repo all match the installed instance).
    let oldbest_str = || -> String {
        if entry.oldbest.is_empty() {
            return String::new();
        }
        let parts: Vec<String> = entry
            .oldbest
            .iter()
            .map(|r| {
                let v = r.version.strip_suffix("-r0").unwrap_or(&r.version);
                if v3 {
                    // Real `convert_myoldbest`'s non-`new_slot` branch
                    // appends the old instance's sub-slot on a *second*
                    // disjunct the entry's own `_append_slot` does not
                    // have: `old.slot == pkg.slot and old.sub_slot !=
                    // pkg.sub_slot`. Only reachable for an `oldbest`
                    // whose slot matches but whose sub-slot moved -- a
                    // `--changed-slot` / slot-operator reinstall
                    // (#24 S3 made `Reinstall` carry an `oldbest` at all).
                    let force_sub =
                        !entry.new_slot && r.slot == entry_slot && r.sub_slot != entry_sub;
                    decorate_version(DecorateVersionArgs {
                        version: v,
                        slot: &r.slot,
                        sub_slot: &r.sub_slot,
                        repo: &r.repo,
                        show_slot,
                        force_sub_slot: force_sub,
                    })
                } else {
                    v.to_string()
                }
            })
            .collect();
        format!("[{}]", parts.join(", "))
    };
    // Real `output.py:856-861`: `darkgreen("to " + pkg.root)`, shared by
    // the merge, `nomerge` and uninstall arms.
    let root_col = |r: &str| {
        if r.is_empty() {
            String::new()
        } else {
            color.c("darkgreen", r)
        }
    };
    // One merge line, shared by `New`/`Upgrade`/`Downgrade`/`Reinstall`.
    // Real `_set_no_columns`: `f"[{type} {attr}] {indent}{pkg_str}
    // {oldbest}"` -- the space before `oldbest` is always there even when
    // `oldbest` is empty. The running-root `to <root>` suffix (real
    // `output.py:856-861`) and the `USE="…"` display (real
    // `print_messages`' own `" " + verboseadd`) follow, each already
    // carrying its own leading space via `root_suffix`/`use_suffix`.
    let emit = |f: &str, version: &str| {
        if onlydeps_suppressed {
            return;
        }
        let (system, world) = classify(version);
        let disp_ver = disp_version(version);
        let oldbest = oldbest_str();
        let use_str = use_suffix(UseSuffixArgs {
            entry,
            verbose,
            quiet,
            alphabetical,
            color,
        });
        // Real `output.py::verbose_size` (`conf.verbosity == 3` only):
        // `verboseadd += localized_size(mysize)` -- the bytes still to
        // fetch, appended after the USE string. Portuale renders it
        // only for a `--getbinpkg` remote binary (the one case that's
        // ever non-zero here -- an ebuild's distfiles / a local `$PKGDIR`
        // binary are already present, so real would show a bare ` 0 KiB`
        // that portuale's `-pv` lines have always omitted; closing that
        // wider gap would re-pin every `-pv` assertion and is left out)
        // or under `force_sizes` (backlog #206 S2: the circular
        // re-display runs forced-verbose, so its rows carry real's
        // `0 KiB` like the live oracle).
        let size_suffix = if v3 && (entry.remote_binary || force_sizes) {
            let bytes: u64 = entry.download_files.iter().map(|(_, s)| s).sum();
            let size = localized_size(bytes);
            // Backlog #272: real `verbose_size` concatenates directly onto
            // the trailing-spaced USE string (`verboseadd +=
            // localized_size(mysize)`), so the size carries no leading
            // space of its own when a USE display precedes it
            // (`USE="x" 0 KiB`, one space). With no USE display the
            // leading space separates the size from the version slot.
            if use_str.is_empty() {
                format!(" {size}")
            } else {
                size
            }
        } else {
            String::new()
        };
        if columns {
            let root_str = if root_annotation.is_empty() {
                String::new()
            } else {
                format!(" {}", root_col(&root_annotation))
            };
            println!(
                "{}{root_str}{use_str}{size_suffix}",
                columns_line(ColumnsLineArgs {
                    bracket_word: bracket,
                    field: f,
                    indent,
                    category: &entry.category,
                    package: &entry.package,
                    version: &disp_ver,
                    oldbest: &oldbest,
                    columnwidth,
                    color,
                    binary,
                    system,
                    world
                })
            );
            return;
        }
        // Real `_set_no_columns`: `f"[{pkgprint(type)} {attr}]
        // {indent}{pkgprint(pkg_str)} {oldbest}"`.
        let bword = color.pkgprint(PkgprintArgs {
            text: bracket,
            binary,
            system,
            world,
        });
        let pkg_str = color.pkgprint(PkgprintArgs {
            text: &format!("{}/{}-{disp_ver}", entry.category, entry.package),
            binary,
            system,
            world,
        });
        let mut tail = String::from(" ");
        if !oldbest.is_empty() {
            tail.push_str(&color.c("blue", &oldbest));
        }
        if !root_annotation.is_empty() {
            if !oldbest.is_empty() {
                tail.push(' ');
            }
            tail.push_str(&root_col(&root_annotation));
        }
        tail.push_str(&use_str);
        tail.push_str(&size_suffix);
        println!("[{bword} {f}] {indent}{pkg_str}{tail}");
    };
    // #75 C1: real `_set_no_columns`' non-merge arm for a tree ancestor
    // occurrence of a merge node (`set_pkg_info` re-labels it `nomerge`,
    // `output.py:612-614`): `[<operation ljust 13><empty_space_in_brackets>]
    // <indent><cpv> <oldbest>` -- no attr columns and no size suffix
    // (`verbose_size` is merge-gated), but the `USE="…"` display IS
    // appended: `Display.__call__` runs `_display_use` for every package
    // with no ordered/merge gate, and `print_messages` appends
    // `" " + verboseadd` to the row (#154). The running-root suffix is
    // appended like every other row.
    let emit_nomerge = |version: &str| {
        let oldbest = oldbest_str();
        let mut tail = String::new();
        if !oldbest.is_empty() {
            tail.push(' ');
            tail.push_str(&color.c("blue", &oldbest));
        }
        if !root_annotation.is_empty() {
            // Real pastes `pkg_str + " " + oldbest + suffix`: the space
            // after the version is unconditional, so the suffix carries
            // its own leading space even when oldbest is empty (else the
            // version glues to `to`, e.g. `diamond-1.0to <root>`).
            tail.push(' ');
            tail.push_str(&root_col(&root_annotation));
        }
        // #154: the nomerge arm carries the same USE suffix as a merge
        // row (empty string when the package has no USE display, so
        // USE-less nomerge rows are byte-unchanged).
        tail.push_str(&use_suffix(UseSuffixArgs {
            entry,
            verbose,
            quiet,
            alphabetical,
            color,
        }));
        // `"nomerge".ljust(13)` + `empty_space_in_brackets()`.
        println!(
            "{}",
            nomerge_row(NomergeRowArgs {
                category: &entry.category,
                package: &entry.package,
                disp_ver: &disp_version(version),
                indent,
                quiet,
                tail: &tail
            })
        );
    };
    match &entry.outcome {
        PretendOutcome::New { version } => {
            // Real `_get_installed_best`: brand-new -> `attr.new`; into a
            // fresh slot while another slot is installed -> `attr.new`
            // *and* `attr.new_slot` (`GraphEntry::new_slot`). No oldbest
            // for a brand-new package; the other-slot version list real
            // portage shows for a new-slot install (`myoldbest =
            // installed_versions`) is deferred to a follow-up increment
            // (portuale doesn't carry the other-slot versions on the
            // entry yet).
            if !ordered {
                emit_nomerge(version);
                return;
            }
            emit(&field(true, entry.new_slot, false, false, false), version);
            blocker_lines.extend(trailing_blocker_lines(TrailingBlockerLinesArgs {
                entries,
                root,
                owner_index: index,
                include_mask: !quiet,
                columns,
                color,
                circular,
            }));
        }
        PretendOutcome::Upgrade { from: _, to } => {
            // Real: an in-slot version bump -> `attr.new_version` only
            // (the exact new cpv isn't installed, so `attr.replace`
            // stays clear -> `U`, no `R`). oldbest = the in-slot
            // installed version(s) (`myinslotlist`), from `entry.oldbest`.
            if !ordered {
                emit_nomerge(to);
                return;
            }
            emit(&field(false, false, false, true, false), to);
            blocker_lines.extend(trailing_blocker_lines(TrailingBlockerLinesArgs {
                entries,
                root,
                owner_index: index,
                include_mask: !quiet,
                columns,
                color,
                circular,
            }));
        }
        PretendOutcome::Downgrade { from: _, to } => {
            // Real: in-slot downgrade -> `attr.new_version` *and*
            // `attr.downgrade` (`U` and `D`). oldbest as for `Upgrade`.
            if !ordered {
                emit_nomerge(to);
                return;
            }
            emit(&field(false, false, false, true, true), to);
            blocker_lines.extend(trailing_blocker_lines(TrailingBlockerLinesArgs {
                entries,
                root,
                owner_index: index,
                include_mask: !quiet,
                columns,
                color,
                circular,
            }));
        }
        PretendOutcome::Reinstall {
            version,
            changed_flags: _,
            deps_changed: _,
            slot_changed: _,
            rebuilt_binary: _,
            new_repo: _,
            slot_operator_rebuild: _,
        } => {
            // Real `_get_installed_best`: the exact cpv is already
            // installed -> `attr.replace` (the yellow `R` at column 2),
            // and `myoldbest` stays empty for a same-slot/same-repo
            // reinstall -> no `[from]`. Real portage's `-pv` shows no
            // inline "why" for a reinstall at all -- portuale's former
            // `(reinstall for changed …)` prose is dropped here (the
            // USE diff still shows in the `USE="…"` section for
            // `--changed-use`; `--changed-deps`/`--changed-slot` reasons
            // are genuinely invisible in real `-pv` too).
            if !ordered {
                emit_nomerge(version);
                return;
            }
            emit(&field(false, false, true, false, false), version);
            blocker_lines.extend(trailing_blocker_lines(TrailingBlockerLinesArgs {
                entries,
                root,
                owner_index: index,
                include_mask: !quiet,
                columns,
                color,
                circular,
            }));
        }
        PretendOutcome::AlreadyInstalled { .. } => {
            // Nothing on stdout: real `emerge -p` / `-pu` simply omits an
            // already-satisfied package from the merge list, whether it
            // was reached as a dependency or requested directly (and
            // whether or not `@world`/`@system` pulled it in) -- it never
            // prints a per-package "is already installed" notice. Portuale
            // used to print one for a top-level atom; it polluted
            // `emerge -puD @world` (one line per up-to-date world member)
            // and matched nothing in real, so it was dropped. The
            // `already_installed` JSON status still carries the outcome
            // for `--json` consumers.
            //
            // #73: an already-installed *blocker owner* still contributes
            // its **unresolved** rows to real's trailing
            // `Display.blockers` group (`output.py:573-592`), which does
            // not depend on the owner printing a package line. Cell g/g3/
            // g4 are exactly that (installed `bparent` kept out of the
            // merge list, real still prints `[blocks B]`); the satisfied
            // `b` rows of such an owner are not part of the group (real
            // appends those inline on a *scheduled uninstall*, #72), so
            // only `unsolvable` trailing rows are collected here.
            for b in entry.blockers.iter().filter(|b| {
                // #84: only `unsolvable` rows reach this filter, and an
                // unresolved row never carries a `Replacement` arm
                // (`resolve_blockers` writes `satisfied_by: None`
                // whenever `unsolvable`), so the wait predicate cannot
                // fire here: the empty kept set is both sufficient and
                // free.
                b.unsolvable
                    && blocker_row_disposition(entries, root, index, b, &[])
                        == BlockerRowDisposition::Trailing
            }) {
                blocker_lines.push(format_blocker_row(
                    &format!(
                        "{}/{}-{}",
                        entry.category,
                        entry.package,
                        entry_display_version(entry)
                    ),
                    !quiet,
                    "",
                    color,
                    b,
                ));
            }
        }
        PretendOutcome::Uninstall { version } => {
            // Real `_set_no_columns`' non-merge bracket arm
            // (`output.py:514-529`) with `PKG_UNINSTALL` (`:285-286`):
            // `[uninstall     ] <cpv> `, one fewer pad space under `-q`
            // (verbosity 1 -> `empty_space_in_brackets()` is empty,
            // u5b), and skipped entirely under `--columns`
            // (`output.py:869`, u5a). `-pv` decorates the cpv like every
            // other row (`_append_slot`/`_append_repository`, `:523-524`);
            // a non-`/`-rooted entry carries real's `to <root>` like every
            // other row (`root_suffix`).
            if columns {
                return;
            }
            let pad = if quiet { "    " } else { "     " };
            let mut tail = String::from(" ");
            if !root_annotation.is_empty() {
                // `darkgreen("to " + pkg.root)`, same as `root_col` inside
                // the merge `emit` closure.
                tail.push_str(&color.c("darkgreen", &root_annotation));
            }
            println!(
                "[{}{pad}] {indent}{}{tail}",
                color.c("PKG_UNINSTALL", "uninstall"),
                color.c(
                    "PKG_UNINSTALL",
                    &format!(
                        "{}/{}-{}",
                        entry.category,
                        entry.package,
                        disp_version(version)
                    ),
                ),
            );
        }
        PretendOutcome::NoVisibleCandidate => {
            // USE-unsatisfied dependency disclosure (backlog #20, real
            // `_show_unsatisfied_dep`'s "no ebuilds built with USE flags
            // to satisfy" block): real renders this *before* the masked
            // block when both apply (`show_missing_use` wins over
            // `masked_packages`), so the lookup is ordered the same way.
            if let Some(report) = use_unsat_deps
                .iter()
                .find(|r| r.category == entry.category && r.package == entry.package)
            {
                eprintln!(
                    "\nemerge: there are no ebuilds built with USE flags to satisfy {:?}.",
                    report.atom
                );
                eprintln!(
                    "!!! One of the following packages is required to complete your request:"
                );
                for (cpv, reasons) in &report.rows {
                    eprintln!("- {cpv} ({})", reasons.join(", "));
                }
                for (node, ty) in &report.chain {
                    eprintln!("(dependency required by \"{node}\" [{ty}])");
                }
            } else if let Some(report) = masked_deps
                .iter()
                .find(|r| r.category == entry.category && r.package == entry.package)
            {
                eprintln!(
                    "\n!!! All ebuilds that could satisfy {:?} have been masked.",
                    report.atom
                );
                eprintln!(
                    "!!! One of the following masked packages is required to complete your request:"
                );
                for (cpv, reasons) in &report.masked {
                    eprintln!("- {cpv} (masked by: {})", reasons.join(", "));
                }
                eprintln!();
                for (node, ty) in &report.chain {
                    eprintln!("(dependency required by \"{node}\" [{ty}])");
                }
                eprintln!("For more information, see the MASKED PACKAGES section in the emerge");
                eprintln!("man page or refer to the Gentoo Handbook.");
            } else if let Some(report) = plain_miss_deps
                .iter()
                .find(|r| r.category == entry.category && r.package == entry.package)
            {
                // #135 (d): real `_show_unsatisfied_dep`'s plain-miss
                // block -- the unevaluated atom as queued plus the
                // `_get_dep_chain` rows -- in place of the bare line.
                // The staging `for <root>.` suffix stays absent (the
                // `fixture-miss-message-unsuffixed` class), matching
                // the top-level miss and the two sibling blocks above.
                eprintln!(
                    "\nemerge: there are no ebuilds to satisfy {:?}.",
                    report.atom
                );
                for (node, ty) in &report.chain {
                    eprintln!("(dependency required by \"{node}\" [{ty}])");
                }
            } else {
                eprintln!(
                    "!!! no visible ebuild for dependency \"{}/{}\"",
                    entry.category, entry.package
                );
            }
            // `--autounmask`'s own keyword-suggestion sub-feature,
            // extended to a dependency's own NoVisibleCandidate -- see
            // GraphEntry::keyword_suggestion's own doc comment.
            // Previously only a top-level atom's own fatal
            // NoVisibleCandidate got this note (as part of the Err
            // message that aborts the whole call).
            if let Some((version, keyword)) = &entry.keyword_suggestion {
                eprintln!(
                    "!!! note: {}/{}-{version} exists but is masked by KEYWORDS; \
                     --autounmask-keep-keywords=n suggests adding \"{}/{} {keyword}\" \
                     to package.accept_keywords",
                    entry.category, entry.package, entry.category, entry.package,
                );
            }
            // `--autounmask-use`'s own suggestion sub-feature -- see
            // GraphEntry::use_suggestion's own doc comment.
            if let Some((version, flip)) = &entry.use_suggestion {
                let adjustments: Vec<String> = flip
                    .iter()
                    .map(|(flag, enabled)| {
                        if *enabled {
                            flag.clone()
                        } else {
                            format!("-{flag}")
                        }
                    })
                    .collect();
                eprintln!(
                    "!!! note: {}/{}-{version} exists but its USE flags don't satisfy \
                     this atom; --autounmask-use suggests adding \"={}/{}-{version} {}\" \
                     to package.use",
                    entry.category,
                    entry.package,
                    entry.category,
                    entry.package,
                    adjustments.join(" "),
                );
            }
            // `--autounmask-use`'s own opt=-aware *parent* flip -- see
            // GraphEntry::parent_use_suggestion's own doc comment. When
            // that flip resolves the dep, `resolve_pretend_graph` applies
            // it and this entry is no longer NoVisibleCandidate, so this
            // arm is a fallback hint for the (currently unreachable) case
            // where the suggestion exists but wasn't applied.
            if let Some((parent_category, parent_package, parent_version, flip)) =
                &entry.parent_use_suggestion
            {
                let adjustments: Vec<String> = flip
                    .iter()
                    .map(|(flag, enabled)| {
                        if *enabled {
                            flag.clone()
                        } else {
                            format!("-{flag}")
                        }
                    })
                    .collect();
                eprintln!(
                    "!!! note: {parent_category}/{parent_package}-{parent_version}'s own USE \
                     flags need to change to satisfy this dependency; --autounmask-use \
                     suggests adding \"={parent_category}/{parent_package}-{parent_version} {}\" \
                     to package.use",
                    adjustments.join(" "),
                );
            }
        }
    }
}
