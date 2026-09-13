//! Real `config.environ()` for portuale's build-phase environment -- the
//! **export set** a source build's every phase (and the `install_qa_check`
//! misc-functions call after it) must see, built from the resolved
//! [`Config`] instead of a curated allowlist. Backlog #37, slice S1
//! (`docs/037_Build-phase-env-completeness.plan.md`, gates G1/G2/G3 as
//! decided 2026-09-13); the S0 recon that fixed the spec is
//! `TEST/findings/l2.md` "S0 recon (#37, 2026-09-13)".
//!
//! Real semantics being mirrored (`3rdparty/portage/lib/portage/package/
//! ebuild/config.py`, `_config/special_env_vars.py`):
//!
//! * The config's `env` layer is **the whole calling environment**:
//!   `self.backupenv = os.environ.copy()`; `configdict["env"] =
//!   LazyItemsDict(self.backupenv)` (`config.py:551,567`), highest
//!   `USE_ORDER` priority -- so for the export set, a process-env key
//!   overrides the same key from `make.conf`/profile.
//! * Keys in `env_blacklist` (`special_env_vars.py:15-74`) never enter
//!   the config from the environment or config files at all (`A`, `AA`,
//!   `SLOT`, `EAPI`, `ROOT`, `PORTAGE_REPO_NAME`, `IUSE_EFFECTIVE`, …) --
//!   they are set programmatically. Portuale's phase runner and the
//!   per-entry threading own those; see [`ENV_BLACKLIST`].
//! * `config.environ()` (`config.py:3263-3350`) iterates every string
//!   value, skips `environ_filter` (`special_env_vars.py:257-348`,
//!   [`ENVIRON_FILTER`]), then forces `PORTAGE_FEATURES = FEATURES`
//!   (`:3326`) and `USE = PORTAGE_USE` (`:3329`; `PORTAGE_USE` itself is
//!   in `environ_filter`, so only `USE` reaches the phase), and pops `AA`
//!   for every EAPI >= 4 (`:3331-3333`; portuale's EAPI floor is 5+, so
//!   always). The `environ_whitelist` restriction (`:3275-3305`) only
//!   applies from the *second* phase on, once `$T/environment` exists;
//!   portuale runs every phase in a fresh shell with the same env
//!   (`ebuild_phases::run_commands_async`), so the phase-1 full set is
//!   the spec and the phase-2+ re-export narrowing is deliberately not
//!   replicated (the "ebuild `unset` persists" nuance of bug 189417 is
//!   filed as a pre-existing gap, not this module's).
//! * Incrementals are exported folded and sorted (`config.regenerate()`:
//!   `" ".join(sorted(myflags))`) -- `FEATURES`, `ENV_UNSET`, the
//!   `USE_EXPAND*`/`IUSE_IMPLICIT` name lists.
//! * Every `USE_EXPAND` variable gets a value derived from the package's
//!   effective `USE` (`config.py:2218-2252`, the `_lazy_use_expand`
//!   singletons at `:2247`): `ABI_X86="64"` when `abi_x86_64` is in
//!   `USE`, `L10N=""` otherwise -- one entry per `USE_EXPAND` name even
//!   when empty (S0: 49 such placeholders in the real oracle env).
//!   `USE_EXPAND_UNPREFIXED` variables (`ARCH`) keep their profile
//!   scalar value.
//! * `IUSE_EFFECTIVE` is the package's declared `IUSE` (bare names)
//!   plus the profile-level implicit set (`config.py::
//!   _calc_iuse_effective`, portuale's [`Config::iuse_effective`]).
//!
//! What this module deliberately does **not** set -- the keys portuale's
//! own phase runner computes from the build directory / graph entry
//! ([`PORTUALE_COMPUTED`]: `D`, `T`, `HOME`, `PATH`, `SLOT`, `A`, …) and
//! the dynamic `doebuild_environment()` values (`PORTAGE_COMPRESSION_
//! COMMAND`, `PORTAGE_REPO_*`) that the per-entry caller adds on top.
//! `phase_env_vars` appends the caller's `extra_env` last, so anything
//! this module returns can still be overridden downstream by design.

