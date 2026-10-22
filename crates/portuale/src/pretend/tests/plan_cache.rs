//! Tests for the `--pretend` plan cache (backlog #441 S3).
//!
//! Strategy: subprocess runs of the built binary (like every other pretend
//! test here), comparing a cached run against a cache-off baseline. A
//! sensitivity test per key component warms the cache, mutates exactly one
//! input, and asserts the next run stores a new entry whose output still
//! matches the cache-off baseline. Each test was checked to fail when its
//! key component is removed (see the code note); a test passing either way
//! would be vacuous.

use super::*;
use crate::pretend::plan_cache;

/// A scratch tree for one cache test: the base (deleted by the caller), a
/// `ROOT`, a `PORTAGE_TMPDIR` and an empty cache dir.
fn cache_test_tree(tag: &str) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let base = TempDir::new(&format!("plan_cache_{tag}")).keep();
    let root = base.join("root");
    std::fs::create_dir_all(&root).unwrap();
    let cache_dir = base.join("cache");
    std::fs::create_dir_all(&cache_dir).unwrap();
    (base, root, cache_dir)
}

/// Child env: the fixture resolve env plus the cache dir and the debug
/// switch. The harness's own cache controls and debug switches are removed
/// so they can neither disable the cache nor pollute compared stderr.
fn cache_child_env(
    root: &std::path::Path,
    portage_tmpdir: &std::path::Path,
    cache_dir: &std::path::Path,
) -> Vec<(String, String)> {
    let mut env = fixture_resolve_env(root, portage_tmpdir);
    env.push((
        "PORTUALE_CACHE_DIR".to_string(),
        cache_dir.display().to_string(),
    ));
    env.push(("PORTUALE_CACHE_DEBUG".to_string(), "1".to_string()));
    env
}

/// The same env with the cache switched off (baseline runs).
fn cache_off_env(env: &[(String, String)]) -> Vec<(String, String)> {
    env.iter()
        .filter(|(k, _)| k != "PORTUALE_CACHE_DIR" && k != "PORTUALE_CACHE_DEBUG")
        .cloned()
        .collect()
}

/// Run `emerge <args>` with `env`, piped stdio, null stdin.
fn cache_run(
    env: &[(String, String)],
    args: &[&str],
    cwd: Option<&std::path::Path>,
) -> std::process::Output {
    let mut cmd = std::process::Command::new(built_portuale_bin());
    cmd.arg("emerge");
    cmd.args(args);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.envs(env.to_vec());
    // Never inherit the harness's own cache controls or debug switches;
    // an explicitly passed test value always wins over the removal.
    for var in [
        "PORTUALE_NO_CACHE",
        "PORTUALE_CACHE_VERIFY",
        "PORTUALE_PROFILE",
        "PORTUALE_REPORT_UNPARSED_DEP_TOKENS",
    ] {
        if !env.iter().any(|(k, _)| k == var) {
            cmd.env_remove(var);
        }
    }
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.output().expect("portuale emerge spawns")
}

/// The `portuale plan cache: …` lines of a captured stderr, prefix stripped.
fn cache_debug_lines(stderr: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(stderr)
        .lines()
        .filter_map(|l| l.strip_prefix("portuale plan cache: ").map(str::to_string))
        .collect()
}

/// Stderr without the debug lines: what a cache-off run prints.
fn strip_cache_debug(stderr: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(stderr);
    let mut out = String::new();
    for line in text.split_inclusive('\n') {
        if !line.starts_with("portuale plan cache: ") {
            out.push_str(line);
        }
    }
    out.into_bytes()
}

/// The 64-hex key out of a `stored`/`hit` debug line, if any.
fn cache_key(stderr: &[u8]) -> Option<String> {
    cache_debug_lines(stderr).iter().find_map(|l| {
        let at = l.find("key=")?;
        let key = &l[at + 4..];
        (key.len() >= 64 && key[..64].chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| key[..64].to_string())
    })
}

/// Assert `output` is a hit: one `hit` debug line, and stdout/stderr/rc
/// equal the baseline's.
fn assert_hit(output: &std::process::Output, baseline: &std::process::Output) {
    let debug = cache_debug_lines(&output.stderr);
    assert_eq!(debug.len(), 1, "one debug line, got: {debug:?}");
    assert!(
        debug[0].starts_with("hit (key="),
        "expected a hit, got: {debug:?}"
    );
    assert_eq!(
        output.stdout, baseline.stdout,
        "hit stdout must equal fresh"
    );
    assert_eq!(
        strip_cache_debug(&output.stderr),
        strip_cache_debug(&baseline.stderr),
        "hit stderr must equal fresh"
    );
    assert_eq!(
        output.status.code(),
        baseline.status.code(),
        "hit exit code must equal fresh"
    );
}

/// Assert `output` is a cold store (one `stored` debug line) and still
/// matches the cache-off baseline run with the same mutated inputs.
fn assert_cold_matches_baseline(output: &std::process::Output, baseline: &std::process::Output) {
    let debug = cache_debug_lines(&output.stderr);
    assert_eq!(debug.len(), 1, "one debug line, got: {debug:?}");
    assert!(
        debug[0].starts_with("stored (key="),
        "expected a cold store, got: {debug:?}"
    );
    assert_eq!(
        output.stdout, baseline.stdout,
        "store stdout must equal fresh"
    );
    assert_eq!(
        strip_cache_debug(&output.stderr),
        strip_cache_debug(&baseline.stderr),
        "store stderr must equal fresh"
    );
    assert_eq!(
        output.status.code(),
        baseline.status.code(),
        "store exit code must equal fresh"
    );
}

