//! The failed-build endgame shared by the serial merge loop and the `-jN`
//! scheduler: Portage's `Failed to emerge` block, the `--keep-going` resume
//! banner and dropped-package notices, the end-of-run elog summary and the
//! stderr list of failed packages.

use crate::emerge_build::*;

/// The error text of a failure whose output is already printed: the CLI
/// boundary exits 1 without adding an `emerge:` line.
pub(crate) const REPORTED_FAILURE: &str = "failed packages already reported";

/// Whether `e` is [`REPORTED_FAILURE`].
pub(crate) fn is_reported_failure(e: &str) -> bool {
    e == REPORTED_FAILURE
}

/// Whether `err` is a failed ebuild phase (`merge failed (1)`, `build failed
/// (1)`, `clean failed (1)`, `binpkg build failed (1)`), not a setup error.
pub(crate) fn is_phase_failure(err: &str) -> bool {
    err.contains(" failed (") && !err.contains('\n')
}

/// Whether `err` is a failed `pkg_postinst` whose merge otherwise landed
/// (see [`postinst_failed_error`]). Checked before [`is_phase_failure`]:
/// the text also contains `" failed ("`.
pub(crate) fn is_postinst_failure(err: &str) -> bool {
    err.contains(": postinst failed (") && !err.contains('\n')
}

/// The error of one entry whose `pkg_postinst` exited `status` after an
/// otherwise successful merge: files and the vdb entry are live (the
/// `_postinst_failure` rule), so the loops print `>>> Completed`, skip the
/// world record and the post-merge clean, and report `Failed to install`.
// Portage: vartree.py, dblink.merge (`RETURNCODE_POSTINST_FAILURE`)
pub(crate) fn postinst_failed_error(cp: &str, version: &str, status: i32) -> String {
    format!("{cp}-{version}: postinst failed ({status})")
}

/// What one merge pass finished with: postinst-failed entries merged (their
/// vdb entry is live) but must neither be world-recorded nor retried by a
/// resume pass. The loops return it on success too (usually empty).
#[derive(Debug, Default)]
pub(crate) struct MergePassReport {
    pub(crate) postinst_failed: Vec<(String, String)>,
    /// A `--keep-going` resume pass re-saved the resume list: the failure
    /// tail must not resurrect entries the passes already resolved.
    pub(crate) resumed: bool,
}

/// A merge-pass failure: the display string for the `emerge:` line (or
/// [`REPORTED_FAILURE`]) plus the pass report.
#[derive(Debug)]
pub(crate) struct MergePassError {
    pub(crate) message: String,
    pub(crate) report: MergePassReport,
}

impl From<String> for MergePassError {
    /// An internal error carries no pass report (no entry merged).
    fn from(message: String) -> Self {
        MergePassError {
            message,
            report: MergePassReport::default(),
        }
    }
}

impl std::fmt::Display for MergePassError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(f)
    }
}

/// Where a source entry's build left its log and its `${T}`.
pub(crate) struct SourcePaths {
    pub(crate) log: PathBuf,
    pub(crate) t_dir: PathBuf,
}

/// What the loops need to report a failure.
pub(crate) struct FailReport<'a> {
    pub(crate) root: &'a Path,
    /// `Some` for a source entry that builds here; binary entries print their own tail.
    pub(crate) source_paths: &'a dyn Fn(&GraphEntry) -> Option<SourcePaths>,
    /// The resolved `FEATURES`, for `split-elog`.
    pub(crate) features: String,
    /// `--quiet`: no `Calculating dependencies` notice on the resumed pass.
    pub(crate) quiet: bool,
    /// The run is in the scheduler's background mode (`-jN`, `--quiet`): status lines carry
    /// no leading blank and a lone failure shows its whole build log.
    pub(crate) background: bool,
    /// `--quiet-fail`: no whole-log dump.
    pub(crate) quiet_fail: bool,
}

