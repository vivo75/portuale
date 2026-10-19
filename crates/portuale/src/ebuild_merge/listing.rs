//! The direct-path merge/unmerge listing: the collision line, `>>> Merging`,
//! one `>>>`/`---`/`===` line per merged entry, the replace loop's
//! `>>> Safely unmerging …` / `--- replaced` / `<<<` / `>>> Original …`
//! lines, and `>>> <cpv> merged.` — all on stdout, like Portage's
//! `dblink._display_merge` on the foreground path.
//!
//! One printer shared by the source merge, the binpkg merge and the
//! replace-unmerge; a no-op on the `emerge`-scheduler path (Portage's own
//! listing then reaches only the discarded merge-child output, never the
//! terminal or a persistent log — see `LLM/evidence/2026-10-18-403`).
//! Portage: lib/portage/dbapi/vartree.py, `dblink._display_merge`,
//! `dblink._show_unmerge`, `dblink.treewalk`, `dblink.mergeme`.

use crate::color::Colorizer;

/// Whether the listing colorizes its ` * ` line: Portage's merge child
/// re-arms `havecolor` from `no_color(settings)` only, so piped output
/// stays colored and `TERM=dumb` stays colored; only `NO_COLOR` (set,
/// non-empty) or `NOCOLOR=yes|true` (case-insensitive) disables it.
/// Portage: bin/ebuild:128-134, dbapi/_MergeProcess.py:248.
pub(crate) fn listing_color_enabled() -> bool {
    if std::env::var("NO_COLOR")
        .ok()
        .is_some_and(|v| !v.is_empty())
    {
        return false;
    }
    let nocolor = std::env::var("NOCOLOR")
        .ok()
        .filter(|v| !v.is_empty())
        .map(|v| v.to_ascii_lowercase());
    !matches!(nocolor.as_deref(), Some("yes" | "true"))
}

/// The per-file listing printer. Enabled listings print every line to
/// stdout as it is emitted and record it in `lines` (the unit tests
/// assert on those); a disabled listing is a complete no-op.
/// All lines are stored without the trailing newline.
pub(crate) struct MergeListing {
    enabled: bool,
    color: Colorizer,
    lines: Vec<String>,
}

