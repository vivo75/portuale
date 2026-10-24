use crate::ebuild_merge::MergeOptions;
use crate::ebuild_merge::from_binpkg::{ExtractLogFacts, binary_hook_env, extract_log_text};

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

/// A binary-merge hook sees `QEMU_SOFTMMU_TARGETS=""` for a package built
/// without any `qemu_softmmu_targets_*` flag, not the profile's value, and
/// `qemu_softmmu_targets_x86_64` in the binpkg's `USE` yields `x86_64`.
/// Expected values from the L1 bash merge on Portage 3.0.83-r1
/// (`LLM/evidence/2026-10-24-vdb-env/`: `declare -x QEMU_SOFTMMU_TARGETS=""`).
/// Portage: config.py, setcpv/environ
#[test]
fn binary_hook_env_recomputes_use_expand_from_the_binpkg() {
    let mut config = portage_profile::Config::default();
    config.use_expand.insert("QEMU_SOFTMMU_TARGETS".into());
    let mut options = MergeOptions {
        build_env: vec![("QEMU_SOFTMMU_TARGETS".into(), "x86_64".into())],
        resolved_config: Some(std::sync::Arc::new(config)),
        ..MergeOptions::default()
    };
    let iuse = "qemu_softmmu_targets_x86_64 foo";
    let get = |env: &[(String, String)]| {
        env.iter()
            .find(|(k, _)| k == "QEMU_SOFTMMU_TARGETS")
            .map(|(_, v)| v.clone())
    };
    assert_eq!(
        get(&binary_hook_env(&options, iuse, "foo")),
        Some(String::new())
    );
    assert_eq!(
        get(&binary_hook_env(
            &options,
            iuse,
            "qemu_softmmu_targets_x86_64"
        )),
        Some("x86_64".to_string())
    );
    options.resolved_config = None;
    assert_eq!(
        get(&binary_hook_env(&options, iuse, "foo")),
        Some("x86_64".to_string())
    );
}