/// The `${T}/build.log` and `${T}` of `entry` under `portage_tmpdir`,
/// for a source or a binary entry (both merge under
/// `portage/<cat>/<pf>`; a binary entry's log is the captured hook
/// output the merge writes there).
pub(crate) fn source_paths_for(
    options: &ebuild_merge::MergeOptions,
    entry: &GraphEntry,
    portage_tmpdir: &Path,
) -> Option<SourcePaths> {
    let version = entry_version(&entry.outcome)?;
    let tmp = entry_portage_tmpdir(options, entry, version, portage_tmpdir).ok()?;
    let log = build_log_path(
        &tmp,
        &entry.category,
        &entry.package,
        version,
        &resolved_features(options),
    );
    let t_dir = tmp
        .join("portage")
        .join(&entry.category)
        .join(format!("{}-{version}", entry.package))
        .join("temp");
    Some(SourcePaths { log, t_dir })
}

enum ElogItem {
    Failed {
        cpv: String,
        t_dir: PathBuf,
        log: PathBuf,
    },
    Dropped {
        cpv: String,
        text: Vec<String>,
    },
}

/// The failures of one merge run, in the order they happened.
#[derive(Default)]
pub(crate) struct Ledger {
    rows: Vec<String>,
    elog: Vec<ElogItem>,
    reported: bool,
}

/// The chunks `textwrap.wrap` breaks between: whitespace runs, and words split
/// after a hyphen that follows two letters and precedes `letter[-]letter`
/// (`TextWrapper.wordsep_re`, `break_on_hyphens=True`).
fn textwrap_chunks(text: &str) -> Vec<Vec<char>> {
    let chars: Vec<char> = text.chars().collect();
    let letter = |c: char| c.is_alphabetic();
    let mut chunks: Vec<Vec<char>> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            let start = i;
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            chunks.push(chars[start..i].to_vec());
            continue;
        }
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() {
            let hyphen_break = chars[i] == '-'
                && i >= start + 2
                && letter(chars[i - 1])
                && letter(chars[i - 2])
                && chars.get(i + 1).is_some_and(|c| letter(*c))
                && (chars.get(i + 2).is_some_and(|c| letter(*c))
                    || (chars.get(i + 2) == Some(&'-')
                        && chars.get(i + 3).is_some_and(|c| letter(*c))));
            i += 1;
            if hyphen_break {
                break;
            }
        }
        chunks.push(chars[start..i].to_vec());
    }
    chunks
}

/// `textwrap.wrap(text, width)`: greedy fill over [`textwrap_chunks`], whitespace
/// dropped at line ends and starts, over-long words cut at `width`.
pub(crate) fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let mut chunks = textwrap_chunks(text.trim());
    chunks.reverse();
    let mut lines = Vec::new();
    while !chunks.is_empty() {
        let mut line: Vec<char> = Vec::new();
        // Leading whitespace of a continuation line is dropped.
        if !lines.is_empty() && chunks.last().is_some_and(|c| c[0].is_whitespace()) {
            chunks.pop();
        }
        while let Some(chunk) = chunks.last() {
            if line.len() + chunk.len() <= width {
                line.extend(chunks.pop().unwrap_or_default());
            } else {
                break;
            }
        }
        // A chunk longer than a whole line is cut.
        if let Some(chunk) = chunks.last_mut()
            && chunk.len() > width
        {
            let room = width.saturating_sub(line.len());
            line.extend(chunk.drain(..room));
        }
        while line.last().is_some_and(|c| c.is_whitespace()) {
            line.pop();
        }
        if !line.is_empty() {
            lines.push(line.into_iter().collect());
        }
    }
    lines
}

/// ` (cpv:slot/sub::repo, ebuild scheduled for merge to 'root')`, one
/// failed package (`binary scheduled for merge` for a `Binary` entry).
// Portage: Scheduler.py (`_failed_pkgs_all` summary), Package.__str__
pub(crate) fn failed_pkg_label(entry: &GraphEntry, cpv_slot: &str, root: &Path) -> String {
    let kind = if entry.source == CandidateSource::Binary {
        "binary"
    } else {
        "ebuild"
    };
    format!(
        " ({cpv_slot}::{}, {kind} scheduled for merge to '{}')",
        entry_repo(entry),
        root.display()
    )
}

