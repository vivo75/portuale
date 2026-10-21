//! The autounmask suggestion blocks and the --autounmask-only / --autounmask-continue exits.

use crate::pretend::*;
use portage_profile::Config;
use portage_repo::GraphResult;
use portage_repo::RepoConfig;

/// The values [`show_autounmask`] reads from its caller.
pub(crate) struct ShowAutounmaskArgs<'a> {
    pub(crate) opts: &'a EmergeOpts<'a>,
    pub(crate) color: &'a Colorizer,
    pub(crate) result: &'a GraphResult,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) root: &'a Path,
    pub(crate) config: &'a Config,
    pub(crate) has_autounmask_changes: bool,
}

/// The autounmask suggestion blocks and the --autounmask-only / --autounmask-continue exits.
pub(crate) fn show_autounmask(args: ShowAutounmaskArgs<'_>) -> Option<ExitCode> {
    let ShowAutounmaskArgs {
        opts,
        color,
        result,
        repos,
        root,
        config,
        has_autounmask_changes,
    } = args;
    let &EmergeOpts {
        autounmask_only,
        pretend,
        autounmask_continue,
        ..
    } = opts;
    // The `unstable_keyword_msg` half: `--autounmask` accepted a
    // `KEYWORDS`-alone mask to make the graph resolve, so the implicit
    // `package.accept_keywords` change is reported after the merge list.
    // Portage: depgraph.py, _display_autounmask
    let print_autounmask_block =
        |reason: &str, file: &str, changes: &[portage_repo::AutounmaskChange]| {
            if changes.is_empty() {
                return;
            }
            eprintln!(
                "\nThe following {} are necessary to proceed:",
                color.c("BAD", reason)
            );
            eprintln!(" (see \"{file}\" in the portage(5) man page for more details)");
            for change in changes {
                for line in &change.dep_chain {
                    eprintln!("# {line}");
                }
                let line = if change.token.is_empty() {
                    change.atom.clone()
                } else {
                    format!("{} {}", change.atom, change.token)
                };
                eprintln!("{}", color.c("INFORM", &line));
            }
        };
    // Block order: keyword, mask, USE, license.
    print_autounmask_block(
        "keyword changes",
        "package.accept_keywords",
        &result.autounmask_keyword_changes,
    );
    print_autounmask_block(
        "mask changes",
        "package.unmask",
        &result.autounmask_mask_changes,
    );
    print_autounmask_block("USE changes", "package.use", &result.autounmask_use_changes);
    print_autounmask_block(
        "license changes",
        "package.license",
        &result.autounmask_license_changes,
    );

    // Portage prints this after every change block once a `package.unmask`
    // change was proposed (`p_mask_change_msg` non-empty; portuale never
    // suggests `**` keywords, so the `masked_by_missing_keywords` half of
    // Portage's condition can't fire). Suppressed under `--quiet`.
    // Portage: depgraph.py, _display_autounmask
    if !opts.quiet && !result.autounmask_mask_changes.is_empty() {
        eprintln!();
        eprintln!(
            "{}",
            color.c(
                "INFORM",
                "NOTE: The --autounmask-keep-masks option will prevent emerge"
            )
        );
        eprintln!(
            "{}",
            color.c(
                "INFORM",
                "      from creating package.unmask or ** keyword changes."
            )
        );
    }

    // `--autounmask-only` returns 0 right after the problem display;
    // nothing below runs, but the end notice still prints.
    // Portage: actions.py, action_build
    if autounmask_only {
        pretend_end_news_notice(pretend, repos, root, config, color);
        return Some(ExitCode::SUCCESS);
    }

    // With autounmask changes pending, the run fails (exit 1) after the
    // problem display -- under `--pretend` too -- unless a non-pretend
    // `--autounmask-continue` writes the changes and proceeds.
    // Portage: actions.py, action_build
    let autounmask_continue_active = !pretend && autounmask_continue == Some(true);

    // With `--autounmask-backtrack` off (the default), the resolver stops
    // after the first autounmask batch instead of re-driving, and says so.
    // Portage: depgraph.py, _display_autounmask
    if result.autounmask_backtrack_disabled(config.autounmask_backtrack) {
        eprintln!();
        for line in [
            "In order to avoid wasting time, backtracking has terminated early",
            "due to the above autounmask change(s). The --autounmask-backtrack=y",
            "option can be used to force further backtracking, but there is no",
            "guarantee that it will produce a solution.",
        ] {
            eprintln!(" {} {line}", color.c("WARN", "*"));
        }
    }

    if has_autounmask_changes && !autounmask_continue_active {
        pretend_end_news_notice(pretend, repos, root, config, color);
        return Some(ExitCode::from(1));
    }
    None
}
