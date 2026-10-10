use super::*;

#[test]
fn parse_eapi_reads_the_first_real_lines_own_assignment() {
    assert_eq!(parse_eapi("EAPI=8\nDESCRIPTION=x\n"), "8");
    assert_eq!(parse_eapi("EAPI=\"8\"\n"), "8");
    assert_eq!(parse_eapi("EAPI='8'\n"), "8");
    assert_eq!(parse_eapi("# comment\n\nEAPI=7\n"), "7");
    assert_eq!(parse_eapi("EAPI=8 # trailing comment\n"), "8");
}

#[test]
fn parse_eapi_defaults_to_0_when_the_first_real_line_is_not_an_assignment() {
    assert_eq!(parse_eapi("DESCRIPTION=x\nEAPI=8\n"), "0");
    assert_eq!(parse_eapi(""), "0");
    assert_eq!(parse_eapi("# only comments\n"), "0");
}

#[test]
fn phase_prerequisites_chains_up_to_and_including_the_requested_phase() {
    assert_eq!(phase_prerequisites("pretend"), vec!["pretend"]);
    assert_eq!(
        phase_prerequisites("compile"),
        vec![
            "pretend",
            "setup",
            "unpack",
            "prepare",
            "configure",
            "compile"
        ]
    );
    assert_eq!(
        phase_prerequisites("install"),
        vec![
            "pretend",
            "setup",
            "unpack",
            "prepare",
            "configure",
            "compile",
            "test",
            "install"
        ]
    );
}

#[test]
fn phase_prerequisites_is_empty_for_an_unrecognized_command() {
    assert_eq!(phase_prerequisites("merge"), Vec::<&str>::new());
    assert_eq!(phase_prerequisites("qmerge"), Vec::<&str>::new());
}

#[test]
fn is_phase_command_covers_exactly_the_actionmap_deps_chain() {
    for cmd in [
        "pretend",
        "setup",
        "unpack",
        "prepare",
        "configure",
        "compile",
        "test",
        "install",
    ] {
        assert!(
            is_phase_command(cmd),
            "{cmd} should be a real phase command"
        );
    }
    for cmd in [
        "merge", "qmerge", "unmerge", "package", "clean", "digest", "info",
    ] {
        assert!(
            !is_phase_command(cmd),
            "{cmd} should NOT be a real phase command"
        );
    }
}

#[test]
fn is_standalone_phase_command_covers_exactly_config_info_prerm_postrm() {
    for cmd in ["config", "info", "prerm", "postrm"] {
        assert!(
            is_standalone_phase_command(cmd),
            "{cmd} should be a real standalone phase command"
        );
    }
    for cmd in [
        "merge", "qmerge", "unmerge", "package", "clean", "install", "preinst", "postinst", "help",
    ] {
        assert!(
            !is_standalone_phase_command(cmd),
            "{cmd} should NOT be a real standalone phase command"
        );
    }
}

#[test]
fn split_package_separates_pv_from_the_revision() {
    let split = split_package(Path::new("/repo/dev-libs/foo/foo-1.2.3-r1.ebuild"), "foo").unwrap();
    assert_eq!(split.pn, "foo");
    assert_eq!(split.pv, "1.2.3");
    assert_eq!(split.pr, "r1");
    assert_eq!(split.pvr, "1.2.3-r1");
    assert_eq!(split.p, "foo-1.2.3");
    assert_eq!(split.pf, "foo-1.2.3-r1");
}

#[test]
fn split_package_defaults_pr_to_r0_when_no_revision_is_present() {
    let split = split_package(Path::new("/repo/dev-libs/foo/foo-1.0.ebuild"), "foo").unwrap();
    assert_eq!(split.pv, "1.0");
    assert_eq!(split.pr, "r0");
    assert_eq!(split.pvr, "1.0");
    assert_eq!(split.pf, "foo-1.0");
}

#[test]
fn split_package_rejects_a_filename_not_matching_the_parent_directory() {
    assert!(split_package(Path::new("/repo/dev-libs/foo/bar-1.0.ebuild"), "foo").is_err());
}
