//! Portage's `_do_global_updates`: apply `profiles/updates/` `move` and
//! `slotmove` directives to the installed roots (vdb entries, world,
//! `package.*`, binpkgs), then record and force `AUTOCLEAN`.
//! Portage: lib/portage/_global_updates.py, _do_global_updates

pub(crate) mod binpkg;
pub(crate) mod config_files;
pub(crate) mod output;
pub(crate) mod vdb;

use crate::global_updates::binpkg::{
    BinpkgCmdArgs, BinpkgState, UpdateBinpkgEntsArgs, WriterConfig, WriterConfigArgs,
    apply_binpkg_cmd, now_secs, update_binpkg_ents,
};
use crate::global_updates::config_files::{
    UpdateConfigFilesArgs, UpdateWorldArgs, update_config_files, update_world,
};
use crate::global_updates::output::{GlobalUpdatesPrinter, GlobalUpdatesPrinterArgs};
use crate::global_updates::vdb::{UpdateEntsArgs, VdbCmdArgs, apply_vdb_cmd, update_ents};
use portage_repo::{RepoConfig, UpdateCmd};
use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The `_do_global_updates` `repo_match` closure: an entry from
/// `repository` moves under `repo_name`'s updates when it names that
/// repo, or when that repo is the main repo and the entry's repo has
/// no updates of its own (it appears nowhere in the map, so the main
/// repo's updates cover it).
// Portage: lib/portage/_global_updates.py, _do_global_updates
pub(crate) struct RepoMatch {
    repos_in_map: Vec<String>,
    master: Option<String>,
}

impl RepoMatch {
    /// Snapshot the map's repo names plus the main repo's name; one
    /// `matches` call per update repo then needs no other context.
    pub(crate) fn new(map: &[(String, Vec<UpdateCmd>)], master: Option<String>) -> Self {
        Self {
            repos_in_map: map.iter().map(|(name, _)| name.clone()).collect(),
            master,
        }
    }

    /// Whether `repository` counts as `repo_name`'s for one update.
    pub(crate) fn matches(&self, repo_name: &str, repository: &str) -> bool {
        repository == repo_name
            || (Some(repo_name) == self.master.as_deref()
                && !self.repos_in_map.iter().any(|n| n == repository))
    }
}

/// Whole-second `st_mtime` of `path`, or `None` when unreadable.
// Portage: lib/portage/update.py, grab_updates (`mystat[stat.ST_MTIME]`)
pub(crate) fn updates_file_mtime(path: &Path) -> Option<i64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| m.mtime())
}

/// The per-root settings halves `_do_global_updates` reads: the two
/// `CONFIG_PROTECT` lists, the case flag, the `$PKGDIR` under test and
/// the binpkg writer halves.
#[derive(Clone)]
pub(crate) struct GlobalUpdatesSettings {
    pub config_protect: String,
    pub config_protect_mask: String,
    pub case_insensitive: bool,
    pub pkgdir: PathBuf,
    pub multi_instance: bool,
    pub binpkg_compress: Option<String>,
    pub binpkg_compress_flags: Option<String>,
    pub portage_bzip2_command: Option<String>,
    pub makeopts: Option<String>,
    pub compress_index: bool,
}

