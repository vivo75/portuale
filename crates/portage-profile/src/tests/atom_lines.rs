//! Profile-file atom validation: `filter_atom_lines` keeps Portage's lines
//! and drops (with warnings) Portage's drops, under per-node EAPI rules.

use super::*;
use portage_util::TempDir;

fn rules(eapi: &str) -> NodeAtomRules {
    NodeAtomRules {
        eapi: Some(eapi.to_string()),
        allow_repo: false,
        allow_build_id: false,
        allow_wildcard: false,
    }
}

fn src(path: &Path, lines: &[&str]) -> Vec<(PathBuf, String)> {
    lines
        .iter()
        .map(|l| (path.to_path_buf(), l.to_string()))
        .collect()
}

/// Oracle run4 shape: the EAPI-5 node keeps `:=`, `:0` and `[x(+)]`, the
/// no-`eapi` (EAPI 0) node drops them with the EAPI-0 messages; `::repo`,
/// build-ids, wildcards and bare versions drop in both.
#[test]
fn mask_lines_filter_like_grabfile_package() {
    // Portage: run4.sh (oracle), S0 cell B-prof-eapi5-noeapi (miss-state ×2).
    let path = Path::new("/repo/profiles/t341/package.mask");
    let lines = [
        "dev-libs/t341-dep::t341",
        "=dev-libs/t341-dep-1-2",
        "*/t341-dep",
        "dev-libs/t341-dep:=",
        "dev-libs/t341-dep:0",
        "dev-libs/t341-dep[x(+)]",
        "dev-libs/t341-dep-1",
        "dev-libs/t341-dep",
    ];
    let mut warn = Vec::new();
    let kept5 = filter_atom_lines(
        &src(path, &lines),
        &rules("5"),
        AtomLineKind::File {
            packages_file: false,
        },
        WARN_RANK_MASK,
        &mut warn,
    );
    assert_eq!(
        kept5,
        vec![
            "dev-libs/t341-dep:=".to_string(),
            "dev-libs/t341-dep:0".to_string(),
            "dev-libs/t341-dep[x(+)]".to_string(),
            "dev-libs/t341-dep".to_string(),
        ]
    );
    assert_eq!(
        warn.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(),
        vec![
            "--- Invalid atom in /repo/profiles/t341/package.mask: dev-libs/t341-dep::t341",
            "--- Invalid atom in /repo/profiles/t341/package.mask: =dev-libs/t341-dep-1-2",
            "--- Invalid atom in /repo/profiles/t341/package.mask: */t341-dep",
            "--- Invalid atom in /repo/profiles/t341/package.mask: dev-libs/t341-dep-1",
        ]
    );
    warn.clear();
    let kept0 = filter_atom_lines(
        &src(path, &lines),
        &rules("0"),
        AtomLineKind::File {
            packages_file: false,
        },
        WARN_RANK_MASK,
        &mut warn,
    );
    assert_eq!(kept0, vec!["dev-libs/t341-dep".to_string()]);
    assert_eq!(
        warn.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(),
        vec![
            "--- Invalid atom in /repo/profiles/t341/package.mask: dev-libs/t341-dep::t341",
            "--- Invalid atom in /repo/profiles/t341/package.mask: =dev-libs/t341-dep-1-2",
            "--- Invalid atom in /repo/profiles/t341/package.mask: */t341-dep",
            "--- Invalid atom in /repo/profiles/t341/package.mask: dev-libs/t341-dep:=",
            "--- Invalid atom in /repo/profiles/t341/package.mask: Slot deps are not allowed in EAPI 0: 'dev-libs/t341-dep:0'",
            "--- Invalid atom in /repo/profiles/t341/package.mask: Use deps are not allowed in EAPI 0: 'dev-libs/t341-dep[x(+)]'",
            "--- Invalid atom in /repo/profiles/t341/package.mask: dev-libs/t341-dep-1",
        ]
    );
}

/// `packages` strips a leading `*` before parsing (the message shows the
/// stripped atom); `-*` passes through untouched.
// Portage: util/__init__.py:572-581
#[test]
fn packages_lines_strip_star_before_parsing() {
    let path = Path::new("/repo/profiles/t341/packages");
    let mut warn = Vec::new();
    let kept = filter_atom_lines(
        &src(
            path,
            &[
                "*/t341-dep",
                "-*",
                "*dev-libs/t341-dep",
                "dev-libs/t341-dep::t341",
            ],
        ),
        &rules("5"),
        AtomLineKind::File {
            packages_file: true,
        },
        WARN_RANK_PACKAGES,
        &mut warn,
    );
    assert_eq!(
        kept,
        vec!["-*".to_string(), "*dev-libs/t341-dep".to_string()]
    );
    assert_eq!(
        warn.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(),
        vec![
            "--- Invalid atom in /repo/profiles/t341/packages: /t341-dep",
            "--- Invalid atom in /repo/profiles/t341/packages: dev-libs/t341-dep::t341",
        ]
    );
}

