//! Merge-list row attributes: columns, version decoration, USE suffix, sizes.

use crate::color::PkgprintArgs;
use crate::pretend::*;

/// Prints one `[blocks]` line per conflict recorded on `entry` (see
/// `portage_repo::BlockerConflict`), right after its own `[ebuild ...]`
/// line -- purely informational, matching real `--pretend`'s "show what
/// would happen" spirit: v1 neither refuses nor changes the exit code for
/// a blocker match, strong or weak (see resolve_pretend_graph's doc
/// comment).
/// `  USE="flag1 -flag2" VIDEO_CARDS="-amdgpu nvidia"` (two leading
/// spaces, matching real `--pretend -v`'s own line format), or an empty
/// string when `--verbose` wasn't given or this entry has no displayable
/// flags at all. The `USE_EXPAND` grouping (plain `USE` group, then one
/// `VAR="…"` per non-hidden `USE_EXPAND` variable, empty groups omitted)
/// is real -- `output.py::_display_use`, computed in
/// `portage_repo::build_use_expand_display` and carried on
/// `GraphEntry::use_expand_display`. Still not shown: real portage's own
/// ANSI colorization and its installed-vs-new `*`/`%` diff markers
/// (a separate documented cut -- this shows the plain enabled/disabled
/// set, `_alnum_sort_key`-ordered within each group).
/// Real `output_helpers.py`'s own `columnwidth` resolution
/// (`MergeListItem.__init__`): 130 by default, overridden by a
/// `COLUMNWIDTH` setting -- portuale only ever reads it as a plain
/// environment variable (real portage's own `frozen_config.settings` is
/// env + `make.conf` + profile merged together; parsing `COLUMNWIDTH`
/// out of `make.conf` too would need a new generic scalar-lookup path
/// through `portage_profile::Config`, which nothing else in portuale
/// needs yet -- a deliberate v1 narrowing, same spirit as every other
/// scope cut in this codebase). An unparsable value warns and falls back
/// to the default, exactly like real portage's own `except ValueError`
/// branch, rather than treating it as a hard error. Real portage's own
/// warning has a first line echoing the raw exception text
/// (`f"!!! {e!s}\n"`) -- omitted here, same as every other parse-error
/// message in portuale (see `--deep`'s own invalid-value handling):
/// Rust's `ParseIntError` and Python's `ValueError` never stringify
/// identically, so echoing either verbatim would make this the one
/// message the two implementations could never agree on byte-for-byte.
pub(crate) fn columnwidth_from_env() -> i64 {
    match std::env::var("COLUMNWIDTH") {
        Ok(value) => match value.parse::<i64>() {
            Ok(width) => width,
            Err(_) => {
                eprintln!("!!! Unable to parse COLUMNWIDTH={value:?}");
                130
            }
        },
        Err(_) => 130,
    }
}

/// Arguments of [`columns_line`].
pub(crate) struct ColumnsLineArgs<'a> {
    pub(crate) bracket_word: &'a str,
    pub(crate) field: &'a str,
    pub(crate) indent: &'a str,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) version: &'a str,
    pub(crate) oldbest: &'a str,
    pub(crate) columnwidth: i64,
    pub(crate) color: &'a Colorizer,
    pub(crate) binary: bool,
    pub(crate) system: bool,
    pub(crate) world: bool,
}

