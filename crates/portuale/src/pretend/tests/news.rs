use super::*;

#[test]
fn news_item_format_and_valid_gate_the_1x_vs_2x_atom_split() {
    // Portage: news.py, DisplayInstalledRestriction. Format 1.x allows plain atoms
    // only; 2.x allows slots and USE deps.
    let item =
        |fmt: &str, atom: &str| format!("News-Item-Format: {fmt}\nDisplay-If-Installed: {atom}\n");
    assert_eq!(news_item_format("x"), None);
    assert_eq!(news_item_format("News-Item-Format: 1.0\n"), Some(true));
    assert_eq!(news_item_format("News-Item-Format: 2.13\n"), Some(false));
    // First matching header wins.
    assert_eq!(
        news_item_format("News-Item-Format: 2.0\nNews-Item-Format: 1.0\n"),
        Some(false)
    );

    for a in [
        "dev-libs/foo",
        "dev-libs/foo:2",
        "dev-libs/foo:2/3",
        "dev-libs/foo:=",
        "dev-libs/foo:*",
        "dev-libs/foo[bar]",
        "dev-libs/foo[bar(+)]",
    ] {
        assert!(news_item_valid(&item("2.0", a)), "2.x {a} valid");
    }
    assert!(news_item_valid(&item("1.0", "dev-libs/foo")));
    for a in [
        "dev-libs/foo:2",
        "dev-libs/foo:2/3",
        "dev-libs/foo:=",
        "dev-libs/foo:*",
        "dev-libs/foo[bar]",
        "dev-libs/foo[bar(+)]",
    ] {
        assert!(!news_item_valid(&item("1.0", a)), "1.x {a} invalid");
    }
    assert!(!news_item_valid(&item("2.0", "dev-libs/foo[[bad")));
    assert!(!news_item_valid("Display-If-Installed: dev-libs/foo\n"));
}

#[test]
fn filesystem_news_selector_evaluates_one_repo_through_the_seam() {
    // One `FilesystemNews` per repo evaluates validity and relevance over
    // `metadata/news`. Relevant items land in `unread` and `skip`.
    use mrg_director::NewsSelector;
    let base = TempDir::new("pretend-test-filesystem_news");
    let _ = std::fs::remove_dir_all(&base);
    let repo = base.join("repo");
    let root = base.join("root");
    let item = |id: &str, atom: &str| {
        let dir = repo.join("metadata/news").join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{id}.en.txt")),
            format!("News-Item-Format: 2.0\nDisplay-If-Installed: {atom}\nTitle: {id}\n"),
        )
        .unwrap();
    };
    item("2026-09-01-relevant", "dev-libs/installed-pkg");
    item("2026-09-02-irrelevant", "dev-libs/missing-pkg");
    std::fs::create_dir_all(root.join("var/db/pkg/dev-libs/installed-pkg-1.0")).unwrap();

    let context = test_news_context();
    let selector = FilesystemNews {
        repo_location: &repo,
        repo_name: "testrepo",
        root: &root,
        context: &context,
    };
    assert_eq!(selector.repo_name(), "testrepo");
    let eval = selector.evaluate().expect("news dir exists");
    assert_eq!(
        eval.unread,
        HashSet::from(["2026-09-01-relevant".to_string()])
    );
    assert_eq!(
        eval.skip,
        HashSet::from(["2026-09-01-relevant".to_string()])
    );
    assert_eq!(selector.unread_ids(), vec!["2026-09-01-relevant"]);

    // A repo with no news dir is nothing pending, never a panic.
    let empty = FilesystemNews {
        repo_location: &base.join("no-such-repo"),
        repo_name: "testrepo",
        root: &root,
        context: &context,
    };
    assert!(empty.evaluate().is_none());
    assert!(empty.unread_ids().is_empty());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn news_profile_and_keyword_restrictions_match_like_portage() {
    // Portage: news.py, DisplayProfileRestriction and DisplayKeywordRestriction.
    let root = TempDir::new("pretend-test-news_restrictions");
    let context = NewsContext {
        profile: Some("default/linux/amd64/23.0/systemd".to_string()),
        ..test_news_context()
    };
    let item = |format: &str, headers: &str| {
        format!("Title: t\nNews-Item-Format: {format}\n{headers}\nbody\n")
    };
    let relevant = |text: &str| news_item_relevant(text, &root, &context);
    assert!(relevant(&item(
        "2.0",
        "Display-If-Profile: default/linux/amd64/23.0/systemd"
    )));
    assert!(relevant(&item(
        "2.0",
        "Display-If-Profile: default/linux/amd64/23.0/*"
    )));
    assert!(!relevant(&item(
        "2.0",
        "Display-If-Profile: default/linux/amd64/23.0"
    )));
    assert!(!relevant(&item(
        "2.0",
        "Display-If-Profile: default/linux/s390/23.0\nDisplay-If-Profile: default/linux/ia64/23.0/*"
    )));
    assert!(!relevant(&item(
        "1.0",
        "Display-If-Profile: default/linux/amd64/23.0/*"
    )));
    assert!(relevant(&item("2.0", "Display-If-Keyword: amd64")));
    assert!(relevant(&item(
        "2.0",
        "Display-If-Keyword: sparc\nDisplay-If-Keyword: amd64"
    )));
    assert!(!relevant(&item("2.0", "Display-If-Keyword: ~amd64")));
    assert!(!relevant(&item("2.0", "Display-If-Keyword: hppa")));
    // Kinds AND; values within a kind OR.
    assert!(!relevant(&item(
        "2.0",
        "Display-If-Keyword: amd64\nDisplay-If-Profile: default/linux/sparc/23.0"
    )));
    let no_profile = NewsContext {
        profile: None,
        ..test_news_context()
    };
    assert!(!news_item_relevant(
        &item("2.0", "Display-If-Profile: default/*"),
        &root,
        &no_profile
    ));
    // A 1.x `*` anywhere but a trailing `/*` fails the whole item; a 2.x one never does.
    assert!(news_item_valid(&item(
        "1.0",
        "Display-If-Profile: default/*"
    )));
    assert!(!news_item_valid(&item(
        "1.0",
        "Display-If-Profile: default/*/amd64"
    )));
    assert!(news_item_valid(&item(
        "2.0",
        "Display-If-Profile: default/*/amd64"
    )));
}

