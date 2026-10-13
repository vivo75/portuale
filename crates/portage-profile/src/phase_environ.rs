//! Portage `config.environ()` for build-phase environments: the export
//! set a source build's every phase sees, built from the resolved
//! [`Config`]. Expected values verified live against Portage.
//!
//! Rules mirrored (`ebuild/config.py`, `_config/special_env_vars.py`):
//! the `env` layer is the whole calling environment (highest priority);
//! `env_blacklist` keys never enter the config; `environ()` skips
//! `environ_filter`, forces `PORTAGE_FEATURES = FEATURES` and
//! `USE = PORTAGE_USE`, and pops `AA`; incrementals export folded and
//! sorted; every `USE_EXPAND` variable derives from the package's
//! effective USE; `IUSE_EFFECTIVE` is declared IUSE plus the implicit set.
//!
//! Deliberately not set here: keys the phase runner computes from the
//! build directory / graph entry ([`PORTUALE_COMPUTED`]) and the dynamic
//! `doebuild_environment()` values the per-entry caller adds on top.
//! `phase_env_vars` appends the caller's `extra_env` last, so anything
//! returned here can still be overridden downstream.
// Portage: ebuild/config.py, _config/special_env_vars.py

use std::collections::{BTreeMap, BTreeSet};

use crate::Config;

/// `environ_filter` keys: never exported to the ebuild environment.
// Portage: special_env_vars.py, environ_filter (vendored 3.0.82.2)
pub const ENVIRON_FILTER: &[&str] = &[
    "DEPEND",
    "RDEPEND",
    "PDEPEND",
    "SRC_URI",
    "BDEPEND",
    "IDEPEND",
    "INFOPATH",
    "MANPATH",
    "PYTHONUTF8",
    "USER",
    "GLOBSORT",
    "HISTFILE",
    "POSIXLY_CORRECT",
    "ACCEPT_CHOSTS",
    "ACCEPT_KEYWORDS",
    "ACCEPT_PROPERTIES",
    "ACCEPT_RESTRICT",
    "AUTOCLEAN",
    "BINPKG_COMPRESS",
    "BINPKG_COMPRESS_FLAGS",
    "CLEAN_DELAY",
    "COLLISION_IGNORE",
    "CONFIG_PROTECT",
    "CONFIG_PROTECT_MASK",
    "EGENCACHE_DEFAULT_OPTS",
    "EMERGE_DEFAULT_OPTS",
    "EMERGE_LOG_DIR",
    "EMERGE_WARNING_DELAY",
    "FETCH_WRAPPER",
    "FETCHCOMMAND",
    "FETCHCOMMAND_FTP",
    "FETCHCOMMAND_HTTP",
    "FETCHCOMMAND_HTTPS",
    "FETCHCOMMAND_RSYNC",
    "FETCHCOMMAND_SFTP",
    "FETCHCOMMAND_SSH",
    "GENTOO_MIRRORS",
    "NOCONFMEM",
    "O",
    "PORTAGE_BACKGROUND",
    "PORTAGE_BACKGROUND_UNMERGE",
    "PORTAGE_BINHOST",
    "PORTAGE_BINPKG_FORMAT",
    "PORTAGE_BUILDDIR_LOCKED",
    "PORTAGE_CHECKSUM_FILTER",
    "PORTAGE_ELOG_CLASSES",
    "PORTAGE_ELOG_MAILFROM",
    "PORTAGE_ELOG_MAILSUBJECT",
    "PORTAGE_ELOG_MAILURI",
    "PORTAGE_ELOG_SYSTEM",
    "PORTAGE_FETCH_CHECKSUM_TRY_MIRRORS",
    "PORTAGE_FETCH_RESUME_MIN_SIZE",
    "PORTAGE_GPG_DIR",
    "PORTAGE_GPG_KEY",
    "PORTAGE_GPG_SIGNING_COMMAND",
    "PORTAGE_IONICE_COMMAND",
    "PORTAGE_PACKAGE_EMPTY_ABORT",
    "PORTAGE_REPO_DUPLICATE_WARN",
    "PORTAGE_RO_DISTDIRS",
    "PORTAGE_RSYNC_EXTRA_OPTS",
    "PORTAGE_RSYNC_OPTS",
    "PORTAGE_RSYNC_RETRIES",
    "PORTAGE_SSH_OPTS",
    "PORTAGE_SYNC_STALE",
    "PORTAGE_TRUST_HELPER",
    "PORTAGE_USE",
    "PORTAGE_LOG_FILTER_FILE_CMD",
    "PORTAGE_LOGDIR",
    "PORTAGE_LOGDIR_CLEAN",
    "QUICKPKG_DEFAULT_OPTS",
    "REPOMAN_DEFAULT_OPTS",
    "RESUMECOMMAND",
    "RESUMECOMMAND_FTP",
    "RESUMECOMMAND_HTTP",
    "RESUMECOMMAND_HTTPS",
    "RESUMECOMMAND_RSYNC",
    "RESUMECOMMAND_SFTP",
    "RESUMECOMMAND_SSH",
    "UNINSTALL_IGNORE",
    "USE_EXPAND_HIDDEN",
    "USE_ORDER",
    "__PORTAGE_HELPER",
    "SYNC",
];

