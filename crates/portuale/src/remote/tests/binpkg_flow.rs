use super::*;

#[test]
fn binary_plan_check_accepts_binaries_and_names_sources() {
    use portage_repo::PretendOutcome;
    assert!(check_binary_plan(&[binary_entry("a", true)]).is_ok());
    assert!(check_binary_plan(&[]).is_ok());
    let mut installed = binary_entry("b", true);
    installed.outcome = PretendOutcome::AlreadyInstalled {
        version: "1.0".to_string(),
    };
    installed.source = portage_repo::CandidateSource::Ebuild;
    assert!(check_binary_plan(&[installed]).is_ok());
    let err = check_binary_plan(&[binary_entry("c", false)]).unwrap_err();
    assert!(err.contains("dev-libs/c-1.0"), "{err}");
}