/// One `--columns` line: real `_set_root_columns`'s own layout algorithm
/// (the `pkg_info.merge == True` branch only -- see this function's own
/// call sites' doc comments for why the "not merging" branch never
/// applies to any outcome portuale prints in brackets at all), with
/// color stripped for increment 1 (real's `nc_len`/plain `len()`
/// distinction collapses to just `len()` until increment 2 adds ANSI
/// color). `bracket`/`field` reproduce the exact same `"[{bracket}
/// {field}]"` segment the non-columns format prints -- `field` is the
/// full fixed-width `attr_display_field` -- only what comes after it
/// differs: `category/package` (no version -- that's the whole point of
/// `--columns`) padded out to `columnwidth - 60` (`newlp`), then
/// `[version]` right-padded to `columnwidth - 30` (`oldlp`), then
/// `oldbest` (`"[from]"` for an `Upgrade`/`Downgrade`, empty otherwise --
/// real `pkg_info.oldbest_list`, mirrored here via data portuale
/// already has rather than a new installed-candidate lookup). Padding is
/// skipped once the line's already past the target width, exactly like
/// real portage's own `if (newlp - nc_len(myprint)) > 0` guard -- never
/// truncates, just doesn't pad further.
pub(crate) fn columns_line(args: ColumnsLineArgs<'_>) -> String {
    let ColumnsLineArgs {
        bracket_word,
        field,
        indent,
        category,
        package,
        version,
        oldbest,
        columnwidth,
        color,
        binary,
        system,
        world,
    } = args;
    let newlp = (columnwidth - 60).max(0) as usize;
    let oldlp = (columnwidth - 30).max(0) as usize;
    // Real `_set_root_columns` merge branch: the type word and `pkg.cp`
    // both go through `pkgprint`; the version column is `green("[ver]")`;
    // `oldbest` arrives already `blue("[from]")` from `convert_myoldbest`.
    // Padding measures visible width (`color::nc_len`), never raw bytes.
    let cp = color.pkgprint(PkgprintArgs {
        text: &format!("{category}/{package}"),
        binary,
        system,
        world,
    });
    let bword = color.pkgprint(PkgprintArgs {
        text: bracket_word,
        binary,
        system,
        world,
    });
    let mut line = format!("[{bword} {field}] {indent}{cp}");
    let pad_to = |line: &mut String, target: usize| {
        let w = color::nc_len(line);
        if target > w {
            line.push_str(&" ".repeat(target - w));
        }
    };
    pad_to(&mut line, newlp);
    line.push_str(&format!(" {} ", color.c("green", &format!("[{version}]"))));
    pad_to(&mut line, oldlp);
    if !oldbest.is_empty() {
        line.push_str(&color.c("blue", oldbest));
    }
    line
}

/// Arguments of [`attr_display_field`].
pub(crate) struct AttrDisplayFieldArgs<'a> {
    pub(crate) interactive: bool,
    pub(crate) new: bool,
    pub(crate) force_reinstall: bool,
    pub(crate) new_slot: bool,
    pub(crate) replace: bool,
    pub(crate) fetch_restrict: bool,
    pub(crate) fetch_restrict_satisfied: bool,
    pub(crate) remote_binary: bool,
    pub(crate) new_version: bool,
    pub(crate) downgrade: bool,
    pub(crate) mask: Option<char>,
    pub(crate) include_mask: bool,
    pub(crate) color: &'a Colorizer,
}

