//! Config and repo loading, EMERGE_DEFAULT_OPTS, scheduling policy.

use crate::pretend::*;

/// Which running root, if any, portuale resolves a package's `BDEPEND`/
/// `IDEPEND` (and, for a native build, `DEPEND` -- `ESYSROOT` collapses
/// to `BROOT` when nothing is cross-compiling) against instead of the
/// target `ROOT`.
///
/// Real EAPI-7+ portage does this **unconditionally** (`depgraph.py`'s
/// own `depend_root` selection): those keys always target the running
/// root `/`. It is only *observable* for a genuine cross-root build --
/// `target_root` != `running_root`, i.e. populating a stage3 / chroot --
/// and a strict no-op when the two coincide, which is every ordinary
/// `ROOT=/` invocation. `--root-deps` (bare / `=True`) forces the same
/// running-root resolution on top; it stays the only trigger when `ROOT`
/// *is* the running root (its real debugging purpose), and folds these
/// keys into `RDEPEND` too (approximated here as the same running-root
/// walk -- see `portage_repo::root_deps_satisfied_atoms`). `=rdeps` is a
/// complete no-op at EAPI 7+ and needs nothing here.
pub(crate) fn resolve_root_deps_running_root(
    root_deps: bool,
    target_root: &Path,
    running_root: std::path::PathBuf,
) -> Option<std::path::PathBuf> {
    (root_deps || running_root != target_root).then_some(running_root)
}

