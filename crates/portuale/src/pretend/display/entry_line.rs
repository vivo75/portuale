//! One merge-list row.

use crate::color::PkgprintArgs;
use crate::pretend::*;
use portage_repo::MaskedDepReport;
use portage_repo::PlainMissDepReport;
use portage_repo::UseUnsatDepReport;

/// Arguments of [`print_entry_line`].
pub(crate) struct PrintEntryLineArgs<'a> {
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) root: &'a Path,
    pub(crate) index: usize,
    pub(crate) indent: &'a str,
    /// A merge node reached as a tree ancestor is re-labelled `nomerge` and
    /// rendered with no attr columns -- but WITH the USE display. Flat
    /// callers pass `true`.
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
    /// Masked-dependency disclosures for this resolve: the
    /// `NoVisibleCandidate` arm renders the recorded block in place of the
    /// bare line. Empty when nothing was masked-only.
    /// Portage: depgraph.py, _show_unsatisfied_dep
    pub(crate) masked_deps: &'a [portage_repo::MaskedDepReport],
    /// `[use]`-unsatisfied dependency disclosures, rendered ahead of the
    /// masked block.
    pub(crate) use_unsat_deps: &'a [portage_repo::UseUnsatDepReport],
    /// Plain-miss disclosures, rendered last in the `NoVisibleCandidate` arm.
    pub(crate) plain_miss_deps: &'a [portage_repo::PlainMissDepReport],
    /// Show fetch sizes even when they are `0 KiB`. True only for the
    /// circular re-display; every other caller passes false.
    pub(crate) force_sizes: bool,
    /// The settling pass's recorded cycle edges, for the trailing blocker
    /// disposition.
    pub(crate) circular: &'a HashMap<(bool, String, String), Vec<portage_repo::CircularDepChild>>,
}