use std::collections::{BTreeMap, BTreeSet};

use crate::Config;

/// Real `environ_filter` (`special_env_vars.py:257-348`): keys never
/// exported to the ebuild environment. Transcribed verbatim from the
/// vendored 3.0.82.2 checkout; refresh when the vendored portage moves.
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

/// Real `env_blacklist` (`special_env_vars.py:15-74`): keys that never
/// enter the config from the calling environment or config files --
/// they are set programmatically by portage (here: by portuale's phase
/// runner and the per-entry threading). Applied to both halves of the
/// export set; the ones this module itself computes (`USE`,
/// `IUSE_EFFECTIVE`) are inserted after the gate on purpose.
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

/// Keys portuale's own phase runner computes and must always win --
/// `ebuild_phases::phase_env_vars` / `run_commands_async` (the
/// `doebuild_environment()` dynamic set, `doebuild.py:381-545`) and the
/// per-entry threading of `emerge_build::entry_build_env`. Excluded from
/// both halves of the export set so a same-named `make.conf` scalar or
/// calling-env variable (`HOME`, `PATH`, `DISTDIR`, `PORTAGE_TMPDIR`, …)
/// can never clobber the computed value. Real reaches the same result
/// by having `doebuild_environment()` assign these *after* the config
/// is built (its own values then sit in the highest layer).
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
    // per-entry threading (`entry_build_env`, #37 S2)
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

/// The per-package inputs `phase_environ` needs on top of the run-wide
/// config: the candidate's declared `IUSE` (raw tokens, `+`/`-`
/// defaults allowed) and its enabled flag set (real `Package.use.
/// enabled` -- `portage_repo::effective_use_flags`'s result).
#[derive(Debug, Clone, Copy)]
pub struct PhaseUse<'a> {
    pub iuse: &'a [String],
    pub enabled: &'a [String],
}

/// Real `PORTAGE_USE` (`config.py:2261`, "filtered by IUSE and implicit
/// IUSE"): `enabled ∩ (IUSE ∪ IUSE_EFFECTIVE)`, sorted and deduplicated.
/// This is the value real exports as `USE` (`config.py:3329`) **and**
/// writes to a binpkg's `metadata/USE` / the `Packages` index (S0
/// oracle: `abi_x86_64 amd64 elibc_glibc kernel_linux` for an empty-IUSE
/// package -- the implicit profile flags, nothing the package doesn't
/// declare or inherit implicitly). G3 (2026-09-13): the same value feeds
/// both consumers.
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

/// Real `IUSE_EFFECTIVE` for one package (`config.py::
/// _calc_iuse_effective`): the profile-level implicit set plus the
/// package's own declared `IUSE` names, sorted.
fn iuse_effective_for(config: &Config, iuse: &[String]) -> Vec<String> {
    iuse.iter()
        .map(|f| iuse_bare(f).to_string())
        .chain(config.iuse_effective.iter().cloned())
        .collect::<BTreeSet<String>>()
        .into_iter()
        .collect()
}

fn iuse_bare(flag: &str) -> &str {
    flag.trim_start_matches(['+', '-'])
}

/// A key bash can `export`: `[A-Za-z_][A-Za-z0-9_]*`. Real's env layer
/// carries exported-function entries (`BASH_FUNC_x%%`) and the like; they
/// cannot be re-exported as plain variables and are dropped here.
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