/// Resolve the driver halves from one root's config: the incremental
/// folds for the protect lists and `FEATURES`, the resolved chain for
/// the binpkg scalars.
// Portage: lib/portage/_global_updates.py, _do_global_updates
pub(crate) fn settings_for(config: &portage_profile::Config) -> GlobalUpdatesSettings {
    let features = config.resolved_incremental("FEATURES").unwrap_or_default();
    let has = |token: &str| features.iter().any(|f| f == token);
    let chain = |key: &str| portage_profile::env_over_config_scalar(config, key);
    GlobalUpdatesSettings {
        config_protect: config
            .resolved_incremental("CONFIG_PROTECT")
            .map(|v| v.join(" "))
            .unwrap_or_default(),
        config_protect_mask: config
            .resolved_incremental("CONFIG_PROTECT_MASK")
            .map(|v| v.join(" "))
            .unwrap_or_default(),
        case_insensitive: has("case-insensitive-fs"),
        pkgdir: crate::ebuild_package::resolve_pkgdir(chain),
        multi_instance: has("binpkg-multi-instance"),
        binpkg_compress: Some(crate::ebuild_package::resolve_binpkg_compress(chain)),
        binpkg_compress_flags: Some(crate::ebuild_package::resolve_binpkg_compress_flags(
            chain,
            &crate::ebuild_package::resolve_binpkg_compress(chain),
        )),
        portage_bzip2_command: Some(crate::ebuild_package::resolve_portage_bzip2_command(chain)),
        makeopts: chain("MAKEOPTS"),
        compress_index: has("compress-index"),
    }
}
/// `vartree._aux_get`'s collapse, shared by the vdb and binpkg halves:
/// every whitespace run becomes one blank (the binary database reads
/// through the same rule).
// Portage: lib/portage/dbapi/vartree.py, _aux_get
pub(crate) fn collapse_ws(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One repo's updates, in `repos` order, plus the `DEFAULT` alias for the
/// main repo's list. The ungated full read the half-tests drive in
/// isolation: every file counts as changed, into a void printer.
// Portage: lib/portage/_global_updates.py, _do_global_updates
#[cfg(test)]
pub(crate) fn repo_update_map(repos: &[RepoConfig]) -> Vec<(String, Vec<UpdateCmd>)> {
    let mut sink = Vec::new();
    let color = crate::color::Colorizer::new(false);
    let printer = GlobalUpdatesPrinter::new(GlobalUpdatesPrinterArgs {
        out: &mut sink,
        color: &color,
        quiet: true,
    });
    let prev = BTreeMap::new();
    scan_updates(ScanUpdatesArgs {
        repos,
        prev: &prev,
        printer: &printer,
    })
    .map
}

/// What [`scan_updates`] needs: the repos (in `portdb` order), the
/// root's `mtimedb["updates"]` record, and the run's printer for the
/// notice and per-file dots.
pub(crate) struct ScanUpdatesArgs<'a, 'b> {
    pub repos: &'a [RepoConfig],
    pub prev: &'a BTreeMap<String, i64>,
    pub printer: &'b GlobalUpdatesPrinter<'b>,
}

/// The mtime-gated grab: the per-repo command lists (with the `DEFAULT`
/// alias), the error-free changed files' mtimes for the record, and
/// whether any valid directive was found.
pub(crate) struct ScannedUpdates {
    pub map: Vec<(String, Vec<UpdateCmd>)>,
    pub timestamps: BTreeMap<String, i64>,
    pub retupd: bool,
}

/// The `_do_global_updates` scan loop: every repo's updates directory
/// (shared ones once) contributes only its changed files' valid
/// directives; the notice prints for the first changed file and each one
/// gets its dots. An errored file applies but stays unrecorded.
// Portage: lib/portage/_global_updates.py, _do_global_updates
pub(crate) fn scan_updates(args: ScanUpdatesArgs<'_, '_>) -> ScannedUpdates {
    let mut out: Vec<(String, Vec<UpdateCmd>)> = Vec::new();
    let mut seen: HashMap<PathBuf, usize> = HashMap::new();
    let mut timestamps = BTreeMap::new();
    let mut retupd = false;
    for repo in args.repos {
        let updpath = repo.location.join("profiles").join("updates");
        if !updpath.is_dir() {
            continue;
        }
        if let Some(&idx) = seen.get(&updpath) {
            let cmds = out[idx].1.clone();
            out.push((repo.name.clone(), cmds));
            continue;
        }
        let mut myupd = Vec::new();
        let mut valid_total = 0usize;
        for (key, mtime, text) in changed_updates_files(&updpath, args.prev) {
            args.printer.notice();
            let parsed = portage_repo::parse_updates_with_errors(&text);
            args.printer.file(&key, parsed.valid);
            valid_total += parsed.valid;
            myupd.extend(parsed.cmds);
            // An errored file applies but is never recorded, so it
            // forces again on the next run.
            if !parsed.has_errors {
                timestamps.insert(key, mtime);
            }
        }
        if valid_total > 0 {
            retupd = true;
        }
        seen.insert(updpath, out.len());
        out.push((repo.name.clone(), myupd));
    }
    if let Some(master) = main_repo_name(args.repos)
        && let Some(cmds) = out
            .iter()
            .find(|(name, _)| name == &master)
            .map(|(_, cmds)| cmds.clone())
    {
        out.push(("DEFAULT".to_string(), cmds));
    }
    ScannedUpdates {
        map: out,
        timestamps,
        retupd,
    }
}

/// The main repo's name, for the `DEFAULT` alias and the repo match.
pub(crate) fn main_repo_name(repos: &[RepoConfig]) -> Option<String> {
    repos.iter().find(|r| r.is_main).map(|r| r.name.clone())
}

/// Every changed updates file under `updpath`: dotfiles, non-files,
/// record-current files and unreadable files are skipped, in sorted
/// name order.
fn changed_updates_files(
    updpath: &Path,
    prev: &BTreeMap<String, i64>,
) -> Vec<(String, i64, String)> {
    let Ok(entries) = portage_util::read_dir_entries(updpath) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .into_iter()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort();
    let mut changed = Vec::new();
    for name in names {
        let path = updpath.join(&name);
        let Some(mtime) = updates_file_mtime(&path) else {
            continue;
        };
        let key = path.display().to_string();
        if prev.get(&key) == Some(&mtime) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        changed.push((key, mtime, String::from_utf8_lossy(&bytes).into_owned()));
    }
    changed
}

/// What [`run_global_updates`] needs: the root this run covers, the
/// repos (in `portdb` order), the root's settings halves, the
/// target-root privilege (`secpass >= 2`, computed once by the caller
/// for both roots), whether the caller runs under a sandbox
/// (`SANDBOX_ACTIVE`), the `--quiet` flag, and the stdout stream.
pub(crate) struct RunGlobalUpdatesArgs<'a> {
    pub root: &'a Path,
    pub repos: &'a [RepoConfig],
    pub settings: &'a GlobalUpdatesSettings,
    pub privileged: bool,
    pub sandboxed: bool,
    pub quiet: bool,
    pub out: &'a mut dyn Write,
}

