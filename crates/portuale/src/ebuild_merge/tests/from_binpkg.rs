use crate::ebuild_merge::from_binpkg::{ExtractLogFacts, extract_log_text};

/// The setup phase's header lists only the `_features_display` tokens, in
/// Portage's order, colored even off a terminal, between the two
/// `Extracting` lines.
/// Portage: _emerge/Binpkg.py, _unpack_metadata; _emerge/EbuildPhase.py, _async_start
#[test]
fn extract_log_text_matches_the_portage_log_shape() {
    let text = extract_log_text(&ExtractLogFacts {
        cpv: "dev-util/f459-good-1",
        slot: "0/1.2",
        repository: Some("fetchrepo"),
        built_use: "",
        features: "xz sandbox preserve-libs news",
    });
    let bullet = "\x1b[32m * \x1b[39;49;00m";
    assert_eq!(
        text,
        format!(
            ">>> Extracting info\n{bullet}Package:    dev-util/f459-good-1:0/1.2\n\
             {bullet}Repository: fetchrepo\n{bullet}USE:        \n\
             {bullet}FEATURES:   preserve-libs sandbox\n>>> Extracting dev-util/f459-good-1\n"
        )
    );
}

/// No repository record and no displayable feature drop their lines.
#[test]
fn extract_log_text_omits_absent_repository_and_features() {
    let text = extract_log_text(&ExtractLogFacts {
        cpv: "a/b-1",
        slot: "0",
        repository: None,
        built_use: "x -y",
        features: "news",
    });
    assert!(!text.contains("Repository:") && !text.contains("FEATURES:"));
    assert!(text.contains("USE:        x -y\n"));
}