/// Real `config.environ()` for a build phase, from the resolved config
/// (see the module doc comment for every rule and its citation).
///
/// Returns `(name, value)` pairs in deterministic (sorted-by-name)
/// order. Set-but-empty values are exported as empty (G2: real's
/// `PORTAGE_COMPRESS=""` is the documented "disable compression",
/// `bin/ecompress:254`); keys that are unset everywhere are simply
/// absent. Precedence within the set: profile/`make.globals`/`make.conf`
/// scalars first (`Config::other_vars`), then the calling environment on
/// top (real's `env` layer is highest), then the folded incrementals and
/// the per-package values, which always win.
///
/// `pkg` is `None` for a run without a resolved candidate (a standalone
/// `ebuild <file> <phase>`): no `USE`/`IUSE_EFFECTIVE` entry is produced
/// (the caller's own base `USE` stands) and every `USE_EXPAND` variable
/// is exported as its profile scalar if set, else `""`.
pub fn phase_environ(config: &Config, pkg: Option<PhaseUse<'_>>) -> Vec<(String, String)> {
    let mut env: BTreeMap<String, String> = BTreeMap::new();

    // 1. Resolved config scalars: make.globals + profile chain +
    //    make.conf (+ the ENV_*_VARS the config already folded in).
    for (k, v) in &config.other_vars {
        if exportable(k) {
            env.insert(k.clone(), v.clone());
        }
    }

    // 2. The calling environment, wholesale (`config.py:551,567`) --
    //    highest layer, so it overrides step 1 for the same key.
    for (k, v) in crate::config_env_all() {
        if exportable(&k) {
            env.insert(k, v);
        }
    }

    // 3. Incrementals, folded and sorted like real `regenerate()`.
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
    if let Some(unset) = config.resolved_incremental("ENV_UNSET") {
        env.insert("ENV_UNSET".to_string(), unset.join(" "));
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

    // 4. Per-package rows. With a package, its derived values overwrite
    //    anything step 1/2/3 put in (`VIDEO_CARDS=""` even though the
    //    profile sets `nvidia` -- the package doesn't declare it). With
    //    no package there is no derivation, so the placeholders only
    //    fill *gaps*: a profile scalar (`VIDEO_CARDS="nvidia"`, `ARCH`)
    //    must survive (`or_insert`, not `insert`).
    for (key, value) in phase_environ_pkg(config, pkg) {
        if pkg.is_some() {
            env.insert(key, value);
        } else {
            env.entry(key).or_insert(value);
        }
    }

    env.into_iter().collect()
}

/// The **per-package** rows of `phase_environ`, split out so the
/// per-entry build threading (`emerge_build::entry_build_env`) can add
/// them on top of the run-wide `phase_environ(config, None)` result
/// without recomputing the ~160-key base for every entry.
///
/// `Some(pkg)`: `USE` (real `PORTAGE_USE`), `IUSE_EFFECTIVE` (the
/// package's declared IUSE ∪ the profile implicit set), and every
/// `USE_EXPAND` variable's value derived from the effective USE
/// (`config.py:2218-2252`), empty when the flag is off. `None`: no
/// `USE`/`IUSE_EFFECTIVE` (there is no resolved candidate; the caller's
/// own base stands) and every `USE_EXPAND` variable gets an empty
/// placeholder -- unprefixed names (`ARCH`) keep their profile scalar.
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
    use crate::{TEST_ENV_OVERRIDE, resolve_config};
    use std::collections::HashMap;
    use std::fs;
    use std::path::{Path, PathBuf};

    /// Same thread-local override `lib.rs`'s `with_test_env` uses: the
    /// map is the *entire* process env for this thread (so
    /// `config_env_all` returns exactly it), never touching `std::env`.
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

    /// A synthetic amd64-shaped profile under a temp `config_root`
    /// (which therefore has **no** `make.globals` -- everything real
    /// would get from there is seeded in `make.conf` explicitly, the
    /// #37 plan's trap §7.5). Mirrors the real `arch/amd64/make.defaults`
    /// + `base/make.defaults` lines the S0 oracle env came from.
    fn synthetic_root(name: &str, make_conf: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("portage-profile-phase-environ-{name}"));
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
        resolve_config(root, repo, &[], &[], "testrepo", &HashMap::new(), root)
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
            // Empty IUSE (the S0 `porttest/emptydirs` shape): only the
            // implicit flags survive -- `baseflag` is enabled globally
            // but undeclared, so real hides it from `use()`.
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

            // Sorted-by-name, no duplicate keys.
            let keys: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
            let mut sorted = keys.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(keys, sorted);

            // The S0 oracle rows: arch/multilib family, USE_EXPAND
            // machinery, implicit IUSE, COMMON_FLAGS, MULTILIB vars.
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

            // FEATURES: the incremental fold (make.conf `-sandbox` removes,
            // `splitdebug` adds), sorted, and mirrored as PORTAGE_FEATURES.
            let features = "assume-digests binpkg-docompress binpkg-dostrip splitdebug";
            assert_eq!(get(&env, "FEATURES"), Some(features));
            assert_eq!(get(&env, "PORTAGE_FEATURES"), Some(features));

            // Plain make.conf scalars pass through untouched.
            assert_eq!(get(&env, "PORTAGE_COMPRESS"), Some("bzip2"));
            assert_eq!(get(&env, "SOURCE_DATE_EPOCH"), Some("1740000000"));
            assert_eq!(get(&env, "MAKEOPTS"), Some("-j1"));

            // USE is PORTAGE_USE (G3); IUSE_EFFECTIVE is the implicit set
            // for an empty IUSE; PORTAGE_USE itself is environ_filter'ed.
            assert_eq!(
                get(&env, "USE"),
                Some("abi_x86_64 amd64 elibc_glibc kernel_linux")
            );
            assert_eq!(
                get(&env, "IUSE_EFFECTIVE"),
                Some("abi_x86_64 amd64 elibc_glibc elibc_musl kernel_linux prefix x86")
            );
            assert_eq!(get(&env, "PORTAGE_USE"), None);

            // USE_EXPAND values derive from the package's USE, not from
            // the profile scalar: ABI_X86="64" (abi_x86_64 enabled),
            // VIDEO_CARDS="" even though the profile says nvidia (the
            // package doesn't declare video_cards_nvidia), L10N="" as a
            // placeholder. ARCH (unprefixed) keeps its scalar.
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
                // calling env: harness-style vars real would carry
                ("L2_JOBS", "1"),
                ("container", "podman"),
                ("TZ", "UTC"),
                // blacklisted / computed / filtered: must never reach the set
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
                // a plain env scalar overriding make.conf (env layer is highest)
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

                // Calling env passes through (real imports all of os.environ).
                assert_eq!(get(&env, "L2_JOBS"), Some("1"));
                assert_eq!(get(&env, "container"), Some("podman"));
                assert_eq!(get(&env, "TZ"), Some("UTC"));
                assert_eq!(get(&env, "MAKEOPTS"), Some("-j7"));

                // The env's `USE`/`FEATURES` never leak verbatim: FEATURES
                // is the incremental fold (the env token `evil` *stacks* on
                // the profile list, exactly like real), USE is absent for
                // `pkg == None`.
                assert_eq!(
                    get(&env, "FEATURES"),
                    Some("assume-digests binpkg-docompress binpkg-dostrip evil sandbox")
                );
                assert_eq!(get(&env, "USE"), None);

                // G2: set-but-empty is exported as empty.
                assert_eq!(get(&env, "PORTAGE_COMPRESS"), Some(""));
                // Unset-everywhere keys are simply absent.
                assert_eq!(get(&env, "PORTAGE_COMPRESS_FLAGS"), None);

                // `pkg == None`: USE_EXPAND placeholders exist (profile scalar
                // kept when set, "" otherwise); unprefixed ARCH is a scalar.
                assert_eq!(get(&env, "VIDEO_CARDS"), Some("nvidia"));
                assert_eq!(get(&env, "L10N"), Some(""));
                assert_eq!(get(&env, "ABI_X86"), Some(""));
                assert_eq!(get(&env, "ARCH"), Some("amd64"));
            },
        );
    }

    #[test]
    fn the_three_key_sets_are_transcribed_consistently() {
        // Every entry is a valid identifier (except real's literal `_`),
        // and nothing this module computes is accidentally also filtered
        // out of its own output.
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
        // `PORTAGE_USE` and `O` are the S0 corrections: filtered by real.
        assert!(ENVIRON_FILTER.contains(&"PORTAGE_USE"));
        assert!(ENVIRON_FILTER.contains(&"O"));
        assert!(ENV_BLACKLIST.contains(&"AA"));
    }
}
