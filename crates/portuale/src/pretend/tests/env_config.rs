use super::*;

#[test]
fn buildpkg_comes_from_the_cli_then_the_resolved_features() {
    // #37 S3: real `create_depgraph_params.py` sets buildpkg='y' from
    // `settings.features`; an explicit CLI `--buildpkg[=n]` wins.
    let mut config = portage_profile::Config::default();
    config.other_vars.insert(
        "FEATURES".to_string(),
        "buildpkg binpkg-multi-instance".to_string(),
    );
    // No CLI flag: resolved `FEATURES=buildpkg` enables it.
    assert!(buildpkg_from_config(None, &config));
    // An explicit `--buildpkg=n` wins over the feature.
    assert!(!buildpkg_from_config(Some(false), &config));
    assert!(buildpkg_from_config(Some(true), &config));
    // Without the token in the resolved list it stays off.
    let mut off = portage_profile::Config::default();
    off.other_vars
        .insert("FEATURES".to_string(), "-buildpkg sandbox".to_string());
    assert!(!buildpkg_from_config(None, &off));
}

#[test]
fn pkgdir_index_trusted_folds_the_resolved_features_chain() {
    // Backlog #199 fix round 1: `pkgdir_index_trusted` is real
    // `bintree.populate`'s own `reindex =
    // "pkgdir-index-trusted" not in features` (`bintree.py:936-938`)
    // over the folded `FEATURES` chain, defaulting to trusted when
    // nothing configures `FEATURES` at all (real's host
    // `make.globals` default, `cnf/make.globals:81`).
    let layered = |layers: Vec<Vec<&str>>| {
        let mut config = portage_profile::Config::default();
        config.incremental_sources.insert(
            "FEATURES".to_string(),
            layers
                .into_iter()
                .map(|l| l.into_iter().map(String::from).collect())
                .collect(),
        );
        config
    };
    // Nothing configured anywhere: trusted (the seeded default).
    assert!(pkgdir_index_trusted(&portage_profile::Config::default()));
    // Token present, never negated: trusted.
    assert!(pkgdir_index_trusted(&layered(vec![vec![
        "sandbox",
        "pkgdir-index-trusted"
    ]])));
    // A later layer negates it (the `make.conf`-layer case): distrusted.
    assert!(!pkgdir_index_trusted(&layered(vec![
        vec!["pkgdir-index-trusted"],
        vec!["-pkgdir-index-trusted"],
    ])));
    // `-*` clears the chain, a later re-add restores it: trusted.
    assert!(pkgdir_index_trusted(&layered(vec![
        vec!["pkgdir-index-trusted", "-*"],
        vec!["pkgdir-index-trusted"],
    ])));
    // `-*` last, with no re-add: distrusted.
    assert!(!pkgdir_index_trusted(&layered(vec![
        vec!["pkgdir-index-trusted"],
        vec!["-*"],
    ])));
    // The `other_vars` fallback (the case `config_features_list`
    // falls back for): `FEATURES` carried only there still decides.
    let mut fallback_on = portage_profile::Config::default();
    fallback_on.other_vars.insert(
        "FEATURES".to_string(),
        "sandbox pkgdir-index-trusted".to_string(),
    );
    assert!(pkgdir_index_trusted(&fallback_on));
    let mut fallback_off = portage_profile::Config::default();
    fallback_off.other_vars.insert(
        "FEATURES".to_string(),
        "sandbox -pkgdir-index-trusted".to_string(),
    );
    assert!(!pkgdir_index_trusted(&fallback_off));
}

