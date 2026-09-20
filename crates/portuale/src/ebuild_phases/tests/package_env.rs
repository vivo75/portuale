use super::*;

/// Every non-`USE` key is accepted unless blacklisted, filtered,
/// profile-only, or portuale-computed; see notes for the matrix source.
#[test]
fn package_env_accepts_real_scalars_and_rejects_non_user_variables() {
    let profile_only = vec![
        "ARCH".to_string(),
        "ELIBC".to_string(),
        "IUSE_IMPLICIT".to_string(),
        "USE_EXPAND".to_string(),
    ];
    let vars: Vec<(String, String)> = [
        ("CC", "clang"),
        ("CXX", "clang++"),
        ("CPP", "clang-cpp"),
        ("AR", "llvm-ar"),
        ("NM", "llvm-nm"),
        ("RANLIB", "llvm-ranlib"),
        ("LD", "ld.lld"),
        ("RUSTFLAGS", "-C target-cpu=znver3"),
        ("CGO_CFLAGS", "-O2"),
        ("GOFLAGS", "-mod=mod"),
        ("NINJAOPTS", "-j13"),
        ("COMMON_FLAGS", "-O2"),
        ("INSTALL_MASK", "/usr/share/mask"),
        ("PORTAGE_NICENESS", "7"),
        ("__PROBE_LOCAL", "secret"),
        ("SLOT", "bogus"),
        ("DEPEND", "dev-libs/bogus"),
        ("RDEPEND", "dev-libs/bogus"),
        ("EAPI", "8"),
        ("ROOT", "/"),
        ("PKGUSE", "x"),
        ("CONFIG_PROTECT", "/protect"),
        ("ACCEPT_KEYWORDS", "~amd64"),
        ("CONFIG_PROTECT_MASK", "/mask"),
        ("USE_ORDER", "env"),
        ("USE", "lto"),
        ("ARCH", "x86"),
        ("ELIBC", "musl"),
        ("IUSE_IMPLICIT", "x"),
        ("USE_EXPAND", "VIDEO_CARDS"),
        ("FEATURES", "probe-feature"),
        ("PORTAGE_TMPDIR", "/tmp/probe"),
        ("DISTDIR", "/d"),
        ("PATH", "/bin"),
        ("D", "/d"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let out = match_package_env_vars(
        &[("dev-libs/penvccpkg".to_string(), vars)],
        "dev-libs/penvccpkg-1.0:0/0",
        &profile_only,
        &[],
        &[],
    );
    let keys: Vec<&str> = out.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        keys,
        vec![
            "CC",
            "CXX",
            "CPP",
            "AR",
            "NM",
            "RANLIB",
            "LD",
            "RUSTFLAGS",
            "CGO_CFLAGS",
            "GOFLAGS",
            "NINJAOPTS",
            "COMMON_FLAGS",
            "INSTALL_MASK",
            "PORTAGE_NICENESS",
            "__PROBE_LOCAL",
        ]
    );
    assert_eq!(
        out.iter().find(|(k, _)| k == "CC").map(|(_, v)| v.as_str()),
        Some("clang")
    );
}

/// Incrementals fold `[lower layers, pkg, calling-env]` with `-tok`
/// pruning and a sorted union; the calling env folds last.
#[test]
fn package_env_incrementals_fold_onto_the_base_like_regenerate() {
    let cpv = "dev-libs/penvccpkg-1.0:0/0";
    let base = vec![(
        "ENV_UNSET".to_string(),
        "BASE_UNSET PROFILE_UNSET".to_string(),
    )];
    let one_file = |v: &str| {
        vec![(
            "dev-libs/penvccpkg".to_string(),
            vec![("ENV_UNSET".to_string(), v.to_string())],
        )]
    };
    let out = match_package_env_vars(&one_file("-PROFILE_UNSET PKG_UNSET"), cpv, &[], &base, &[]);
    assert_eq!(
        out,
        vec![("ENV_UNSET".to_string(), "BASE_UNSET PKG_UNSET".to_string())]
    );
    let out = match_package_env_vars(&one_file("-* ONLY"), cpv, &[], &base, &[]);
    assert_eq!(out, vec![("ENV_UNSET".to_string(), "ONLY".to_string())]);
    let two_files = vec![(
        "dev-libs/penvccpkg".to_string(),
        vec![
            ("ENV_UNSET".to_string(), "ONE".to_string()),
            ("ENV_UNSET".to_string(), "TWO".to_string()),
        ],
    )];
    let out = match_package_env_vars(&two_files, cpv, &[], &[], &[]);
    assert_eq!(out, vec![("ENV_UNSET".to_string(), "ONE TWO".to_string())]);
    let calling = vec![("ENV_UNSET".to_string(), "-PKG_UNSET".to_string())];
    let out = match_package_env_vars(&one_file("PKG_UNSET"), cpv, &[], &base, &calling);
    assert_eq!(
        out,
        vec![(
            "ENV_UNSET".to_string(),
            "BASE_UNSET PROFILE_UNSET".to_string()
        )]
    );
    let calling = vec![("ENV_UNSET".to_string(), "KEEP".to_string())];
    let out = match_package_env_vars(&one_file("-KEEP PKG_UNSET"), cpv, &[], &base, &calling);
    assert_eq!(
        out,
        vec![(
            "ENV_UNSET".to_string(),
            "BASE_UNSET KEEP PKG_UNSET PROFILE_UNSET".to_string()
        )]
    );
}

/// Scalars are set, not appended: a later file replaces, an empty
/// value blanks the variable, and a non-matching atom adds nothing.
#[test]
fn package_env_scalars_last_wins_and_empty_blanks() {
    let cpv = "dev-libs/penvccpkg-1.0:0/0";
    let files = vec![
        (
            "dev-libs/otherpkg".to_string(),
            vec![("CC".to_string(), "not-me".to_string())],
        ),
        (
            "dev-libs/penvccpkg".to_string(),
            vec![
                ("CC".to_string(), "first".to_string()),
                ("CC".to_string(), String::new()),
                ("CXX".to_string(), "clang++".to_string()),
            ],
        ),
    ];
    let out = match_package_env_vars(&files, cpv, &[], &[], &[]);
    assert_eq!(
        out,
        vec![
            ("CC".to_string(), String::new()),
            ("CXX".to_string(), "clang++".to_string()),
        ]
    );
}
