use super::*;

/// Real `_grab_pkg_env`'s acceptance gate (`config.py:2269-2300`):
/// every non-`USE` key is accepted unless it is in `env_blacklist`,
/// `environ_filter`, the profile's dynamic `PROFILE_ONLY_VARIABLES`,
/// or portuale's own `PORTUALE_COMPUTED` set. The live host capture
/// behind this matrix is pmtest's `findings/l2.md` "#95 S0".
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
        // rejected: env_blacklist
        ("SLOT", "bogus"),
        ("DEPEND", "dev-libs/bogus"),
        ("RDEPEND", "dev-libs/bogus"),
        ("EAPI", "8"),
        ("ROOT", "/"),
        ("PKGUSE", "x"),
        // rejected: global_only_vars + environ_filter
        ("CONFIG_PROTECT", "/protect"),
        // rejected: environ_filter (accepted into real's config,
        // never exported to a phase)
        ("ACCEPT_KEYWORDS", "~amd64"),
        ("CONFIG_PROTECT_MASK", "/mask"),
        ("USE_ORDER", "env"),
        // rejected: `USE` has its own `package_env_use` path
        ("USE", "lto"),
        // rejected: the profile's dynamic PROFILE_ONLY_VARIABLES
        ("ARCH", "x86"),
        ("ELIBC", "musl"),
        ("IUSE_IMPLICIT", "x"),
        ("USE_EXPAND", "VIDEO_CARDS"),
        // rejected: PORTUALE_COMPUTED (FEATURES/PORTAGE_TMPDIR are
        // residues #98/#99; the rest are portuale-owned)
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

/// Real's three-layer incremental semantics: `_grab_pkg_env` appends
/// within the container (`container[k] += " " + v`), then
/// `regenerate()` folds `[lower layers, pkg, calling-env]` with
/// `-*`/`-tok` pruning and a sorted union (`config.py:2735`,
/// `:2778-2825`) — the calling env folds last, so its `-tok` prunes
/// a package.env token (S0 cell B) but not vice versa. The three
/// host captures in pmtest's `findings/l2.md` "#95 S0" are the
/// expected values for the first three arms (empty calling env).
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
    // pkg prunes a base token, then the union is sorted.
    let out = match_package_env_vars(&one_file("-PROFILE_UNSET PKG_UNSET"), cpv, &[], &base, &[]);
    assert_eq!(
        out,
        vec![("ENV_UNSET".to_string(), "BASE_UNSET PKG_UNSET".to_string())]
    );
    // `-*` clears every base token.
    let out = match_package_env_vars(&one_file("-* ONLY"), cpv, &[], &base, &[]);
    assert_eq!(out, vec![("ENV_UNSET".to_string(), "ONLY".to_string())]);
    // Several matching files of one entry append in order.
    let two_files = vec![(
        "dev-libs/penvccpkg".to_string(),
        vec![
            ("ENV_UNSET".to_string(), "ONE".to_string()),
            ("ENV_UNSET".to_string(), "TWO".to_string()),
        ],
    )];
    let out = match_package_env_vars(&two_files, cpv, &[], &[], &[]);
    assert_eq!(out, vec![("ENV_UNSET".to_string(), "ONE TWO".to_string())]);
    // A `-tok` in the calling-env layer prunes a pkg-layer token
    // (S0 cell B: `FEATURES="-probe-feature"` beats the env file).
    let calling = vec![("ENV_UNSET".to_string(), "-PKG_UNSET".to_string())];
    let out = match_package_env_vars(&one_file("PKG_UNSET"), cpv, &[], &base, &calling);
    assert_eq!(
        out,
        vec![(
            "ENV_UNSET".to_string(),
            "BASE_UNSET PROFILE_UNSET".to_string()
        )]
    );
    // A pkg-layer `-tok` does NOT prune a calling-env token: the
    // calling env folds last and re-adds it (S0 cell B's mirror).
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

/// Scalars are set, not appended: a later file replaces, and an empty
/// value blanks the variable (real `CC=""` in an env file leaves the
/// phase's `CC` empty, S0 capture). A non-matching atom contributes
/// nothing.
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

/// Real's `env`-over-`pkg` scalar precedence (`USE_ORDER`,
/// `config.py:1031-1035`): a package.env scalar is dropped when the
/// calling environment carries the same key, so the process value
/// the caller already layered wins (backlog #101, S0 cell A). Keys
/// the calling env does not carry still arrive.
#[test]
fn package_env_scalar_loses_to_the_calling_environment() {
    let cpv = "dev-libs/penvccpkg-1.0:0/0";
    let files = vec![(
        "dev-libs/penvccpkg".to_string(),
        vec![
            ("CFLAGS".to_string(), "-Os -march=pkg".to_string()),
            ("CC".to_string(), "pkg-cc".to_string()),
        ],
    )];
    let calling = vec![("CFLAGS".to_string(), "-O2 -pipe".to_string())];
    let out = match_package_env_vars(&files, cpv, &[], &[], &calling);
    assert_eq!(out, vec![("CC".to_string(), "pkg-cc".to_string())]);
}

/// The raw single-key matchers behind the `PORTUALE_COMPUTED`
/// recomputes (#98/#99): scalars replace (last matching file wins),
/// incrementals append across matching files in order, non-matching
/// atoms and other keys contribute nothing — real `_grab_pkg_env`'s
/// container behaviour without the acceptance gate or the fold.
#[test]
fn package_env_raw_matchers_follow_the_container_semantics() {
    let cpv = "dev-libs/penvccpkg-1.0:0/0";
    let files = vec![
        (
            "dev-libs/otherpkg".to_string(),
            vec![
                ("FEATURES".to_string(), "not-me".to_string()),
                ("PORTAGE_TMPDIR".to_string(), "/not-me".to_string()),
            ],
        ),
        (
            "dev-libs/penvccpkg".to_string(),
            vec![
                ("FEATURES".to_string(), "ONE".to_string()),
                ("PORTAGE_TMPDIR".to_string(), "/first".to_string()),
                ("CC".to_string(), "ignored".to_string()),
            ],
        ),
        (
            "dev-libs/penvccpkg".to_string(),
            vec![
                ("FEATURES".to_string(), "TWO".to_string()),
                ("PORTAGE_TMPDIR".to_string(), "/second".to_string()),
            ],
        ),
    ];
    assert_eq!(
        match_package_env_scalar_raw(&files, cpv, "PORTAGE_TMPDIR"),
        Some("/second".to_string())
    );
    assert_eq!(
        match_package_env_incremental_raw(&files, cpv, "FEATURES"),
        "ONE TWO".to_string()
    );
    assert_eq!(match_package_env_scalar_raw(&files, cpv, "MISSING"), None);
    assert_eq!(
        match_package_env_incremental_raw(&files, cpv, "MISSING"),
        String::new()
    );
    assert_eq!(
        match_package_env_scalar_raw(&files, "dev-libs/other-1.0:0/0", "PORTAGE_TMPDIR"),
        None
    );
}