/// `env_blacklist` keys: never enter the config from the environment or
/// config files; set programmatically by the phase runner instead. The
/// keys this module computes (`USE`, `IUSE_EFFECTIVE`) insert after the
/// gate on purpose.
// Portage: special_env_vars.py, env_blacklist
pub const ENV_BLACKLIST: &[&str] = &[
    "A",
    "AA",
    "BDEPEND",
    "BROOT",
    "CATEGORY",
    "DEPEND",
    "DESCRIPTION",
    "DOCS",
    "EAPI",
    "EBUILD_FORCE_TEST",
    "EBUILD_PHASE",
    "EBUILD_PHASE_FUNC",
    "EBUILD_SKIP_MANIFEST",
    "ED",
    "EMERGE_FROM",
    "EPREFIX",
    "EROOT",
    "GREP_OPTIONS",
    "HOMEPAGE",
    "IDEPEND",
    "INHERITED",
    "IUSE",
    "IUSE_EFFECTIVE",
    "KEYWORDS",
    "LICENSE",
    "MERGE_TYPE",
    "PDEPEND",
    "PF",
    "PKGUSE",
    "PORTAGE_BACKGROUND",
    "PORTAGE_BACKGROUND_UNMERGE",
    "PORTAGE_BUILDDIR_LOCKED",
    "PORTAGE_BUILT_USE",
    "PORTAGE_CONFIGROOT",
    "PORTAGE_EXPLICIT_INHERIT",
    "PORTAGE_INTERNAL_CALLER",
    "PORTAGE_IUSE",
    "PORTAGE_NONFATAL",
    "PORTAGE_PIPE_FD",
    "PORTAGE_REPO_NAME",
    "PORTAGE_USE",
    "PROPERTIES",
    "RDEPEND",
    "REPOSITORY",
    "REQUIRED_USE",
    "RESTRICT",
    "ROOT",
    "SANDBOX_LOG",
    "SLOT",
    "SRC_URI",
    "_",
];

/// Keys the phase runner computes and must always win: excluded from both
/// halves of the export set so no `make.conf` scalar or calling-env
/// variable can clobber them. Portage reaches the same result by assigning
/// these after the config is built.
// Portage: doebuild.py, doebuild_environment
pub const PORTUALE_COMPUTED: &[&str] = &[
    // build directory / identity (`compute_environment` + `phase_env_vars`)
    "D",
    "ED",
    "T",
    "S",
    "WORKDIR",
    "HOME",
    "PORTAGE_BUILDDIR",
    "FILESDIR",
    "DISTDIR",
    "ROOT",
    "EROOT",
    "EPREFIX",
    "PATH",
    "P",
    "PN",
    "PV",
    "PR",
    "PVR",
    "PF",
    "CATEGORY",
    "EAPI",
    "EBUILD",
    "EBUILD_PHASE",
    "EBUILD_PHASE_FUNC",
    "O",
    "PORTAGE_TMPDIR",
    "PORTAGE_BIN_PATH",
    "PORTAGE_PYM_PATH",
    "PORTAGE_PYTHON",
    "PORTAGE_ECLASS_LOCATIONS",
    "PORTAGE_COLORMAP",
    "PORTAGE_QUIET",
    "PORTAGE_DEBUG",
    "PORTAGE_RESTRICT",
    "PORTAGE_PROPERTIES",
    "INHERITED",
    "EMERGE_FROM",
    "MERGE_TYPE",
    "SANDBOX_LOG",
    "SANDBOX_DISABLED",
    "SANDBOX_ON",
    // `run_commands_async` (fetch results) and the vdb-regeneration path
    "A",
    "AA",
    "PORTAGE_UPDATE_ENV",
    // per-entry threading (`entry_build_env`)
    "SLOT",
    "PORTAGE_REPO_NAME",
    "PORTAGE_REPO_REVISIONS",
    // `doebuild_environment()` dynamic; the packaging path owns it
    "PORTAGE_COMPRESSION_COMMAND",
    // computed by this module itself (never taken from a config file or
    // the calling env verbatim)
    "USE",
    "FEATURES",
    "PORTAGE_FEATURES",
    "IUSE_EFFECTIVE",
];