/// Real `_emerge/unmerge.py::unmerge`'s own removal loop, reached when
/// `emerge -C`/`--unmerge <atoms>` runs **without** `--pretend`: after
/// `_unmerge_display` (the preview above) returns, each `selected`
/// package is really removed -- `>>> Unmerging (N of M) <cpv>...`, then
/// `dblink.unmerge()` (`pkg_prerm` from that version's own vdb-saved
/// env -> delete its files -> `pkg_postrm`) + `dblink.delete()` (drop
/// the vdb dir), via `ebuild_merge::unmerge_one_installed`. On success
/// each removed package is deselected from the world file (real
/// `WorldSelectedPackagesSet.cleanPackage`, called per-package right
/// after its removal).
///
/// The caller (`run_unmerge_pretend`) already handled `--ask` (exit 130
/// on decline) and the `CLEAN_DELAY` countdown before reaching here;
/// `FEATURES=unmerge-backup` is honored inside `execute_unmerge` (a
/// `quickpkg` of each package first). The trailing
/// `for s in setconfig.active: selected.remove(@s)` pass is a no-op here
/// (portuale's world writer already drops `@set` lines on any rewrite). A `pkg_prerm`/`pkg_postrm` failure is logged
/// and removal continues (`unmerge_one_installed`); real `unmerge()`
/// `sys.exit`s on a `portage.unmerge()` non-zero, but that return only
/// tracks the file-removal core, which portuale still surfaces as a
/// hard `Err`.
/// Real `BINPKG_COMPRESS`/`BINPKG_COMPRESS_FLAGS[_<NAME>]`/
/// `PORTAGE_BZIP2_COMMAND`/`PKGDIR`/`BINPKG_FORMAT` resolution, via the
/// full resolved chain -- calling env over `make.conf`/profile/
/// `make.globals` (real `config` precedence, via
/// `portage_profile::env_over_config_scalar` over the caller's resolved
/// `config`) -- falling back to real `make.globals`'s own defaults.
/// Shared by the
/// non-`--pretend` build/merge dispatch and `execute_unmerge`'s own
/// `FEATURES=unmerge-backup` `quickpkg`.
/// The one exception is the gpkg compression pair
/// (`BINPKG_COMPRESS`/`BINPKG_COMPRESS_FLAGS[_<NAME>]`, backlog #180's
/// Q6 rule, pmtest `bf0692f`): real's `environ_filter`
/// (`_config/special_env_vars.py:280-281`) keeps a calling-environment value
/// out of the phase env, and `bin/gpkg-helper.py` rebuilds
/// `portage.settings` from that filtered env plus the config files, so
/// for `gpkg` runs only a config-file value reaches the compressor --
/// the lookup there is files-only (`config.other_vars`), never the
/// calling env. `PORTAGE_BZIP2_COMMAND` keeps the full chain for both
/// formats (whitelisted, never filtered), as does `PKGDIR`.
/// The resolved-`FEATURES` sibling of `feature_enabled` for this
/// function: `Some` means the caller resolved a real config (#37 S2/S3),
/// so the token is read from the folded list; `None` keeps the raw
/// process-env fallback. The `config` parameter is independent of it:
/// `None` keeps the env-only fallback for every variable, reached only
/// when a caller could resolve no chain (today only `execute_unmerge`
/// when `resolve_unmerge_config` fails) -- with no file chain there is
/// no `make.conf` for the env to lose to, so the calling env stands,
/// even for the gpkg pair; every production caller otherwise passes its
/// resolved config, including `execute_unmerge`.
pub(crate) fn package_options_from_env(
    shell: ebuild_phases::ShellBackend,
    debug: bool,
    resolved_features: Option<&str>,
    config: Option<&portage_profile::Config>,
) -> ebuild_package::PackageOptions {
    let feature_on = |token: &str| match resolved_features {
        Some(f) => f.split_whitespace().any(|t| t == token),
        None => feature_enabled(token),
    };
    let d = ebuild_package::PackageOptions::default();
    // The two chain lookups (backlog #173's mechanism, reused for
    // #180): the full calling-env-over-files chain, and the files-only
    // chain for the gpkg compression pair (the Q6 rule above).
    let chain = |key: &str| match config {
        Some(c) => portage_profile::env_over_config_scalar(c, key),
        // Config-less arm (only `execute_unmerge`'s
        // `FEATURES=unmerge-backup` quickpkg reaches here -- and only
        // when the chain resolution there failed; otherwise it passes
        // its resolved config above): calling env only, through the
        // same `config_env_var` test hook, else real `make.globals`'s
        // own default.
        None => portage_profile::config_env_var(key),
    };
    let files = |key: &str| match config {
        Some(c) => c.other_vars.get(key).cloned(),
        // No chain, no files: nothing resolves (the caller falls back
        // to the real default inside each resolver).
        None => None,
    };
    let binpkg_format = ebuild_package::resolve_binpkg_format(chain);
    // The Q6 split: `xpak` compresses through the full chain (real
    // `doebuild.py:697` reads the package's own `mysettings`, env
    // layer highest); `gpkg` compresses through the files only (real
    // `bin/gpkg-helper.py:49` rebuilds its settings from the filtered
    // env plus the config files). In the config-less `None` arm both
    // sides are env-only (see the `chain` comment above).
    let (binpkg_compress, binpkg_compress_flags) = if binpkg_format == "gpkg" && config.is_some() {
        let compress = ebuild_package::resolve_binpkg_compress(files);
        let flags = ebuild_package::resolve_binpkg_compress_flags(files, &compress);
        (compress, flags)
    } else {
        let compress = ebuild_package::resolve_binpkg_compress(chain);
        let flags = ebuild_package::resolve_binpkg_compress_flags(chain, &compress);
        (compress, flags)
    };
    ebuild_package::PackageOptions {
        debug,
        pkgdir: ebuild_package::resolve_pkgdir(chain),
        distdir: std::env::var_os("DISTDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| d.distdir.clone()),
        shell,
        binpkg_compress,
        binpkg_compress_flags,
        portage_bzip2_command: ebuild_package::resolve_portage_bzip2_command(chain),
        binpkg_format,
        config_root: portage_repo::config_root_from_env(),
        // Real default-on FEATURES token: enabled unless explicitly
        // negated (`-buildpkg-live`) -- unlike the sibling fields above,
        // this doesn't fall back to `d.buildpkg_live` on an *unset*
        // `FEATURES` var, since a default-on flag mustn't flip off just
        // because `FEATURES` happens to be set to something else that
        // doesn't mention it (`ebuild_package::PackageOptions::
        // buildpkg_live`'s own doc comment has the full real grounding).
        // Resolved (production): the token's presence is the answer
        // (`-buildpkg-live` was already folded away). Raw fallback: the
        // old default-on-unless-negated read.
        buildpkg_live: match resolved_features {
            Some(f) => f.split_whitespace().any(|t| t == "buildpkg-live"),
            None => !feature_enabled("-buildpkg-live"),
        },
        // Real default is on, but this defaults off here (a deliberate
        // conservatism -- see `ebuild_package::PackageOptions::
        // binpkg_multi_instance`'s own doc comment). Explicit opt-in;
        // honoured by both `package_after_install` and
        // `quickpkg_from_vdb` (the `FEATURES=unmerge-backup` path).
        binpkg_multi_instance: feature_on("binpkg-multi-instance"),
        binpkg_signing: feature_on("binpkg-signing"),
        binpkg_gpg_signing_base_command: std::env::var("BINPKG_GPG_SIGNING_BASE_COMMAND")
            .unwrap_or_default(),
        binpkg_gpg_signing_digest: std::env::var("BINPKG_GPG_SIGNING_DIGEST").unwrap_or_default(),
        binpkg_gpg_signing_gpg_home: std::env::var("BINPKG_GPG_SIGNING_GPG_HOME")
            .unwrap_or_default(),
        binpkg_gpg_signing_key: std::env::var("BINPKG_GPG_SIGNING_KEY").unwrap_or_default(),
    }
}

/// Minimal POSIX-shell word splitting for `PORTAGE_IONICE_COMMAND` (real
/// `shlex.split`): whitespace-separated, with `'...'` / `"..."` quoting
/// and backslash escaping. Enough for the `ionice -c N -p ${PID}` shapes
/// this variable ever carries; an unterminated quote just consumes the
/// rest of the string.
pub(crate) fn shell_split(s: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut has_word = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if has_word {
                    words.push(std::mem::take(&mut cur));
                    has_word = false;
                }
            }
            '\'' => {
                has_word = true;
                for q in chars.by_ref() {
                    if q == '\'' {
                        break;
                    }
                    cur.push(q);
                }
            }
            '"' => {
                has_word = true;
                while let Some(q) = chars.next() {
                    match q {
                        '"' => break,
                        '\\' => match chars.peek() {
                            Some(&n) if matches!(n, '"' | '\\' | '$' | '`') => {
                                cur.push(n);
                                chars.next();
                            }
                            _ => cur.push('\\'),
                        },
                        _ => cur.push(q),
                    }
                }
            }
            '\\' => {
                has_word = true;
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            _ => {
                has_word = true;
                cur.push(c);
            }
        }
    }
    if has_word {
        words.push(cur);
    }
    words
}

