use super::*;

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