/// The per-package inputs `phase_environ` needs: the candidate's declared
/// `IUSE` (raw tokens, `+`/`-` defaults allowed) and its enabled flag set.
// Portage: Package.use.enabled
#[derive(Debug, Clone, Copy)]
pub struct PhaseUse<'a> {
    pub iuse: &'a [String],
    pub enabled: &'a [String],
}

/// `PORTAGE_USE`: `enabled ∩ (IUSE ∪ IUSE_EFFECTIVE)`, sorted. Exported
/// as `USE` and written to a binpkg's `metadata/USE` / `Packages` index.
// Portage: config.py, PORTAGE_USE / environ
pub fn portage_use(config: &Config, iuse: &[String], enabled: &[String]) -> Vec<String> {
    let valid: BTreeSet<&str> = iuse
        .iter()
        .map(|f| iuse_bare(f))
        .chain(config.iuse_effective.iter().map(String::as_str))
        .collect();
    enabled
        .iter()
        .map(String::as_str)
        .filter(|f| valid.contains(f))
        .collect::<BTreeSet<&str>>()
        .into_iter()
        .map(String::from)
        .collect()
}

/// `IUSE_EFFECTIVE` for one package: the profile implicit set plus the
/// package's own declared `IUSE` names, sorted.
// Portage: config.py, _calc_iuse_effective
fn iuse_effective_for(config: &Config, iuse: &[String]) -> Vec<String> {
    iuse.iter()
        .map(|f| iuse_bare(f).to_string())
        .chain(config.iuse_effective.iter().cloned())
        .collect::<BTreeSet<String>>()
        .into_iter()
        .collect()
}

/// The `___in_portage_iuse` exported-function body for `portage_iuse`
/// (already sorted): the hash form Portage emits for EAPIs with
/// `iuse_effective` (5+). Byte shape from `config.py:2184-2190`
/// (`BASH_FUNC____in_portage_iuse%%`), which `ebuild.sh:11` un-exports
/// on entry (`export -n -f`), so phases see the function but never the
/// variable itself.
// Portage: config.py, setcpv
pub fn in_portage_iuse_body(portage_iuse: &[String]) -> String {
    let hashed = portage_iuse
        .iter()
        .map(|f| format!("[\"{f}\"]=1"))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "() {{ if [[ ${{#___PORTAGE_IUSE_HASH[@]}} -lt 1 ]]; then   declare -gA ___PORTAGE_IUSE_HASH=({hashed}); fi; [[ -n ${{___PORTAGE_IUSE_HASH[$1]}} ]]; }}"
    )
}

/// `IUSE_EFFECTIVE` plus the `___in_portage_iuse` body for a standalone
/// phase (no resolved candidate): the md5-cache declared `IUSE` plus the
/// profile implicit set. `None` when the EAPI predates `iuse_effective`
/// (below the floor: Portage serves the `PORTAGE_IUSE`-regex form there,
/// which needs the lazy implicit-iuse regex this port never builds).
// Portage: config.py, setcpv (`_calc_iuse_effective`, `config.py:2181-2185`)
pub fn standalone_iuse_effective(
    config: &Config,
    iuse: &[String],
    attrs: portage_dep::eapi::EapiAttrs,
) -> Option<(String, String)> {
    if !attrs.iuse_effective {
        return None;
    }
    let effective = iuse_effective_for(config, iuse);
    let body = in_portage_iuse_body(&effective);
    Some((effective.join(" "), body))
}

fn iuse_bare(flag: &str) -> &str {
    flag.trim_start_matches(['+', '-'])
}

