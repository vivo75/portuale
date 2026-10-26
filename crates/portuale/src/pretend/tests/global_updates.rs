use super::*;

use std::collections::BTreeMap;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

/// The applied-updates AUTOCLEAN force. Portage runs `_global_updates` over every repo's `profiles/updates/` before the
/// resolve (`lib/_emerge/actions.py:3944-3964`) and forces
/// `AUTOCLEAN=yes` iff a changed updates file yields >=1 valid
/// directive (`lib/portage/_global_updates.py:139`); files with parse
/// errors are applied but their mtime stays unrecorded (`:131-135`).
fn updates_test_repo(tag: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let base = portage_util::TempDir::new(&format!("updates_force_{tag}")).keep();
    let repo = base.join("repo");
    let upd = repo.join("profiles").join("updates");
    std::fs::create_dir_all(&upd).unwrap();
    for (name, content) in files {
        std::fs::write(upd.join(name), content).unwrap();
    }
    repo
}

fn updates_test_repo_config(repo: &std::path::Path) -> portage_repo::RepoConfig {
    portage_repo::RepoConfig {
        name: "testrepo".to_string(),
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
    }
}

fn updates_test_root(tag: &str) -> std::path::PathBuf {
    portage_util::TempDir::new(&format!("updates_force_root_{tag}")).keep()
}

fn updates_test_config() -> portage_profile::Config {
    portage_profile::Config::default()
}

fn moves_on_opts() -> EmergeOpts<'static> {
    EmergeOpts {
        package_moves: true,
        ..Default::default()
    }
}

fn updates_file_key(repo: &std::path::Path, name: &str) -> String {
    repo.join("profiles")
        .join("updates")
        .join(name)
        .display()
        .to_string()
}

/// The target-root force through the two-root core, with a scratch
/// stdout stream.
fn target_force(
    repos: &[portage_repo::RepoConfig],
    config: &mut portage_profile::Config,
    target: &std::path::Path,
    running: &std::path::Path,
    opts: &EmergeOpts<'_>,
) -> bool {
    let mut out = Vec::new();
    updates_force_for_roots(UpdatesForRootsArgs {
        repos,
        config,
        config_root: target,
        target,
        running,
        quiet: opts.quiet,
        out: &mut out,
    })
}

