//! Merge-list row attributes: columns, version decoration, USE suffix, sizes.

use crate::color::PkgprintArgs;
use crate::pretend::*;

/// `COLUMNWIDTH` from the environment (default 130); an unparsable value
/// warns and falls back to the default. Only the environment is read, not
/// `make.conf`.
/// Portage: output_helpers.py, MergeListItem
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

/// One `--columns` line: Portage's `_set_root_columns` merge-branch layout.
/// `category/package` (no version) pads to `columnwidth - 60`, `[version]`
/// to `columnwidth - 30`, then `oldbest`. Padding is skipped past the target
/// width, never truncating.
/// Portage: output.py, _set_root_columns
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
    // Portage's `_set_root_columns` merge branch: the type word and `pkg.cp`
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

/// Portage's `PkgAttrDisplay.__str__`: the fixed-width status field inside
/// the `[ebuild …]` bracket. One column per attribute, a space where absent:
/// `I`nteractive, `N`ew (`r` for force-reinstall), new `S`lot (`R` for
/// replace), `f`etch-restrict/`F`/remote-binary `g`, `U`pdate, `D`owngrade,
/// mask char (`#`/`~`/`*`). The mask column shows at `-p`/`-pv`, not under
/// `--quiet` (`include_mask`). Present letters are coloured; the field is 7
/// visible columns either way.
/// Portage: resolver/output_helpers.py, PkgAttrDisplay.__str__
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
    // The mask column shows only above verbosity 1 (`-p`/`-pv`, not `--quiet`).
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

/// Portage's `_create_use_string` per-flag colour, re-derived from an
/// already-rendered token's shape: plain `flag` is red, `-flag` blue,
/// `%`/`%*` yellow (newly in IUSE), lone `*` green (polarity flipped).
/// Only the core is coloured; markers and `( )` wrap stay plain.
/// Portage: output_helpers.py, _create_use_string
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

/// Arguments of [`use_suffix`].
pub(crate) struct UseSuffixArgs<'a> {
    pub(crate) entry: &'a GraphEntry,
    pub(crate) verbose: bool,
    pub(crate) quiet: bool,
    pub(crate) alphabetical: bool,
    pub(crate) color: &'a Colorizer,
}

/// Renders the ` USE="…"` (and `VAR="…"` per USE_EXPAND group) suffix.
/// Shown unless `--quiet` without `-v`. At `-pv`/`-pq` every flag renders;
/// at plain `-p` only changed flags render (for `New`, both are equal).
/// Portage: output.py, _display_use
pub(crate) fn use_suffix(args: UseSuffixArgs<'_>) -> String {
    let UseSuffixArgs {
        entry,
        verbose,
        quiet,
        alphabetical,
        color,
    } = args;
    // `print_use_string = verbosity != 1 or "--verbose"`.
    if quiet && !verbose {
        return String::new();
    }
    // `all_flags = verbosity == 3 or quiet`.
    let display = if verbose || quiet {
        &entry.use_expand_display
    } else {
        &entry.use_expand_display_p
    };
    if display.is_empty() {
        return String::new();
    }
    // `USE="…"` first, then one `VAR="…"` per non-hidden USE_EXPAND group.
    // Colour is applied per token *after* the sort, so `--alphabetical`
    // still sees plain tokens.
    //
    // Each group renders with its own trailing space; a single leading
    // space joins the display to the line.
    let groups: Vec<String> = display
        .iter()
        .map(|(name, rendered)| {
            let mut toks: Vec<&str> = rendered.split(' ').collect();
            if alphabetical {
                // Portage's combined-list sort is natural (`_alnum_sort_key`).
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
    // A single space joins the USE display to the line, which already ends
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

/// Pure constructor for the `[nomerge …]` row. `tail` carries the oldbest /
/// running-root / USE pieces (each empty when absent). The bracket pad is
/// `"nomerge".ljust(13)` plus `empty_space_in_brackets()`.
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

/// Portage's `darkgreen("to " + pkg.root)` suffix, per merge row: an entry
/// whose own root is not `/` is annotated with where it installs; a
/// `/`-rooted row stays bare. Returned bare (no leading space); the call
/// site reproduces Portage's spacing.
/// Portage: resolver/output.py
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

/// `math.ceil(num_bytes / 1024)` KiB -- always round up, so small files never
/// show `0 KiB`. Always `KiB`, never `MiB`/`GiB`. No `LC_NUMERIC` grouping.
/// Portage: portage.localization, localized_size
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
        // A removal is not a merge target.
        PretendOutcome::AlreadyInstalled { .. }
        | PretendOutcome::NoVisibleCandidate
        | PretendOutcome::Uninstall { .. } => None,
    }
}

/// The version the blocker parents text uses -- the version the owner's own
/// package line shows (for an installed, nomerge owner: the installed
/// version; for an absent-owner row: the removal's own cpv).
/// Portage: output.py, _blockers
pub(crate) fn entry_display_version(entry: &GraphEntry) -> &str {
    merge_bound_version(entry).unwrap_or(match &entry.outcome {
        PretendOutcome::AlreadyInstalled { version } => version,
        // An absent-owner nomerge row rides its own removal entry.
        PretendOutcome::Uninstall { version } => version,
        _ => "",
    })
}