#[test]
fn news_profile_path_strips_the_main_repo_profiles_prefix() {
    // Portage: news.py, NewsManager. The profile is the symlinked path minus the
    // main repo's `profiles/` prefix.
    let fixtures = fixtures_root();
    let repos = vec![portage_repo::RepoConfig {
        name: "testrepo".to_string(),
        location: std::fs::canonicalize(fixtures.join("repo")).unwrap(),
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
    assert_eq!(
        news_profile_path(&repos, &fixtures).as_deref(),
        Some("default")
    );
    let mut not_main = repos.clone();
    not_main[0].is_main = false;
    assert_eq!(news_profile_path(&not_main, &fixtures), None);
    assert_eq!(
        news_profile_path(&repos, &fixtures.join("no-such-config-root")),
        None
    );
}

#[test]
fn news_counts_follow_portage_count_unread_news_gates() {
    // Portage: news.py. No profile counts nothing; an unusable state dir returns
    // early; the count reads `.unread` as it stands.
    let base = TempDir::new("pretend-test-news_gates");
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("root");
    let repo = portage_repo::RepoConfig {
        name: "testrepo".to_string(),
        location: base.join("no-news-repo"),
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
    };
    let repos = vec![repo];
    let no_profile = NewsContext {
        profile: None,
        has_profile: false,
        ..test_news_context()
    };
    assert!(unread_news_counts(&repos, &root, &no_profile).is_empty());
    assert!(!root.join("var/lib/gentoo/news").exists());

    // A stale `.unread` in an unwritable state dir still counts for a non-root caller.
    use std::os::unix::fs::PermissionsExt;
    let state = root.join("var/lib/gentoo/news");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(
        state.join("news-testrepo.unread"),
        "2026-09-01-a\n2026-09-02-b\n",
    )
    .unwrap();
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o555)).unwrap();
    // SAFETY: `geteuid` has no preconditions.
    if unsafe { libc::geteuid() } != 0 {
        assert_eq!(
            unread_news_counts(&repos, &root, &test_news_context()),
            vec![("testrepo".to_string(), 2)]
        );
    }
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        grab_news_state_file(&base.join("missing")),
        Vec::<String>::new()
    );
    std::fs::write(
        base.join("unread"),
        "# header\n2026-09-01-a\n\n  2026-09-02-b  # note\n",
    )
    .unwrap();
    assert_eq!(
        grab_news_state_file(&base.join("unread")),
        vec!["2026-09-01-a".to_string(), "2026-09-02-b".to_string()]
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn news_state_files_get_the_group_write_bit_like_portage() {
    // Portage: news.py. The group-write bits are OR'd onto whatever the umask left.
    use std::os::unix::fs::PermissionsExt;
    let base = TempDir::new("pretend-test-news_perms");
    let _ = std::fs::remove_dir_all(&base);
    let dir = base.join("var/lib/gentoo/news");
    let context = test_news_context();
    assert!(ensure_news_state_dir(&dir, &context));
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o7777;
    assert_eq!(mode(&dir) & 0o074, 0o074);
    let orig = HashSet::new();
    let updated = HashSet::from(["2026-09-01-a".to_string()]);
    write_news_state_if_changed(WriteNewsStateIfChangedArgs {
        news_state_dir: &dir,
        repo_name: "testrepo",
        suffix: "unread",
        orig: &orig,
        updated: &updated,
        context: &context,
    });
    let file = dir.join("news-testrepo.unread");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "2026-09-01-a\n");
    assert_eq!(mode(&file) & 0o064, 0o064);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn news_notice_gate_needs_news_in_resolved_features() {
    // Portage: post_emerge.py. The notice needs `news` in the resolved `FEATURES`.
    let mut with = portage_profile::Config::default();
    with.other_vars
        .insert("FEATURES".to_string(), "sandbox news userpriv".to_string());
    assert!(news_notice_enabled(&with));
    let mut without = portage_profile::Config::default();
    without
        .other_vars
        .insert("FEATURES".to_string(), "sandbox userpriv".to_string());
    assert!(!news_notice_enabled(&without));
    assert!(!news_notice_enabled(&portage_profile::Config::default()));
}

#[test]
fn unread_news_counts_reuses_the_check_news_evaluation() {
    // Counts reuse the `--check-news` evaluation, over an isolated ROOT sharing
    // the fixture vdb read-only. Nothing is written into the fixtures tree itself.
    let fixtures = fixtures_root();
    let base = TempDir::new("pretend-test-news_counts");
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("root");
    std::fs::create_dir_all(root.join("var/lib")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(fixtures.join("var/db"), root.join("var/db")).unwrap();
    let repo_config = |name: &str, dir: &str| portage_repo::RepoConfig {
        name: name.to_string(),
        location: fixtures.join(dir),
        priority: 0,
        is_main: name == "testrepo",
        masters: Vec::new(),
        profile_formats: Vec::new(),
        cache_formats: Vec::new(),
        aliases: Vec::new(),
        sync_type: None,
        sync_uri: None,
        volatile: false,
        module_specific_options: Vec::new(),
    };
    let repos = vec![
        repo_config("testrepo", "repo"),
        repo_config("overlay", "overlay"),
    ];
    let context = test_news_context();
    let counts = unread_news_counts(&repos, &root, &context);
    assert_eq!(
        counts,
        vec![("testrepo".to_string(), 5), ("overlay".to_string(), 0),]
    );
    let news_dir = root.join("var/lib/gentoo/news");
    let unread: Vec<String> = std::fs::read_to_string(news_dir.join("news-testrepo.unread"))
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(unread.len(), 5);
    assert_eq!(unread, {
        let mut sorted = unread.clone();
        sorted.sort();
        sorted
    });
    assert_eq!(unread_news_counts(&repos, &root, &context), counts);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn pretend_end_notice_prints_only_under_pretend() {
    // Portage: post_emerge.py. The end notice prints only under `--pretend`.
    let color = crate::color::Colorizer::new(false);
    let (base, root, repos, config) = news_notice_test_tree("pretend_end");
    pretend_end_news_notice(false, &repos, &root, &config, &color);
    assert!(
        !root.join("var/lib/gentoo/news").exists(),
        "pretend=false must not evaluate news at all"
    );
    pretend_end_news_notice(true, &repos, &root, &config, &color);
    let unread: Vec<String> =
        std::fs::read_to_string(root.join("var/lib/gentoo/news/news-testrepo.unread"))
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect();
    assert_eq!(unread.len(), 5);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn merge_failure_notice_prints_only_when_the_vdb_changed() {
    // Portage: post_emerge.py. The failure notice prints only when the vdb changed.
    let color = crate::color::Colorizer::new(false);
    let (base, root, repos, config) = news_notice_test_tree("merge_failure");
    merge_failure_news_notice(false, &repos, &root, &config, &color);
    assert!(
        !root.join("var/lib/gentoo/news").exists(),
        "a failure with nothing merged must not evaluate news at all"
    );
    merge_failure_news_notice(true, &repos, &root, &config, &color);
    let unread: Vec<String> =
        std::fs::read_to_string(root.join("var/lib/gentoo/news/news-testrepo.unread"))
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect();
    assert_eq!(unread.len(), 5);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn display_news_notice_reports_whether_it_printed() {
    // Portage: actions.py. The prompt fires only when the notice actually printed.
    let color = crate::color::Colorizer::new(false);
    let (base, root, repos, mut config) = news_notice_test_tree("notice_bool");
    config
        .other_vars
        .insert("FEATURES".to_string(), String::new());
    assert!(!display_news_notice_if_any(&repos, &root, &config, &color));
    assert!(
        !root.join("var/lib/gentoo/news").exists(),
        "a gated-off notice must not evaluate news at all"
    );
    config
        .other_vars
        .insert("FEATURES".to_string(), "sandbox news userpriv".to_string());
    assert!(display_news_notice_if_any(&repos, &root, &config, &color));
    let unread: Vec<String> =
        std::fs::read_to_string(root.join("var/lib/gentoo/news/news-testrepo.unread"))
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect();
    assert_eq!(unread.len(), 5);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn ask_read_news_yes_spawns_eselect_and_continues() {
    // Portage: actions.py. "Yes" spawns `eselect news read`, then continues into the resolve.
    let (stubdir, marker) = eselect_stub("yes");
    let path = format!(
        "{}:{}",
        stubdir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    use std::io::Write;
    let portuale_bin = built_portuale_bin();
    let base = TempDir::new("ask_read_news_yes");
    let root = base.join("root");
    std::fs::create_dir_all(&root).unwrap();
    let mut env = news_resolve_env(&root, &base.join("pt"));
    env.push(("PATH".to_string(), path));
    let (mut master, slave_stdio) = pty_pair();
    let child = std::process::Command::new(&portuale_bin)
        .args([
            "emerge",
            "--ask",
            "--read-news",
            "--oneshot",
            "dev-libs/schedok",
        ])
        .stdin(slave_stdio)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .envs(env)
        .spawn()
        .expect("portuale emerge spawns");
    master.write_all(b"Yes\n").expect("answer the news prompt");
    master.write_all(b"No\n").expect("decline the merge prompt");
    let output = wait_pty_output(child);
    drop(master);
    assert_eq!(
        output.status.code(),
        Some(130),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Would you like to read the news items while calculating dependencies?"),
        "{stdout}"
    );
    assert_eq!(
        stdout.matches("news items need reading").count(),
        1,
        "{stdout}"
    );
    assert!(stdout.contains("Quitting."), "{stdout}");
    assert_eq!(
        std::fs::read_to_string(&marker).unwrap_or_default(),
        "news read\n",
    );
    let _ = std::fs::remove_dir_all(&base);
    let _ = std::fs::remove_dir_all(stubdir);
}

#[test]
#[cfg(unix)]
fn ask_read_news_no_skips_eselect_and_continues() {
    // "No" skips eselect and continues into the resolve.
    ask_read_news_declined("no", ["--ask", "--read-news"]);
}

#[test]
#[cfg(unix)]
fn ask_read_news_true_spellings_prompt_like_portage() {
    // Portage `true_y_or_n`: the `=True` spellings prompt like the bare pair.
    let stdout = ask_read_news_declined("true_spellings", ["--ask=True", "--read-news=True"]);
    assert!(stdout.contains("Quitting."), "{stdout}");
}