/// One `GraphEntry`'s display line, shared by flat and `--tree` modes so they
/// cannot drift apart. A directly-requested top-level atom's own line is
/// hidden under `--onlydeps`, whatever its outcome. `columns` switches the
/// merge arms to `columns_line`'s layout (never combined with `--tree`).
/// The entry's own trailing blocker rows are filtered here; the inline ones
/// are printed by the caller right after this line.
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
    // Verbosity: `--quiet` wins over `-v`. `v3` gates `::repo`/`:slot`
    // decoration, the `Total:` line, and the fetch-size suffix.
    // Portage: output.py, _DisplayConfig
    let v3 = verbose && !quiet;
    let onlydeps_suppressed =
        onlydeps && top_level_pkgs.contains(&(entry.category.clone(), entry.package.clone()));
    // `world` when this package already matches a `world` atom, OR when it's
    // a directly-requested target `create_world_atom` would actually add
    // (not `--oneshot`/`--onlydeps`, not an unslotted `@system` member).
    // `system` = matches a `@system` atom. The full `create_world_atom`
    // slot/repo/virtual logic is cut (see `update_world_file`).
    // Portage: output.py, Display.check_system_world
    let binary = entry.source == portage_repo::CandidateSource::Binary;
    let is_favorite = top_level_pkgs.contains(&(entry.category.clone(), entry.package.clone()));
    let unslotted = entry.slot.as_deref().unwrap_or("0") == "0";
    // Placed right before `use_suffix` in each arm, matching Portage's own
    // ordering (`pkg_str + " " + oldbest + "to " + pkg.root`).
    let root_annotation = root_suffix(entry, root, running_root);
    // The bracket word is literally `pkg.type_name`: a binary merge prints
    // `"[binary"`, never `"[ebuild"`, regardless of outcome.
    // Portage: RootConfig.py, pkg_tree_map
    let bracket = match entry.source {
        portage_repo::CandidateSource::Binary => "binary",
        portage_repo::CandidateSource::Ebuild => "ebuild",
    };
    // `force_reinstall` (the red `r` column) is set when this `(cat, pkg)`
    // is on either side of a slot-operator ABI-rebuild edge.
    // Portage: output_helpers.py, PkgAttrDisplay.force_reinstall
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
    // At `-pv`, the bracket cpv *and* every `[old-ver]` gain `:slot`/`::repo`.
    // `show_slot` is computed once for the whole line.
    let entry_slot = entry.slot.as_deref().unwrap_or("0");
    let entry_sub = entry.sub_slot.as_deref().unwrap_or("0");
    let entry_repo = entry.repo_name.as_deref().unwrap_or("");
    let is_non00 = |s: &str, ss: &str| format!("{s}/{ss}") != "0/0";
    let show_slot = entry.new_slot
        || is_non00(entry_slot, entry_sub)
        || entry.oldbest.iter().any(|r| is_non00(&r.slot, &r.sub_slot));
    // The displayed version: bare at `-p`, `:slot::repo`-decorated at `-pv`.
    // A binary whose index carried `BUILD_ID` shows `<cpv>-<build_id>`.
    // `blue("[" + versions + "]")`, each `-r0`-stripped and (at `-pv`)
    // `:slot::repo`-decorated with its own slot/sub_slot/repo. Empty with
    // no `oldbest`.
    // `darkgreen("to " + pkg.root)`, shared by the merge, `nomerge` and
    // uninstall arms.
    // One merge line, shared by `New`/`Upgrade`/`Downgrade`/`Reinstall`.
    // The space before `oldbest` is always there, even when empty; the
    // `to <root>` suffix and USE display each carry their own leading space.
    // Portage: output.py, _set_no_columns
    // The non-merge arm for a tree ancestor occurrence: no attr columns and
    // no size suffix, but the USE display IS appended, like every other row.
    // Portage: output.py, _set_no_columns
    let ctx = EntryLineCtx {
        entry,
        system_atoms,
        world_atoms,
        is_favorite,
        oneshot,
        onlydeps,
        unslotted,
        quiet,
        color,
        v3,
        entry_slot,
        entry_sub,
        entry_repo,
        show_slot,
        verbose,
        alphabetical,
        root_annotation: &root_annotation,
        bracket,
        onlydeps_suppressed,
        force_sizes,
        columns,
        indent,
        columnwidth,
        binary,
    };
    match &entry.outcome {
        PretendOutcome::New { version } => {
            // Brand-new -> `attr.new`; into a fresh slot while another slot
            // is installed -> `attr.new` *and* `attr.new_slot`. No oldbest
            // for a brand-new package.
            // Portage: output.py, _get_installed_best
            if !ordered {
                ctx.emit_nomerge(version);
                return;
            }
            ctx.emit(&field(true, entry.new_slot, false, false, false), version);
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
            // An in-slot version bump -> `attr.new_version` only (`U`, no
            // `R`); oldbest = the in-slot installed version(s).
            if !ordered {
                ctx.emit_nomerge(to);
                return;
            }
            ctx.emit(&field(false, false, false, true, false), to);
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
            // In-slot downgrade -> `attr.new_version` *and* `attr.downgrade`.
            if !ordered {
                ctx.emit_nomerge(to);
                return;
            }
            ctx.emit(&field(false, false, false, true, true), to);
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
            // The exact cpv is already installed -> `attr.replace` (yellow `R`),
            // with an empty `myoldbest`. No inline reinstall "why" at `-pv`.
            if !ordered {
                ctx.emit_nomerge(version);
                return;
            }
            ctx.emit(&field(false, false, true, false, false), version);
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
            print_already_installed_blockers(PrintAlreadyInstalledBlockersArgs {
                entry,
                entries,
                root,
                index,
                blocker_lines,
                quiet,
                color,
            });
        }
        PretendOutcome::Uninstall { version } => {
            print_uninstall_row(PrintUninstallRowArgs {
                columns,
                quiet,
                root_annotation: &root_annotation,
                color,
                entry,
                ctx: &ctx,
                version,
                indent,
            });
        }
        PretendOutcome::NoVisibleCandidate => {
            print_no_visible_candidate(use_unsat_deps, entry, masked_deps, plain_miss_deps);
        }
    }
}

/// The values [`print_uninstall_row`] reads from its caller.
pub(crate) struct PrintUninstallRowArgs<'a> {
    pub(crate) columns: bool,
    pub(crate) quiet: bool,
    pub(crate) root_annotation: &'a str,
    pub(crate) color: &'a Colorizer,
    pub(crate) entry: &'a GraphEntry,
    pub(crate) ctx: &'a EntryLineCtx<'a>,
    pub(crate) version: &'a String,
    pub(crate) indent: &'a str,
}