/// Real `_emerge/main.py`'s second `parse_opts` pass (`:1352-1360`):
/// `shlex.split(settings["EMERGE_DEFAULT_OPTS"])` is **prepended** to
/// argv, so a command-line option always wins over the same option in the
/// variable. The value is read from the resolved config
/// (`config.other_vars`, where `portage-profile`'s `ENV_SCALAR_VARS`
/// stores it -- process env over `make.conf`, including `source`d
/// files), matching real's `emerge_config.target_config.settings.get(...)`.
/// An unset variable yields an empty prefix (the input args unchanged,
/// same allocation-wise shape as before this existed).
pub(crate) fn args_with_emerge_defaults(
    config: &portage_profile::Config,
    args: &[String],
) -> Vec<String> {
    // Real `main.py:1352-1353`: `--ignore-default-opts` comes from the
    // command line (the first parse pass, before defaults exist), and
    // skips the whole variable.
    if args.iter().any(|arg| arg == "--ignore-default-opts") {
        return args.to_vec();
    }
    let mut out = config
        .other_vars
        .get("EMERGE_DEFAULT_OPTS")
        .map(|value| shell_split(value))
        .unwrap_or_default();
    out.extend(args.iter().cloned());
    out
}

/// Real `_emerge/actions.py::apply_priorities` (via `run_action`, before
/// any action): applies `PORTAGE_NICENESS` (`renice -n <n> <pid>`),
/// `PORTAGE_IONICE_COMMAND` (`shlex.split`, `${PID}`-expanded, spawned)
/// and `PORTAGE_SCHEDULING_POLICY` / `PORTAGE_SCHEDULING_PRIORITY` (real
/// `set_scheduling_policy` -> `os.sched_setscheduler`) to this process,
/// so every build/merge subprocess it later spawns inherits them. A
/// missing `renice` prints the real "renice command was not found" line
/// and gives up; a non-zero exit prints an eerror-style line and
/// continues. `PORTAGE_IONICE_COMMAND` failing because the tool is absent
/// is silent (real: "kernel probably doesn't support ionice"). Called
/// once, unconditionally -- harmless under `--pretend`.
///
/// Investigated and confirmed a non-issue, not a gap: real also applies
/// this to a Python `multiprocessing` "forkserver" pid when
/// `multiprocessing.get_start_method() == "forkserver"`
/// (`actions.py:3250-3271`) -- a long-lived daemon process real
/// portage's own `Scheduler` forks *worker* processes from under that
/// specific start method, so the forkserver's own policy (not just the
/// main process's) determines what those forked workers inherit.
/// Portuale has no equivalent at all: `emerge_build::run_build_
/// scheduler`'s own `-jN` parallelism is OS *threads*
/// (`std::thread::scope`), not forked worker processes, and every real
/// build subprocess (`ebuild_phases::spawn_trackable`) is spawned
/// directly from whichever thread needs it -- main or a scheduler
/// worker -- via a plain `fork`+`exec`, never through a persistent
/// forker. `sched_setscheduler` applied here, on the main thread,
/// before any worker thread is ever created (this runs once, at the
/// very top of `run()`, well before `run_build_scheduler`'s own
/// `std::thread::scope` call), is therefore already sufficient: POSIX
/// thread creation inherits the creating thread's own scheduling policy
/// by default (`PTHREAD_INHERIT_SCHED`), confirmed empirically on Linux
/// (a `std::thread::spawn`ed thread reports the same `sched_getscheduler`
/// value the main thread set moments earlier) -- so every scheduler
/// worker thread, and every subprocess it in turn spawns, already gets
/// the same policy this call establishes, with nothing further to do.
/// No Python-reference mirror needed either way: a process/thread
/// concern, not a `--pretend` resolution one.
pub(crate) fn apply_portage_scheduling_policy() {
    let pid = std::process::id().to_string();
    apply_scheduling_policy();
    if let Ok(niceness) = std::env::var("PORTAGE_NICENESS") {
        let niceness = niceness.trim();
        if !niceness.is_empty() {
            match std::process::Command::new("renice")
                .args(["-n", niceness, &pid])
                .stdout(std::process::Stdio::null())
                .status()
            {
                Err(_) => eprintln!(
                    " * PORTAGE_NICENESS not applied because the renice command was not found"
                ),
                Ok(s) if !s.success() => {
                    eprintln!(
                        " * renice command returned {} for main pid {pid}",
                        s.code().unwrap_or(-1)
                    );
                }
                Ok(_) => {}
            }
        }
    }
    if let Ok(cmd) = std::env::var("PORTAGE_IONICE_COMMAND") {
        let parts: Vec<String> = shell_split(&cmd)
            .into_iter()
            .map(|p| p.replace("${PID}", &pid).replace("$PID", &pid))
            .collect();
        if let Some((prog, args)) = parts.split_first() {
            match std::process::Command::new(prog).args(args).status() {
                Err(_) => {} // the kernel probably doesn't support ionice
                Ok(s) if !s.success() => {
                    eprintln!(
                        " * PORTAGE_IONICE_COMMAND returned {} for main pid {pid}",
                        s.code().unwrap_or(-1)
                    );
                    eprintln!(
                        " * See the make.conf(5) man page for PORTAGE_IONICE_COMMAND usage \
                         instructions."
                    );
                }
                Ok(_) => {}
            }
        }
    }
}

