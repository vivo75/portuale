use super::*;

#[test]
fn portage_tmpdir_comes_from_the_resolved_scalar_then_the_make_globals_default() {
    // #38 S0: real `make.globals:35` is `PORTAGE_TMPDIR="/var/tmp"`.
    // portuale's CLI boundaries defaulted to `/var/tmp/portage` and
    // then appended `portage/<cat>/<pf>`, doubling the component; the
    // DWARF `DW_AT_comp_dir` (and therefore the salted `.build-id`)
    // diverged from real for every compiled fixture.
    let mut config = test_config();
    assert_eq!(
        portage_tmpdir_from_config(&config),
        PathBuf::from("/var/tmp")
    );
    config
        .other_vars
        .insert("PORTAGE_TMPDIR".to_string(), "/conf/tmp".to_string());
    assert_eq!(
        portage_tmpdir_from_config(&config),
        PathBuf::from("/conf/tmp")
    );
    // A set-but-empty scalar falls back to the real default.
    config
        .other_vars
        .insert("PORTAGE_TMPDIR".to_string(), String::new());
    assert_eq!(
        portage_tmpdir_from_config(&config),
        PathBuf::from("/var/tmp")
    );
}