/// Print the row of a package that is being uninstalled (not shown under --columns).
pub(crate) fn print_uninstall_row(args: PrintUninstallRowArgs<'_>) {
    let PrintUninstallRowArgs {
        columns,
        quiet,
        root_annotation,
        color,
        entry,
        ctx,
        version,
        indent,
    } = args;
    // The non-merge bracket arm with `PKG_UNINSTALL`: one fewer pad
    // space under `-q`, skipped entirely under `--columns`. `-pv`
    // decorates the cpv like every other row, with `to <root>` ditto.
    // Portage: output.py, _set_no_columns
    if columns {
        return;
    }
    let pad = if quiet { "    " } else { "     " };
    let mut tail = String::from(" ");
    if !root_annotation.is_empty() {
        tail.push_str(&color.c("darkgreen", root_annotation));
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
                ctx.disp_version(version)
            ),
        ),
    );
}

/// The values [`print_already_installed_blockers`] reads from its caller.
pub(crate) struct PrintAlreadyInstalledBlockersArgs<'a> {
    pub(crate) entry: &'a GraphEntry,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) root: &'a Path,
    pub(crate) index: usize,
    pub(crate) blocker_lines: &'a mut Vec<String>,
    pub(crate) quiet: bool,
    pub(crate) color: &'a Colorizer,
}