/// The `cat/pkg-ver:slot/sub` identity for a failure label: a `Binary`
/// entry whose index carried `BUILD_ID` shows `<cpv>-<build_id>`, like
/// the merge-list rows do.
// Portage: _emerge/Package.py (`_pkg_str`)
fn label_cpv_slot(entry: &GraphEntry, version: &str) -> String {
    match (&entry.source, &entry.build_id) {
        (CandidateSource::Binary, Some(bid)) if !bid.is_empty() => {
            entry_cpv_slot(entry, &format!("{version}-{bid}"))
        }
        _ => entry_cpv_slot(entry, version),
    }
}

/// The end-of-run `The following package(s) have failed to build, install, or execute
/// postinst:` block on stderr: for two or more failures, or one under `--keep-going`.
// Portage: _emerge/Scheduler.py, merge (`_failed_pkgs_all` summary)
pub(crate) fn failed_pkgs_summary(
    labels: &[String],
    keep_going: bool,
    color: &crate::color::Colorizer,
) -> String {
    if !(labels.len() > 1 || (!labels.is_empty() && keep_going)) {
        return String::new();
    }
    let msg = if labels.len() > 1 {
        format!(
            "The following {} packages have failed to build, install, or execute postinst:",
            labels.len()
        )
    } else {
        "The following package has failed to build, install, or execute postinst:".to_string()
    };
    let star = color.c("BAD", " * ");
    let mut out = format!("{star}\n");
    for line in wrap_words(&msg, 72) {
        out.push_str(&format!("{star}{line}\n"));
    }
    out.push_str(&format!("{star}\n"));
    for label in labels {
        out.push_str(&format!("{star}{label}\n"));
    }
    out.push_str(&format!("{star}\n"));
    out
}

pub(crate) fn print_failed_pkgs_summary(
    labels: &[String],
    keep_going: bool,
    color: &crate::color::Colorizer,
) {
    eprint!("{}", failed_pkgs_summary(labels, keep_going, color));
}

impl Ledger {
    /// One failed `entry`: prints the `Failed to emerge` block when it is a source
    /// phase failure (or the `Failed to install` block for a postinst failure
    /// whose merge landed), keeps its summary row and its elog for the end of
    /// the run. Returns whether the failure is fully reported (no `emerge:`
    /// line wanted).
    pub(crate) fn failed(
        &mut self,
        report: &FailReport<'_>,
        entry: &GraphEntry,
        err: &str,
    ) -> bool {
        let Some(version) = entry_version(&entry.outcome) else {
            return false;
        };
        let cpv = format!("{}/{}-{version}", entry.category, entry.package);
        let cpv_slot = label_cpv_slot(entry, version);
        let color = progress_color();
        let paths = (report.source_paths)(entry);
        let mut label = failed_pkg_label(entry, &cpv_slot, report.root);
        let mut reported = false;
        if let Some(paths) = &paths
            && is_postinst_failure(err)
        {
            // The merge landed (`>>> Completed` already printed by the
            // caller, like `PackageMerge._install_exit`): report the
            // install-phase failure, never the build-phase one. The summary
            // row carries no `(postinst failed)` suffix on this path (that
            // suffix is the returncode-OK binary-merge arm, which never
            // reaches here).
            // Portage: Scheduler.py, _failed_pkg_msg / _do_merge_exit
            let msg = crate::emerge_getbinpkg::failed_install_pkg_msg(
                &cpv,
                report.root,
                Some(&paths.log),
            );
            print!(
                "{}",
                if report.background {
                    msg.trim_start_matches('\n').replace("\n\n", "\n")
                } else {
                    msg
                }
            );
            label.push_str(&format!(
                ", Log file:\n{}  '{}'",
                color.c("BAD", " * "),
                paths.log.display()
            ));
            self.elog.push(ElogItem::Failed {
                cpv,
                t_dir: paths.t_dir.clone(),
                log: paths.log.clone(),
            });
            self.reported = true;
            self.rows.push(label);
            return true;
        }
        if let Some(paths) = &paths
            && entry.source != CandidateSource::Binary
            && is_phase_failure(err)
        {
            // Source entries only: a binary entry's non-postinst merge
            // error (`binpkg merge failed`, digest/missing sentinels) keeps
            // its own tail below -- the `Failed to emerge` block and its
            // unconditional `, Log file:` suffix are the source
            // build-phase shape, un-oracled for binary merges.
            let msg = crate::emerge_getbinpkg::failed_pkg_msg(&cpv, report.root, Some(&paths.log));
            // `_status_msg` writes a leading blank line only in the foreground.
            print!(
                "{}",
                if report.background {
                    msg.trim_start_matches('\n').replace("\n\n", "\n")
                } else {
                    msg
                }
            );
            label.push_str(&format!(
                ", Log file:\n{}  '{}'",
                color.c("BAD", " * "),
                paths.log.display()
            ));
            self.elog.push(ElogItem::Failed {
                cpv,
                t_dir: paths.t_dir.clone(),
                log: paths.log.clone(),
            });
            reported = true;
        } else if crate::emerge_getbinpkg::is_binpkg_digest_failure(err)
            || crate::emerge_getbinpkg::is_binpkg_missing_failure(err)
        {
            reported = true;
        }
        self.rows.push(label);
        self.reported |= reported;
        reported
    }