/// Dict files with `allow_use = false` drop use-carrying atoms with the
/// other message; `-` removal lines are ordinary dict keys here.
// Portage: util/__init__.py:532-538
#[test]
fn dict_lines_drop_use_atoms_with_their_own_message() {
    let path = Path::new("/repo/profiles/t341/package.use");
    let mut warn = Vec::new();
    let kept = filter_atom_lines(
        &src(path, &["dev-libs/t341-dep[x(+)] x", "dev-libs/t341-dep x"]),
        &rules("5"),
        AtomLineKind::Dict { allow_use: false },
        WARN_RANK_USE,
        &mut warn,
    );
    assert_eq!(kept, vec!["dev-libs/t341-dep x".to_string()]);
    assert_eq!(
        warn.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(),
        vec![
            "--- Atom is not allowed to have USE flag(s) in /repo/profiles/t341/package.use: dev-libs/t341-dep[x(+)]",
        ]
    );
}

/// Repo formats gate `::repo` and build-ids explicitly, even with an EAPI
/// given (S0 `formats` cell: both kept everywhere but accept_keywords).
// Portage: LocationsManager.py:340, repository/config.py:1476-1485
#[test]
fn repo_and_build_id_flags_come_from_formats() {
    let path = Path::new("/repo/profiles/t341/package.mask");
    let open = NodeAtomRules {
        eapi: Some("5".to_string()),
        allow_repo: true,
        allow_build_id: true,
        allow_wildcard: false,
    };
    let mut warn = Vec::new();
    let kept = filter_atom_lines(
        &src(path, &["dev-libs/t341-dep::t341", "=dev-libs/t341-dep-1-2"]),
        &open,
        AtomLineKind::File {
            packages_file: false,
        },
        WARN_RANK_MASK,
        &mut warn,
    );
    assert_eq!(
        kept,
        vec![
            "dev-libs/t341-dep::t341".to_string(),
            "=dev-libs/t341-dep-1-2".to_string(),
        ]
    );
    assert!(warn.is_empty());
    // accept_keywords never passes build-ids, formats or not (S0 formats
    // cell still warns on `=…-1-2` there): same rules, build-id off.
    let mut no_bid = open.clone();
    no_bid.allow_build_id = false;
    let mut warn = Vec::new();
    let kept = filter_atom_lines(
        &src(path, &["=dev-libs/t341-dep-1-2"]),
        &no_bid,
        AtomLineKind::Dict { allow_use: false },
        WARN_RANK_KEYWORDS,
        &mut warn,
    );
    assert!(kept.is_empty());
    assert_eq!(warn.len(), 1);
}

/// Node EAPI: the `eapi` file wins (`readline().strip()`); missing falls
/// back to the repo default; unsupported raises Portage's `ParseError`.
// Portage: LocationsManager.py:216-242
#[test]
fn node_eapi_reads_the_eapi_file_then_the_repo_default() {
    let root = TempDir::new("portage-profile-test-node-eapi").keep();
    let node = root.join("profiles/t341/noeapi");
    fs::create_dir_all(&node).unwrap();
    let formats: Vec<String> = vec![];
    // No `eapi` file: the repo default ("0" here).
    let r = node_atom_rules(&node, &formats, "0").unwrap();
    assert_eq!(r.eapi.as_deref(), Some("0"));
    assert!(!r.allow_repo && !r.allow_build_id && !r.allow_wildcard);
    // An `eapi` file wins, first line stripped.
    fs::write(node.join("eapi"), "5\n").unwrap();
    let r = node_atom_rules(&node, &formats, "0").unwrap();
    assert_eq!(r.eapi.as_deref(), Some("5"));
    // Unsupported: ParseError carrying the EAPI and the file path.
    fs::write(node.join("eapi"), "10\n").unwrap();
    let err = node_atom_rules(&node, &formats, "0").unwrap_err();
    assert!(matches!(err, Error::UnsupportedProfileEapi { .. }));
    assert_eq!(
        err.to_string(),
        format!(
            "!!! ParseError: Profile contains unsupported EAPI '10': '{}'\n\
             !!! Your current profile is invalid. If you have just changed your profile\n\
             !!! configuration, you should revert back to the previous configuration.\n\
             !!! Allowed actions are limited to --help, --info, --search, --sync, and\n\
             !!! --version.",
            node.join("eapi").display()
        )
    );
}