#[test]
fn shell_split_handles_quotes_and_escapes() {
    assert_eq!(
        shell_split("ionice -c 3 -p ${PID}"),
        ["ionice", "-c", "3", "-p", "${PID}"]
    );
    assert_eq!(
        shell_split("/bin/sh -c 'exit 7' x"),
        ["/bin/sh", "-c", "exit 7", "x"]
    );
    assert_eq!(shell_split(r#"a "b c" d"#), ["a", "b c", "d"]);
    assert_eq!(shell_split(r#"a\ b c"#), ["a b", "c"]);
    assert_eq!(shell_split("   "), Vec::<String>::new());
    // Unterminated quote: consume the rest.
    assert_eq!(shell_split("foo 'bar baz"), ["foo", "bar baz"]);
}

#[test]
fn emerge_default_opts_are_prepended_to_argv() {
    // Real `main.py:1352-1360`: `EMERGE_DEFAULT_OPTS` tokens come
    // first, argv after -- so a command-line option wins.
    let config = portage_profile::Config {
        other_vars: [(
            "EMERGE_DEFAULT_OPTS".to_string(),
            "--usepkg=n --getbinpkg=y".to_string(),
        )]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let args: Vec<String> = ["-uDpvN", "@world"].iter().map(|s| s.to_string()).collect();
    assert_eq!(
        args_with_emerge_defaults(&config, &args),
        ["--usepkg=n", "--getbinpkg=y", "-uDpvN", "@world"]
    );

    // The splitter is the same shlex-shaped one `PORTAGE_IONICE_COMMAND`
    // uses, so quoting in the variable is honoured.
    let quoted = portage_profile::Config {
        other_vars: [(
            "EMERGE_DEFAULT_OPTS".to_string(),
            "--exclude 'dev-libs/a b'".to_string(),
        )]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let bare: Vec<String> = vec!["--pretend".to_string()];
    assert_eq!(
        args_with_emerge_defaults(&quoted, &bare),
        ["--exclude", "dev-libs/a b", "--pretend"]
    );

    // Unset variable: argv unchanged.
    let unset = portage_profile::Config::default();
    assert_eq!(args_with_emerge_defaults(&unset, &args), args);

    // `--ignore-default-opts` on the command line skips the variable
    // entirely (real `main.py:1352-1353`).
    let with_ignore: Vec<String> = ["--ignore-default-opts", "@world"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        args_with_emerge_defaults(&config, &with_ignore),
        with_ignore
    );
}

#[test]
fn config_bzip2_command_prefers_the_placed_config() {
    // Backlog #171 review: a client `make.conf` compressor (e.g.
    // `lbzip2`) wins over the `make.globals` default.
    let configured = portage_profile::Config {
        other_vars: [("PORTAGE_BZIP2_COMMAND".to_string(), "lbzip2".to_string())]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    assert_eq!(config_bzip2_command(&configured), "lbzip2");
    // Unset (or empty): the `cnf/make.globals:105` default.
    assert_eq!(
        config_bzip2_command(&portage_profile::Config::default()),
        "bzip2"
    );
    let empty = portage_profile::Config {
        other_vars: [("PORTAGE_BZIP2_COMMAND".to_string(), String::new())]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    assert_eq!(config_bzip2_command(&empty), "bzip2");
}

#[test]
fn root_deps_running_root_is_none_only_when_flag_off_and_roots_coincide() {
    use std::path::{Path, PathBuf};
    let slash = || PathBuf::from("/");
    let stage = || PathBuf::from("/mnt/stage3");

    // Ordinary ROOT=/ invocation, no flag: strict no-op.
    assert_eq!(
        resolve_root_deps_running_root(false, Path::new("/"), slash()),
        None
    );
    // Cross-root build (ROOT != running root), no flag: routes to the
    // running root -- real portage's unconditional BDEPEND/IDEPEND
    // behavior, now reachable without --root-deps.
    assert_eq!(
        resolve_root_deps_running_root(false, Path::new("/mnt/stage3"), slash()),
        Some(slash())
    );
    // --root-deps forces it even when the roots coincide.
    assert_eq!(
        resolve_root_deps_running_root(true, Path::new("/"), slash()),
        Some(slash())
    );
    assert_eq!(
        resolve_root_deps_running_root(true, Path::new("/"), stage()),
        Some(stage())
    );
}