/// Real `PkgAttrDisplay.__str__` (`_emerge/resolver/output_helpers.py`):
/// the fixed-width status field rendered inside the `[ebuild …]` bracket,
/// exactly `[{pkg.type_name} {attr_display}]`. One column per attribute,
/// a literal space where the attribute is absent, in this exact order:
///
/// 0. `I` -- `interactive` (`GraphEntry::interactive`).
/// 1. `N` -- `new`; `r` instead when `force_reinstall`. Portuale has no
///    `--emptytree`/`arg.force_reinstall` concept, so always `N` or space
///    here -- a plain reinstall shows `R` at col 2, see real
///    `_get_installed_best`: `replace=True` is set only when the exact cpv
///    is already installed.
/// 2. `S` -- `new_slot`; `R` instead when `replace` (the cpv is already
///    installed -- every one of portuale's `Reinstall` outcomes).
/// 3. `f`/`F`/`g` -- fetch-restrict satisfied / unsatisfied / remote binary
///    (`g` is out of scope, needs `--getbinpkg`).
/// 4. `U` -- `new_version` (an in-slot version change -- `Upgrade`/`Downgrade`).
/// 5. `D` -- `downgrade`.
/// 6. the mask column -- the `#`/`~`/`*` char from `gen_mask_str`
///    (`GraphEntry::keyword_mask`) or a space. Real `set_pkg_info` fills
///    it in only `if self.include_mask_str()` (`verbosity > 1`), and real
///    default `emerge -p` verbosity is *2* (`_DisplayConfig.__init__`:
///    `--quiet and 1 or --verbose and 3 or 2`) -- so the column is
///    present at plain `-p` *and* `-pv`, and absent (the field is 6 chars,
///    not 7) only under `--quiet` (verbosity 1). `include_mask` carries
///    that gate.
///
/// Each present letter is ANSI-coloured per real `PkgAttrDisplay.__str__`
/// (`green("N")`, `yellow("R")`, `turquoise("U")`, `blue("D")`,
/// `colorize("WARN", "I")`, the `#`/`*`/`~` mask via `BAD`/`WARN`, …)
/// when `color.enabled`; a space is never coloured. When colour is off
/// every call returns the bare char, so the field is exactly 7 visible
/// columns either way (`color::nc_len` recovers that width for
/// `--columns` padding).
pub(crate) fn attr_display_field(args: AttrDisplayFieldArgs<'_>) -> String {
    let AttrDisplayFieldArgs {
        interactive,
        new,
        force_reinstall,
        new_slot,
        replace,
        fetch_restrict,
        fetch_restrict_satisfied,
        remote_binary,
        new_version,
        downgrade,
        mask,
        include_mask,
        color,
    } = args;
    let col = |key: &str, ch: char| color.c(key, &ch.to_string());
    let mut f = String::new();
    f.push_str(&if interactive {
        col("WARN", 'I')
    } else {
        " ".to_string()
    });
    f.push_str(&if force_reinstall {
        col("red", 'r')
    } else if new {
        col("green", 'N')
    } else {
        " ".to_string()
    });
    f.push_str(&if replace {
        col("yellow", 'R')
    } else if new_slot {
        col("green", 'S')
    } else {
        " ".to_string()
    });
    f.push_str(&if fetch_restrict_satisfied {
        col("green", 'f')
    } else if fetch_restrict {
        col("red", 'F')
    } else if remote_binary {
        col("fuchsia", 'g')
    } else {
        " ".to_string()
    });
    f.push_str(&if new_version {
        col("turquoise", 'U')
    } else {
        " ".to_string()
    });
    f.push_str(&if downgrade {
        col("blue", 'D')
    } else {
        " ".to_string()
    });
    // Real `__str__` appends `self.mask` only `if self.mask is not None`,
    // and `set_pkg_info` sets it only `if self.include_mask_str()`
    // (`verbosity > 1`) -- true at real portage's default `emerge -p`
    // verbosity of 2, absent under `--quiet` (verbosity 1). Real
    // `gen_mask_str`: `#`/`*` -> `BAD` (red), `~` -> `WARN` (yellow), no
    // mark -> a space.
    if include_mask {
        f.push_str(&match mask {
            Some(c @ ('#' | '*')) => col("BAD", c),
            Some('~') => col("WARN", '~'),
            _ => " ".to_string(),
        });
    }
    f
}

/// The bare flag name inside a rendered `USE=` token, for the
/// `--alphabetical` re-sort: strip a leading `(` / `-` and any trailing
/// `)` / `*` / `%` (`(-maskflag)` -> `maskflag`, `foo%*` -> `foo`).
pub(crate) fn use_flag_sort_key(tok: &str) -> &str {
    tok.trim_start_matches('(')
        .trim_start_matches('-')
        .trim_end_matches([')', '*', '%'])
}

