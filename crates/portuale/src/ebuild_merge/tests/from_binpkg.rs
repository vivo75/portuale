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

/// Backlog #542: the `user_patch.digests` member parses exactly like
/// Portage (`strip(b"\0").split(b"\0")`, hex/basename validation, pairwise
/// count). Oracle: the shell-written bytes (`hash\0basename\0`, trailing
/// NUL included).
#[test]
fn parse_user_patch_digests_accepts_the_shell_format() {
    use crate::ebuild_merge::parse_user_patch_digests;

    let raw = b"813aab24dd88be987aee6db477fa25415712efddf7214ba779b03a715ac32b99\0user.patch\0";
    assert_eq!(
        parse_user_patch_digests(raw),
        Some(vec![(
            "813aab24dd88be987aee6db477fa25415712efddf7214ba779b03a715ac32b99".to_string(),
            "user.patch".to_string(),
        )]),
    );
    // Uppercase hashes are lowered (Portage `.decode().lower()`).
    let lower = "813aab24dd88be987aee6db477fa25415712efddf7214ba779b03a715ac32b99";
    let upper = format!("{}\0user.patch\0", lower.to_ascii_uppercase()).into_bytes();
    assert_eq!(upper.len(), 64 + 1 + 10 + 1);
    assert_eq!(
        parse_user_patch_digests(&upper).unwrap()[0],
        (lower.to_string(), "user.patch".to_string()),
    );
}

/// Backlog #542: malformed members are invalid, never a table. Oracle:
/// Portage's `else` arm (`This binary package has invalid user patch
/// digest metadata!!!`).
#[test]
fn parse_user_patch_digests_rejects_malformed_members() {
    use crate::ebuild_merge::parse_user_patch_digests;

    // Odd token count (a dangling hash with no basename).
    assert_eq!(parse_user_patch_digests(b"abc\0"), None);
    // Empty file: one empty token, odd.
    assert_eq!(parse_user_patch_digests(b""), None);
    // Only NULs: strips to empty, same as above.
    assert_eq!(parse_user_patch_digests(b"\0\0"), None);
    // Short hash.
    assert_eq!(parse_user_patch_digests(b"813aab\0user.patch\0"), None);
    // Non-hex hash.
    assert_eq!(
        parse_user_patch_digests(
            b"zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz\0user.patch\0"
        ),
        None
    );
    // Basename with a slash.
    assert_eq!(
        parse_user_patch_digests(
            b"813aab24dd88be987aee6db477fa25415712efddf7214ba779b03a715ac32b99\0sub/user.patch\0"
        ),
        None
    );
    // Basename without a patch/diff suffix.
    assert_eq!(
        parse_user_patch_digests(
            b"813aab24dd88be987aee6db477fa25415712efddf7214ba779b03a715ac32b99\0user.txt\0"
        ),
        None
    );
    // Bare `.patch` (Python `[^/]+\.` needs a character before the dot).
    assert_eq!(
        parse_user_patch_digests(
            b"813aab24dd88be987aee6db477fa25415712efddf7214ba779b03a715ac32b99\0.patch\0"
        ),
        None
    );
}

/// Backlog #542: the table on the matching merge is byte-identical to the
/// oracle (`captures/14-merge-match.portage.out`, `term_columns` 80 off a
/// terminal): the header, the 77-wide bar, the padded digest row, the
/// closing bar and the blank line -- with no stderr.
#[test]
fn user_patch_table_matches_the_oracle_on_a_match() {
    use crate::ebuild_merge::user_patch_extract_report;

    let digest = "813aab24dd88be987aee6db477fa25415712efddf7214ba779b03a715ac32b99";
    let raw = format!("{digest}\0user.patch\0");
    let (stdout, stderr) =
        user_patch_extract_report(Some(raw.as_bytes()), true, digest, digest, 80)
            .expect("member present");
    assert_eq!(
        stdout,
        vec![
            "This binary package was built with user patches applied:".to_string(),
            "=".repeat(77),
            format!("user.patch  {digest}"),
            "=".repeat(77),
            String::new(),
        ]
    );
    assert!(stderr.is_empty());
}

/// Backlog #542: a differing local set warns on stderr (oracle
/// `captures/16-merge-mismatch.portage.{out,err}`); so does a missing local
/// set, and a malformed member warns invalid instead of a table.
#[test]
fn user_patch_table_warns_on_mismatch_and_malformed_members() {
    use crate::ebuild_merge::{parse_user_patch_digests, user_patch_extract_report};

    let recorded = "b7bab6cdb78ca1425eeae16508c0124ccfc71953f3e2b7c6b1b753837a1a11d3";
    assert_eq!(recorded.len(), 64);
    let raw = format!("{recorded}\0user.patch\0");
    // Differing local digest: same table, plus the mismatch warning.
    let (stdout, stderr) = user_patch_extract_report(
        Some(raw.as_bytes()),
        true,
        "0000000000000000000000000000000000000000000000000000000000000000",
        recorded,
        80,
    )
    .expect("member present");
    assert_eq!(stdout.len(), 5);
    assert!(stdout[2].contains(recorded));
    assert_eq!(
        stderr,
        vec![
            String::new(),
            "User patches in binary package different than configured for ebuild!!!".to_string(),
            String::new(),
        ]
    );
    // No local patch set at all: the same warning (Portage's `not in` arm).
    let (_, stderr) = user_patch_extract_report(Some(raw.as_bytes()), false, "", recorded, 80)
        .expect("member present");
    assert_eq!(stderr.len(), 3);
    // Malformed member: no table, the invalid warning.
    let (stdout, stderr) =
        user_patch_extract_report(Some(b"nope\0"), true, "", "", 80).expect("member present");
    assert!(stdout.is_empty());
    assert_eq!(
        stderr,
        vec![
            String::new(),
            "This binary package has invalid user patch digest metadata!!!".to_string(),
            String::new(),
        ]
    );
    // No member at all: nothing, on either stream.
    assert_eq!(user_patch_extract_report(None, true, "", "", 80), None);
    let _ = parse_user_patch_digests;
}

/// Backlog #542: truncation mirrors Portage's character-count rules (long
/// basenames cut to `max(term_columns - 8 - 6, 25)`, digests to
/// `max(term_columns - len(basename) - 6, 8)`, rows padded to
/// `term_columns - 4`).
#[test]
fn user_patch_table_truncates_long_names_like_portage() {
    use crate::ebuild_merge::user_patch_extract_report;

    let digest = "8".repeat(64);
    let long_base = format!("{}.patch", "p".repeat(100));
    let raw = format!("{digest}\0{long_base}\0");
    let (stdout, _) = user_patch_extract_report(Some(raw.as_bytes()), true, &digest, &digest, 80)
        .expect("member present");
    // `max_basename` is 66 at width 80; the row is 66 + pad + 8 = 76 wide.
    let row = &stdout[2];
    assert_eq!(row.chars().count(), 76, "{row}");
    assert!(row.starts_with(&"p".repeat(66)));
    assert!(row.ends_with(&"8".repeat(8)));
}
