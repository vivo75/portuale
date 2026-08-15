use super::*;

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
