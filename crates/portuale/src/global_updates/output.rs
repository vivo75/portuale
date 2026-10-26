//! The `Performing Global Updates` stdout block: the header printed for
//! the first changed updates file, the per-file dots, the move progress
//! characters and the trailing blank lines.
// Portage: lib/portage/_global_updates.py, _do_global_updates

use crate::color::Colorizer;
use std::cell::{Cell, RefCell};
use std::io::Write;

/// What `GlobalUpdatesPrinter::new` needs: the stdout stream
/// (Portage's `sys.stdout`), the colour switch and the `--quiet` flag.
/// The driver holds one printer across every half so the header prints
/// once and the progress characters stream in call order.
pub(crate) struct GlobalUpdatesPrinterArgs<'a> {
    pub out: &'a mut dyn Write,
    pub color: &'a Colorizer,
    pub quiet: bool,
}

/// The updates block writer: `notice` once, one `file` per updates
/// file, `progress`/`vdb_step`/`binpkg_step` while the halves run, and
/// `finish` when updates were applied. Every write flushes, like
/// Portage's `writemsg` pair. The methods take `&self` so the halves'
/// `Fn` progress hooks can call them directly; the stream itself is
/// only ever touched sequentially.
pub(crate) struct GlobalUpdatesPrinter<'a> {
    out: RefCell<&'a mut dyn Write>,
    color: &'a Colorizer,
    quiet: bool,
    notice_printed: Cell<bool>,
}

impl<'a> GlobalUpdatesPrinter<'a> {
    /// Hold the stream for the whole updates run.
    pub(crate) fn new(args: GlobalUpdatesPrinterArgs<'a>) -> Self {
        Self {
            out: RefCell::new(args.out),
            color: args.color,
            quiet: args.quiet,
            notice_printed: Cell::new(false),
        }
    }

    /// The header for the first changed updates file: a blank line, the
    /// `GOOD` title, the duration warning, and the legend unless quiet.
    /// Later calls are silent.
    ///
    /// # Panics
    ///
    /// Panics when the stream write fails, like `println!`.
    pub(crate) fn notice(&self) {
        if self.notice_printed.get() {
            return;
        }
        self.notice_printed.set(true);
        self.emit("\n");
        self.emit(&self.color.c("GOOD", "Performing Global Updates\n"));
        self.emit("(Could take a couple of minutes if you have a lot of binary packages.)\n");
        if !self.quiet {
            self.emit(&self.legend());
        }
    }

    /// One updates file's line: its path plus one dot per valid
    /// directive, unless quiet. Malformed-directive `ERROR:` lines stay
    /// unprinted: the shared parser drops them silently, and portuale
    /// keeps no such output until the updates re-read lands (see the
    /// code note).
    ///
    /// # Panics
    ///
    /// Panics when the stream write fails, like `println!`.
    pub(crate) fn file(&self, path: &str, valid: usize) {
        if self.quiet {
            return;
        }
        self.emit(&format!(
            "{}{}\n",
            self.color.c("bold", path),
            ".".repeat(valid)
        ));
    }

    /// `count` copies of one progress character (`'@'`/`'%'`/`'s'`/`'S'`
    /// from the move hooks, `'p'` per rewritten config line), with no
    /// newline. Never quiet-gated: every Portage move/config write is
    /// unconditional.
    ///
    /// # Panics
    ///
    /// Panics when the stream write fails, like `println!`.
    pub(crate) fn progress(&self, ch: char, count: usize) {
        if count == 0 {
            return;
        }
        let mut text = String::with_capacity(count);
        text.extend(std::iter::repeat_n(ch, count));
        self.emit(&text);
    }

    /// The vdb `update_ents` hook: `'#'` once per rewritten entry, never
    /// for the initial call. The driver passes no hook at all when quiet.
    ///
    /// # Panics
    ///
    /// Panics when the stream write fails, like `println!`.
    pub(crate) fn vdb_step(&self, _maxval: usize, curval: usize) {
        if curval > 0 {
            self.emit("#");
        }
    }

    /// The binpkg `update_ents` hook: `'*'` once per rewritten file,
    /// never for the initial call. Unlike the vdb hook this one runs
    /// under `--quiet` too.
    ///
    /// # Panics
    ///
    /// Panics when the stream write fails, like `println!`.
    pub(crate) fn binpkg_step(&self, _maxval: usize, curval: usize) {
        if curval > 0 {
            self.emit("*");
        }
    }

    /// The two blank lines separating the progress characters from
    /// whatever follows. The driver calls this only when updates were
    /// applied.
    ///
    /// # Panics
    ///
    /// Panics when the stream write fails, like `println!`.
    pub(crate) fn finish(&self) {
        self.emit("\n\n");
    }

    /// The `'  '`-joined legend of every progress character.
    fn legend(&self) -> String {
        [
            String::new(),
            format!("{}='update pass'", self.color.c("bold", ".")),
            format!("{}='binary update'", self.color.c("bold", "*")),
            format!("{}='/var/db update'", self.color.c("bold", "#")),
            format!("{}='/var/db move'\n", self.color.c("bold", "@")),
            format!("{}='/var/db SLOT move'", self.color.c("bold", "s")),
            format!("{}='binary move'", self.color.c("bold", "%")),
            format!("{}='binary SLOT move'\n", self.color.c("bold", "S")),
            format!(
                "{}='update /etc/portage/package.*'\n",
                self.color.c("bold", "p")
            ),
        ]
        .join("  ")
    }

