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
    let base = TempDir::new("pretend-test-filesystem_news").keep();
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
fn unread_news_counts_reuses_the_check_news_evaluation() {
    // Counts reuse the `--check-news` evaluation, over an isolated ROOT sharing
    // the fixture vdb read-only. Nothing is written into the fixtures tree itself.
    let fixtures = fixtures_root();
    let base = TempDir::new("pretend-test-news_counts").keep();
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
    let base = TempDir::new("ask_read_news_yes").keep();
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