/// Real `_emerge/actions.py::set_scheduling_policy`: map
/// `PORTAGE_SCHEDULING_POLICY` to a `SCHED_*` constant (`linux/sched.h`
/// IDs), resolve `PORTAGE_SCHEDULING_PRIORITY` (default
/// `sched_get_priority_min(policy)`), and `sched_setscheduler(2)` this
/// process. An unknown policy name / out-of-range priority prints the
/// real eerror pair and returns; a failing syscall (e.g. EPERM for a
/// real-time policy without `CAP_SYS_NICE`) prints the real
/// "Unable to apply" line. Unset / empty is a no-op.
pub(crate) fn apply_scheduling_policy() {
    let Ok(policy_name) = std::env::var("PORTAGE_SCHEDULING_POLICY") else {
        return;
    };
    let policy_name = policy_name.trim();
    if policy_name.is_empty() {
        return;
    }
    // IDs sourced from linux/sched.h (real `set_scheduling_policy`).
    let policy: libc::c_int = match policy_name {
        "other" => 0,
        "fifo" => 1,
        "round-robin" => 2,
        "batch" => 3,
        "idle" => 5,
        "deadline" => 6,
        _ => {
            eprintln!(" * Invalid policy in PORTAGE_SCHEDULING_POLICY.");
            eprintln!(
                " * See the make.conf(5) man page for PORTAGE_SCHEDULING_POLICY usage instructions."
            );
            return;
        }
    };
    // SAFETY: both take a plain `int` policy and have no memory effects.
    let min_prio = unsafe { libc::sched_get_priority_min(policy) };
    let max_prio = unsafe { libc::sched_get_priority_max(policy) };
    let priority = match std::env::var("PORTAGE_SCHEDULING_PRIORITY") {
        Ok(p) if !p.trim().is_empty() => match p.trim().parse::<libc::c_int>() {
            Ok(n) if n >= min_prio && n <= max_prio => n,
            _ => {
                eprintln!(" * Invalid priority in PORTAGE_SCHEDULING_PRIORITY.");
                eprintln!(
                    " * See the make.conf(5) man page for PORTAGE_SCHEDULING_PRIORITY usage instructions."
                );
                return;
            }
        },
        _ => min_prio.max(0),
    };
    // Zero-init, then set the one field real's `os.sched_param`
    // conversion sets: CPython builds `struct sched_param` as a C11
    // compound literal with only `sched_priority` assigned, so every
    // other member is zero -- including musl's four SCHED_DEADLINE
    // fields, which the libc crate's glibc definition does not carry
    // (a plain struct literal naming only `sched_priority` fails to
    // compile for musl, backlog #61).
    // SAFETY: all-zeroes is a valid `sched_param` value; `sched_priority`
    // is overwritten immediately, and `param` outlives the call below.
    let mut param: libc::sched_param = unsafe { std::mem::zeroed() };
    param.sched_priority = priority;
    let pid = std::process::id() as libc::pid_t;
    // SAFETY: `param` outlives the call; `pid` / `policy` are plain ints.
    let rc = unsafe { libc::sched_setscheduler(pid, policy, &param) };
    if rc != 0 {
        eprintln!(
            " * Unable to apply PORTAGE_SCHEDULING_POLICY to main pid {pid}: {}",
            std::io::Error::last_os_error()
        );
    }
}

