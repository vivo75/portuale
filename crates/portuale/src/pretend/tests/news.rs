use super::*;

#[test]
fn news_item_format_and_valid_gate_the_1x_vs_2x_atom_split() {
    // Real `DisplayInstalledRestriction.isValid`'s own
    // `eapi="0"`/`eapi="5"` split, as a field-level (not crate EAPI
    // parametrization) gate: 1.x rejects `:slot`/`:sub`/`:=`/`:*`/
    // `[use]`, 2.x accepts them.
    let item =
        |fmt: &str, atom: &str| format!("News-Item-Format: {fmt}\nDisplay-If-Installed: {atom}\n");
    // Format extraction.
    assert_eq!(news_item_format("x"), None);
    assert_eq!(news_item_format("News-Item-Format: 1.0\n"), Some(true));
    assert_eq!(news_item_format("News-Item-Format: 2.13\n"), Some(false));
    // First matching header wins (real's break-on-first-match).
    assert_eq!(
        news_item_format("News-Item-Format: 2.0\nNews-Item-Format: 1.0\n"),
        Some(false)
    );

    // 2.x: everything portuale parses is legal (superset grammar).
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
    // 1.x: plain only -- every slot / slot-op / use form is invalid.
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
    // Both formats reject a malformed atom; a missing format header
    // rejects too.
    assert!(!news_item_valid(&item("2.0", "dev-libs/foo[[bad")));
    assert!(!news_item_valid("Display-If-Installed: dev-libs/foo\n"));
}