/// A key bash can `export`: `[A-Za-z_][A-Za-z0-9_]*`. Exported-function
/// entries (`BASH_FUNC_x%%`) cannot be re-exported and are dropped here.
fn exportable_name(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The gate every candidate key passes before entering the export set:
/// a valid identifier, not `environ_filter`ed, not blacklisted, not
/// portuale-computed.
fn exportable(key: &str) -> bool {
    exportable_name(key)
        && !ENVIRON_FILTER.contains(&key)
        && !ENV_BLACKLIST.contains(&key)
        && !PORTUALE_COMPUTED.contains(&key)
}

fn sorted_joined<'a>(set: impl IntoIterator<Item = &'a String>) -> String {
    set.into_iter()
        .map(String::as_str)
        .collect::<BTreeSet<&str>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Portage `config.environ()` for a build phase, from the resolved config
/// (rules: see the module doc comment).
///
/// Returns `(name, value)` pairs in sorted-by-name order. Set-but-empty
/// values export as empty; unset-everywhere keys are absent. Precedence:
/// config scalars first, then the calling environment on top, then the
/// folded incrementals and per-package values, which always win.
///
/// `pkg` is `None` for a run without a resolved candidate: no
/// `USE`/`IUSE_EFFECTIVE` is produced and every `USE_EXPAND` variable is
/// its profile scalar if set, else `""`.
// Portage: config.py, environ
pub fn phase_environ(config: &Config, pkg: Option<PhaseUse<'_>>) -> Vec<(String, String)> {
    let mut env: BTreeMap<String, String> = BTreeMap::new();

    // 1. Resolved config scalars.
    for (k, v) in &config.other_vars {
        if exportable(k) {
            env.insert(k.clone(), v.clone());
        }
    }

    // 2. The calling environment, wholesale -- highest layer.
    for (k, v) in crate::config_env_all() {
        if exportable(&k) {
            env.insert(k, v);
        }
    }

    // 2b. `doebuild_environment()` config-only dynamic defaults: `PATH`
    //     when env.d sets it; `MAKEOPTS`/`GNUMAKEFLAGS` from the CPU count
    //     when neither is set.
    // Portage: doebuild.py, _doebuild_path and MAKEOPTS defaults
    if config.envd_sets_path
        && let Some(path) = config.other_vars.get("PATH")
    {
        env.insert("PATH".to_string(), path.clone());
    }
    if !env.contains_key("MAKEOPTS") && !env.contains_key("MAKEFLAGS") {
        let nproc = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        env.insert("MAKEOPTS".to_string(), format!("-j{nproc}"));
        env.entry("GNUMAKEFLAGS".to_string())
            .or_insert_with(|| format!("--load-average {nproc} --output-sync=line"));
    }

    // 3. Incrementals, folded and sorted.
    let features = config
        .resolved_incremental("FEATURES")
        .or_else(|| {
            config
                .other_vars
                .get("FEATURES")
                .map(|f| f.split_whitespace().map(String::from).collect())
        })
        .unwrap_or_default()
        .join(" ");
    env.insert("FEATURES".to_string(), features.clone());
    env.insert("PORTAGE_FEATURES".to_string(), features);
    for key in ["ENV_UNSET", "PROFILE_ONLY_VARIABLES"] {
        if let Some(folded) = config.resolved_incremental(key) {
            env.insert(key.to_string(), folded.join(" "));
        }
    }
    env.insert("USE_EXPAND".to_string(), sorted_joined(&config.use_expand));
    env.insert(
        "USE_EXPAND_UNPREFIXED".to_string(),
        sorted_joined(&config.use_expand_unprefixed),
    );
    env.insert(
        "USE_EXPAND_IMPLICIT".to_string(),
        sorted_joined(&config.use_expand_implicit),
    );
    env.insert(
        "IUSE_IMPLICIT".to_string(),
        sorted_joined(&config.iuse_implicit),
    );

    // 4. Per-package rows. With a package they overwrite steps 1-3; with
    //    no package the placeholders only fill gaps (`or_insert`).
    for (key, value) in phase_environ_pkg(config, pkg) {
        if pkg.is_some() {
            env.insert(key, value);
        } else {
            env.entry(key).or_insert(value);
        }
    }

    env.into_iter().collect()
}

/// The per-package rows of `phase_environ`, split out so the per-entry
/// build threading can add them on top of the run-wide
/// `phase_environ(config, None)` result without recomputing the base.
///
/// `Some(pkg)`: `USE`, `IUSE_EFFECTIVE`, and every `USE_EXPAND` value
/// derived from the effective USE (empty when off). `None`: no
/// `USE`/`IUSE_EFFECTIVE`; unprefixed names (`ARCH`) keep their scalar.
// Portage: config.py, setcpv
pub fn phase_environ_pkg(config: &Config, pkg: Option<PhaseUse<'_>>) -> Vec<(String, String)> {
    let mut env: BTreeMap<String, String> = BTreeMap::new();
    match pkg {
        Some(p) => {
            let use_flags = portage_use(config, p.iuse, p.enabled);
            env.insert(
                "IUSE_EFFECTIVE".to_string(),
                iuse_effective_for(config, p.iuse).join(" "),
            );
            for var in &config.use_expand {
                if config.use_expand_unprefixed.contains(var) {
                    continue;
                }
                let prefix = format!("{}_", var.to_lowercase());
                let values: Vec<&str> = use_flags
                    .iter()
                    .filter_map(|f| f.strip_prefix(prefix.as_str()))
                    .collect();
                env.insert(var.clone(), values.join(" "));
            }
            env.insert("USE".to_string(), use_flags.join(" "));
        }
        None => {
            for var in &config.use_expand {
                if config.use_expand_unprefixed.contains(var) {
                    continue;
                }
                env.entry(var.clone()).or_default();
            }
        }
    }
    env.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ResolveConfigArgs;
    use crate::{TEST_ENV_OVERRIDE, resolve_config};
    use portage_util::TempDir;
    use std::collections::HashMap;
    use std::fs;
    use std::path::{Path, PathBuf};

    /// Thread-local override: the map is the entire process env for this
    /// thread, never touching `std::env`.
    fn with_test_env(vars: &[(&str, &str)], f: impl FnOnce()) {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        TEST_ENV_OVERRIDE.with(|o| *o.borrow_mut() = Some(map));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        TEST_ENV_OVERRIDE.with(|o| *o.borrow_mut() = None);
        if let Err(e) = result {
            std::panic::resume_unwind(e);
        }
    }

    /// A synthetic amd64-shaped profile under a temp `config_root` (no
    /// `make.globals`; everything from there is seeded in `make.conf`).
    /// Expected values from Portage (oracle env).
    fn synthetic_root(name: &str, make_conf: &str) -> (std::path::PathBuf, PathBuf) {
        let root = TempDir::new(&format!("portage-profile-phase-environ-{name}")).keep();
        let _ = fs::remove_dir_all(&root);
        let repo = root.join("repo");
        let prof = repo.join("profiles/default");
        let portage_dir = root.join("etc/portage");
        fs::create_dir_all(&prof).unwrap();
        fs::create_dir_all(&portage_dir).unwrap();
        fs::write(
            prof.join("make.defaults"),
            "ARCH=\"amd64\"\nACCEPT_KEYWORDS=\"${ARCH}\"\n\
             ELIBC=\"glibc\"\nKERNEL=\"linux\"\n\
             CHOST=\"x86_64-pc-linux-gnu\"\nCOMMON_FLAGS=\"-O2 -pipe\"\nCFLAGS=\"${COMMON_FLAGS}\"\n\
             MULTILIB_ABIS=\"amd64 x86\"\nDEFAULT_ABI=\"amd64\"\nABI=\"amd64\"\n\
             LIBDIR_amd64=\"lib64\"\nLIBDIR_x86=\"lib\"\n\
             USE_EXPAND=\"ABI_X86 ELIBC KERNEL L10N VIDEO_CARDS\"\nUSE_EXPAND_UNPREFIXED=\"ARCH\"\n\
             USE_EXPAND_IMPLICIT=\"ARCH ELIBC KERNEL\"\n\
             USE_EXPAND_VALUES_ARCH=\"amd64 x86\"\nUSE_EXPAND_VALUES_ELIBC=\"glibc musl\"\n\
             USE_EXPAND_VALUES_KERNEL=\"linux\"\n\
             IUSE_IMPLICIT=\"abi_x86_64 prefix\"\n\
             ENV_UNSET=\"DISPLAY PERL5LIB\"\n\
             PROFILE_ONLY_VARIABLES=\"KERNEL ELIBC\"\n\
             PROFILE_ONLY_VARIABLES=\"${PROFILE_ONLY_VARIABLES} ARCH\"\n\
             USE=\"abi_x86_64 baseflag\"\nVIDEO_CARDS=\"nvidia\"\n\
             FEATURES=\"assume-digests binpkg-docompress binpkg-dostrip sandbox\"\n\
             SRC_URI=\"should-never-export\"\nHOME=\"/profile/home\"\n",
        )
        .unwrap();
        fs::write(portage_dir.join("make.conf"), make_conf).unwrap();
        let make_profile = portage_dir.join("make.profile");
        let _ = fs::remove_file(&make_profile);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&prof, &make_profile).unwrap();
        (root, repo)
    }

    fn resolve(root: &Path, repo: &Path) -> Config {
        resolve_config(ResolveConfigArgs {
            config_root: root,
            main_repo_location: repo,
            overlay_repos: &[],
            repo_aliases: &[],
            main_repo_name: "testrepo",
            repo_masters: &HashMap::new(),
            eroot: root,
        })
        .expect("synthetic profile resolves")
    }

    fn get<'a>(env: &'a [(String, String)], key: &str) -> Option<&'a str> {
        env.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn portage_use_is_enabled_intersected_with_iuse_and_implicit_iuse() {
        let (root, repo) = synthetic_root("portage-use", "");
        with_test_env(&[], || {
            let c = resolve(&root, &repo);
            // Empty IUSE: only implicit flags survive; an enabled but
            // undeclared flag is hidden from `use()`.
            let enabled = strs(&[
                "abi_x86_64",
                "amd64",
                "elibc_glibc",
                "kernel_linux",
                "baseflag",
            ]);
            assert_eq!(
                portage_use(&c, &[], &enabled),
                strs(&["abi_x86_64", "amd64", "elibc_glibc", "kernel_linux"])
            );
            // A declared flag (with a `+` default marker) joins; order is
            // sorted, duplicates collapse.
            let iuse = strs(&["+baseflag", "other"]);
            let dup = strs(&["baseflag", "baseflag", "amd64"]);
            assert_eq!(portage_use(&c, &iuse, &dup), strs(&["amd64", "baseflag"]));
        });
    }

    #[test]
    fn standalone_iuse_effective_matches_portage_setcpv_shape() {
        use portage_dep::eapi::eapi_attrs;
        let (root, repo) = synthetic_root("standalone-iuse", "");
        with_test_env(&[], || {
            let c = resolve(&root, &repo);
            let attrs8 = eapi_attrs(Some("8")).unwrap();
            // Declared IUSE (with a `+` default marker) joins the profile
            // implicit set, sorted; the body carries the same set hashed.
            let (effective, body) =
                standalone_iuse_effective(&c, &strs(&["+foo", "bar"]), attrs8).unwrap();
            let implicit = c.iuse_effective.iter().cloned().collect::<Vec<_>>();
            let mut want: Vec<String> = implicit
                .into_iter()
                .chain(["foo", "bar"].map(String::from))
                .collect();
            want.sort();
            want.dedup();
            assert_eq!(effective, want.join(" "));
            let hashed = want
                .iter()
                .map(|f| format!("[\"{f}\"]=1"))
                .collect::<Vec<_>>()
                .join(" ");
            assert_eq!(
                body,
                format!(
                    "() {{ if [[ ${{#___PORTAGE_IUSE_HASH[@]}} -lt 1 ]]; then   declare -gA ___PORTAGE_IUSE_HASH=({hashed}); fi; [[ -n ${{___PORTAGE_IUSE_HASH[$1]}} ]]; }}"
                )
            );
            // Below `iuse_effective` (EAPI 4): no pair (Portage serves the
            // `PORTAGE_IUSE`-regex form there instead).
            let attrs4 = eapi_attrs(Some("4")).unwrap();
            assert!(standalone_iuse_effective(&c, &strs(&["foo"]), attrs4).is_none());
        });
    }

    #[test]
    fn phase_environ_exports_the_profile_family_and_folded_incrementals() {
        let (root, repo) = synthetic_root(
            "family",
            "FEATURES=\"-sandbox splitdebug\"\nPORTAGE_COMPRESS=\"bzip2\"\n\
             SOURCE_DATE_EPOCH=\"1740000000\"\nMAKEOPTS=\"-j1\"\n",
        );
        with_test_env(&[], || {
            let c = resolve(&root, &repo);
            let iuse: Vec<String> = Vec::new();
            let enabled = strs(&[
                "abi_x86_64",
                "amd64",
                "elibc_glibc",
                "kernel_linux",
                "baseflag",
            ]);
            let env = phase_environ(
                &c,
                Some(PhaseUse {
                    iuse: &iuse,
                    enabled: &enabled,
                }),
            );

            // Sorted by name, no duplicate keys.
            let keys: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
            let mut sorted = keys.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(keys, sorted);

            // Expected values from Portage (oracle rows).
            assert_eq!(get(&env, "ARCH"), Some("amd64"));
            assert_eq!(get(&env, "ELIBC"), Some("glibc"));
            assert_eq!(get(&env, "KERNEL"), Some("linux"));
            assert_eq!(get(&env, "ABI"), Some("amd64"));
            assert_eq!(get(&env, "DEFAULT_ABI"), Some("amd64"));
            assert_eq!(get(&env, "MULTILIB_ABIS"), Some("amd64 x86"));
            assert_eq!(get(&env, "LIBDIR_amd64"), Some("lib64"));
            assert_eq!(get(&env, "COMMON_FLAGS"), Some("-O2 -pipe"));
            assert_eq!(get(&env, "CFLAGS"), Some("-O2 -pipe"));
            assert_eq!(get(&env, "USE_EXPAND_IMPLICIT"), Some("ARCH ELIBC KERNEL"));
            assert_eq!(get(&env, "USE_EXPAND_UNPREFIXED"), Some("ARCH"));
            assert_eq!(
                get(&env, "USE_EXPAND"),
                Some("ABI_X86 ELIBC KERNEL L10N VIDEO_CARDS")
            );
            assert_eq!(get(&env, "IUSE_IMPLICIT"), Some("abi_x86_64 prefix"));
            assert_eq!(get(&env, "USE_EXPAND_VALUES_ARCH"), Some("amd64 x86"));
            assert_eq!(get(&env, "ENV_UNSET"), Some("DISPLAY PERL5LIB"));

            // FEATURES is the incremental fold, mirrored as PORTAGE_FEATURES.
            let features = "assume-digests binpkg-docompress binpkg-dostrip splitdebug";
            assert_eq!(get(&env, "FEATURES"), Some(features));
            assert_eq!(get(&env, "PORTAGE_FEATURES"), Some(features));

            // Plain make.conf scalars pass through.
            assert_eq!(get(&env, "PORTAGE_COMPRESS"), Some("bzip2"));
            assert_eq!(get(&env, "SOURCE_DATE_EPOCH"), Some("1740000000"));
            assert_eq!(get(&env, "MAKEOPTS"), Some("-j1"));

            // USE is PORTAGE_USE; PORTAGE_USE itself is filtered out.
            assert_eq!(
                get(&env, "USE"),
                Some("abi_x86_64 amd64 elibc_glibc kernel_linux")
            );
            assert_eq!(
                get(&env, "IUSE_EFFECTIVE"),
                Some("abi_x86_64 amd64 elibc_glibc elibc_musl kernel_linux prefix x86")
            );
            assert_eq!(get(&env, "PORTAGE_USE"), None);

            // USE_EXPAND values derive from the package's USE.
            assert_eq!(get(&env, "ABI_X86"), Some("64"));
            assert_eq!(get(&env, "VIDEO_CARDS"), Some(""));
            assert_eq!(get(&env, "L10N"), Some(""));
        });
    }

    #[test]
    fn phase_environ_filters_blacklists_and_never_clobbers_computed_keys() {
        let (root, repo) = synthetic_root(
            "filter",
            "HOME=\"/conf/home\"\nDISTDIR=\"/conf/distfiles\"\nPORTAGE_TMPDIR=\"/conf/tmp\"\n\
             GENTOO_MIRRORS=\"https://mirror\"\nCONFIG_PROTECT=\"/etc\"\n\
             EMERGE_DEFAULT_OPTS=\"--ask\"\nACCEPT_KEYWORDS=\"~amd64\"\n\
             PORTAGE_COMPRESS=\"\"\n",
        );
        with_test_env(
            &[
                // Calling env Portage would carry.
                ("L2_JOBS", "1"),
                ("container", "podman"),
                ("TZ", "UTC"),
                // Blacklisted / computed / filtered entries.
                ("SLOT", "9"),
                ("A", "evil.tar"),
                ("AA", "evil.tar"),
                ("EAPI", "0"),
                ("ROOT", "/evil"),
                ("PORTAGE_REPO_NAME", "evil"),
                ("PORTAGE_CONFIGROOT", "/evil"),
                ("PATH", "/evil/bin"),
                ("HOME", "/env/home"),
                ("O", "/evil/ebuild/dir"),
                ("PORTAGE_USE", "evil"),
                ("SRC_URI", "http://evil"),
                ("USE", "evil"),
                ("FEATURES", "evil"),
                ("_", "/usr/bin/env"),
                ("BASH_FUNC_foo%%", "() { :; }"),
                ("1BAD", "x"),
                // A plain env scalar overriding make.conf.
                ("MAKEOPTS", "-j7"),
            ],
            || {
                let c = resolve(&root, &repo);
                let env = phase_environ(&c, None);

                for absent in [
                    "SLOT",
                    "A",
                    "AA",
                    "EAPI",
                    "ROOT",
                    "PORTAGE_REPO_NAME",
                    "PORTAGE_CONFIGROOT",
                    "PATH",
                    "HOME",
                    "O",
                    "PORTAGE_USE",
                    "SRC_URI",
                    "DISTDIR",
                    "PORTAGE_TMPDIR",
                    "GENTOO_MIRRORS",
                    "CONFIG_PROTECT",
                    "EMERGE_DEFAULT_OPTS",
                    "ACCEPT_KEYWORDS",
                    "_",
                    "BASH_FUNC_foo%%",
                    "1BAD",
                    "IUSE_EFFECTIVE",
                ] {
                    assert_eq!(get(&env, absent), None, "{absent} must not be exported");
                }

                // Calling env passes through.
                assert_eq!(get(&env, "L2_JOBS"), Some("1"));
                assert_eq!(get(&env, "container"), Some("podman"));
                assert_eq!(get(&env, "TZ"), Some("UTC"));
                assert_eq!(get(&env, "MAKEOPTS"), Some("-j7"));

                // Env `USE`/`FEATURES` never leak verbatim (folded, not copied).
                assert_eq!(
                    get(&env, "FEATURES"),
                    Some("assume-digests binpkg-docompress binpkg-dostrip evil sandbox")
                );
                assert_eq!(get(&env, "USE"), None);

                // Set-but-empty exports as empty.
                assert_eq!(get(&env, "PORTAGE_COMPRESS"), Some(""));
                // Unset keys are absent.
                assert_eq!(get(&env, "PORTAGE_COMPRESS_FLAGS"), None);

                // `pkg == None`: USE_EXPAND placeholders fill gaps only.
                assert_eq!(get(&env, "VIDEO_CARDS"), Some("nvidia"));
                assert_eq!(get(&env, "L10N"), Some(""));
                assert_eq!(get(&env, "ABI_X86"), Some(""));
                assert_eq!(get(&env, "ARCH"), Some("amd64"));
            },
        );
    }

    #[test]
    fn phase_environ_applies_the_doebuild_environment_config_defaults() {
        let (root, repo) = synthetic_root("dynamic", "PATH=\"/conf/bin:/usr/bin\"\n");
        // No env.d PATH: PATH stays a computed key, nothing exported.
        with_test_env(&[("PATH", "/env/bin")], || {
            let c = resolve(&root, &repo);
            assert!(!c.envd_sets_path);
            let env = phase_environ(&c, None);
            assert_eq!(get(&env, "PATH"), None);
            // Incremental fold, sorted.
            assert_eq!(
                get(&env, "PROFILE_ONLY_VARIABLES"),
                Some("ARCH ELIBC KERNEL")
            );
            // Neither MAKEOPTS nor MAKEFLAGS set: the CPU-count default.
            let nproc = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1);
            let makeopts = format!("-j{nproc}");
            assert_eq!(get(&env, "MAKEOPTS"), Some(makeopts.as_str()));
            let gnumake = format!("--load-average {nproc} --output-sync=line");
            assert_eq!(get(&env, "GNUMAKEFLAGS"), Some(gnumake.as_str()));
        });
        with_test_env(&[("MAKEFLAGS", "-j3")], || {
            let env = phase_environ(&resolve(&root, &repo), None);
            assert_eq!(get(&env, "MAKEOPTS"), None);
            assert_eq!(get(&env, "GNUMAKEFLAGS"), None);
        });
        // env.d sets PATH: the config-file stack's value is exported.
        fs::create_dir_all(root.join("etc")).unwrap();
        fs::write(
            root.join("etc/profile.env"),
            "export PATH='/usr/local/bin:/usr/bin:/opt/bin'\n",
        )
        .unwrap();
        with_test_env(&[("PATH", "/env/bin")], || {
            let c = resolve(&root, &repo);
            assert!(c.envd_sets_path);
            let env = phase_environ(&c, None);
            assert_eq!(get(&env, "PATH"), Some("/conf/bin:/usr/bin"));
        });
    }

    #[test]
    fn the_three_key_sets_are_transcribed_consistently() {
        // Every entry is a valid identifier (except Portage's literal
        // `_`); nothing computed here is filtered from its own output.
        for k in ENVIRON_FILTER
            .iter()
            .chain(ENV_BLACKLIST)
            .chain(PORTUALE_COMPUTED)
        {
            assert!(*k == "_" || exportable_name(k), "{k}");
        }
        for k in [
            "FEATURES",
            "PORTAGE_FEATURES",
            "USE",
            "IUSE_EFFECTIVE",
            "ENV_UNSET",
            "USE_EXPAND",
            "USE_EXPAND_UNPREFIXED",
            "USE_EXPAND_IMPLICIT",
            "IUSE_IMPLICIT",
        ] {
            assert!(
                !ENVIRON_FILTER.contains(&k),
                "{k} is computed here yet filtered"
            );
        }
        // `PORTAGE_USE` and `O` are filtered by Portage.
        assert!(ENVIRON_FILTER.contains(&"PORTAGE_USE"));
        assert!(ENVIRON_FILTER.contains(&"O"));
        assert!(ENV_BLACKLIST.contains(&"AA"));
    }
}