/// Port `_global_updates` + `_do_global_updates` for one root: the
/// early-return gates, the mtime-gated scan, the world decisions, the
/// per-command vdb and binpkg moves, the world write,
/// `update_config_files`, the record inside `if retupd`, the vdb then
/// binpkg `update_ents`, and the closing blank lines. Returns whether
/// any valid directive was applied (the caller's `AUTOCLEAN` force).
///
/// A failed record write is reported on stderr and the run continues:
/// the moves already applied, and the missing record retries them next
/// time.
///
/// # Errors
///
/// Returns `Err` when a move, config-file or dep-string pass fails; the
/// record is untouched then, so nothing is half-committed.
// Portage: lib/portage/_global_updates.py, _global_updates
pub(crate) fn run_global_updates(args: RunGlobalUpdatesArgs<'_>) -> Result<bool, String> {
    // `secpass < 2`, and anything under a sandbox, runs nothing.
    if !args.privileged || args.sandboxed {
        return Ok(false);
    }
    // One vdb lock for the whole run; a denied lock refuses everything
    // before the scan prints anything.
    let Some(_lock) = lock_vdb_for_updates(args.root)? else {
        return Ok(false);
    };
    let prev_mtimes = crate::mtimedb::read_updates_mtimes(args.root);
    // Colour is off while the updates run: `emerge_main` disables it at
    // startup and `adjust_configs` re-enables it only afterwards.
    let color = crate::color::Colorizer::new(false);
    let printer = GlobalUpdatesPrinter::new(GlobalUpdatesPrinterArgs {
        out: args.out,
        color: &color,
        quiet: args.quiet,
    });
    let scanned = scan_updates(ScanUpdatesArgs {
        repos: args.repos,
        prev: &prev_mtimes,
        printer: &printer,
    });
    if !scanned.retupd {
        return Ok(false);
    }
    let master = main_repo_name(args.repos);
    let matcher = RepoMatch::new(&scanned.map, master);
    let db: Arc<dyn portage_vdb::InstalledDb> = portage_vdb::for_root(args.root);
    update_world(UpdateWorldArgs {
        root: args.root,
        repos: args.repos,
        map: &scanned.map,
    })?;
    let pkgdir = args.settings.pkgdir.clone();
    let mut state = BinpkgState::load(&pkgdir)?;
    let writer = WriterConfig::new(WriterConfigArgs {
        multi_instance: args.settings.multi_instance,
        binpkg_compress: args.settings.binpkg_compress.as_deref(),
        binpkg_compress_flags: args.settings.binpkg_compress_flags.as_deref(),
        portage_bzip2_command: args.settings.portage_bzip2_command.as_deref(),
        makeopts: args.settings.makeopts.as_deref(),
        compress_index: args.settings.compress_index,
        now: now_secs(),
    });
    for (repo_name, cmds) in &scanned.map {
        if repo_name == "DEFAULT" || cmds.is_empty() {
            continue;
        }
        for cmd in cmds {
            apply_vdb_cmd(VdbCmdArgs {
                db: &db,
                repo_name,
                matcher: &matcher,
                cmd,
                on_progress: Some(&|ch, n| printer.progress(ch, n)),
            })?;
            apply_binpkg_cmd(BinpkgCmdArgs {
                state: &mut state,
                writer: &writer,
                repo_name,
                matcher: &matcher,
                cmd,
                on_progress: Some(&|ch, n| printer.progress(ch, n)),
            })?;
        }
    }
    update_config_files(UpdateConfigFilesArgs {
        root: args.root,
        repos: args.repos,
        map: &scanned.map,
        config_protect: &args.settings.config_protect,
        config_protect_mask: &args.settings.config_protect_mask,
        case_insensitive: args.settings.case_insensitive,
        on_progress: Some(&|| printer.progress('p', 1)),
    })?;
    if !scanned.timestamps.is_empty() {
        // `prev_mtimes.update(timestamps)`: the record accumulates, so
        // files unchanged by this run keep their entries.
        let mut merged = prev_mtimes;
        merged.extend(scanned.timestamps);
        if let Err(e) = crate::mtimedb::write_updates_mtimes(args.root, &merged) {
            eprintln!(
                "\n!!! {e}\n!!! An error occurred while recording global updates: '{}'",
                crate::mtimedb::mtimedb_path(args.root).display()
            );
        }
    }
    let vdb_step = |maxval, curval| printer.vdb_step(maxval, curval);
    update_ents(UpdateEntsArgs {
        root: args.root,
        repos: args.repos,
        map: &scanned.map,
        on_update: (!args.quiet).then_some(&vdb_step),
    })?;
    update_binpkg_ents(UpdateBinpkgEntsArgs {
        pkgdir: &pkgdir,
        repos: args.repos,
        map: &scanned.map,
        binpkg_compress: args.settings.binpkg_compress.as_deref(),
        binpkg_compress_flags: args.settings.binpkg_compress_flags.as_deref(),
        portage_bzip2_command: args.settings.portage_bzip2_command.as_deref(),
        makeopts: args.settings.makeopts.as_deref(),
        compress_index: args.settings.compress_index,
        on_update: Some(&|maxval, curval| printer.binpkg_step(maxval, curval)),
    })?;
    printer.finish();
    Ok(true)
}

