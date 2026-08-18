//! Merge-list row attributes: columns, version decoration, USE suffix, sizes.

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