/// Real `_create_use_string`'s per-flag colour (`output_helpers.py:262-334`),
/// re-derived from an already-rendered token's shape -- the marker suffix
/// and sign fully determine it: a plain enabled `flag` is `red`, a plain
/// disabled `-flag` is `blue`, a `%`/`%*` marker means `yellow` (newly in
/// IUSE), a lone `*` means `green` (polarity flipped). Only the
/// `flag`/`-flag` core is coloured -- the `*`/`%` markers and any `( )`
/// forced/removed wrap stay plain, exactly as real (`yellow(flag) + "%*"`
/// etc). Known imperfection (no fixture reaches it, and it matches the
/// portuale's own `render_flag` `%`-collapse for forced flags): a forced
/// *disabled* flag newly added to IUSE on an Upgrade renders `(-flag)`
/// and is coloured `blue` here, where real portage would `yellow` it.
pub(crate) fn colorize_use_token(tok: &str, color: &Colorizer) -> String {
    let (open, inner, close) = match tok.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        Some(i) => ("(", i, ")"),
        None => ("", tok, ""),
    };
    let (core, markers) = if let Some(c) = inner.strip_suffix("%*") {
        (c, "%*")
    } else if let Some(c) = inner.strip_suffix('*') {
        (c, "*")
    } else if let Some(c) = inner.strip_suffix('%') {
        (c, "%")
    } else {
        (inner, "")
    };
    let key = match markers {
        "%*" | "%" => "yellow",
        "*" => "green",
        _ if core.starts_with('-') => "blue",
        _ => "red",
    };
    format!("{open}{}{markers}{close}", color.c(key, core))
}

/// Arguments of [`decorate_version`].
pub(crate) struct DecorateVersionArgs<'a> {
    pub(crate) version: &'a str,
    pub(crate) slot: &'a str,
    pub(crate) sub_slot: &'a str,
    pub(crate) repo: &'a str,
    pub(crate) show_slot: bool,
    pub(crate) force_sub_slot: bool,
}

/// Real `output.py::_append_slot` + `_append_repository`, which decorate
/// the bracket-line version *only at `emerge -pv`* (`verbosity == 3`):
/// `:slot` (plus `/sub_slot` when it differs from `slot`) and `::repo`.
/// `show_slot` carries real `_append_slot`'s own gate -- `new_slot`, or
/// any package involved in this line (the entry or one of its
/// `oldbest` refs) has a slot/sub-slot other than `0/0`. Real portage
/// omits `::repo` only under `--quiet-repo-display` (not modelled here --
/// its default is off, so `::repo` is always shown at `-pv`).
pub(crate) fn decorate_version(args: DecorateVersionArgs<'_>) -> String {
    let DecorateVersionArgs {
        version,
        slot,
        sub_slot,
        repo,
        show_slot,
        force_sub_slot,
    } = args;
    let mut s = String::from(version);
    if show_slot {
        s.push(':');
        s.push_str(slot);
        if slot != sub_slot || force_sub_slot {
            s.push('/');
            s.push_str(sub_slot);
        }
    }
    s.push_str("::");
    s.push_str(repo);
    s
}

/// Arguments of [`use_suffix`].
pub(crate) struct UseSuffixArgs<'a> {
    pub(crate) entry: &'a GraphEntry,
    pub(crate) verbose: bool,
    pub(crate) quiet: bool,
    pub(crate) alphabetical: bool,
    pub(crate) color: &'a Colorizer,
}