#[test]
fn applied_updates_force_fires_on_a_fresh_root_and_records() {
    // Portage: fresh ROOT means an empty mtimedb["updates"], so every
    // updates file is new; the fixture-style file below yields two
    // valid directives, forcing AUTOCLEAN even though it is set
    // nowhere (make.globals default `no`).
    let repo = updates_test_repo(
        "fresh",
        &[(
            "2Q-2024",
            "move dev-libs/oldmovepkg dev-libs/newmovepkg\nslotmove dev-libs/slotmovepkg 0 1\n",
        )],
    );
    let root = updates_test_root("fresh");
    let repos = vec![updates_test_repo_config(&repo)];
    let mut config = updates_test_config();
    assert!(target_force(
        &repos,
        &mut config,
        &root,
        &root,
        &moves_on_opts()
    ));
    let record = crate::mtimedb::read_updates_mtimes(&root);
    assert!(
        record.contains_key(&updates_file_key(&repo, "2Q-2024")),
        "error-free mtimes are recorded: {record:?}"
    );
    let _ = std::fs::remove_dir_all(repo.parent().unwrap());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn applied_updates_force_is_silent_on_a_repeat_run() {
    // Portage Control 2: a second merge into the same ROOT finds the
    // record current, so no force and no tail.
    let repo = updates_test_repo(
        "repeat",
        &[("2Q-2024", "move dev-libs/oldmovepkg dev-libs/newmovepkg\n")],
    );
    let root = updates_test_root("repeat");
    let repos = vec![updates_test_repo_config(&repo)];
    let mut config = updates_test_config();
    assert!(target_force(
        &repos,
        &mut config,
        &root,
        &root,
        &moves_on_opts()
    ));
    assert!(
        !target_force(&repos, &mut config, &root, &root, &moves_on_opts()),
        "a current record must not force again"
    );
    let _ = std::fs::remove_dir_all(repo.parent().unwrap());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn applied_updates_force_skips_package_moves_n() {
    // Portage Control 1: `--package-moves=n` never calls
    // `_global_updates` -- silent, and nothing recorded.
    let repo = updates_test_repo(
        "movesn",
        &[("2Q-2024", "move dev-libs/oldmovepkg dev-libs/newmovepkg\n")],
    );
    let root = updates_test_root("movesn");
    let repos = vec![updates_test_repo_config(&repo)];
    let opts = EmergeOpts {
        package_moves: false,
        ..Default::default()
    };
    let mut config = updates_test_config();
    let mut out = Vec::new();
    assert!(!global_updates_force(GlobalUpdatesArgs {
        opts: &opts,
        repos: &repos,
        config: &mut config,
        config_root: &root,
        root: &root,
        out: &mut out,
    }));
    assert!(
        !crate::mtimedb::mtimedb_path(&root).exists(),
        "a skipped check must not create any mtimedb"
    );
    let _ = std::fs::remove_dir_all(repo.parent().unwrap());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn applied_updates_force_skips_info_sync_and_version() {
    // Portage: no `_global_updates` for the help/info/sync/version
    // actions (`actions.py:3944-3948`; `--help` returns before any
    // config load, so only the other three need pins here).
    let repo = updates_test_repo(
        "actions",
        &[("2Q-2024", "move dev-libs/oldmovepkg dev-libs/newmovepkg\n")],
    );
    let root = updates_test_root("actions");
    let repos = vec![updates_test_repo_config(&repo)];
    let mut config = updates_test_config();
    for (tag, opts) in [
        (
            "info",
            EmergeOpts {
                info_action: true,
                package_moves: true,
                ..Default::default()
            },
        ),
        (
            "sync",
            EmergeOpts {
                sync_action: true,
                package_moves: true,
                ..Default::default()
            },
        ),
        (
            "version",
            EmergeOpts {
                version_action: true,
                package_moves: true,
                ..Default::default()
            },
        ),
    ] {
        let mut out = Vec::new();
        assert!(
            !global_updates_force(GlobalUpdatesArgs {
                opts: &opts,
                repos: &repos,
                config: &mut config,
                config_root: &root,
                root: &root,
                out: &mut out,
            }),
            "{tag} must skip"
        );
    }
    assert!(
        !crate::mtimedb::mtimedb_path(&root).exists(),
        "skipped actions must not create any mtimedb"
    );
    let _ = std::fs::remove_dir_all(repo.parent().unwrap());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn applied_updates_error_file_fires_but_stays_unrecorded() {
    // Portage `_global_updates.py:131-135`: a file with parse errors
    // still contributes its valid directives to the force, but its
    // mtime is never recorded -- so it forces again on every run.
    let repo = updates_test_repo(
        "errorfile",
        &[(
            "2Q-2024",
            "move dev-libs/oldmovepkg dev-libs/newmovepkg\nbogus line here\n",
        )],
    );
    let root = updates_test_root("errorfile");
    let repos = vec![updates_test_repo_config(&repo)];
    let mut config = updates_test_config();
    assert!(target_force(
        &repos,
        &mut config,
        &root,
        &root,
        &moves_on_opts()
    ));
    assert!(
        !crate::mtimedb::read_updates_mtimes(&root)
            .contains_key(&updates_file_key(&repo, "2Q-2024")),
        "an error file's mtime must stay unrecorded"
    );
    assert!(
        target_force(&repos, &mut config, &root, &root, &moves_on_opts()),
        "the unrecorded file forces again next run"
    );
    let _ = std::fs::remove_dir_all(repo.parent().unwrap());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn applied_updates_directiveless_file_stays_silent() {
    // Portage `_global_updates.py:139`: the force needs >=1 valid
    // directive -- blank lines and unknown commands alone force
    // nothing, and without a force nothing is committed.
    for (tag, content) in [
        ("blanks", "\n   \n"),
        ("unknown", "frob dev-libs/a dev-libs/b\n"),
    ] {
        let repo = updates_test_repo(tag, &[("9Q-2099", content)]);
        let root = updates_test_root(tag);
        let repos = vec![updates_test_repo_config(&repo)];
        let mut config = updates_test_config();
        assert!(!target_force(
            &repos,
            &mut config,
            &root,
            &root,
            &moves_on_opts()
        ));
        assert!(
            !crate::mtimedb::mtimedb_path(&root).exists(),
            "{tag}: no force means no commit"
        );
        let _ = std::fs::remove_dir_all(repo.parent().unwrap());
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[test]
fn applied_updates_running_root_record_never_forces_the_tail() {
    // Portage forces AUTOCLEAN on the root_config whose updates fired
    // (`actions.py:3958-3962`) while the tail reads the *target*
    // settings (`action_build`, unpacked from `emerge_config`), so a
    // running-root-only force stays silent on the tail -- but its
    // record is still maintained.
    let repo = updates_test_repo(
        "running",
        &[("2Q-2024", "move dev-libs/oldmovepkg dev-libs/newmovepkg\n")],
    );
    let target = updates_test_root("running_target");
    let running = updates_test_root("running_running");
    let repos = vec![updates_test_repo_config(&repo)];
    // Pre-record the target root only: its check is current while the
    // running root's is fresh.
    let key = updates_file_key(&repo, "2Q-2024");
    let mtime = std::fs::metadata(repo.join("profiles").join("updates").join("2Q-2024"))
        .unwrap()
        .mtime();
    let mut pre: BTreeMap<String, i64> = BTreeMap::new();
    pre.insert(key, mtime);
    crate::mtimedb::write_updates_mtimes(&target, &pre).unwrap();
    let mut config = updates_test_config();
    assert!(
        !target_force(&repos, &mut config, &target, &running, &moves_on_opts()),
        "a running-root-only force must not read as target force"
    );
    assert!(
        crate::mtimedb::read_updates_mtimes(&running).len() == 1,
        "the running record is still maintained"
    );
    let _ = std::fs::remove_dir_all(repo.parent().unwrap());
    let _ = std::fs::remove_dir_all(&target);
    let _ = std::fs::remove_dir_all(&running);
}

#[test]
fn applied_updates_force_survives_an_unwritable_root() {
    // Portage's `MtimeDB.__write_to_disk` swallows `OSError`
    // (`util/mtimedb.py`): a non-writable record still forces, and the
    // run never fails on the record.
    let repo = updates_test_repo(
        "readonly",
        &[("2Q-2024", "move dev-libs/oldmovepkg dev-libs/newmovepkg\n")],
    );
    let root = updates_test_root("readonly");
    let edb = root.join("var/cache/edb");
    std::fs::create_dir_all(&edb).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&edb, std::fs::Permissions::from_mode(0o555)).unwrap();
    }
    let repos = vec![updates_test_repo_config(&repo)];
    let mut config = updates_test_config();
    assert!(target_force(
        &repos,
        &mut config,
        &root,
        &root,
        &moves_on_opts()
    ));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&edb, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let _ = std::fs::remove_dir_all(repo.parent().unwrap());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn applied_updates_check_leaves_a_portage_written_mtimedb_untouched() {
    // Portage commits the `updates` mtimes only after applying the
    // moves (`lib/portage/_global_updates.py`, `prev_mtimes.update`
    // inside `if retupd`); a record that already matches the files
    // means nothing is applied, so a Portage-written section, foreign
    // keys included, comes back byte-identical. `grab_updates`
    // (`lib/portage/update.py`) is what treats a record-mismatched
    // file as changed.
    let repo = updates_test_repo("readonly", &[("1Q-2026", "move a/old a/new\n")]);
    let root = updates_test_root("readonly");
    // An installed `a/old`: the entry Portage would move (and only
    // then record for).
    std::fs::create_dir_all(root.join("var/db/pkg/a/old-1.0")).unwrap();
    let key = updates_file_key(&repo, "1Q-2026");
    let mtime = std::fs::metadata(repo.join("profiles").join("updates").join("1Q-2026"))
        .unwrap()
        .mtime();
    // A Portage-shaped file whose stamp for this updates file already
    // matches, so the file counts as applied.
    let seed = format!(
        "{{\n\t\"ldpath\": {{\n\t\t\"/lib\": 1700000001\n\t}},\n\t\"updates\": {{\n\t\t\"{key}\": {mtime}\n\t}}\n}}"
    );
    let mtimedb = crate::mtimedb::mtimedb_path(&root);
    std::fs::create_dir_all(mtimedb.parent().unwrap()).unwrap();
    std::fs::write(&mtimedb, &seed).unwrap();
    let before = std::fs::read(&mtimedb).unwrap();
    let repos = vec![updates_test_repo_config(&repo)];
    let mut config = updates_test_config();
    assert!(!target_force(
        &repos,
        &mut config,
        &root,
        &root,
        &moves_on_opts()
    ));
    assert_eq!(std::fs::read(&mtimedb).unwrap(), before);
    let _ = std::fs::remove_dir_all(repo.parent().unwrap());
    let _ = std::fs::remove_dir_all(&root);
}