/// Queue the trailing blocker rows of an already-installed blocker owner.
pub(crate) fn print_already_installed_blockers(args: PrintAlreadyInstalledBlockersArgs<'_>) {
    let PrintAlreadyInstalledBlockersArgs {
        entry,
        entries,
        root,
        index,
        blocker_lines,
        quiet,
        color,
    } = args;
    // Nothing on stdout: an already-satisfied package is simply
    // omitted from the merge list. The `already_installed` JSON
    // status still carries the outcome for `--json` consumers.
    //
    // An already-installed *blocker owner* still contributes its
    // **unresolved** rows to the trailing `Display.blockers` group,
    // so only `unsolvable` trailing rows are collected here.
    // Portage: output.py, Display.blockers
    for b in entry.blockers.iter().filter(|b| {
        // Only `unsolvable` rows reach this filter, and an unresolved
        // row never carries a `Replacement` arm, so the wait predicate
        // cannot fire: the empty kept set is sufficient and free.
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

/// Print the NoVisibleCandidate row with its masked, USE and keyword disclosures.
pub(crate) fn print_no_visible_candidate(
    use_unsat_deps: &[UseUnsatDepReport],
    entry: &GraphEntry,
    masked_deps: &[MaskedDepReport],
    plain_miss_deps: &[PlainMissDepReport],
) {
    // The USE-unsatisfied block renders before the masked block when
    // both apply, so the lookup is ordered the same way.
    // Portage: depgraph.py, _show_unsatisfied_dep
    if let Some(report) = use_unsat_deps
        .iter()
        .find(|r| r.category == entry.category && r.package == entry.package)
    {
        eprintln!(
            "\nemerge: there are no ebuilds built with USE flags to satisfy {:?}.",
            report.atom
        );
        eprintln!("!!! One of the following packages is required to complete your request:");
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
        eprintln!("!!! One of the following masked packages is required to complete your request:");
        for (cpv, reasons) in &report.masked {
            eprintln!("- {cpv} (masked by: {})", reasons.join(", "));
        }
        if report
            .masked
            .iter()
            .any(|(_, reasons)| reasons.iter().any(|r| r.starts_with("EAPI ")))
        {
            eprint!("{}", portage_repo::eapi_mask_upgrade_notice());
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
        // The plain-miss block: the unevaluated atom as queued plus the
        // `_get_dep_chain` rows, in place of the bare line.
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
    // The `--autounmask` keyword suggestion for a dependency's own
    // NoVisibleCandidate (see `GraphEntry::keyword_suggestion`).
    if let Some((version, keyword)) = &entry.keyword_suggestion {
        eprintln!(
            "!!! note: {}/{}-{version} exists but is masked by KEYWORDS; \
                     --autounmask-keep-keywords=n suggests adding \"{}/{} {keyword}\" \
                     to package.accept_keywords",
            entry.category, entry.package, entry.category, entry.package,
        );
    }
    // The `--autounmask-use` suggestion (see `GraphEntry::use_suggestion`).
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
    // The opt-aware *parent* flip (see
    // `GraphEntry::parent_use_suggestion`); a fallback hint for when
    // the suggestion exists but wasn't applied.
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

/// The values the line-printing helpers of [`print_entry_line`] borrow from it.
pub(crate) struct EntryLineCtx<'a> {
    entry: &'a GraphEntry,
    system_atoms: &'a [String],
    world_atoms: &'a [String],
    is_favorite: bool,
    oneshot: bool,
    onlydeps: bool,
    unslotted: bool,
    quiet: bool,
    color: &'a Colorizer,
    v3: bool,
    entry_slot: &'a str,
    entry_sub: &'a str,
    entry_repo: &'a str,
    show_slot: bool,
    verbose: bool,
    alphabetical: bool,
    root_annotation: &'a str,
    bracket: &'a str,
    onlydeps_suppressed: bool,
    force_sizes: bool,
    columns: bool,
    indent: &'a str,
    columnwidth: i64,
    binary: bool,
}

impl EntryLineCtx<'_> {
    fn classify(&self, version: &str) -> (bool, bool) {
        let &EntryLineCtx {
            entry,
            system_atoms,
            world_atoms,
            is_favorite,
            oneshot,
            onlydeps,
            unslotted,
            ..
        } = self;
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
    }

    fn disp_version(&self, v: &str) -> String {
        let &EntryLineCtx {
            entry,
            v3,
            entry_slot,
            entry_sub,
            entry_repo,
            show_slot,
            ..
        } = self;
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
    }

    fn oldbest_str(&self) -> String {
        let &EntryLineCtx {
            entry,
            v3,
            entry_slot,
            entry_sub,
            show_slot,
            ..
        } = self;
        if entry.oldbest.is_empty() {
            return String::new();
        }
        let parts: Vec<String> = entry
            .oldbest
            .iter()
            .map(|r| {
                let v = r.version.strip_suffix("-r0").unwrap_or(&r.version);
                if v3 {
                    // Also append the old sub-slot when only it moved
                    // (`old.slot == pkg.slot and old.sub_slot != pkg.sub_slot`).
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
    }

    fn root_col(&self, r: &str) -> String {
        let &EntryLineCtx { color, .. } = self;
        if r.is_empty() {
            String::new()
        } else {
            color.c("darkgreen", r)
        }
    }

    fn emit(&self, f: &str, version: &str) {
        let &EntryLineCtx {
            entry,
            quiet,
            color,
            v3,
            verbose,
            alphabetical,
            root_annotation,
            bracket,
            onlydeps_suppressed,
            force_sizes,
            columns,
            indent,
            columnwidth,
            binary,
            ..
        } = self;
        if onlydeps_suppressed {
            return;
        }
        let (system, world) = self.classify(version);
        let disp_ver = self.disp_version(version);
        let oldbest = self.oldbest_str();
        let use_str = use_suffix(UseSuffixArgs {
            entry,
            verbose,
            quiet,
            alphabetical,
            color,
        });
        // The fetch size renders only for a `--getbinpkg` remote binary or
        // under `force_sizes` (the circular re-display runs forced-verbose).
        // Portage: output.py, verbose_size
        let size_suffix = if v3 && (entry.remote_binary || force_sizes) {
            let bytes: u64 = entry.download_files.iter().map(|(_, s)| s).sum();
            let size = localized_size(bytes);
            // The USE string already ends with a space, so the size carries
            // its own leading space only without one.
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
                format!(" {}", self.root_col(root_annotation))
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
            tail.push_str(&self.root_col(root_annotation));
        }
        tail.push_str(&use_str);
        tail.push_str(&size_suffix);
        println!("[{bword} {f}] {indent}{pkg_str}{tail}");
    }

    fn emit_nomerge(&self, version: &str) {
        let &EntryLineCtx {
            entry,
            quiet,
            color,
            verbose,
            alphabetical,
            root_annotation,
            indent,
            ..
        } = self;
        let oldbest = self.oldbest_str();
        let mut tail = String::new();
        if !oldbest.is_empty() {
            tail.push(' ');
            tail.push_str(&color.c("blue", &oldbest));
        }
        if !root_annotation.is_empty() {
            // The space after the version is unconditional, so the suffix
            // carries its own leading space even when oldbest is empty.
            tail.push(' ');
            tail.push_str(&self.root_col(root_annotation));
        }
        // The nomerge arm carries the same USE suffix as a merge row.
        tail.push_str(&use_suffix(UseSuffixArgs {
            entry,
            verbose,
            quiet,
            alphabetical,
            color,
        }));
        println!(
            "{}",
            nomerge_row(NomergeRowArgs {
                category: &entry.category,
                package: &entry.package,
                disp_ver: &self.disp_version(version),
                indent,
                quiet,
                tail: &tail
            })
        );
    }
}