#[test]
#[cfg(unix)]
fn replay_equals_fresh_on_several_argv() {
    // The core promise: a hit replays byte-identical bytes and code.
    for (tag, args) in [
        ("plain", vec!["-p", "dev-libs/schedleaf-a"]),
        ("clean", vec!["-p", "dev-libs/newpkg"]),
        ("verbose", vec!["-pv", "dev-libs/schedleaf-a"]),
        ("quiet", vec!["-pq", "dev-libs/schedleaf-a"]),
        ("tree", vec!["-p", "-t", "dev-libs/schedleaf-a"]),
        (
            "two_atoms",
            vec!["-p", "dev-libs/schedleaf-a", "dev-libs/schedleaf-b"],
        ),
    ] {
        let (base, root, cache_dir) = cache_test_tree(tag);
        let pt = base.join("pt");
        let env = cache_child_env(&root, &pt, &cache_dir);
        let miss = cache_run(&env, &args, None);
        let key = cache_key(&miss.stderr).expect("first run must store");
        assert!(
            cache_dir
                .join("plans")
                .join(format!("{key}.plan"))
                .is_file(),
            "{tag}: one file per key"
        );
        let hit = cache_run(&env, &args, None);
        let baseline = cache_run(&cache_off_env(&env), &args, None);
        assert_hit(&hit, &baseline);
        assert!(
            cache_debug_lines(&baseline.stderr).is_empty(),
            "{tag}: cache off prints no debug lines"
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}

#[test]
#[cfg(unix)]
fn sensitivity_argv() {
    // A different atom is a different key: cold store, but the same bytes
    // a cache-off run prints.
    let (base, root, cache_dir) = cache_test_tree("argv");
    let pt = base.join("pt");
    let env = cache_child_env(&root, &pt, &cache_dir);
    let _ = cache_run(&env, &["-p", "dev-libs/schedleaf-a"], None);
    let miss = cache_run(&env, &["-p", "dev-libs/schedleaf-b"], None);
    let baseline = cache_run(&cache_off_env(&env), &["-p", "dev-libs/schedleaf-b"], None);
    assert_cold_matches_baseline(&miss, &baseline);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn sensitivity_env_columnwidth() {
    // `COLUMNWIDTH` changes `--columns` padding: cold store, fresh bytes.
    let (base, root, cache_dir) = cache_test_tree("columnwidth");
    let pt = base.join("pt");
    let args = ["-p", "--columns", "dev-libs/schedleaf-a"];
    let mut wide = cache_child_env(&root, &pt, &cache_dir);
    wide.push(("COLUMNWIDTH".to_string(), "200".to_string()));
    let _ = cache_run(&wide, &args, None);
    let mut narrow = cache_child_env(&root, &pt, &cache_dir);
    narrow.push(("COLUMNWIDTH".to_string(), "80".to_string()));
    let miss = cache_run(&narrow, &args, None);
    let baseline = cache_run(&cache_off_env(&narrow), &args, None);
    assert_cold_matches_baseline(&miss, &baseline);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn sensitivity_cwd() {
    // The working directory feeds the key (the `./path` atom check): a run
    // from elsewhere stores cold, with identical bytes.
    let (base, root, cache_dir) = cache_test_tree("cwd");
    let pt = base.join("pt");
    let env = cache_child_env(&root, &pt, &cache_dir);
    let args = ["-p", "dev-libs/schedleaf-a"];
    let _ = cache_run(&env, &args, None);
    let elsewhere = base.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let miss = cache_run(&env, &args, Some(&elsewhere));
    let baseline = cache_run(&cache_off_env(&env), &args, Some(&elsewhere));
    assert_cold_matches_baseline(&miss, &baseline);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn sensitivity_root() {
    // A different `ROOT` stores cold; each root still matches its own
    // baseline (the root path embeds in the display).
    let (base, root, cache_dir) = cache_test_tree("root");
    let pt = base.join("pt");
    let env = cache_child_env(&root, &pt, &cache_dir);
    let args = ["-p", "dev-libs/schedleaf-a"];
    let _ = cache_run(&env, &args, None);
    let root2 = base.join("root2");
    std::fs::create_dir_all(&root2).unwrap();
    let mut env2 = env.clone();
    for (k, v) in env2.iter_mut() {
        if k == "ROOT" {
            *v = root2.display().to_string();
        }
    }
    let miss = cache_run(&env2, &args, None);
    let baseline = cache_run(&cache_off_env(&env2), &args, None);
    assert_cold_matches_baseline(&miss, &baseline);
    let _ = std::fs::remove_dir_all(&base);
}

/// A scratch `PORTAGE_CONFIGROOT`: the fixture `etc/portage` copied, the
/// repos symlinked in (their `location` lines are config-root-relative, and
/// `make.profile`'s relative link resolves through the `repo` symlink).
#[cfg(unix)]
fn scratch_configroot(base: &std::path::Path) -> std::path::PathBuf {
    let cfg = base.join("cfgroot");
    copy_tree(
        &fixtures_root().join("etc/portage"),
        &cfg.join("etc/portage"),
    );
    for repo in [
        "repo",
        "overlay",
        "independentoverlay",
        "layoutmasteroverlay",
        "repnamerepo",
    ] {
        std::os::unix::fs::symlink(fixtures_root().join(repo), cfg.join(repo)).unwrap();
    }
    cfg
}

#[test]
#[cfg(unix)]
fn sensitivity_config_use() {
    // A `make.conf` change stores cold even when the output is unchanged.
    let (base, root, cache_dir) = cache_test_tree("config_use");
    let pt = base.join("pt");
    let cfgroot = scratch_configroot(&base);
    let mut env = cache_child_env(&root, &pt, &cache_dir);
    for (k, v) in env.iter_mut() {
        if k == "PORTAGE_CONFIGROOT" {
            *v = cfgroot.display().to_string();
        }
    }
    let args = ["-p", "dev-libs/schedleaf-a"];
    let _ = cache_run(&env, &args, None);
    let make_conf = cfgroot.join("etc/portage/make.conf");
    let mut text = std::fs::read_to_string(&make_conf).unwrap();
    text.push_str("\nUSE=\"sensitivity-probe-flag\"\n");
    std::fs::write(&make_conf, text).unwrap();
    let miss = cache_run(&env, &args, None);
    let baseline = cache_run(&cache_off_env(&env), &args, None);
    assert_cold_matches_baseline(&miss, &baseline);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn sensitivity_config_mask_changes_output() {
    // Masking the target changes the plan itself: the all-masked hard
    // error has no `GraphResult`, so nothing is stored — but the fresh
    // abort disclosure must print, matching the cache-off baseline.
    let (base, root, cache_dir) = cache_test_tree("config_mask");
    let pt = base.join("pt");
    let cfgroot = scratch_configroot(&base);
    let mut env = cache_child_env(&root, &pt, &cache_dir);
    for (k, v) in env.iter_mut() {
        if k == "PORTAGE_CONFIGROOT" {
            *v = cfgroot.display().to_string();
        }
    }
    let args = ["-p", "dev-libs/schedleaf-a"];
    let before = cache_run(&env, &args, None);
    assert!(cache_key(&before.stderr).is_some());
    let mask = cfgroot.join("etc/portage/package.mask");
    let mut text = std::fs::read_to_string(&mask).unwrap();
    text.push_str("\ndev-libs/schedleaf-a\n");
    std::fs::write(&mask, text).unwrap();
    let miss = cache_run(&env, &args, None);
    let baseline = cache_run(&cache_off_env(&env), &args, None);
    let debug = cache_debug_lines(&miss.stderr);
    assert!(
        debug.iter().any(|l| l.contains("skip store")),
        "the hard error stores nothing, got: {debug:?}"
    );
    assert_eq!(
        miss.stdout, baseline.stdout,
        "fresh abort disclosure must print"
    );
    assert_eq!(miss.status.code(), baseline.status.code());
    assert_ne!(
        miss.stdout, before.stdout,
        "the mask must change the plan itself"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn sensitivity_repo_timestamp() {
    // A rewritten `timestamp.chk` stores cold without changing a byte of
    // output. The overlay carries no packages, so only its identity feeds
    // the key.
    let (base, root, cache_dir) = cache_test_tree("repo_timestamp");
    let pt = base.join("pt");
    let overlay = base.join("overlayts");
    std::fs::create_dir_all(overlay.join("metadata")).unwrap();
    std::fs::create_dir_all(overlay.join("profiles")).unwrap();
    std::fs::write(
        overlay.join("metadata/layout.conf"),
        "masters =\nprofile-formats = portage-2\n",
    )
    .unwrap();
    std::fs::write(overlay.join("profiles/repo_name"), "tsoverlay\n").unwrap();
    std::fs::write(overlay.join("metadata/timestamp.chk"), "old\n").unwrap();
    let cfgroot = scratch_configroot(&base);
    std::os::unix::fs::symlink(&overlay, cfgroot.join("overlayts")).unwrap();
    let repos_conf = cfgroot.join("etc/portage/repos.conf/repos.conf");
    let mut repos_text = std::fs::read_to_string(&repos_conf).unwrap();
    repos_text.push_str("\n[tsoverlay]\nlocation = overlayts\n");
    std::fs::write(&repos_conf, repos_text).unwrap();
    let mut env = cache_child_env(&root, &pt, &cache_dir);
    for (k, v) in env.iter_mut() {
        if k == "PORTAGE_CONFIGROOT" {
            *v = cfgroot.display().to_string();
        }
    }
    let args = ["-p", "dev-libs/schedleaf-a"];
    let first = cache_run(&env, &args, None);
    assert!(
        cache_key(&first.stderr).is_some(),
        "the overlay must resolve: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    std::fs::write(overlay.join("metadata/timestamp.chk"), "new\n").unwrap();
    let miss = cache_run(&env, &args, None);
    let baseline = cache_run(&cache_off_env(&env), &args, None);
    assert_cold_matches_baseline(&miss, &baseline);
    assert_eq!(
        miss.stdout, first.stdout,
        "a timestamp bump alone changes no output"
    );
    // `layout.conf` (a comment: same parsed values), the `profiles/` tree
    // (an inert file) and a new md5-cache category each store cold with
    // identical bytes.
    let layout = overlay.join("metadata/layout.conf");
    let mut layout_text = std::fs::read_to_string(&layout).unwrap();
    layout_text.push_str("# plan-cache probe comment\n");
    std::fs::write(&layout, layout_text).unwrap();
    let miss = cache_run(&env, &args, None);
    let baseline = cache_run(&cache_off_env(&env), &args, None);
    assert_cold_matches_baseline(&miss, &baseline);
    std::fs::write(overlay.join("profiles/probe-note.txt"), "inert\n").unwrap();
    let miss = cache_run(&env, &args, None);
    let baseline = cache_run(&cache_off_env(&env), &args, None);
    assert_cold_matches_baseline(&miss, &baseline);
    let md5cat = overlay.join("metadata/md5-cache/probecat");
    std::fs::create_dir_all(&md5cat).unwrap();
    std::fs::write(md5cat.join("probe-1.0"), "EAPI=8\nSLOT=0\n").unwrap();
    let miss = cache_run(&env, &args, None);
    let baseline = cache_run(&cache_off_env(&env), &args, None);
    assert_cold_matches_baseline(&miss, &baseline);
    assert_eq!(
        miss.stdout, first.stdout,
        "repo bookkeeping alone changes no output"
    );
    // And a git identity: the overlay gains a `.git/HEAD` naming a ref, and
    // moving the ref stores cold.
    let git = overlay.join(".git");
    std::fs::create_dir_all(git.join("refs/heads")).unwrap();
    std::fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(git.join("refs/heads/main"), "aaa\n").unwrap();
    let miss = cache_run(&env, &args, None);
    let baseline = cache_run(&cache_off_env(&env), &args, None);
    assert_cold_matches_baseline(&miss, &baseline);
    std::fs::write(git.join("refs/heads/main"), "bbb\n").unwrap();
    let miss = cache_run(&env, &args, None);
    let baseline = cache_run(&cache_off_env(&env), &args, None);
    assert_cold_matches_baseline(&miss, &baseline);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn sensitivity_world() {
    // The world file feeds the key: changing it stores cold, and `@world`
    // expands the new member into the plan.
    let (base, root, cache_dir) = cache_test_tree("world");
    let pt = base.join("pt");
    let env = cache_child_env(&root, &pt, &cache_dir);
    let world_dir = root.join("var/lib/portage");
    std::fs::create_dir_all(&world_dir).unwrap();
    std::fs::write(world_dir.join("world"), "dev-libs/schedleaf-a\n").unwrap();
    let args = ["-p", "@world"];
    let first = cache_run(&env, &args, None);
    assert!(cache_key(&first.stderr).is_some());
    std::fs::write(world_dir.join("world"), "").unwrap();
    let miss = cache_run(&env, &args, None);
    let baseline = cache_run(&cache_off_env(&env), &args, None);
    assert_cold_matches_baseline(&miss, &baseline);
    let baseline = cache_run(&cache_off_env(&env), &args, None);
    assert_cold_matches_baseline(&miss, &baseline);
    assert_ne!(
        miss.stdout, first.stdout,
        "emptying world must change the @world plan"
    );
    // And the `world_sets` file: naming a real set changes the seeds (and
    // the plan), so it stores cold like any other key change. (An
    // unresolvable name fails in `expand_targets`, before the cache point,
    // on every path -- deterministically uncached.)
    std::fs::write(world_dir.join("world_sets"), "@nestedtestset\n").unwrap();
    let miss = cache_run(&env, &args, None);
    let baseline = cache_run(&cache_off_env(&env), &args, None);
    assert_cold_matches_baseline(&miss, &baseline);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn sensitivity_pkgdir() {
    // A new local `Packages` index stores cold; the pool was empty before.
    let (base, root, cache_dir) = cache_test_tree("pkgdir");
    let pt = base.join("pt");
    let cfgroot = scratch_configroot(&base);
    let pkgdir = cfgroot.join("pkgdir");
    std::fs::create_dir_all(&pkgdir).unwrap();
    let mut env = cache_child_env(&root, &pt, &cache_dir);
    for (k, v) in env.iter_mut() {
        if k == "PORTAGE_CONFIGROOT" {
            *v = cfgroot.display().to_string();
        }
    }
    let args = ["-p", "dev-libs/schedleaf-a"];
    let _ = cache_run(&env, &args, None);
    std::fs::write(
        pkgdir.join("Packages"),
        "TIMESTAMP: 1\nVERSION: 0\nPACKAGES: 0\n",
    )
    .unwrap();
    let miss = cache_run(&env, &args, None);
    let baseline = cache_run(&cache_off_env(&env), &args, None);
    assert_cold_matches_baseline(&miss, &baseline);
    // An unindexed file moves only the listing fingerprint; the pool (and
    // the output) does not move.
    let before = miss.stdout.clone();
    std::fs::write(pkgdir.join("top-level-junk.txt"), "junk\n").unwrap();
    let miss = cache_run(&env, &args, None);
    let baseline = cache_run(&cache_off_env(&env), &args, None);
    assert_cold_matches_baseline(&miss, &baseline);
    assert_eq!(miss.stdout, before, "an unindexed file changes no output");
    // Same bytes on disk, new index content: with the mtime restored, only
    // the content hash moves.
    let packages = pkgdir.join("Packages");
    let mtime = std::fs::metadata(&packages).unwrap().modified().unwrap();
    std::fs::write(&packages, "TIMESTAMP: 2\nVERSION: 0\nPACKAGES: 0\n").unwrap();
    filetime::set_file_mtime(&packages, filetime::FileTime::from_system_time(mtime)).unwrap();
    let miss = cache_run(&env, &args, None);
    let baseline = cache_run(&cache_off_env(&env), &args, None);
    assert_cold_matches_baseline(&miss, &baseline);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn sensitivity_distdir() {
    // DISTDIR presence+size gates the totals: a new file stores cold.
    let (base, root, cache_dir) = cache_test_tree("distdir");
    let pt = base.join("pt");
    let distdir = base.join("dist");
    std::fs::create_dir_all(&distdir).unwrap();
    let mut env = cache_child_env(&root, &pt, &cache_dir);
    for (k, v) in env.iter_mut() {
        if k == "DISTDIR" {
            *v = distdir.display().to_string();
        }
    }
    let args = ["-p", "dev-libs/schedleaf-a"];
    let _ = cache_run(&env, &args, None);
    std::fs::write(distdir.join("probe-1.0.tar.gz"), vec![0u8; 64]).unwrap();
    let miss = cache_run(&env, &args, None);
    let baseline = cache_run(&cache_off_env(&env), &args, None);
    assert_cold_matches_baseline(&miss, &baseline);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn sensitivity_tty_stdout() {
    // A tty stdout renders differently (colour) and keys differently: the
    // pty run stores cold against the piped cache entry, matching a
    // cache-off pty run byte for byte.
    let (base, root, cache_dir) = cache_test_tree("tty");
    let pt = base.join("pt");
    let env = cache_child_env(&root, &pt, &cache_dir);
    let args = ["-p", "dev-libs/schedleaf-a"];
    let _ = cache_run(&env, &args, None);
    fn pty_run(
        args: &[&str],
        env: &[(String, String)],
    ) -> (Vec<u8>, Vec<u8>, std::process::ExitStatus) {
        use std::io::Read as _;
        // The slave closed reads as EIO, not EOF, on Linux ptys.
        fn drain_pty(master: &mut std::fs::File) -> Vec<u8> {
            let mut out = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                match master.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => out.extend_from_slice(&buf[..n]),
                    Err(e) if e.raw_os_error() == Some(libc::EIO) => break,
                    Err(e) => panic!("drain pty stdout: {e}"),
                }
            }
            out
        }
        let (mut master, slave_stdio) = pty_pair();
        let mut child = std::process::Command::new(built_portuale_bin())
            .arg("emerge")
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(slave_stdio)
            .stderr(std::process::Stdio::piped())
            .envs(env.to_vec())
            .spawn()
            .expect("portuale emerge spawns on a pty");
        let pty_out = drain_pty(&mut master);
        let mut err = Vec::new();
        child
            .stderr
            .take()
            .expect("piped stderr")
            .read_to_end(&mut err)
            .expect("drain stderr");
        let status = child.wait().expect("reap pty child");
        (pty_out, err, status)
    }
    let (pty_out, err, status) = pty_run(&args, &env);
    let debug = cache_debug_lines(&err);
    assert_eq!(debug.len(), 1, "one debug line, got: {debug:?}");
    assert!(
        debug[0].starts_with("stored (key="),
        "a tty stdout must store cold against the piped entry, got: {debug:?}"
    );
    let (pty_baseline, err_baseline, status_baseline) = pty_run(&args, &cache_off_env(&env));
    assert_eq!(
        pty_out, pty_baseline,
        "pty bytes must equal the fresh pty run"
    );
    assert_eq!(status.code(), status_baseline.code());
    assert_eq!(strip_cache_debug(&err), strip_cache_debug(&err_baseline));
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn ask_requested_never_caches() {
    // `--ask` is dropped under `--pretend`, but the cache still refuses a
    // run that asked: the debug line says why, and nothing is stored.
    let (base, root, cache_dir) = cache_test_tree("ask");
    let pt = base.join("pt");
    let env = cache_child_env(&root, &pt, &cache_dir);
    let out = cache_run(&env, &["-p", "--ask", "dev-libs/schedleaf-a"], None);
    let debug = cache_debug_lines(&out.stderr);
    assert_eq!(debug.len(), 1, "one debug line, got: {debug:?}");
    assert!(debug[0].contains("off (--ask requested)"), "{debug:?}");
    assert!(
        std::fs::read_dir(cache_dir.join("plans"))
            .map(|mut d| d.next().is_none())
            .unwrap_or(true),
        "an --ask run stores nothing"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn no_cache_flag_disables() {
    // `--no-cache` (and `PORTUALE_NO_CACHE=1`) bypass the cache entirely.
    let (base, root, cache_dir) = cache_test_tree("nocache");
    let pt = base.join("pt");
    let env = cache_child_env(&root, &pt, &cache_dir);
    let out = cache_run(&env, &["-p", "--no-cache", "dev-libs/schedleaf-a"], None);
    let debug = cache_debug_lines(&out.stderr);
    assert_eq!(debug.len(), 1, "one debug line, got: {debug:?}");
    assert!(debug[0].contains("off (--no-cache)"), "{debug:?}");
    let mut env_no = env.clone();
    env_no.push(("PORTUALE_NO_CACHE".to_string(), "1".to_string()));
    let out = cache_run(&env_no, &["-p", "dev-libs/schedleaf-a"], None);
    let debug = cache_debug_lines(&out.stderr);
    assert!(debug[0].contains("off (PORTUALE_NO_CACHE)"), "{debug:?}");
    assert!(
        std::fs::read_dir(cache_dir.join("plans"))
            .map(|mut d| d.next().is_none())
            .unwrap_or(true),
        "disabled runs store nothing"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn non_pretend_runs_never_cache() {
    // A real (non-`--pretend`) run is out of scope even when it resolves:
    // `--fetchonly` of a sourceless package fetches nothing (hermetic),
    // prints the refusal reason, and stores nothing.
    let (base, root, cache_dir) = cache_test_tree("nonpretend");
    let pt = base.join("pt");
    let env = cache_child_env(&root, &pt, &cache_dir);
    let out = cache_run(&env, &["--fetchonly", "dev-libs/schedleaf-a"], None);
    let debug = cache_debug_lines(&out.stderr);
    assert_eq!(debug.len(), 1, "one debug line, got: {debug:?}");
    assert!(debug[0].contains("off (not a --pretend run)"), "{debug:?}");
    assert!(
        std::fs::read_dir(cache_dir.join("plans"))
            .map(|mut d| d.next().is_none())
            .unwrap_or(true),
        "a real run stores nothing"
    );
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn unusable_cache_dir_disables() {
    // A `PORTUALE_CACHE_DIR` that cannot hold the store disables caching
    // instead of failing the run.
    let (base, root, _cache_dir) = cache_test_tree("baddir");
    let pt = base.join("pt");
    let blocker = base.join("blocker");
    std::fs::write(&blocker, b"not a dir").unwrap();
    let env = cache_child_env(&root, &pt, &blocker);
    let out = cache_run(&env, &["-p", "dev-libs/schedleaf-a"], None);
    let debug = cache_debug_lines(&out.stderr);
    assert_eq!(debug.len(), 1, "one debug line, got: {debug:?}");
    assert!(
        debug[0].contains("off (no writable cache dir)"),
        "{debug:?}"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn group_writable_entry_is_a_miss() {
    // Poisoning safety: a group/world-writable cache file is never replayed.
    let (base, root, cache_dir) = cache_test_tree("perms");
    let pt = base.join("pt");
    let env = cache_child_env(&root, &pt, &cache_dir);
    let first = cache_run(&env, &["-p", "dev-libs/schedleaf-a"], None);
    let key = cache_key(&first.stderr).expect("stored line carries the key");
    let entry = cache_dir.join("plans").join(format!("{key}.plan"));
    assert!(entry.is_file());
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o664)).unwrap();
    let miss = cache_run(&env, &["-p", "dev-libs/schedleaf-a"], None);
    let debug = cache_debug_lines(&miss.stderr);
    assert!(
        debug.iter().any(|l| l.contains("group/world-writable")),
        "writable entry must miss, got: {debug:?}"
    );
    let baseline = cache_run(&cache_off_env(&env), &["-p", "dev-libs/schedleaf-a"], None);
    assert_eq!(miss.stdout, baseline.stdout);
    assert_eq!(miss.status.code(), baseline.status.code());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn news_notice_stays_live_on_a_hit() {
    // The GLEP 42 notice is live output: marking one item read between two
    // identical runs keeps the plan part a hit while the notice changes.
    let (base, root, _repos, _config) = news_notice_test_tree("cache_live");
    let pt = base.join("pt");
    let cachedir = base.join("cache");
    std::fs::create_dir_all(&cachedir).unwrap();
    let mut env = news_resolve_env(&root, &pt);
    env.push((
        "PORTUALE_CACHE_DIR".to_string(),
        cachedir.display().to_string(),
    ));
    env.push(("PORTUALE_CACHE_DEBUG".to_string(), "1".to_string()));
    let run = |env: &[(String, String)]| {
        std::process::Command::new(built_portuale_bin())
            .args(["emerge", "-p", "dev-libs/schedleaf-a"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .envs(env.to_vec())
            .output()
            .expect("portuale emerge spawns")
    };
    let first = run(&env);
    let first_out = String::from_utf8_lossy(&first.stdout).into_owned();
    assert!(
        first_out.contains("news items need reading"),
        "fixture news must fire: {first_out}"
    );
    assert!(
        cache_debug_lines(&first.stderr)
            .iter()
            .any(|l| l.starts_with("stored (key=")),
        "first run stores"
    );
    // Mark one item read: the plan inputs did not move, only the live news.
    let unread_path = root.join("var/lib/gentoo/news/news-testrepo.unread");
    let unread = std::fs::read_to_string(&unread_path).unwrap();
    let mut ids: Vec<&str> = unread.lines().collect();
    assert!(!ids.is_empty());
    let read_id = ids.pop().unwrap().to_string();
    std::fs::write(
        root.join("var/lib/gentoo/news/news-testrepo.read"),
        format!("{read_id}\n"),
    )
    .unwrap();
    let second = run(&env);
    let debug = cache_debug_lines(&second.stderr);
    assert!(
        debug.iter().any(|l| l.starts_with("hit (key=")),
        "the plan part must still hit, got: {debug:?}"
    );
    let second_out = String::from_utf8_lossy(&second.stdout).into_owned();
    assert_ne!(first_out, second_out, "the live notice must change");
    // Everything but the count is the replayed plan.
    let unread_before: usize = unread.lines().count();
    let expect = first_out.replace(
        &format!("{unread_before} news items"),
        &format!("{} news items", unread_before - 1),
    );
    assert_eq!(second_out, expect, "only the live count may differ");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn verify_mode_accepts_a_hit() {
    // `PORTUALE_CACHE_VERIFY=1` recomputes on a hit and stays silent on a
    // match: same bytes, same code, no exit 99.
    let (base, root, cache_dir) = cache_test_tree("verify_ok");
    let pt = base.join("pt");
    let env = cache_child_env(&root, &pt, &cache_dir);
    let args = ["-p", "dev-libs/schedleaf-a"];
    let _ = cache_run(&env, &args, None);
    let mut verify_env = env.clone();
    verify_env.push(("PORTUALE_CACHE_VERIFY".to_string(), "1".to_string()));
    let out = cache_run(&verify_env, &args, None);
    assert_ne!(
        out.status.code(),
        Some(99),
        "a matching hit must not fail verify"
    );
    let baseline = cache_run(&cache_off_env(&env), &args, None);
    assert_eq!(out.stdout, baseline.stdout);
    assert_eq!(
        strip_cache_debug(&out.stderr),
        strip_cache_debug(&baseline.stderr)
    );
    assert_eq!(out.status.code(), baseline.status.code());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn verify_mode_rejects_a_divergence() {
    // Corrupt the stored stdout bytes (same length, valid shape): verify
    // must abort with exit 99 instead of replaying.
    let (base, root, cache_dir) = cache_test_tree("verify_bad");
    let pt = base.join("pt");
    let env = cache_child_env(&root, &pt, &cache_dir);
    let args = ["-p", "dev-libs/schedleaf-a"];
    let first = cache_run(&env, &args, None);
    let key = cache_key(&first.stderr).expect("stored line carries the key");
    let entry = cache_dir.join("plans").join(format!("{key}.plan"));
    // Layout: magic(8) version(4) code(1) stdout_len(8) stdout…: flip a
    // byte well inside stdout (the fixture prints >64 bytes there).
    let mut bytes = std::fs::read(&entry).unwrap();
    assert!(bytes.len() > 8 + 4 + 1 + 8 + 64);
    bytes[8 + 4 + 1 + 8 + 16] ^= 0xFF;
    std::fs::write(&entry, bytes).unwrap();
    let mut verify_env = env.clone();
    verify_env.push(("PORTUALE_CACHE_VERIFY".to_string(), "1".to_string()));
    let out = cache_run(&verify_env, &args, None);
    assert_eq!(out.status.code(), Some(99), "divergence must exit 99");
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(err.contains("VERIFY FAILED"), "{err}");
    assert!(err.contains(&key), "{err}");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn exit_code_to_u8_matches_from_u8() {
    // Pins the `Debug`-shape conversion the cache edge relies on.
    for code in [0u8, 1, 2, 99, 130, 141] {
        assert_eq!(
            plan_cache::exit_code_to_u8(std::process::ExitCode::from(code)),
            code,
            "ExitCode({code}) must parse back"
        );
    }
}

#[test]
#[cfg(unix)]
fn cache_entry_roundtrip_and_rejections() {
    // The file format: exact-length validation, version gate, corruption
    // and permission fallbacks. `store`/`load` against a scratch dir.
    let base = TempDir::new("plan_cache_format").keep();
    let dir = base.join("cache");
    std::fs::create_dir_all(dir.join("plans")).unwrap();
    std::fs::create_dir_all(dir.join("tmp")).unwrap();
    let fps = vec![
        plan_cache::FileFp {
            path: std::path::PathBuf::from("/repo/cat/pkg/Manifest"),
            state: plan_cache::FileFpState::Absent,
        },
        plan_cache::FileFp {
            path: std::path::PathBuf::from("/repo/cat/pkg/pkg-1.0.ebuild"),
            state: plan_cache::FileFpState::Present {
                size: 7,
                mtime_secs: 1_700_000_000,
                mtime_nanos: 42,
            },
        },
    ];
    plan_cache::store(
        &dir,
        "k",
        plan_cache::StoredEntry {
            code: 3,
            stdout: b"out",
            stderr: b"err",
            fps: &fps,
        },
    );
    let entry_path = dir.join("plans").join("k.plan");
    assert!(entry_path.is_file());
    let loaded = plan_cache::load(&dir, "k").expect("roundtrip");
    assert_eq!(loaded.code, 3);
    assert_eq!(loaded.stdout, b"out");
    assert_eq!(loaded.stderr, b"err");
    assert_eq!(loaded.fps, fps);
    // Missing key is a silent miss.
    assert!(plan_cache::load(&dir, "absent").is_none());
    // Every truncation is a miss, never a replay.
    let good = std::fs::read(&entry_path).unwrap();
    for cut in [0, 1, 7, 8, 12, 20, good.len() - 1] {
        std::fs::write(&entry_path, &good[..cut]).unwrap();
        assert!(
            plan_cache::load(&dir, "k").is_none(),
            "truncated at {cut} must miss"
        );
    }
    std::fs::write(&entry_path, &good).unwrap();
    // Bad magic, bad version, trailing garbage: all miss.
    let mut bad = good.clone();
    bad[0] ^= 0xFF;
    std::fs::write(&entry_path, &bad).unwrap();
    assert!(plan_cache::load(&dir, "k").is_none(), "bad magic must miss");
    let mut bad = good.clone();
    bad[8] = 0xFF;
    std::fs::write(&entry_path, &bad).unwrap();
    assert!(
        plan_cache::load(&dir, "k").is_none(),
        "bad version must miss"
    );
    let mut bad = good.clone();
    bad.push(0);
    std::fs::write(&entry_path, &bad).unwrap();
    assert!(
        plan_cache::load(&dir, "k").is_none(),
        "trailing bytes must miss"
    );
    std::fs::write(&entry_path, &good).unwrap();
    // A group/world-writable file is never replayed.
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(&entry_path, std::fs::Permissions::from_mode(0o664)).unwrap();
    assert!(plan_cache::load(&dir, "k").is_none(), "writable must miss");
    std::fs::set_permissions(&entry_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(plan_cache::load(&dir, "k").is_some());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn cache_gc_keeps_the_newest_entries() {
    // Past the cap, the oldest plans go; the newest stay replayable.
    let base = TempDir::new("plan_cache_gc").keep();
    let dir = base.join("cache");
    std::fs::create_dir_all(dir.join("plans")).unwrap();
    std::fs::create_dir_all(dir.join("tmp")).unwrap();
    for i in 0..40 {
        plan_cache::store(
            &dir,
            &format!("k{i:02}"),
            plan_cache::StoredEntry {
                code: 0,
                stdout: format!("out{i}").as_bytes(),
                stderr: b"",
                fps: &[],
            },
        );
        // Distinct mtimes so "newest" is well-defined.
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let mut kept: Vec<String> = std::fs::read_dir(dir.join("plans"))
        .unwrap()
        .map(|e| {
            e.unwrap()
                .path()
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    kept.sort();
    assert_eq!(kept.len(), 32, "GC keeps 32, got {}", kept.len());
    assert!(
        kept.contains(&"k39".to_string()),
        "the newest entry survives: {kept:?}"
    );
    assert!(
        !kept.contains(&"k00".to_string()),
        "the oldest entry is collected: {kept:?}"
    );
    assert!(plan_cache::load(&dir, "k39").is_some());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn proc_facts_feed_the_key() {
    // The seam for the unvaryable process facts: different host or binary
    // bytes key differently, identical inputs key identically.
    let root = TempDir::new("plan_cache_keyfacts").keep();
    let config = portage_profile::Config::default();
    let argv = ["emerge".to_string(), "-p".to_string()];
    let key_inputs = |facts: Option<plan_cache::ProcFacts>| plan_cache::KeyInputs {
        argv: &argv,
        config: &config,
        repos: &[],
        root: &root,
        config_root: &root,
        proc_facts: facts,
    };
    let base = plan_cache::build_key(key_inputs(None)).expect("key builds");
    let same = plan_cache::build_key(key_inputs(None)).expect("key builds");
    assert_eq!(base, same, "the key is deterministic");
    assert_eq!(base.len(), 64);
    let mk = |host: &[u8], exe: &[u8]| {
        plan_cache::build_key(key_inputs(Some(plan_cache::ProcFacts {
            hostname: host.to_vec(),
            exe_bytes: exe.to_vec(),
        })))
        .expect("key builds")
    };
    // Each half isolated: same exe with different hostnames, and same
    // hostname with different bytes, must both move the key. (Removing
    // either hash line fails exactly one of these.)
    assert_ne!(
        mk(b"host-a", b"exe"),
        mk(b"host-b", b"exe"),
        "hostname feeds the key"
    );
    assert_ne!(
        mk(b"host-a", b"exe-a"),
        mk(b"host-a", b"exe-b"),
        "exe bytes feed the key"
    );
    assert_eq!(
        mk(b"host-a", b"exe"),
        mk(b"host-a", b"exe"),
        "seamed inputs are deterministic"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn ineligible_matrix() {
    // Every out-of-scope shape refuses the cache; a plain pretend run is
    // the only eligible one.
    fn ineligible_with(modify: impl FnOnce(&mut EmergeOpts)) -> bool {
        let mut opts = EmergeOpts {
            pretend: true,
            ..Default::default()
        };
        modify(&mut opts);
        plan_cache::ineligible_reason(&opts, false).is_some()
    }
    let mut plain = EmergeOpts {
        pretend: true,
        ..Default::default()
    };
    assert_eq!(plan_cache::ineligible_reason(&plain, false), None);
    assert!(ineligible_with(|o| o.resume = true));
    assert!(ineligible_with(|o| o.skipfirst = true));
    assert!(ineligible_with(|o| o.fetchonly = true));
    assert!(ineligible_with(|o| o.fetch_all_uri = true));
    assert!(ineligible_with(|o| o.buildpkgonly = true));
    assert!(ineligible_with(|o| o.buildpkg_opt = Some(true)));
    assert!(ineligible_with(|o| o.deselect = true));
    assert!(ineligible_with(|o| o.depclean = true));
    assert!(ineligible_with(|o| o.prune = true));
    assert!(ineligible_with(|o| o.unmerge = true));
    assert!(ineligible_with(|o| o.clean_action = true));
    assert!(ineligible_with(|o| o.rage_clean = true));
    assert!(ineligible_with(|o| o.config_action = true));
    assert!(ineligible_with(|o| o.no_cache = true));
    assert!(ineligible_with(|o| o.atom_args = vec!["@preserved-rebuild"]));
    assert!(plan_cache::ineligible_reason(&plain, true).is_some());
    plain.pretend = false;
    assert!(plan_cache::ineligible_reason(&plain, false).is_some());
}

#[test]
#[cfg(unix)]
fn sensitivity_profile_env() {
    // `<ROOT>/etc/profile.env` (the env.d layer) feeds the key: writing it
    // stores cold with the same fresh bytes.
    let (base, root, cache_dir) = cache_test_tree("profile_env");
    let pt = base.join("pt");
    let env = cache_child_env(&root, &pt, &cache_dir);
    let args = ["-p", "dev-libs/schedleaf-a"];
    let _ = cache_run(&env, &args, None);
    let etcdir = root.join("etc");
    std::fs::create_dir_all(&etcdir).unwrap();
    std::fs::write(etcdir.join("profile.env"), "USE=\"profile-env-probe\"\n").unwrap();
    let miss = cache_run(&env, &args, None);
    let baseline = cache_run(&cache_off_env(&env), &args, None);
    assert_cold_matches_baseline(&miss, &baseline);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn sensitivity_running_root_config() {
    // A differing running root's config feeds the key: writing its
    // `make.conf` stores cold; the resolve itself does not move.
    let (base, root, cache_dir) = cache_test_tree("running_root");
    let pt = base.join("pt");
    let rroot = base.join("rroot");
    std::fs::create_dir_all(&rroot).unwrap();
    let mut env = cache_child_env(&root, &pt, &cache_dir);
    env.push((
        "PORTAGE_RUNNING_ROOT".to_string(),
        rroot.display().to_string(),
    ));
    let args = ["-p", "dev-libs/schedleaf-a"];
    let _ = cache_run(&env, &args, None);
    let retcdir = rroot.join("etc/portage");
    std::fs::create_dir_all(&retcdir).unwrap();
    std::fs::write(retcdir.join("make.conf"), "USE=\"running-probe\"\n").unwrap();
    let miss = cache_run(&env, &args, None);
    let baseline = cache_run(&cache_off_env(&env), &args, None);
    assert_cold_matches_baseline(&miss, &baseline);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn volatile_env_does_not_bust_the_key() {
    // The documented volatile list (`_`, `OLDPWD`, `SHLVL`, the cache
    // controls) is excluded from the key: turning the debug switch on
    // still hits the entry warmed without it.
    let (base, root, cache_dir) = cache_test_tree("volatile");
    let pt = base.join("pt");
    let env = cache_child_env(&root, &pt, &cache_dir);
    let mut quiet = cache_off_env(&env);
    quiet.push((
        "PORTUALE_CACHE_DIR".to_string(),
        cache_dir.display().to_string(),
    ));
    let args = ["-p", "dev-libs/schedleaf-a"];
    let _ = cache_run(&quiet, &args, None);
    let hit = cache_run(&env, &args, None);
    assert_hit(&hit, &cache_run(&cache_off_env(&env), &args, None));
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn sensitivity_binhost_packages() {
    // The refreshed binhost `Packages` content feeds the key: bumping its
    // `TIMESTAMP` stores cold; the plan does not move.
    let (base, root, cache_dir) = cache_test_tree("binhost");
    let pt = base.join("pt");
    let cfgroot = scratch_configroot(&base);
    let binhost = cfgroot.join("binhost");
    std::fs::create_dir_all(&binhost).unwrap();
    let mut packages = std::fs::read_to_string(fixtures_root().join("binhost/Packages")).unwrap();
    assert!(packages.contains("TIMESTAMP: 0"));
    std::fs::write(binhost.join("Packages"), &packages).unwrap();
    let binrepos = cfgroot.join("etc/portage/binrepos.conf");
    std::fs::write(
        &binrepos,
        format!(
            "[scratchbinhost]\nsync-uri = file://{}\npriority = 1\n",
            binhost.display()
        ),
    )
    .unwrap();
    let mut env = cache_child_env(&root, &pt, &cache_dir);
    for (k, v) in env.iter_mut() {
        if k == "PORTAGE_CONFIGROOT" {
            *v = cfgroot.display().to_string();
        }
    }
    let args = ["-p", "--getbinpkg", "dev-libs/schedleaf-a"];
    let first = cache_run(&env, &args, None);
    assert!(
        cache_key(&first.stderr).is_some(),
        "the getbinpkg run must store: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    packages = packages.replacen("TIMESTAMP: 0", "TIMESTAMP: 1", 1);
    std::fs::write(binhost.join("Packages"), &packages).unwrap();
    let miss = cache_run(&env, &args, None);
    let baseline = cache_run(&cache_off_env(&env), &args, None);
    assert_cold_matches_baseline(&miss, &baseline);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn vdb_generation_and_count_feed_the_key() {
    // A fabricated files-VDB: adding an entry (count) and touching a
    // category dir (generation, same count) each change the key, with no
    // resolve involved.
    let root = TempDir::new("plan_cache_minivdb").keep();
    let config = portage_profile::Config::default();
    let key = || {
        plan_cache::build_key(plan_cache::KeyInputs {
            argv: &["emerge".to_string(), "-p".to_string()],
            config: &config,
            repos: &[],
            root: &root,
            config_root: &root,
            proc_facts: None,
        })
        .expect("key builds")
    };
    let k0 = key();
    let cat = root.join("var/db/pkg/cat-a");
    std::fs::create_dir_all(cat.join("pkg-1.0")).unwrap();
    let k1 = key();
    assert_ne!(k0, k1, "a new entry must change the key");
    // Same count, new generation: re-stamp the category dir's mtime.
    filetime::set_file_mtime(&cat, filetime::FileTime::from_unix_time(1_800_000_000, 0)).unwrap();
    let k2 = key();
    assert_ne!(k1, k2, "a new generation must change the key");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
#[cfg(unix)]
fn sensitivity_manifest_posthoc() {
    // The post-hoc Manifest fingerprint: appearing and disappearing between
    // identical runs each stores cold. The overlay package resolves from
    // its md5-cache entry (whose `_md5_` matches the ebuild), so no ebuild
    // phase ever runs.
    let (base, root, cache_dir) = cache_test_tree("manifest");
    let pt = base.join("pt");
    let overlay = base.join("overlaymf");
    for d in [
        "metadata",
        "profiles",
        "testcat/probepkg",
        "metadata/md5-cache/testcat",
    ] {
        std::fs::create_dir_all(overlay.join(d)).unwrap();
    }
    std::fs::write(
        overlay.join("metadata/layout.conf"),
        "masters =\nprofile-formats = portage-2\n",
    )
    .unwrap();
    std::fs::write(overlay.join("profiles/repo_name"), "tsmanifest\n").unwrap();
    let ebuild = "EAPI=8\nDESCRIPTION=\"plan-cache probe package\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\nSRC_URI=\"http://example.com/probe-1.0.tar.gz\"\n\nsrc_install() {\n\tinsinto /usr/share/${PN}\n}\n";
    std::fs::write(overlay.join("testcat/probepkg/probepkg-1.0.ebuild"), ebuild).unwrap();
    use md5::Digest as _;
    let digest = format!("{:x}", md5::Md5::digest(ebuild.as_bytes()));
    std::fs::write(
        overlay.join("metadata/md5-cache/testcat/probepkg-1.0"),
        format!(
            "DEFINED_PHASES=install\nDESCRIPTION=plan-cache probe package\nEAPI=8\nKEYWORDS=amd64\nSLOT=0\nSRC_URI=http://example.com/probe-1.0.tar.gz\n_md5_={digest}\n_eclasses_=\n"
        ),
    )
    .unwrap();
    let cfgroot = scratch_configroot(&base);
    std::os::unix::fs::symlink(&overlay, cfgroot.join("overlaymf")).unwrap();
    let repos_conf = cfgroot.join("etc/portage/repos.conf/repos.conf");
    let mut repos_text = std::fs::read_to_string(&repos_conf).unwrap();
    repos_text.push_str("\n[tsmanifest]\nlocation = overlaymf\n");
    std::fs::write(&repos_conf, repos_text).unwrap();
    let mut env = cache_child_env(&root, &pt, &cache_dir);
    for (k, v) in env.iter_mut() {
        if k == "PORTAGE_CONFIGROOT" {
            *v = cfgroot.display().to_string();
        }
    }
    let args = ["-p", "testcat/probepkg"];
    let first = cache_run(&env, &args, None);
    assert!(
        cache_key(&first.stderr).is_some(),
        "the overlay package must resolve: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    // A Manifest appears: the download totals are live-read, so the plan
    // moves. The lookup finds the stale candidate first (same pre-resolve
    // key) and the post-hoc fingerprint rejects it, then the fresh bytes
    // store: two debug lines, miss-then-store.
    let manifest = overlay.join("testcat/probepkg/Manifest");
    std::fs::write(
        &manifest,
        "DIST probe-1.0.tar.gz 64 MD5 d41d8cd98f00b204e9800998ecf8427e\n",
    )
    .unwrap();
    let miss = cache_run(&env, &args, None);
    let baseline = cache_run(&cache_off_env(&env), &args, None);
    let debug = cache_debug_lines(&miss.stderr);
    assert_eq!(debug.len(), 2, "post-hoc miss then store, got: {debug:?}");
    assert!(debug[0].contains("repo files changed"), "{debug:?}");
    assert!(debug[0].starts_with("miss (key="), "{debug:?}");
    assert!(debug[1].starts_with("stored (key="), "{debug:?}");
    assert_eq!(miss.stdout, baseline.stdout);
    assert_eq!(miss.status.code(), baseline.status.code());
    // And disappears again: `Absent` is a verified state too.
    std::fs::remove_file(&manifest).unwrap();
    let miss = cache_run(&env, &args, None);
    let baseline = cache_run(&cache_off_env(&env), &args, None);
    let debug = cache_debug_lines(&miss.stderr);
    assert_eq!(debug.len(), 2, "post-hoc miss then store, got: {debug:?}");
    assert!(debug[0].contains("repo files changed"), "{debug:?}");
    assert_eq!(miss.stdout, baseline.stdout);
    assert_eq!(
        miss.stdout, first.stdout,
        "removing the Manifest restores the plan"
    );
    let _ = std::fs::remove_dir_all(&base);
}