/// The run-wide half of the build-phase flag env: real `preinst_mask()` (`bin/misc-functions.sh`) driven by the
/// resolved config rather than the process env: `make.conf`/profile
/// `INSTALL_MASK` folded with the `no{man,info,doc}` `FEATURES` tokens.
/// `MergeOptions::from_env` reads the same two from the environment as
/// its `ebuild <file> merge` fallback; the `emerge <atom>` production
/// paths call this to override both fields with the real config values.
///
/// `INSTALL_MASK` is in real portage's `environ_whitelist` and is
/// non-incremental, so the `env` configdict (highest-priority layer)
/// overrides `make.conf`'s value when it is set in the calling
/// environment -- an `emerge INSTALL_MASK="…" -k …` invocation. A
/// non-empty process-env value therefore wins here; otherwise the
/// resolved config value stands. (An explicit empty `INSTALL_MASK=""`
/// blanking the config value is a corner real handles but we do not --
/// unset and empty behave the same, matching `MergeOptions::from_env`.)
/// Real `create_depgraph_params.py`'s own `if "buildpkg" in
/// settings.features: myparams.setdefault("--buildpkg", "y")`: an
/// explicit `--buildpkg[=n]` wins, otherwise the **resolved** `FEATURES`
/// list decides (make.conf/`make.globals`, not the calling env -- #37
/// S3). Split out of the CLI dispatcher so the precedence is directly
/// testable.
pub(crate) fn buildpkg_from_config(
    buildpkg_opt: Option<bool>,
    config: &portage_profile::Config,
) -> bool {
    buildpkg_opt.unwrap_or_else(|| config_features_list(config).iter().any(|t| t == "buildpkg"))
}

/// The resolved `FEATURES` incremental list, if it was assigned
/// anywhere: the folded `make.globals` + profile + `make.conf` +
/// calling-env stack ([`Config::resolved_incremental`], with real's
/// own `-*` / `-tok` / `+tok` semantics), falling back to a raw
/// whitespace split of `other_vars["FEATURES"]` for configs that carry
/// `FEATURES` only there. `None` means nothing configured anywhere --
/// callers that need real's host-`make.globals` default seed it
/// themselves (see `pkgdir_index_trusted`).
pub(crate) fn config_features_opt(config: &portage_profile::Config) -> Option<Vec<String>> {
    config.resolved_incremental("FEATURES").or_else(|| {
        config
            .other_vars
            .get("FEATURES")
            .map(|f| f.split_whitespace().map(String::from).collect())
    })
}