/// The `_global_updates` vdb gate: a missing vdb is created (like
/// `ensure_dirs` in the lock itself); an un-writable one refuses the
/// whole run, like `PermissionDenied` from `vardb.lock()`.
// Portage: lib/portage/_global_updates.py, _global_updates
fn lock_vdb_for_updates(
    root: &Path,
) -> Result<Option<crate::portage_lock::PortageLockfile>, String> {
    let db = portage_vdb::for_root(root);
    let Some(vdb) = db.vdb_dir() else {
        return Ok(None);
    };
    if let Err(e) = std::fs::create_dir_all(&vdb) {
        if !writable_or_ancestor(&vdb) {
            return Ok(None);
        }
        return Err(format!("{}: {e}", vdb.display()));
    }
    match crate::portage_lock::PortageLockfile::acquire_new_lockfile(&vdb) {
        Ok(guard) => Ok(Some(guard)),
        // The lock sibling lives in the vdb's parent, so that
        // directory's writability is the `PermissionDenied` probe.
        Err(_) if vdb.parent().is_some_and(|p| !writable_or_ancestor(p)) => Ok(None),
        Err(e) => Err(e),
    }
}

/// `os.access` writability of `path` or its nearest existing ancestor.
fn writable_or_ancestor(path: &Path) -> bool {
    let mut probe = Some(path);
    while let Some(p) = probe {
        if p.exists() {
            return crate::privileges::path_writable(p);
        }
        probe = p.parent();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Portage 3.0.83 records only the error-free changed files, after
    /// every pass ran, inside `if retupd` (`_global_updates.py:246-251`):
    /// the full fixture run leaves `mtimedb["updates"]` holding exactly
    /// the one updates file with its current mtime, while foreign keys
    /// seeded beforehand survive verbatim.
    #[test]
    fn record_holds_only_the_error_free_changed_files() {
        let scratch = portage_util::TempDir::new("global_updates_record");
        let root = scratch.join("root");
        let repo = scratch.join("repo");
        copy_fixture(&root, &repo);
        let updates_key = repo
            .join("profiles")
            .join("updates")
            .join("1Q-2026")
            .display()
            .to_string();
        let mtime = updates_file_mtime(&repo.join("profiles").join("updates").join("1Q-2026"))
            .expect("updates file readable");
        let mut seed = BTreeMap::new();
        seed.insert("/lib".to_string(), 1_700_000_001);
        seed.insert(updates_key.clone(), mtime - 100);
        let mtimedb = crate::mtimedb::mtimedb_path(&root);
        std::fs::create_dir_all(mtimedb.parent().unwrap()).unwrap();
        std::fs::write(
            &mtimedb,
            "{\n\t\"ldpath\": {\n\t\t\"/lib\": 1700000001\n\t}\n}",
        )
        .unwrap();
        let mut out = Vec::new();
        let applied = run_global_updates(RunGlobalUpdatesArgs {
            root: &root,
            repos: &gutest_repos(&repo),
            settings: &fixture_settings(&root),
            privileged: crate::privileges::is_privileged(&root),
            sandboxed: false,
            quiet: false,
            out: &mut out,
        })
        .expect("fixture updates apply");
        assert!(
            applied,
            "the fixture's changed files yield valid directives"
        );
        let record = crate::mtimedb::read_updates_mtimes(&root);
        assert_eq!(
            record,
            BTreeMap::from([(updates_key, mtime)]),
            "exactly the error-free changed file, with its live mtime"
        );
        let text = std::fs::read_to_string(&mtimedb).unwrap();
        assert!(
            text.contains("\"ldpath\": {\n\t\t\"/lib\": 1700000001\n\t}"),
            "foreign keys survive verbatim: {text}"
        );
    }

    /// Portage 3.0.83 prints the notice and the per-file dots for every
    /// changed file, even a directive-less one (`_global_updates.py:99-130`
    /// runs before the valid-directive check at `:139`): the run records
    /// nothing and forces nothing, but it is not silent.
    #[test]
    fn directiveless_changed_file_records_nothing() {
        let scratch = portage_util::TempDir::new("global_updates_directiveless");
        let root = scratch.join("root");
        let repo = scratch.join("repo");
        std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
        let upd = repo.join("profiles").join("updates");
        std::fs::create_dir_all(&upd).unwrap();
        std::fs::write(upd.join("1Q-2026"), "\n   \n").unwrap();
        let mut out = Vec::new();
        let applied = run_global_updates(RunGlobalUpdatesArgs {
            root: &root,
            repos: &gutest_repos(&repo),
            settings: &fixture_settings(&root),
            privileged: crate::privileges::is_privileged(&root),
            sandboxed: false,
            quiet: false,
            out: &mut out,
        })
        .expect("directiveless scan applies nothing");
        assert!(!applied, "no valid directive, no force");
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains("Performing Global Updates\n"),
            "the notice prints for the changed file: {text:?}"
        );
        assert!(
            !crate::mtimedb::mtimedb_path(&root).exists(),
            "no record without applied updates"
        );
    }

    /// An errored file still forces when it yields valid directives, but
    /// `timestamps` stays empty so no record is written at all
    /// (`_global_updates.py:131-135,246`): the `mtimedb` file is never
    /// created, and the next run forces again.
    #[test]
    fn error_only_run_applies_but_records_nothing() {
        let scratch = portage_util::TempDir::new("global_updates_erroronly");
        let root = scratch.join("root");
        let repo = scratch.join("repo");
        std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
        let upd = repo.join("profiles").join("updates");
        std::fs::create_dir_all(&upd).unwrap();
        std::fs::write(
            upd.join("2Q-2024"),
            "move dev-libs/oldmovepkg dev-libs/newmovepkg\nbogus line here\n",
        )
        .unwrap();
        let mut out = Vec::new();
        let applied = run_global_updates(RunGlobalUpdatesArgs {
            root: &root,
            repos: &gutest_repos(&repo),
            settings: &fixture_settings(&root),
            privileged: crate::privileges::is_privileged(&root),
            sandboxed: false,
            quiet: false,
            out: &mut out,
        })
        .expect("error-file scan applies");
        assert!(applied, "valid directives force despite the error");
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains("Performing Global Updates\n"),
            "the notice prints for the changed file: {text:?}"
        );
        assert!(
            !crate::mtimedb::mtimedb_path(&root).exists(),
            "an errored file is never recorded"
        );
    }

    /// A versioned `slotmove` atom is valid to Portage (`parse_updates`
    /// only rejects blockers for `slotmove`), so its file forces and is
    /// recorded; the move passes themselves match on the atom text.
    #[test]
    fn versioned_slotmove_counts_as_valid() {
        let scratch = portage_util::TempDir::new("global_updates_versionslot");
        let root = scratch.join("root");
        let repo = scratch.join("repo");
        std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
        let upd = repo.join("profiles").join("updates");
        std::fs::create_dir_all(&upd).unwrap();
        std::fs::write(upd.join("1Q-2026"), "slotmove >=dev-libs/ver-2 0 1\n").unwrap();
        let mut out = Vec::new();
        let applied = run_global_updates(RunGlobalUpdatesArgs {
            root: &root,
            repos: &gutest_repos(&repo),
            settings: &fixture_settings(&root),
            privileged: crate::privileges::is_privileged(&root),
            sandboxed: false,
            quiet: false,
            out: &mut out,
        })
        .expect("versioned slotmove scan applies");
        assert!(applied, "the versioned slotmove forces");
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains("1Q-2026."),
            "the versioned line earns its dot: {text:?}"
        );
        assert_eq!(
            crate::mtimedb::read_updates_mtimes(&root).len(),
            1,
            "the error-free file is recorded"
        );
    }

    /// `secpass < 2` runs nothing and reports no force
    /// (`_global_updates.py:39-40`): an unprivileged root leaves the vdb
    /// and the record untouched.
    #[test]
    fn unprivileged_root_runs_nothing() {
        let scratch = portage_util::TempDir::new("global_updates_unprivileged");
        let root = scratch.join("root");
        let repo = scratch.join("repo");
        copy_fixture(&root, &repo);
        let before = tree_mtimes(&root.join("var/db/pkg"));
        let mut out = Vec::new();
        let applied = run_global_updates(RunGlobalUpdatesArgs {
            root: &root,
            repos: &gutest_repos(&repo),
            settings: &fixture_settings(&root),
            privileged: false,
            sandboxed: false,
            quiet: false,
            out: &mut out,
        })
        .expect("gated run never fails");
        assert!(!applied, "no privilege, no force");
        assert!(out.is_empty(), "no notice without privilege");
        assert_eq!(
            tree_mtimes(&root.join("var/db/pkg")),
            before,
            "the vdb is untouched"
        );
        assert!(
            !crate::mtimedb::mtimedb_path(&root).exists(),
            "no record without privilege"
        );
    }

    /// Anything under a sandbox runs nothing and reports no force
    /// (`_global_updates.py:39`).
    #[test]
    fn sandbox_active_runs_nothing() {
        let scratch = portage_util::TempDir::new("global_updates_sandbox");
        let root = scratch.join("root");
        let repo = scratch.join("repo");
        copy_fixture(&root, &repo);
        let mut out = Vec::new();
        let applied = run_global_updates(RunGlobalUpdatesArgs {
            root: &root,
            repos: &gutest_repos(&repo),
            settings: &fixture_settings(&root),
            privileged: true,
            sandboxed: true,
            quiet: false,
            out: &mut out,
        })
        .expect("gated run never fails");
        let printed = !out.is_empty();
        let recorded = crate::mtimedb::mtimedb_path(&root).exists();
        assert!(!applied, "no force under a sandbox");
        assert!(!printed, "no notice under a sandbox");
        assert!(!recorded, "no record under a sandbox");
    }

    /// A vdb that cannot be locked refuses the whole run, like
    /// `PermissionDenied` from `vardb.lock()` (`_global_updates.py:42-46`):
    /// the lock sibling lives in the vdb's parent, so that directory is
    /// the one made read-only here.
    #[test]
    fn unwritable_vdb_runs_nothing() {
        // SAFETY: `geteuid` takes no arguments.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let scratch = portage_util::TempDir::new("global_updates_vdblock");
        let root = scratch.join("root");
        let repo = scratch.join("repo");
        copy_fixture(&root, &repo);
        let lockdir = root.join("var/db");
        set_mode(&lockdir, 0o555);
        let before = tree_mtimes(&root.join("var/db/pkg"));
        let mut out = Vec::new();
        let applied = run_global_updates(RunGlobalUpdatesArgs {
            root: &root,
            repos: &gutest_repos(&repo),
            settings: &fixture_settings(&root),
            privileged: true,
            sandboxed: false,
            quiet: false,
            out: &mut out,
        })
        .expect("gated run never fails");
        set_mode(&lockdir, 0o755);
        assert!(!applied, "no lock, no force");
        assert!(out.is_empty(), "no notice without the lock");
        assert_eq!(
            tree_mtimes(&root.join("var/db/pkg")),
            before,
            "the vdb is untouched"
        );
        assert!(
            !crate::mtimedb::mtimedb_path(&root).exists(),
            "no record without the lock"
        );
    }

    /// A failed record write is reported, never discarded: the moves
    /// already applied, the run still forces, but no record exists so
    /// the next run retries. The `edb` path is a regular file here so
    /// the failure reproduces for any user.
    #[test]
    fn record_write_failure_still_forces() {
        let scratch = portage_util::TempDir::new("global_updates_recordfail");
        let root = scratch.join("root");
        let repo = scratch.join("repo");
        copy_fixture(&root, &repo);
        let edb = root.join("var/cache/edb");
        if edb.exists() {
            std::fs::remove_dir_all(&edb).unwrap();
        }
        std::fs::create_dir_all(edb.parent().unwrap()).unwrap();
        std::fs::write(&edb, "not a directory").unwrap();
        let mut out = Vec::new();
        let applied = run_global_updates(RunGlobalUpdatesArgs {
            root: &root,
            repos: &gutest_repos(&repo),
            settings: &fixture_settings(&root),
            privileged: crate::privileges::is_privileged(&root),
            sandboxed: false,
            quiet: false,
            out: &mut out,
        })
        .expect("record failure never fails the run");
        assert!(applied, "applied updates still force");
        assert!(
            root.join("var/db/pkg/dev-libs/newpkg-1.0").is_dir(),
            "the vdb move applied"
        );
        assert!(
            !crate::mtimedb::mtimedb_path(&root).exists(),
            "no record was committed"
        );
    }

    /// The driver halves resolve from one root's config: the incremental
    /// folds for the protect lists and `FEATURES`, the resolved chain
    /// for the binpkg scalars.
    #[test]
    fn settings_for_reads_the_resolved_config() {
        for key in [
            "BINPKG_COMPRESS",
            "BINPKG_COMPRESS_FLAGS",
            "PORTAGE_BZIP2_COMMAND",
            "MAKEOPTS",
            "PKGDIR",
        ] {
            unsafe {
                std::env::remove_var(key);
            }
        }
        let mut config = portage_profile::Config::default();
        config.incremental_sources.insert(
            "FEATURES".to_string(),
            vec![vec![
                "binpkg-multi-instance".to_string(),
                "compress-index".to_string(),
                "case-insensitive-fs".to_string(),
            ]],
        );
        config
            .incremental_sources
            .insert("CONFIG_PROTECT".to_string(), vec![vec!["/etc".to_string()]]);
        config.incremental_sources.insert(
            "CONFIG_PROTECT_MASK".to_string(),
            vec![vec!["/etc/env.d".to_string()]],
        );
        config
            .other_vars
            .insert("BINPKG_COMPRESS".to_string(), "bzip2".to_string());
        config
            .other_vars
            .insert("MAKEOPTS".to_string(), "-j4".to_string());
        let settings = settings_for(&config);
        assert!(settings.multi_instance);
        assert!(settings.compress_index);
        assert!(settings.case_insensitive);
        assert_eq!(settings.config_protect, "/etc");
        assert_eq!(settings.config_protect_mask, "/etc/env.d");
        assert_eq!(settings.binpkg_compress.as_deref(), Some("bzip2"));
        assert_eq!(settings.makeopts.as_deref(), Some("-j4"));
        assert_eq!(settings.pkgdir, PathBuf::from("/var/cache/binpkgs"));
    }

    /// Portage merges the new mtimes into the record
    /// (`prev_mtimes.update(timestamps)`): a second run that changes a
    /// different file keeps the first run's entries instead of replacing
    /// them, so each file is applied exactly once.
    #[test]
    fn the_record_accumulates_across_runs() {
        let scratch = portage_util::TempDir::new("global_updates_accumulate");
        let root = scratch.join("root");
        let repo = scratch.join("repo");
        std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
        let upd = repo.join("profiles").join("updates");
        std::fs::create_dir_all(&upd).unwrap();
        std::fs::write(
            upd.join("1Q-2026"),
            "move dev-libs/oldmovepkg dev-libs/newmovepkg\n",
        )
        .unwrap();
        let mut out = Vec::new();
        let applied = run_global_updates(RunGlobalUpdatesArgs {
            root: &root,
            repos: &gutest_repos(&repo),
            settings: &fixture_settings(&root),
            privileged: crate::privileges::is_privileged(&root),
            sandboxed: false,
            quiet: false,
            out: &mut out,
        })
        .expect("first run applies");
        assert!(applied);
        let first = crate::mtimedb::read_updates_mtimes(&root);
        assert_eq!(first.len(), 1, "one file recorded: {first:?}");
        std::fs::write(upd.join("2Q-2026"), "slotmove dev-libs/slotmovepkg 0 1\n").unwrap();
        let mut out = Vec::new();
        let applied = run_global_updates(RunGlobalUpdatesArgs {
            root: &root,
            repos: &gutest_repos(&repo),
            settings: &fixture_settings(&root),
            privileged: crate::privileges::is_privileged(&root),
            sandboxed: false,
            quiet: false,
            out: &mut out,
        })
        .expect("second run applies");
        assert!(applied);
        let record = crate::mtimedb::read_updates_mtimes(&root);
        assert_eq!(record.len(), 2, "both files recorded: {record:?}");
        assert_eq!(
            record.get(&updates_key(&repo, "1Q-2026")),
            first.get(&updates_key(&repo, "1Q-2026"))
        );
        let text = String::from_utf8(out).unwrap();
        assert!(
            !text.contains("1Q-2026."),
            "the already-recorded file prints no dots again: {text:?}"
        );
    }

    /// Whole-second `st_mtime` of an updates file under the repo.
    fn updates_key(repo: &Path, name: &str) -> String {
        repo.join("profiles")
            .join("updates")
            .join(name)
            .display()
            .to_string()
    }

    /// One `gutest` main repo over a scratch copy of the fixture repo.
    fn gutest_repos(repo: &Path) -> Vec<RepoConfig> {
        vec![RepoConfig {
            name: "gutest".to_string(),
            location: repo.to_path_buf(),
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
        }]
    }

    /// The driver halves the output pin uses: protected `/etc`, the
    /// fixture `pkgdir`, the fixture's writer halves.
    fn fixture_settings(root: &Path) -> GlobalUpdatesSettings {
        GlobalUpdatesSettings {
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
        }
    }

    /// Scratch copy of the updates fixture tree.
    fn copy_fixture(root: &Path, repo: &Path) {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let fixture = manifest.join("../../fixtures/global-updates");
        for (from, to) in [(fixture.join("root"), root), (fixture.join("repo"), repo)] {
            let ok = std::process::Command::new("cp")
                .args(["-a", &from.to_string_lossy(), &to.to_string_lossy()])
                .status()
                .is_ok_and(|s| s.success());
            assert!(ok, "cp -a {} failed", from.display());
        }
    }

    /// Every file under `dir` as `relative/path -> mtime`: proves a
    /// gated run left the tree alone without hashing bytes.
    fn tree_mtimes(dir: &Path) -> BTreeMap<String, std::time::SystemTime> {
        let mut out = BTreeMap::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(top) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&top) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if let (Ok(rel), Ok(mtime)) = (
                    path.strip_prefix(dir)
                        .map(|p| p.to_string_lossy().into_owned()),
                    std::fs::metadata(&path).and_then(|m| m.modified()),
                ) {
                    out.insert(rel, mtime);
                }
            }
        }
        out
    }

    /// `chmod`, without following the repo's own helper imports.
    fn set_mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }
}
