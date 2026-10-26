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
#[cfg_attr(not(test), allow(dead_code))]
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
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct GlobalUpdatesPrinter<'a> {
    out: RefCell<&'a mut dyn Write>,
    color: &'a Colorizer,
    quiet: bool,
    notice_printed: Cell<bool>,
}

#[cfg_attr(not(test), allow(dead_code))]
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
    use crate::global_updates::binpkg::{
        BinpkgCmdArgs, BinpkgState, UpdateBinpkgEntsArgs, WriterConfig, WriterConfigArgs,
        apply_binpkg_cmd, now_secs, update_binpkg_ents,
    };
    use crate::global_updates::config_files::{
        UpdateConfigFilesArgs, UpdateWorldArgs, update_config_files, update_world,
    };
    use crate::global_updates::vdb::{UpdateEntsArgs, VdbCmdArgs, update_ents};
    use crate::global_updates::{RepoMatch, repo_update_map};
    use portage_repo::RepoConfig;
    use std::path::Path;
    use std::sync::Arc;

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

    /// Drive every half over a scratch copy of the fixture in Portage's
    /// order — world decisions (silent), moves interleaved per command,
    /// config files, vdb then binpkg `update_ents` — with all progress
    /// reporting into the printer. Returns the stream plus the scratch
    /// path for capture normalization.
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
        let color = Colorizer::new(false);
        let mut out = Vec::new();
        let printer = GlobalUpdatesPrinter::new(GlobalUpdatesPrinterArgs {
            out: &mut out,
            color: &color,
            quiet,
        });
        printer.notice();
        for (path, valid) in updates_files(&repo) {
            printer.file(&path, valid);
        }
        update_world(UpdateWorldArgs {
            root: &root,
            repos: &repos,
        })
        .expect("world updates apply");
        let map = repo_update_map(&repos);
        let matcher = RepoMatch::new(&map, Some("gutest".to_string()));
        let db: Arc<dyn portage_vdb::InstalledDb> = portage_vdb::for_root(&root);
        let _lock = db
            .vdb_dir()
            .map(|vdb| crate::portage_lock::PortageLockfile::acquire_new_lockfile(&vdb))
            .transpose()
            .expect("vdb lock");
        let pkgdir = root.join("pkgdir");
        let mut state = BinpkgState::load(&pkgdir).expect("binpkg index loads");
        let writer = WriterConfig::new(WriterConfigArgs {
            multi_instance: true,
            binpkg_compress: Some("zstd"),
            binpkg_compress_flags: None,
            portage_bzip2_command: None,
            makeopts: None,
            compress_index: true,
            now: now_secs(),
        });
        for (repo_name, cmds) in &map {
            if repo_name == "DEFAULT" || cmds.is_empty() {
                continue;
            }
            for cmd in cmds {
                crate::global_updates::vdb::apply_vdb_cmd(VdbCmdArgs {
                    db: &db,
                    repo_name,
                    matcher: &matcher,
                    cmd,
                    on_progress: Some(&|ch, n| printer.progress(ch, n)),
                })
                .expect("vdb move applies");
                apply_binpkg_cmd(BinpkgCmdArgs {
                    state: &mut state,
                    writer: &writer,
                    repo_name,
                    matcher: &matcher,
                    cmd,
                    on_progress: Some(&|ch, n| printer.progress(ch, n)),
                })
                .expect("binpkg move applies");
            }
        }
        update_config_files(UpdateConfigFilesArgs {
            root: &root,
            repos: &repos,
            config_protect: "/etc",
            config_protect_mask: "/etc/env.d",
            case_insensitive: false,
            on_progress: Some(&|| printer.progress('p', 1)),
        })
        .expect("config updates apply");
        if quiet {
            update_ents(UpdateEntsArgs {
                root: &root,
                repos: &repos,
                on_update: None,
            })
            .expect("vdb dep strings rewrite");
        } else {
            update_ents(UpdateEntsArgs {
                root: &root,
                repos: &repos,
                on_update: Some(&|maxval, curval| printer.vdb_step(maxval, curval)),
            })
            .expect("vdb dep strings rewrite");
        }
        update_binpkg_ents(UpdateBinpkgEntsArgs {
            pkgdir: &pkgdir,
            repos: &repos,
            binpkg_compress: Some("zstd"),
            binpkg_compress_flags: None,
            portage_bzip2_command: None,
            makeopts: None,
            compress_index: true,
            on_update: Some(&|maxval, curval| printer.binpkg_step(maxval, curval)),
        })
        .expect("binpkg dep strings rewrite");
        printer.finish();
        (out, scratch.path().display().to_string())
    }

    /// One updates file's display path and valid-directive count, in
    /// file-name order (a single file here, so Portage's `scandir`
    /// order needs no ruling yet).
    fn updates_files(repo: &Path) -> Vec<(String, usize)> {
        let dir = repo.join("profiles").join("updates");
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !n.starts_with('.'))
            .collect();
        names.sort();
        names
            .into_iter()
            .map(|name| {
                let path = dir.join(&name);
                let bytes = std::fs::read(&path).unwrap();
                let valid =
                    portage_repo::parse_updates_content(&String::from_utf8_lossy(&bytes)).len();
                (path.display().to_string(), valid)
            })
            .collect()
    }
}