    /// One write plus flush.
    fn emit(&self, text: &str) {
        let mut out = self.out.borrow_mut();
        out.write_all(text.as_bytes())
            .expect("global updates stdout write");
        out.flush().expect("global updates stdout flush");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Colorizer;
    use crate::global_updates::{GlobalUpdatesSettings, RunGlobalUpdatesArgs, run_global_updates};
    use portage_repo::RepoConfig;
    use std::path::Path;

    /// Portage 3.0.83's whole updates block for the fixture tree
    /// (`_do_global_updates`, `lib/portage/_global_updates.py:99-137`
    /// for the header and dots, `:190-206` for the moves, `:235-274`
    /// for the config/`update_ents` passes): the committed capture's
    /// stdout, byte for byte after the scratch path is normalized. The
    /// fixture has no malformed directives, and portuale prints no
    /// `ERROR:` line at all (see the code note), so stderr stays out of
    /// the pin until the updates re-read lands.
    #[test]
    fn stdout_matches_the_portage_capture() {
        let (out, scratch) = drive_fixture(false);
        let expected = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../LLM/evidence/2026-10-25-review/t5/stdout.txt"),
        )
        .unwrap()
        .replace("/var/tmp/gu-capture.WzRZsi", &scratch);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            expected,
            "the updates block matches the capture byte for byte"
        );
    }

    /// The `--quiet` variant: no legend, no per-file dots, the vdb
    /// `#` pass silenced (`onUpdate = None`,
    /// `_global_updates.py:258-259`) while the binpkg `*` pass still
    /// prints (`:265-269` rebinds the hook after the quiet gate).
    #[test]
    fn quiet_stdout_matches_the_portage_capture() {
        let (out, scratch) = drive_fixture(true);
        let expected = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../LLM/evidence/2026-10-25-review/t5/stdout.quiet.txt"),
        )
        .unwrap()
        .replace("/var/tmp/gu-capture.WzRZsi", &scratch);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            expected,
            "the quiet updates block matches the capture byte for byte"
        );
    }

    /// The colour wiring: the header goes through `GOOD` and every
    /// legend character through `bold`, like Portage's `colorize` and
    /// `bold` (`lib/portage/output.py:397-404`). The absolute codes are
    /// `color.rs`'s job, pinned there; here the keys are pinned by
    /// building the expectation through the same `Colorizer`.
    #[test]
    fn notice_uses_the_colorizer_keys() {
        let color = Colorizer::new(true);
        let mut out = Vec::new();
        let printer = GlobalUpdatesPrinter::new(GlobalUpdatesPrinterArgs {
            out: &mut out,
            color: &color,
            quiet: false,
        });
        printer.notice();
        printer.notice();
        let legend = [
            String::new(),
            format!("{}='update pass'", color.c("bold", ".")),
            format!("{}='binary update'", color.c("bold", "*")),
            format!("{}='/var/db update'", color.c("bold", "#")),
            format!("{}='/var/db move'\n", color.c("bold", "@")),
            format!("{}='/var/db SLOT move'", color.c("bold", "s")),
            format!("{}='binary move'", color.c("bold", "%")),
            format!("{}='binary SLOT move'\n", color.c("bold", "S")),
            format!("{}='update /etc/portage/package.*'\n", color.c("bold", "p")),
        ]
        .join("  ");
        assert_eq!(
            String::from_utf8(out).unwrap(),
            format!(
                "\n{}(Could take a couple of minutes if you have a lot of binary packages.)\n{legend}",
                color.c("GOOD", "Performing Global Updates\n"),
            ),
            "one header, colours through GOOD/bold"
        );
    }

    /// Drive the production driver over a scratch copy of the fixture: the
    /// mtime-gated scan, the world decisions, the per-command vdb and
    /// binpkg moves, the config files, the record, and both `update_ents`
    /// passes, with all progress reporting into the printer. Returns the
    /// stream plus the scratch path for capture normalization.
    fn drive_fixture(quiet: bool) -> (Vec<u8>, String) {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let fixture = manifest.join("../../fixtures/global-updates");
        let scratch = portage_util::TempDir::new("global_updates_output");
        let root = scratch.join("root");
        let repo = scratch.join("repo");
        for (from, to) in [(fixture.join("root"), &root), (fixture.join("repo"), &repo)] {
            let ok = std::process::Command::new("cp")
                .args(["-a", &from.to_string_lossy(), &to.to_string_lossy()])
                .status()
                .is_ok_and(|s| s.success());
            assert!(ok, "cp -a {} failed", from.display());
        }
        let repos = vec![RepoConfig {
            name: "gutest".to_string(),
            location: repo.clone(),
            priority: 0,
            is_main: true,
            masters: Vec::new(),
            profile_formats: Vec::new(),
            cache_formats: Vec::new(),
            aliases: Vec::new(),
            sync_type: None,
            sync_uri: None,
            volatile: false,
            module_specific_options: Vec::new(),
        }];
        let settings = GlobalUpdatesSettings {
            config_protect: "/etc".to_string(),
            config_protect_mask: "/etc/env.d".to_string(),
            case_insensitive: false,
            pkgdir: root.join("pkgdir"),
            multi_instance: true,
            binpkg_compress: Some("zstd".to_string()),
            binpkg_compress_flags: None,
            portage_bzip2_command: None,
            makeopts: None,
            compress_index: true,
        };
        let mut out = Vec::new();
        let applied = run_global_updates(RunGlobalUpdatesArgs {
            root: &root,
            repos: &repos,
            settings: &settings,
            privileged: crate::privileges::is_privileged(&root),
            sandboxed: false,
            quiet,
            out: &mut out,
        })
        .expect("fixture updates apply");
        assert!(
            applied,
            "the fixture's changed files yield valid directives"
        );
        (out, scratch.path().display().to_string())
    }
}