/// Renders the ` USE="…"` (and `VAR="…"` per USE_EXPAND group) suffix.
///
/// Real `_DisplayConfig`: `print_use_string = verbosity != 1 or
/// "--verbose"`, and real default `emerge -p` verbosity is 2 -- so the
/// USE line is *not* `-v`-gated, but `--quiet` (verbosity 1) *does*
/// suppress it entirely unless `-v` is also given (`-pvq`). What `-v`
/// (verbosity 3) *or* `--quiet` changes is `all_flags` (`= verbosity ==
/// 3 or quiet`), i.e. *which* flags render: `emerge -pv` / `-pq` / `-pvq`
/// use `GraphEntry::use_expand_display` (every flag, unchanged ones
/// plain, plus the `(-flag%)` removed-from-IUSE list); plain `emerge -p`
/// uses `use_expand_display_p`, where `_create_use_string` leaves an
/// *unchanged* flag omitted -- so for a `New` package (`is_new` renders
/// everything) the `-p` list equals the `-pv` list, and for a
/// `Reinstall`/`Upgrade`/`Downgrade` only the changed flags
/// (`flag%*`/`flag*`/`-flag%`/`-flag*`) show, often none.
pub(crate) fn use_suffix(args: UseSuffixArgs<'_>) -> String {
    let UseSuffixArgs {
        entry,
        verbose,
        quiet,
        alphabetical,
        color,
    } = args;
    // Real `print_use_string = verbosity != 1 or "--verbose"`.
    if quiet && !verbose {
        return String::new();
    }
    // Real `all_flags = verbosity == 3 or quiet`.
    let display = if verbose || quiet {
        &entry.use_expand_display
    } else {
        &entry.use_expand_display_p
    };
    if display.is_empty() {
        return String::new();
    }
    // Real `output.py:_display_use`: `USE="…"` first, then one `VAR="…"`
    // per non-hidden USE_EXPAND group, each already rendered and ordered
    // (enabled flags first, then disabled) by
    // `portage_repo::build_use_expand_display`. With `--alphabetical`,
    // real `_create_use_string` instead joins one combined list sorted
    // by bare flag name -- reproduced here by re-sorting each group's
    // already-rendered tokens. Colour (real `_create_use_string`'s own
    // `red`/`green`/`blue`/`yellow`) is applied per token *after* the
    // sort, so the `--alphabetical` sort key still sees plain tokens.
    //
    // Backlog #272: real `_create_use_string`
    // (`output_helpers.py:336-338`) returns `f'{name}="{ret}" '` -- one
    // trailing space per non-empty group, concatenated directly -- and
    // `print_messages` (`output.py:542-544`) joins the whole display
    // with a single leading space. Each group is therefore rendered
    // with its own trailing space below.
    let groups: Vec<String> = display
        .iter()
        .map(|(name, rendered)| {
            let mut toks: Vec<&str> = rendered.split(' ').collect();
            if alphabetical {
                // Real `_create_use_string`'s combined-list sort is
                // `_alnum_sort_key` (natural), same as the non-alphabetical
                // within-group sort.
                toks.sort_by_key(|t| portage_repo::alnum_sort_key(use_flag_sort_key(t)));
            }
            let body = toks
                .iter()
                .map(|t| colorize_use_token(t, color))
                .collect::<Vec<_>>()
                .join(" ");
            format!("{name}=\"{body}\" ")
        })
        .collect();
    // Real `print_messages`: `myprint += " " + self.verboseadd` -- a
    // single space joins the USE display to the line, which already ends
    // with the (possibly empty) `oldbest` slot's own trailing space.
    format!(" {}", groups.concat())
}

/// Arguments of [`nomerge_row`].
pub(crate) struct NomergeRowArgs<'a> {
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) disp_ver: &'a str,
    pub(crate) indent: &'a str,
    pub(crate) quiet: bool,
    pub(crate) tail: &'a str,
}

/// Pure constructor for the `[nomerge …]` row, so the ancestor arm's
/// exact rendering contract is unit-pinned below (#154). `tail` carries
/// the oldbest / running-root / USE pieces (each empty when absent --
/// a USE-less nomerge row is byte-identical to the pre-#154 rendering).
/// The bracket pad is real's `"nomerge".ljust(13)` plus
/// `empty_space_in_brackets()`; the `--quiet` narrowing is portuale's
/// established quiet rendering, pinned by the quiet contract tests.
pub(crate) fn nomerge_row(args: NomergeRowArgs<'_>) -> String {
    let NomergeRowArgs {
        category,
        package,
        disp_ver,
        indent,
        quiet,
        tail,
    } = args;
    let pad = if quiet { "      " } else { "       " };
    format!("[nomerge{pad}] {indent}{category}/{package}-{disp_ver}{tail}")
}