#[test]
fn filesystem_news_selector_evaluates_one_repo_through_the_seam() {
    // The director's news slot carries the production `--check-news`
    // read: one `FilesystemNews` per repo evaluates validity +
    // relevance over `metadata/news`, minus the already-read/skipped
    // ids. A relevant item (installed atom matches the vdb) lands in
    // `unread` (and permanently in `skip`); an irrelevant one (atom
    // matches nothing installed) lands in neither; a repo without a
    // `metadata/news` dir evaluates to `None` (nothing pending).
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
fn news_profile_and_keyword_restrictions_match_like_portage() {
    // Backlog #258: real `DisplayProfileRestriction` /
    // `DisplayKeywordRestriction` (`news.py:388-423`). The l3
    // `l3-20260928T195717Z` container probe counted 15 gentoo items
    // for real and 34 for portuale on `default/linux/amd64/23.0/
    // systemd`; the 19 extras were all profile/keyword-restricted
    // to other profiles and arches.
    let root = TempDir::new("pretend-test-news_restrictions").keep();
    let context = NewsContext {
        profile: Some("default/linux/amd64/23.0/systemd".to_string()),
        ..test_news_context()
    };
    let item = |format: &str, headers: &str| {
        format!("Title: t\nNews-Item-Format: {format}\n{headers}\nbody\n")
    };
    let relevant = |text: &str| news_item_relevant(text, &root, &context);
    // Profile: exact, 2.x `/*` prefix, other profiles, 1.x `/*`.
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
    // Keyword: `ARCH` verbatim, OR'd within the kind.
    assert!(relevant(&item("2.0", "Display-If-Keyword: amd64")));
    assert!(relevant(&item(
        "2.0",
        "Display-If-Keyword: sparc\nDisplay-If-Keyword: amd64"
    )));
    assert!(!relevant(&item("2.0", "Display-If-Keyword: ~amd64")));
    assert!(!relevant(&item("2.0", "Display-If-Keyword: hppa")));
    // AND across kinds: a matching keyword does not rescue a
    // non-matching profile.
    assert!(!relevant(&item(
        "2.0",
        "Display-If-Keyword: amd64\nDisplay-If-Profile: default/linux/sparc/23.0"
    )));
    // No profile at all matches no profile restriction.
    let no_profile = NewsContext {
        profile: None,
        ..test_news_context()
    };
    assert!(!news_item_relevant(
        &item("2.0", "Display-If-Profile: default/*"),
        &root,
        &no_profile
    ));
    // Real `DisplayProfileRestriction.isValid`: a 1.x value with a
    // `*` anywhere but a trailing `/*` fails the whole item; a 2.x
    // one never does.
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
    // Real `NewsManager.__init__` (`news.py:88-98`) with the fixture
    // tree's own `etc/portage/make.profile -> ../../repo/profiles/
    // default`: the realpath minus `<main repo>/profiles/`.
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
    // No main repo, or no make.profile: no profile.
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
    // Real `count_unread_news` (`news.py:470-479`): no profile path
    // at all counts nothing. Real `updateItems` (`news.py:121-133`):
    // an unusable state directory returns before the news dir is
    // read, and `getUnreadItems` counts `.unread` as it stands --
    // `grabfile`, so comment lines and trailing comments drop out.
    let base = TempDir::new("pretend-test-news_gates").keep();
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

    // A stale `.unread` in a state directory the caller cannot
    // write still counts, even for a repo with no `metadata/news`.
    // `ensure_dirs(mode=0o074)` adds only group/other bits, so an
    // owner-read-only directory stays unwritable for a non-root
    // caller (root writes anyway, so the check is skipped there).
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
    // Real `apply_secpass_permissions(..., mode=0o064, mask=0)` on
    // each rewritten state file and `ensure_dirs(..., mode=0o074,
    // mask=0)` on the directory (`news.py:121-200`): the bits are
    // OR'd onto whatever the umask left. The l3 probe showed real's
    // files at 0664 and portuale's at 0644.
    use std::os::unix::fs::PermissionsExt;
    let base = TempDir::new("pretend-test-news_perms").keep();
    let _ = std::fs::remove_dir_all(&base);
    let dir = base.join("var/lib/gentoo/news");
    let context = test_news_context();
    assert!(ensure_news_state_dir(&dir, &context));
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o7777;
    assert_eq!(mode(&dir) & 0o074, 0o074);
    let orig = HashSet::new();
    let updated = HashSet::from(["2026-09-01-a".to_string()]);
    write_news_state_if_changed(&dir, "testrepo", "unread", &orig, &updated, &context);
    let file = dir.join("news-testrepo.unread");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "2026-09-01-a\n");
    assert_eq!(mode(&file) & 0o064, 0o064);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn news_notice_gate_needs_news_in_resolved_features() {
    // Real `display_news_notification` (`post_emerge.py:38`): the
    // notice is skipped wholesale unless `news` is in the resolved
    // `FEATURES` (no evaluation, no output). Same `Config::default`
    // + `other_vars` shape `buildpkg_comes_from_the_cli_then_...`
    // uses for its own feature gate.
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
    // Backlog #196 S1's "no second news parser": the count notice
    // reads through the same `FilesystemNews` evaluation (and the
    // same `.unread`/`.skip` write-back) `--check-news` uses, so
    // the hermetic fixture tree counts exactly what
    // `test_check_news_counts_unread_relevant_items` pins over in
    // the contract suite (5 relevant testrepo items). The ROOT is a
    // temp dir sharing the fixtures' own vdb read-only
    // (`Display-If-Installed` needs it) with a fresh `var/lib`, the
    // same isolation `_check_news_isolated_root` gives the contract
    // tests -- nothing is ever written into the git-tracked
    // fixtures tree itself.
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
    // The write-back landed in the temp ROOT (sorted, one id per
    // line), and a second evaluation stays sticky at the same
    // count (real `getUnreadItems`'s `len(.unread)`).
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
    // Backlog #196 fix round 1: real `post_emerge.py:112-117`
    // prints the notice at the end of a `--pretend` run (for every
    // `action_build` retval) and never otherwise. `pretend=false`
    // evaluates nothing (no state dir appears); `pretend=true`
    // evaluates and writes back the hermetic 5-count, exactly like
    // `--check-news` does.
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
    // Backlog #196 fix round 1: real `post_emerge.py:155` prints
    // the notice regardless of retval, but only when the vdb
    // changed (`:112-117`). A total failure (`changed=false`)
    // evaluates nothing; a partial one (`changed=true`) evaluates
    // and writes back the hermetic 5-count.
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
    // Backlog #231 (c): the `--ask` + `--read-news` prompt fires
    // only when the notice actually printed (real
    // `actions.py:4268-4281` short-circuits on `display_news_
    // notification`'s falsy return), so the helper returns real's
    // own boolean. Gated off: `false`, no evaluation. Gated on:
    // `true`, the hermetic 5-count evaluated and written back.
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
    // Backlog #231 (c): real `_emerge/actions.py:4266-4281` --
    // once the pre-resolution notice printed, `--ask --read-news`
    // prompts "Would you like to read the news items while
    // calculating dependencies?" and spawns `eselect news read`
    // on "Yes", then continues into the resolve. Answering "Yes"
    // here runs the stub (argv pinned) and the declined merge
    // prompt still exits 130 with `Quitting.`.
    let (stubdir, marker) = eselect_stub("yes");
    let path = format!(
        "{}:{}",
        stubdir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    // The stub dir is separate from the run's base dir so the
    // marker survives the cleanup below for the assertion.
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
    // Backlog #231 (c), the "No" arm: Portage's `== "Yes"` check
    // fails, so eselect is never spawned and the run continues into
    // the resolve; the declined merge prompt still exits 130.
    ask_read_news_declined("no", ["--ask", "--read-news"]);
}

#[test]
#[cfg(unix)]
fn ask_read_news_true_spellings_prompt_like_portage() {
    // Backlog #234 (b): Portage's `true_y_or_n` (`main.py:321-322,625`)
    // accepts `--ask=True` / `--read-news=True` and normalises them to
    // the bare flags (`main.py:802-805,950-953`), so the pair prompts
    // exactly like `--ask --read-news`.
    let stdout = ask_read_news_declined("true_spellings", ["--ask=True", "--read-news=True"]);
    assert!(stdout.contains("Quitting."), "{stdout}");
}