impl MergeListing {
    /// `enabled` is the direct-path gate (`MergeOptions.direct`); the
    /// color follows the process `NO_COLOR`/`NOCOLOR` environment.
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            color: Colorizer::new(listing_color_enabled()),
            lines: Vec::new(),
        }
    }

    /// Test constructor with an explicit color switch (the process
    /// environment is global and cannot be toggled per test).
    #[cfg(test)]
    pub(crate) fn new_for_test(enabled: bool, color: bool) -> Self {
        Self {
            enabled,
            color: Colorizer::new(color),
            lines: Vec::new(),
        }
    }

    /// Every line emitted so far, without trailing newlines.
    #[cfg(test)]
    pub(crate) fn lines(&self) -> &[String] {
        &self.lines
    }

    fn emit(&mut self, line: String) {
        if !self.enabled {
            return;
        }
        println!("{line}");
        self.lines.push(line);
    }

    /// ` * checking N files for package collisions`: regular files plus
    /// non-directory symlinks in the image (directories excluded).
    /// Portage: vartree.py, `dblink._collision_protect`.
    pub(crate) fn collision_line(&mut self, files: usize) {
        let star = self.color.c("GOOD", "*");
        self.emit(format!(
            " {star} checking {files} files for package collisions"
        ));
    }

    /// `eerror` lines: ` * <line>`, the star and its spaces in `BAD` (stdout,
    /// like Portage's foreground `ebuild`). A no-op unless the listing is on.
    /// Portage: lib/portage/elog/messages.py, eerror
    pub(crate) fn eerror(&mut self, lines: &[String]) {
        let star = self.color.c("BAD", " * ");
        for line in lines {
            self.emit(format!("{star}{line}"));
        }
    }

    /// `>>> Merging <cpv> to <root>/` (exactly one trailing slash).
    /// Portage: vartree.py, `dblink.treewalk`.
    pub(crate) fn merging(&mut self, cpv: &str, root: &std::path::Path) {
        let root = root.to_string_lossy();
        let root = root.trim_end_matches('/');
        self.emit(format!(">>> Merging {cpv} to {root}/"));
    }

    /// `>>> <cpv> merged.`, after a successful `postinst`.
    /// Portage: vartree.py, `dblink.treewalk`.
    pub(crate) fn merged(&mut self, cpv: &str) {
        self.emit(format!(">>> {cpv} merged."));
    }

    /// `>>> Safely unmerging already-installed instance...`, once per
    /// replaced same-slot instance, before its unmerge.
    /// Portage: vartree.py, `dblink.treewalk`.
    pub(crate) fn safely_unmerging(&mut self) {
        self.emit(">>> Safely unmerging already-installed instance...".to_string());
    }

    /// `>>> Original instance of package unmerged safely.`, after each
    /// replaced instance's vdb entry is deleted.
    /// Portage: vartree.py, `dblink.treewalk`.
    pub(crate) fn original_unmerged(&mut self) {
        self.emit(">>> Original instance of package unmerged safely.".to_string());
    }

    /// A merged directory: `--- <dest>/` when the destination already
    /// exists (or is a symlink to a directory), `>>> <dest>/` when the
    /// merge created it. Portage: vartree.py, `dblink.mergeme`.
    pub(crate) fn merge_dir(&mut self, dest: &std::path::Path, existed: bool) {
        let mark = if existed { "---" } else { ">>>" };
        self.emit(format!("{mark} {}/", dest.display()));
    }

    /// A merged regular file: `>>>` when `movefile` replaced it,
    /// `===` when source and destination are identical (utime refresh
    /// only), `---` when config-memory rejects an already-offered
    /// protected update. `dest` is the `._cfgNNNN_` path when diverted.
    /// Portage: vartree.py, `dblink.mergeme`, `dblink._protect`.
    pub(crate) fn merge_file(&mut self, dest: &std::path::Path, kind: MergeFileKind) {
        let mark = match kind {
            MergeFileKind::New => ">>>",
            MergeFileKind::Identical => "===",
            MergeFileKind::ConfmemRejected => "---",
        };
        self.emit(format!("{mark} {}", dest.display()));
    }

    /// A merged symlink `>>> <dest> -> <target>` when moved, `---` when
    /// config-memory rejects it. Portage: vartree.py, `dblink.mergeme`.
    pub(crate) fn merge_symlink(&mut self, dest: &std::path::Path, target: &str, moved: bool) {
        let mark = if moved { ">>>" } else { "---" };
        self.emit(format!("{mark} {} -> {target}", dest.display()));
    }

    /// A merged fifo/device node: `>>>` when created, `!!!` when the
    /// destination already exists (still stdout, like Portage).
    /// Portage: vartree.py, `dblink.mergeme`.
    pub(crate) fn merge_special(&mut self, dest: &std::path::Path, created: bool) {
        let mark = if created { ">>>" } else { "!!!" };
        self.emit(format!("{mark} {}", dest.display()));
    }

    /// One unmerge line: `{zing} {desc-8} {type} {path}` — `<<<` carries
    /// an empty desc (hence 10 spaces before the type), `---` a reason.
    /// Portage: vartree.py, `dblink._show_unmerge`.
    pub(crate) fn show_unmerge(
        &mut self,
        zing: &str,
        desc: &str,
        file_type: &str,
        path: &std::path::Path,
    ) {
        self.emit(format!("{zing} {:<8} {file_type} {}", desc, path.display()));
    }

    /// `--- replaced <type> <path>`: claimed by another same-slot instance.
    pub(crate) fn unmerge_replaced(&mut self, file_type: &str, path: &std::path::Path) {
        self.show_unmerge("---", "replaced", file_type, path);
    }

    /// `<<<          <type> <path>`: actually removed.
    pub(crate) fn unmerge_removed(&mut self, file_type: &str, path: &std::path::Path) {
        self.show_unmerge("<<<", "", file_type, path);
    }

    /// `--- <desc> <type> <path>`: kept with a reason (`cfgpro`,
    /// `!mtime`, `!md5`, `!dir`, `!sym`, `!obj`, `!found`, `!empty`,
    /// `!fif`, or empty for a kept fifo/device).
    pub(crate) fn unmerge_kept(&mut self, desc: &str, file_type: &str, path: &std::path::Path) {
        self.show_unmerge("---", desc, file_type, path);
    }
}

/// Which `>>>`/`===`/`---` mark a merged regular file takes.
/// Portage: vartree.py, `dblink.mergeme` (`zing`), `dblink._protect`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum MergeFileKind {
    /// `movefile` replaced the destination.
    New,
    /// Source and destination identical: utime refresh only.
    Identical,
    /// Config-memory rejected an already-offered protected update.
    ConfmemRejected,
}