/// Real `lib/_emerge/resolver/output.py:841-862`'s own `darkgreen("to " +
/// pkg.root)` suffix, per merge row: an entry whose OWN root is not `/`
/// is annotated with where it actually installs — exactly as real portage
/// annotates any row whose own `pkg.root_config.settings["ROOT"] != "/"`
/// (backlog #242 Slice D S0 map
/// `LLM/evidence/2026-10-02-242-display-sites.md` §1a; the #206 cut
/// reopened per display site). Under a staged target `ROOT` every
/// target-rooted row carries `to <target>` and every running-rooted row
/// `to <running>`; a `/`-rooted row stays bare on both sides (real's
/// blk0x/cyc0y rows). Under `ROOT=/` everything is bare (L0). Empty
/// (defensively) when a running-rooted entry arrives with no running-root
/// path in hand.
/// Returned bare (`"to /"`, no leading space) -- real `output.py:856-861`
/// places it right after the always-present space that follows the
/// package string, with `oldbest` (when non-empty) getting its own
/// trailing space before it; `print_entry_line`'s own `emit` reproduces
/// that spacing.
pub(crate) fn root_suffix(
    entry: &GraphEntry,
    target_root: &Path,
    running_root: Option<&Path>,
) -> String {
    let own_root: Option<&Path> = if entry.targets_running_root {
        running_root
    } else {
        Some(target_root)
    };
    match own_root {
        Some(root) if root != Path::new("/") => format!("to {}", root.display()),
        _ => String::new(),
    }
}

/// Real `math.ceil(num_bytes / 1024)` KiB (`portage.localization.
/// localized_size`) -- "always round up, so that small files don't end
/// up as '0 KiB'". Real portage additionally applies `LC_NUMERIC`
/// thousands grouping to the KiB count; portuale doesn't -- only
/// observable above 999 KiB of downloads, which no fixture reaches, and
/// a locale-dependent separator would break the contract suite's
/// byte-exact determinism. Always `KiB`, never `MiB`/`GiB` (real
/// `localized_size` is the same -- its docstring: "The output will be in
/// kibibytes").
pub(crate) fn localized_size(bytes: u64) -> String {
    format!("{} KiB", bytes.div_ceil(1024))
}

/// The version `entry`'s own package line displays; `None` for the
/// non-merging outcomes. `merge_order`'s own `entry_version` is the same
/// mapping, but it is private to `portage-repo`.
pub(crate) fn merge_bound_version(entry: &GraphEntry) -> Option<&str> {
    match &entry.outcome {
        PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
            Some(version)
        }
        PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => Some(to),
        // #72 B3: a removal is not a merge target.
        PretendOutcome::AlreadyInstalled { .. }
        | PretendOutcome::NoVisibleCandidate
        | PretendOutcome::Uninstall { .. } => None,
    }
}

/// The version the blocker parents text uses -- real `_blockers`'
/// `pnode.cpv`, the version the owner's own package line shows (for an
/// installed, nomerge owner: the installed version).
pub(crate) fn entry_display_version(entry: &GraphEntry) -> &str {
    merge_bound_version(entry).unwrap_or(match &entry.outcome {
        PretendOutcome::AlreadyInstalled { version } => version,
        // #77 A1: an absent-owner nomerge row rides its own removal entry
        // (the removal target **is** the owner), so the parents text is
        // the removal's own cpv.
        PretendOutcome::Uninstall { version } => version,
        _ => "",
    })
}