/// The file-adjacent reader needs exactly one line (only `\n` stripped);
/// anything else warns once and falls back; missing stays silent.
// Portage: util/__init__.py:441-475
#[test]
fn adjacent_eapi_file_needs_exactly_one_line() {
    let root = TempDir::new("portage-profile-test-adjacent-eapi").keep();
    let dir = root.join("profiles");
    fs::create_dir_all(&dir).unwrap();
    let mut warn = Vec::new();
    // Missing: silent default.
    assert_eq!(read_adjacent_eapi(&dir, &mut warn), None);
    assert!(warn.is_empty());
    // One line: value (only `\n` stripped).
    fs::write(dir.join("eapi"), "5\n").unwrap();
    assert_eq!(read_adjacent_eapi(&dir, &mut warn).as_deref(), Some("5"));
    assert!(warn.is_empty());
    // Two lines, or empty: warn once, fall back.
    fs::write(dir.join("eapi"), "5\n6\n").unwrap();
    assert_eq!(read_adjacent_eapi(&dir, &mut warn), None);
    assert_eq!(warn.len(), 1);
    assert!(warn[0].text.starts_with("--- Invalid 'eapi' file "));
    assert_eq!(warn[0].rank, WARN_RANK_REPO);
    fs::write(dir.join("eapi"), "").unwrap();
    assert_eq!(read_adjacent_eapi(&dir, &mut warn), None);
    assert_eq!(warn.len(), 2);
}

/// `repo.eapi`: top-level `profiles/eapi` wins only with
/// `profile_eapi_default` (EAPI >= 9); else the gated layout key, else "0".
/// The key counts only with `profile-default-eapi` in the formats.
// Portage: repository/config.py:524-547, :1688-1727
#[test]
fn repo_eapi_prefers_top_level_default_then_gated_key() {
    let root = TempDir::new("portage-profile-test-repo-eapi").keep();
    let loc = root.join("repo");
    fs::create_dir_all(loc.join("profiles")).unwrap();
    let mut warn = Vec::new();
    // No key, no top file: "0", silent.
    fs::create_dir_all(loc.join("metadata")).unwrap();
    fs::write(
        loc.join("metadata/layout.conf"),
        "masters =\nprofile-formats = portage-2\n",
    )
    .unwrap();
    let repos = vec![("t341".to_string(), loc.clone())];
    let rr = repo_rules_for(&repos, &mut warn);
    assert_eq!(rr[0].eapi, "0");
    assert_eq!(rr[0].file_eapi, "0");
    assert!(warn.is_empty());
    // Key without `profile-default-eapi`: SyntaxWarning, still "0".
    fs::write(
        loc.join("metadata/layout.conf"),
        "masters =\nprofile-formats = portage-2 profile-repo-deps build-id\nprofile_eapi_when_unspecified = 5\n",
    )
    .unwrap();
    let rr = repo_rules_for(&repos, &mut warn);
    assert_eq!(rr[0].eapi, "0");
    assert_eq!(warn.len(), 1);
    assert!(warn[0].text.starts_with("SyntaxWarning: "));
    assert!(warn[0].text.contains("profile_eapi_when_unspecified"));
    assert_eq!(warn[0].rank, WARN_RANK_REPO);
    // Key with the format: honored.
    fs::write(
        loc.join("metadata/layout.conf"),
        "masters =\nprofile-formats = portage-2 profile-repo-deps build-id profile-default-eapi\nprofile_eapi_when_unspecified = 5\n",
    )
    .unwrap();
    warn.clear();
    let rr = repo_rules_for(&repos, &mut warn);
    assert_eq!(rr[0].eapi, "5");
    assert_eq!(rr[0].file_eapi, "5");
    assert!(warn.is_empty());
    // Top-level `profiles/eapi` = 5 (no `profile_eapi_default`): the key
    // still wins for `repo.eapi`, the file for repo-level reads.
    fs::write(loc.join("profiles/eapi"), "5\n").unwrap();
    let rr = repo_rules_for(&repos, &mut warn);
    assert_eq!(rr[0].eapi, "5");
    assert_eq!(rr[0].file_eapi, "5");
    assert!(warn.is_empty());
}