pub(crate) fn config_features_list(config: &portage_profile::Config) -> Vec<String> {
    config_features_opt(config).unwrap_or_default()
}

/// Real `bintree.populate`'s own `reindex = "pkgdir-index-trusted" not
/// in self.settings.features or force_reindex` (`bintree.py:936-938`,
/// backlog #199) for the local `$PKGDIR` scan: `true` means the scan
/// trusts the `Packages` index (real `_populate_local(reindex=False)`),
/// `false` means it re-stats and re-reads (`reindex=True`).
///
/// Read from the same resolved chain every other resolver-side
/// `FEATURES` read uses (`config_features_opt`, the folded
/// `make.globals` + profile + `make.conf` + calling-env stack -- so a
/// `FEATURES=-pkgdir-index-trusted` in `make.conf` or on the calling
/// env reaches the scan; no new env read here), with the same
/// `other_vars` fallback `config_features_list` uses. The fold starts from
/// real's own default: the token is in real `make.globals`' own default
/// `FEATURES` (`cnf/make.globals:81`), which real always sources from
/// the host installation -- while portuale reads `make.globals`
/// `config_root`-relative, so under a test/fixture root it contributes
/// nothing. `None` (nothing configured anywhere) therefore stays
/// trusted (byte-identical to the pre-#199 scan, and what real computes
/// on a live host); only a folded list that lacks the token --
/// an explicit `-pkgdir-index-trusted` (or `-*`, real
/// `resolved_incremental`'s own clear) not undone by a later re-add --
/// distrusts, exactly when real's folded `features` would lack it.
pub(crate) fn pkgdir_index_trusted(config: &portage_profile::Config) -> bool {
    config_features_opt(config)
        .is_none_or(|features| features.iter().any(|t| t == "pkgdir-index-trusted"))
}

/// The resolved, merge-time `FEATURES` incremental list, space-joined --
/// for `MergeOptions::features` (`merge_binpkg`'s `PORTAGE_UPDATE_ENV`
/// vdb-environment regeneration). Shared with the remote-merge server
/// (`run_remote_plan`, backlog #171), which resolves the client's
/// placed config through this same function so a remote merge records
/// exactly what a local one would.
pub(crate) fn config_features_string(config: &portage_profile::Config) -> String {
    config_features_list(config).join(" ")
}

/// The resolved, merge-time `PORTAGE_BZIP2_COMMAND` -- real's
/// merge-time env save records its live value (`cnf/make.globals:105`
/// defaults it to `bzip2`; a client `make.conf` may set e.g.
/// `lbzip2`/`pbzip2`). Shared with the remote-merge server
/// (`run_remote_plan`, backlog #171), which resolves the client's
/// placed config through this same function so the scrubbed regen'd
/// env records exactly what a local merge would; falls back to the
/// `make.globals` default when nothing is configured.
pub(crate) fn config_bzip2_command(config: &portage_profile::Config) -> String {
    config
        .other_vars
        .get("PORTAGE_BZIP2_COMMAND")
        .cloned()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "bzip2".to_string())
}

/// The resolved, merge-time `INSTALL_MASK` plus the `no{man,info,doc}`
/// `FEATURES` fold -- real `preinst_mask()` (`bin/misc-functions.sh`).
/// Shared with the remote-merge server (`run_remote_plan`, backlog #170),
/// which resolves the client's placed config through this same function
/// so a remote merge masks exactly what a local one would.
pub(crate) fn config_install_mask(config: &portage_profile::Config) -> (String, bool) {
    let features = config_features_list(config);
    let configured = std::env::var("INSTALL_MASK")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| config.other_vars.get("INSTALL_MASK").cloned())
        .unwrap_or_default();
    crate::install_mask::resolve(&configured, &features)
}

/// Load the `repos.conf` repos + the fully-resolved config for one
/// config root -- `run()`'s own preamble, factored out so
/// `mrg --remote-binpkg` (which never runs a resolve) reads the *same*
/// client config the resolve path merges with (backlog #170/#171: the
/// `INSTALL_MASK` + `no{man,info,doc}` fold, the merge-time `FEATURES`,
/// and `PORTAGE_BZIP2_COMMAND` all come from the placed config).
/// `Err` is the bare display text; the caller prefixes it (`emerge:` /
/// `mrg: …`) exactly as the inline preamble did.
pub(crate) fn load_repos_and_config(
    config_root: &std::path::Path,
    root: &std::path::Path,
) -> Result<(Vec<portage_repo::RepoConfig>, portage_profile::Config), String> {
    let repos = portage_repo::find_repos(config_root).map_err(|e| e.to_string())?;
    let config = resolve_config_for_root(config_root, &repos, root)?;
    Ok((repos, config))
}