    /// `*** Resuming merge...` plus the re-resolution notice, once per pass that
    /// still has packages left.
    // Portage: Scheduler.py, _merge (resume loop); depgraph.py, backtrack_depgraph
    pub(crate) fn resuming(&self, quiet: bool) {
        let color = progress_color();
        println!("{}", color.c("GOOD", "*** Resuming merge..."));
        if quiet {
            return;
        }
        println!("Calculating dependencies ... done!");
        print!("{}", crate::pretend::timing_line(&color, None));
    }

    /// A package the resumed pass can no longer build: the `dropped because` notice
    /// (also its elog) and a bare summary row.
    // Portage: Scheduler.py, _calc_resume_list (`emerge --keep-going:` messages)
    pub(crate) fn dropped(
        &mut self,
        report: &FailReport<'_>,
        entry: &GraphEntry,
        atoms: &[String],
    ) {
        let Some(version) = entry_version(&entry.outcome) else {
            return;
        };
        let cpv = format!("{}/{}-{version}", entry.category, entry.package);
        let mut msg = format!("emerge --keep-going: {cpv}");
        if report.root != Path::new("/") {
            msg.push_str(&format!(" for {}", report.root.display()));
        }
        if atoms.is_empty() {
            msg.push_str(" dropped because it is masked or unavailable");
        } else {
            msg.push_str(&format!(
                " dropped because it requires {}",
                atoms.join(", ")
            ));
        }
        let lines = wrap_words(&msg, 75);
        let star = progress_color().c("BAD", " * ");
        for line in &lines {
            println!("{star}{line}");
        }
        let cpv_slot = label_cpv_slot(entry, version);
        self.rows
            .push(failed_pkg_label(entry, &cpv_slot, report.root));
        self.elog.push(ElogItem::Dropped { cpv, text: lines });
    }

    /// A background run whose only failure is one built package shows that
    /// package's whole build log, before the elog blocks.
    // Portage: Scheduler.py, merge (`failure_log_shown`), `_locate_failure_log`
    fn show_lone_failure_log(&self, report: &FailReport<'_>) {
        use std::io::{Read, Write};
        if !report.background || report.quiet_fail || self.rows.len() != 1 {
            return;
        }
        let [ElogItem::Failed { log, .. }] = self.elog.as_slice() else {
            return;
        };
        let gz = PathBuf::from(format!("{}.gz", log.display()));
        let Some(raw) = std::fs::read(log).ok().or_else(|| std::fs::read(&gz).ok()) else {
            return;
        };
        // `compress-build-logs` leaves a gzip file whatever name the path has.
        let bytes = if raw.starts_with(&[0x1f, 0x8b]) {
            let mut out = Vec::new();
            if let Err(e) = flate2::read::MultiGzDecoder::new(&raw[..]).read_to_end(&mut out) {
                eprintln!("{e}");
                return;
            }
            out
        } else {
            raw
        };
        let mut stdout = std::io::stdout().lock();
        let _ = stdout.write_all(&bytes);
        let _ = stdout.flush();
    }

