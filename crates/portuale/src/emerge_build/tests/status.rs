use super::*;

/// Backlog #177: the merge progress counters -- real
/// `Scheduler._pkg_count` (`Scheduler.py:296-304`): only merging
/// outcomes count, and each merging entry's `cur` is its 1-based
/// ordinal in merge order. The map is positional (`out[i]` belongs to
/// `entries[i]`), so two merging entries for the same cat/pkg -- two
/// slots of one package in a single run -- keep their own `cur`
/// (review of #177). Grounded in the cited real source (there
/// is no Python mirror); the `testrepo` repo name is this module's
/// own `source_entry` fixture value.
#[test]
fn merge_progress_map_counts_only_merging_entries_in_order() {
    let mut slot1 = source_entry(
        "same-pkg",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    slot1.slot = Some("1".into());
    let mut slot2 = source_entry(
        "same-pkg",
        PretendOutcome::Upgrade {
            from: "0.9".into(),
            to: "1.0".into(),
        },
    );
    slot2.slot = Some("2".into());
    let entries = vec![
        slot1,
        source_entry(
            "b-pkg",
            PretendOutcome::AlreadyInstalled {
                version: "1.0".into(),
            },
        ),
        slot2,
        source_entry(
            "d-pkg",
            PretendOutcome::Uninstall {
                version: "1.0".into(),
            },
        ),
    ];
    let map = merge_progress_map(&entries);
    assert_eq!(map.len(), 4);
    // The two same-cat/pkg entries print their own `cur`.
    assert_eq!(map[0], mrg_director::MergeProgress { cur: 1, max: 2 });
    assert_eq!(map[2], mrg_director::MergeProgress { cur: 2, max: 2 });
    // Non-merging entries carry the never-printed placeholder.
    assert_eq!(map[1], mrg_director::MergeProgress::single());
    assert_eq!(map[3], mrg_director::MergeProgress::single());
    // And the lines really read `(1 of 2)` / `(2 of 2)`.
    let plain = crate::color::Colorizer::new(false);
    let root = std::path::Path::new("/");
    assert_eq!(
        emerging_line(&entries[0], "1.0", map[0], root, &plain),
        ">>> Emerging (1 of 2) dev-libs/same-pkg-1.0::testrepo"
    );
    assert_eq!(
        emerging_line(&entries[2], "1.0", map[2], root, &plain),
        ">>> Emerging (2 of 2) dev-libs/same-pkg-1.0::testrepo"
    );
}

/// Backlog #177: the exact progress-line strings -- real
/// `_emerge/MergeListItem.py::_start` (`MergeListItem.py:60-85`) for
/// `Emerging`/`Emerging binary`, real `_emerge/PackageMerge.py`
/// (`PackageMerge.py:32-79`) for `Installing`/`Completed`. Colour
/// escapes are `color.rs`'s port of `output.py:68-92`
/// (`MERGE_LIST_PROGRESS` = yellow `\x1b[33;01m`, `PKG_MERGE` =
/// darkgreen `\x1b[32m`, `PKG_BINARY_MERGE` = purple `\x1b[35m`,
/// reset `\x1b[39;49;00m`); `ROOT != "/"` appends ` for {root}`
/// (Emerging) vs ` to {root}` (Installing/Completed).
#[test]
fn merge_progress_lines_match_portage_exact_text() {
    use std::path::Path;
    let plain = crate::color::Colorizer::new(false);
    let root = Path::new("/");
    let two_of_three = mrg_director::MergeProgress { cur: 2, max: 3 };
    let a = source_entry(
        "a-pkg",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    let mut b = source_entry(
        "b-pkg",
        PretendOutcome::New {
            version: "2.0".into(),
        },
    );
    b.source = CandidateSource::Binary;

    // Source, colour off, `ROOT == "/"`: the bare shape.
    assert_eq!(
        emerging_line(&a, "1.0", two_of_three, root, &plain),
        ">>> Emerging (2 of 3) dev-libs/a-pkg-1.0::testrepo"
    );
    // Binary: `Emerging binary`, same counters.
    assert_eq!(
        emerging_line(&b, "2.0", two_of_three, root, &plain),
        ">>> Emerging binary (2 of 3) dev-libs/b-pkg-2.0::testrepo"
    );
    assert_eq!(
        installing_line(&a, "1.0", two_of_three, root, &plain),
        ">>> Installing (2 of 3) dev-libs/a-pkg-1.0::testrepo"
    );
    assert_eq!(
        completed_line(&a, "1.0", two_of_three, root, &plain),
        ">>> Completed (2 of 3) dev-libs/a-pkg-1.0::testrepo"
    );

    // `ROOT != "/"`: `for` on Emerging, `to` on Installing/Completed.
    let alt = Path::new("/altroot");
    assert_eq!(
        emerging_line(&a, "1.0", two_of_three, alt, &plain),
        ">>> Emerging (2 of 3) dev-libs/a-pkg-1.0::testrepo for /altroot"
    );
    assert_eq!(
        installing_line(&a, "1.0", two_of_three, alt, &plain),
        ">>> Installing (2 of 3) dev-libs/a-pkg-1.0::testrepo to /altroot"
    );
    assert_eq!(
        completed_line(&a, "1.0", two_of_three, alt, &plain),
        ">>> Completed (2 of 3) dev-libs/a-pkg-1.0::testrepo to /altroot"
    );

    // Colour on: only the two counters and the `cpv::repo` wear
    // escapes; the action word, parens, `of`, and `for ROOT` stay
    // plain -- exactly real `colorize()`'s call sites.
    let live = crate::color::Colorizer::new(true);
    assert_eq!(
        emerging_line(&a, "1.0", two_of_three, root, &live),
        ">>> Emerging (\x1b[33;01m2\x1b[39;49;00m of \x1b[33;01m3\x1b[39;49;00m) \x1b[32mdev-libs/a-pkg-1.0::testrepo\x1b[39;49;00m"
    );
    assert_eq!(
        emerging_line(&b, "2.0", two_of_three, root, &live),
        ">>> Emerging binary (\x1b[33;01m2\x1b[39;49;00m of \x1b[33;01m3\x1b[39;49;00m) \x1b[35mdev-libs/b-pkg-2.0::testrepo\x1b[39;49;00m"
    );
    assert_eq!(
        installing_line(&a, "1.0", two_of_three, root, &live),
        ">>> Installing (\x1b[33;01m2\x1b[39;49;00m of \x1b[33;01m3\x1b[39;49;00m) \x1b[32mdev-libs/a-pkg-1.0::testrepo\x1b[39;49;00m"
    );
}

/// Backlog #197: real `Scheduler._background_mode`
/// (`Scheduler.py:470-531`) -- the serial shapes stay foreground
/// with no Jobs display, anything parallel/quiet/captured goes
/// background, and `--quiet` without `--verbose` kills the Jobs
/// display but keeps the (unblanked) status lines.
#[test]
fn scheduler_status_mode_matches_portages_background_gates() {
    // Serial, nothing quiet: foreground, no Jobs.
    assert_eq!(
        scheduler_status_mode(1, false, None, 3, false),
        StatusMode {
            background: false,
            show_jobs: false,
        }
    );
    // `-j2`: background with Jobs.
    assert_eq!(
        scheduler_status_mode(2, false, None, 3, false),
        StatusMode {
            background: true,
            show_jobs: true,
        }
    );
    // `-j2 --quiet`: background, Jobs suppressed.
    assert_eq!(
        scheduler_status_mode(2, true, None, 3, false),
        StatusMode {
            background: true,
            show_jobs: false,
        }
    );
    // `-j2 --quiet --verbose`: background, Jobs back on.
    assert_eq!(
        scheduler_status_mode(2, true, None, 3, true),
        StatusMode {
            background: true,
            show_jobs: true,
        }
    );
    // `--quiet` serial: background (even single-package), no Jobs.
    assert_eq!(
        scheduler_status_mode(1, true, None, 1, false),
        StatusMode {
            background: true,
            show_jobs: false,
        }
    );
    // `--quiet --verbose` serial (the n197 probe's s3 shape):
    // background, Jobs on.
    assert_eq!(
        scheduler_status_mode(1, true, None, 1, true),
        StatusMode {
            background: true,
            show_jobs: true,
        }
    );
    // `--quiet-build=y` serial (no `--quiet`/`--verbose`): real's
    // `myopts.get("--quiet-build") == "y"` arm -- background, and
    // the Jobs display is live (no `--quiet` to suppress it).
    assert_eq!(
        scheduler_status_mode(1, false, Some(true), 2, false),
        StatusMode {
            background: true,
            show_jobs: true,
        }
    );
    // `--quiet-build=n` serial: foreground, like an uncaptured run.
    assert_eq!(
        scheduler_status_mode(1, false, Some(false), 2, false),
        StatusMode {
            background: false,
            show_jobs: false,
        }
    );
    // `-j2` with a one-package mergelist and no quiet: real resets
    // `max_jobs` to 1 and leaves background mode.
    assert_eq!(
        scheduler_status_mode(2, false, None, 1, false),
        StatusMode {
            background: false,
            show_jobs: false,
        }
    );
    // Same single-package `-j2` under `--quiet`: background stays.
    assert_eq!(
        scheduler_status_mode(2, true, None, 1, false),
        StatusMode {
            background: true,
            show_jobs: false,
        }
    );
    // Bare `-j` (`usize::MAX`, real's `max_jobs is True`) counts as
    // parallel.
    assert_eq!(
        scheduler_status_mode(usize::MAX, false, None, 3, false),
        StatusMode {
            background: true,
            show_jobs: true,
        }
    );
}

/// Backlog #197: the `>>> Jobs:` shape (real
/// `JobStatusDisplay._display_status` on a non-tty) -- conditional
/// `running`/`failed`/`merge wait` arms, space padding to the
/// 68-column jobs field. Real's `Load avg:` trailer is cut
/// (coordinator ruling B14 -- see [`StatusDisplay::jobs_line`]); the
/// padding that precedes it is kept, so the first case below is the
/// n197 probe's s2 line up to the cut (including all 36 pad
/// spaces).
#[test]
fn jobs_line_matches_portages_display_status_shape() {
    let color = crate::color::Colorizer::new(false);
    assert_eq!(
        jobs_line(&color, 0, 3, 1, 0, 0),
        ">>> Jobs: 0 of 3 complete, 1 running                                    ",
    );
    assert_eq!(
        jobs_line(&color, 2, 3, 0, 0, 0),
        ">>> Jobs: 2 of 3 complete                                               ",
    );
    assert_eq!(
        jobs_line(&color, 1, 3, 1, 2, 1),
        ">>> Jobs: 1 of 3 complete, 1 running, 2 failed, 1 merge wait            ",
    );
    // Coloured: the counters wear `INFORM`, the padding is still
    // measured on the plain text (real's `plain_output` gauge) and
    // the line still ends at real's column.
    let tty_color = crate::color::Colorizer::new(true);
    let line = jobs_line(&tty_color, 0, 3, 1, 0, 0);
    assert!(line.starts_with(">>> Jobs: "), "{line}");
    assert!(line.contains(&tty_color.c("INFORM", "0")), "{line}");
    // The escape codes around the counts move the byte indices, so
    // count the trailing spaces instead: 68 - 32.
    assert_eq!(&line[line.len() - 36..], &" ".repeat(36), "{line}");
}

/// Backlog #197: the display counters feed the `>>> Jobs:` line --
/// dispatch raises `running`, a freed build slot drops it, a landed
/// merge raises `curval`, a failure raises `failed` (real
/// `_schedule_tasks_imp` / `_build_exit` / `_merge_exit`). The line
/// is deterministic (no live trailer -- B14), so the full shape is
/// pinned here, not just the head.
#[test]
fn status_display_counters_feed_the_jobs_line() {
    let display = StatusDisplay::new(
        StatusMode {
            background: true,
            show_jobs: false,
        },
        3,
        crate::color::Colorizer::new(false),
    );
    assert_eq!(
        display.jobs_line(),
        ">>> Jobs: 0 of 3 complete                                               "
    );
    display.job_started();
    display.job_started();
    assert_eq!(
        display.jobs_line(),
        ">>> Jobs: 0 of 3 complete, 2 running                                    "
    );
    display.build_finished();
    assert_eq!(
        display.jobs_line(),
        ">>> Jobs: 0 of 3 complete, 1 running                                    "
    );
    display.merge_finished();
    assert_eq!(
        display.jobs_line(),
        ">>> Jobs: 1 of 3 complete, 1 running                                    "
    );
    display.job_failed();
    display.build_finished();
    assert_eq!(
        display.jobs_line(),
        ">>> Jobs: 1 of 3 complete, 1 failed                                     "
    );
}
