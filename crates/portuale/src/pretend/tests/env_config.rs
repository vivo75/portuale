use super::*;

#[test]
fn buildpkg_comes_from_the_cli_then_the_resolved_features() {
    // Portage: create_depgraph_params.py. An explicit CLI flag wins over FEATURES.
    let mut config = portage_profile::Config::default();
    config.other_vars.insert(
        "FEATURES".to_string(),
        "buildpkg binpkg-multi-instance".to_string(),
    );
    assert!(buildpkg_from_config(None, &config));
    assert!(!buildpkg_from_config(Some(false), &config));
    assert!(buildpkg_from_config(Some(true), &config));
    let mut off = portage_profile::Config::default();
    off.other_vars
        .insert("FEATURES".to_string(), "-buildpkg sandbox".to_string());
    assert!(!buildpkg_from_config(None, &off));
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
    // An unterminated quote consumes the rest.
    assert_eq!(shell_split("foo 'bar baz"), ["foo", "bar baz"]);
}

#[test]
fn config_bzip2_command_prefers_the_placed_config() {
    // A configured compressor wins; unset or empty falls back to `bzip2`.
    let configured = portage_profile::Config {
        other_vars: [("PORTAGE_BZIP2_COMMAND".to_string(), "lbzip2".to_string())]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    assert_eq!(config_bzip2_command(&configured), "lbzip2");
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

    assert_eq!(
        resolve_root_deps_running_root(false, Path::new("/"), slash()),
        None
    );
    // Cross-root without the flag routes to the running root (Portage BDEPEND/IDEPEND behavior).
    assert_eq!(
        resolve_root_deps_running_root(false, Path::new("/mnt/stage3"), slash()),
        Some(slash())
    );
    // `--root-deps` forces it even when the roots coincide.
    assert_eq!(
        resolve_root_deps_running_root(true, Path::new("/"), slash()),
        Some(slash())
    );
    assert_eq!(
        resolve_root_deps_running_root(true, Path::new("/"), stage()),
        Some(stage())
    );
}