    /// The end of the run: the elog `Messages for package` blocks on stdout, in the
    /// order the failures happened.
    // Portage: Scheduler.py, merge (`_flush_elog_mod_echo`)
    pub(crate) fn flush_elog(&mut self, report: &FailReport<'_>) {
        let color = progress_color();
        let root = report.root.display().to_string();
        for item in std::mem::take(&mut self.elog) {
            match item {
                ElogItem::Failed { cpv, t_dir, .. } => {
                    crate::elog::process_batch(crate::elog::ProcessBatchArgs {
                        logdir: &crate::elog::logdir(report.root),
                        root_display: &root,
                        items: &[(cpv, t_dir)],
                        phases: None,
                        color: &color,
                        split_elog: report
                            .features
                            .split_whitespace()
                            .any(|t| t == "split-elog"),
                    });
                }
                ElogItem::Dropped { cpv, text } => {
                    if !crate::elog::echo_enabled() {
                        continue;
                    }
                    let messages = text
                        .into_iter()
                        .map(|text| crate::elog::ElogMessage {
                            phase: "other".to_string(),
                            level: "ERROR".to_string(),
                            text,
                        })
                        .collect();
                    crate::elog::echo_summary(
                        &[crate::elog::ElogPackage {
                            cpv,
                            root: root.clone(),
                            logfile: None,
                            messages,
                        }],
                        &color,
                    );
                }
            }
        }
    }

    /// The stderr list of failed packages; the error to return when anything failed
    /// (`REPORTED_FAILURE` when every failure is already shown).
    pub(crate) fn finish(
        &mut self,
        report: &FailReport<'_>,
        keep_going: bool,
        first_error: Option<String>,
    ) -> Result<(), String> {
        if self.rows.is_empty() {
            return Ok(());
        }
        self.show_lone_failure_log(report);
        self.flush_elog(report);
        print_failed_pkgs_summary(&self.rows, keep_going, &progress_color());
        match first_error {
            Some(e) if !self.reported => Err(e),
            _ => Err(REPORTED_FAILURE.to_string()),
        }
    }
}

/// The atoms with which `dependent` requires the package `cp`, sorted.
pub(crate) fn atoms_requiring(dependent: &GraphEntry, cp: &(String, String)) -> Vec<String> {
    let mut atoms: Vec<String> = dependent
        .deps
        .iter()
        .filter(|d| d.category == cp.0 && d.package == cp.1)
        .map(|d| d.atom.clone())
        .collect();
    atoms.sort();
    atoms.dedup();
    atoms
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label_entry(source: CandidateSource, build_id: Option<&str>) -> GraphEntry {
        GraphEntry {
            discovery: 0,
            category: "dev-util".into(),
            package: "f459-postinstfail".into(),
            outcome: PretendOutcome::New {
                version: "1".into(),
            },
            blockers: vec![],
            slot: Some("0".into()),
            sub_slot: Some("0".into()),
            repo_name: Some("testrepo".into()),
            oldbest: vec![],
            use_flags_display: vec![],
            use_expand_display: vec![],
            use_expand_display_p: vec![],
            keyword_mask: None,
            new_slot: false,
            interactive: false,
            fetch_restrict: false,
            fetch_restrict_satisfied: false,
            download_files: Vec::new(),
            required_by: vec![],
            source,
            provenance: Default::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
            remote_binary: false,
            build_id: build_id.map(str::to_string),
            deps: Vec::new(),
        }
    }

    #[test]
    fn label_cpv_slot_appends_the_build_id_for_binaries() {
        // Portage `Package.__str__` shows `<cpv>-<build_id>` for a binary
        // whose index carried `BUILD_ID` (the oracle's `f459-postinstfail`
        // was built with one); an ebuild entry never does.
        let binary = label_entry(CandidateSource::Binary, Some("1"));
        assert_eq!(
            label_cpv_slot(&binary, "1"),
            "dev-util/f459-postinstfail-1-1:0/0"
        );
        let plain = label_entry(CandidateSource::Binary, None);
        assert_eq!(
            label_cpv_slot(&plain, "1"),
            "dev-util/f459-postinstfail-1:0/0"
        );
        let source = label_entry(CandidateSource::Ebuild, None);
        assert_eq!(
            label_cpv_slot(&source, "1"),
            "dev-util/f459-postinstfail-1:0/0"
        );
        assert_eq!(
            failed_pkg_label(&binary, &label_cpv_slot(&binary, "1"), Path::new("/")),
            " (dev-util/f459-postinstfail-1-1:0/0::testrepo, binary scheduled for merge to '/')"
        );
    }
}