/// [`load_repos_and_config`] for an internal probe that runs before the
/// CLI's own load (`mrg::make_conf_lookup`, #318): the repos.conf
/// section-mismatch line stays the CLI's to print
/// (`portage_repo::find_repos_quiet`).
pub(crate) fn load_config_quiet(
    config_root: &std::path::Path,
    root: &std::path::Path,
) -> Result<portage_profile::Config, String> {
    let repos = portage_repo::find_repos_quiet(config_root).map_err(|e| e.to_string())?;
    resolve_config_for_root(config_root, &repos, root)
}

/// `load_repos_and_config`'s profile half over an already-loaded repo
/// list: resolve the `Config` for `root` (`eroot`) reading
/// `config_root`'s `/etc/portage` overrides, with the SHARED repo set
/// (both roots resolve the same trees — Track X Slice B/C; real's
/// running tree inherits `PORTAGE_REPOSITORIES` the same way,
/// `create_trees`' `clean_env`).
pub(crate) fn resolve_config_for_root(
    config_root: &std::path::Path,
    repos: &[portage_repo::RepoConfig],
    root: &std::path::Path,
) -> Result<portage_profile::Config, String> {
    let main_repo = repos
        .iter()
        .find(|r| r.is_main)
        .ok_or_else(|| "no main repo found in repos.conf".to_string())?;

    // Every non-main repo's own (name, location) -- portage-profile's
    // own package.mask/.unmask reading needs each overlay's own name to
    // scope its repo-level entries via "::name" (see resolve_config's
    // own doc comment); ascending-priority order, same as find_repos'
    // own order, which only matters if two overlays' own entries could
    // otherwise interfere, and the "::name" scoping already rules that
    // out regardless. The same list, plus the main repo's own name
    // below, also lets resolve_config follow a profile's own cross-repo
    // "parent" entries (reponame:path syntax).
    let overlay_repos: Vec<(String, std::path::PathBuf)> = repos
        .iter()
        .filter(|r| !r.is_main)
        .map(|r| (r.name.clone(), r.location.clone()))
        .collect();

    // Real `masters` (see `portage_repo::RepoConfig::masters`'s own doc
    // comment): each repo's own already-resolved masters chain, keyed by
    // name, for `resolve_config`'s own package.mask stacking.
    let repo_masters: std::collections::HashMap<String, Vec<std::path::PathBuf>> = repos
        .iter()
        .map(|r| (r.name.clone(), r.masters.clone()))
        .collect();

    // Every repo's own `aliases` (`repos.conf`/`layout.conf`), each
    // paired with that repo's location -- real
    // `repositories.get_location_for_name` resolves an aliased
    // `reponame:path` profile `parent` (see `resolve_config`'s own doc
    // comment).
    let repo_aliases: Vec<(String, std::path::PathBuf)> = repos
        .iter()
        .flat_map(|r| r.aliases.iter().map(|a| (a.clone(), r.location.clone())))
        .collect();

    let config = portage_profile::resolve_config(
        config_root,
        &main_repo.location,
        &overlay_repos,
        &repo_aliases,
        &main_repo.name,
        &repo_masters,
        root,
    )
    .map_err(|e| e.to_string())?;
    Ok(config)
}

/// Backlog #242 Slice D: the running root's own profile (`Config`) for
/// per-node USE paint (S0 map
/// `LLM/evidence/2026-10-02-242-display-sites.md` §2). `None` on a
/// single-root run (the running root coincides with the target — target
/// paint everywhere) and when the running config fails to load (a
/// hermetic test tree carries no `/etc/portage` — the target paint is
/// the documented fallback; real has no such tree at all).
pub(crate) fn load_running_config(
    target_root: &std::path::Path,
    running_root: &std::path::Path,
    repos: &[portage_repo::RepoConfig],
) -> Option<portage_profile::Config> {
    if running_root == target_root {
        return None;
    }
    resolve_config_for_root(running_root, repos, running_root).ok()
}
