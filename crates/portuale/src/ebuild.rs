// `ebuild <file> <command> [command...]`: CLI-surface recognition
// plus real execution for the phase chain (`pretend`/`setup`/`unpack`/
// `prepare`/`configure`/`compile`/`test`/`install`), `merge`/`qmerge`/
// `unmerge`/`package`/`config`/`info`/`prerm`/`postrm`. Every other
// Portage command is recognized but still a dry-run stub (`instprep`
// runs inside every `merge`/`qmerge`; only the standalone command is
// stubbed).
//
// Exit codes mirror Portage's `ebuild`: 2 for missing args, 1 for
// everything else invalid.
//
// Known deviation: `bin/ebuild` uses `parse_known_args`, which swallows
// an unrecognized flag into the positional leftovers (surfacing later
// as a confusing error); portuale rejects it immediately instead.
//
// `-h`/`--help` wins unconditionally, like argparse's own check during
// parsing. The help text is a portuale-specific summary, not a port of
// the argparse usage block; `--version` is deliberately not implemented.
// Portage: bin/ebuild, doebuild.py

use crate::ebuild_merge;
use crate::ebuild_options::{self, Kind};
use crate::ebuild_package;
use crate::ebuild_package::PackageOptions;
use crate::ebuild_phases;
use crate::ebuild_phases::ShellBackend;
use crate::ebuild_unmerge;
use crate::ebuild_unmerge::UnmergeOptions;
use std::path::Path;
use std::process::ExitCode;

/// Whether `--help`/`-h` appears anywhere in `args`. No short-flag
/// bundle scanning: `bin/ebuild` declares no short aliases for its own
/// options, so `-h` is never part of a bundle.
fn wants_help(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--help" || arg == "-h")
}

/// A short, portuale-specific summary (not a port of the argparse
/// usage block; see the module doc).
fn print_help() {
    println!("ebuild: command-line interface to the Portuale package manager");
    println!();
    println!("Usage:");
    println!("   ebuild <ebuild file> <command> [command ...]");
    println!("   ebuild --help");
    println!();
    println!("Commands run for real: the actionmap phase chain (unpack, prepare,");
    println!("configure, compile, test, install), the standalone phases (pretend,");
    println!("setup, ...), and merge / qmerge / unmerge / package / config / info /");
    println!("prerm / postrm. Any other Portage command (clean, digest, manifest, ...)");
    println!("is recognized and accepted but not yet implemented -- a no-op dry run.");
    println!();
    println!("Options:");
    println!("   --force              regenerate digests (with the digest/manifest commands)");
    println!("   --color y|n          enable or disable color output");
    println!(
        "   --debug              show debug output (as Portage: sets PORTAGE_DEBUG, so bin/ebuild.sh runs set -x)"
    );
    println!("   --ignore-default-opts  do not use the EBUILD_DEFAULT_OPTS environment variable");
    println!("   --skip-manifest      skip all manifest checks");
    println!(
        "   --shell bash|brush   real shell backend for phase/hook execution (default: bash; portuale extension)"
    );
    println!("   -h, --help           show this message and exit");
    println!();
    println!(
        "Every other Portage ebuild option is recognized by name (see bin/ebuild) but \
         not yet implemented -- using one reports which option it is, instead of a \
         generic error. --shell is a portuale extension, not a Portage bin/ebuild \
         option."
    );
    println!("See README.md and ebuild(1) for the full picture.");
}

pub fn run(args: &[String]) -> ExitCode {
    if wants_help(args) {
        print_help();
        return ExitCode::SUCCESS;
    }

    let mut ebuild_file: Option<&str> = None;
    let mut commands: Vec<&str> = Vec::new();
    // `--debug` sets `PORTAGE_DEBUG`, so `bin/ebuild.sh` runs `set -x`.
    // Every other boolean option is still a pure no-op.
    let mut debug = false;
    // `--shell bash|brush` (default `bash`): the shell backend for
    // phase/hook execution. A portuale-only flag, checked here rather
    // than through the options table (which transcribes `bin/ebuild`).
    let mut shell = ebuild_phases::ShellBackend::default();

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--shell" || arg.starts_with("--shell=") {
            let value = if let Some(v) = arg.strip_prefix("--shell=") {
                v
            } else if i + 1 < args.len() {
                i += 1;
                args[i].as_str()
            } else {
                eprintln!("ebuild: option '--shell' requires a value");
                return ExitCode::from(2);
            };
            shell = match value {
                "brush" => ebuild_phases::ShellBackend::Brush,
                "bash" => ebuild_phases::ShellBackend::Bash,
                other => {
                    eprintln!("ebuild: --shell: {other:?} is not \"bash\" or \"brush\"");
                    return ExitCode::from(1);
                }
            };
        } else if arg.starts_with('-') {
            match ebuild_options::lookup_option(arg) {
                Some(Kind::Value) => {
                    // `--opt=value` is inline; `--opt value` needs the
                    // next token skipped.
                    if !arg.contains('=') && i + 1 < args.len() {
                        i += 1;
                    }
                }
                Some(Kind::Boolean) => {
                    if arg == "--debug" {
                        debug = true;
                    }
                }
                None => {
                    eprintln!("ebuild: unrecognized option {arg:?}");
                    return ExitCode::from(1);
                }
            }
        } else if ebuild_file.is_none() {
            if !arg.ends_with(".ebuild") {
                eprintln!("ebuild: {arg:?}: does not end with \".ebuild\"");
                return ExitCode::from(1);
            }
            ebuild_file = Some(arg);
        } else {
            commands.push(arg);
        }
        i += 1;
    }

    let Some(ebuild_file) = ebuild_file else {
        eprintln!("ebuild: missing required args");
        return ExitCode::from(2);
    };
    if commands.is_empty() {
        eprintln!("ebuild: missing required args");
        return ExitCode::from(2);
    }

    for cmd in &commands {
        if !ebuild_options::is_valid_command(cmd) {
            eprintln!("ebuild: {cmd:?} is not one of the valid ebuild commands");
            return ExitCode::from(1);
        }
    }

    // Portage `bin/ebuild`: `test` in argv forces the test phase for the
    // whole invocation (`EBUILD_FORCE_TEST=1`, `features.add("test")`,
    // `Forcing test.` on stderr) — before any per-command refusal or
    // gate, so even a refused `test` still prints it. The marker travels
    // to the phase env through `build_env` (whitelisted there); the
    // resolved `FEATURES` gain the token in `phase_env_vars`.
    // Portage: bin/ebuild:296-302
    let force_test = commands.contains(&"test");
    if force_test {
        eprintln!("Forcing test.");
    }
    // Empty unless forcing: the only `build_env` the direct path threads.
    let force_test_env: Vec<(String, String)> = if force_test {
        vec![("EBUILD_FORCE_TEST".to_string(), "1".to_string())]
    } else {
        Vec::new()
    };

    // An ebuild whose EAPI Portage itself does not know refuses before
    // anything else runs — including the dry-run stubs: `doebuild_environment`
    // raises `UnsupportedAPIException` before any phase dispatch, and
    // `clean` is no exception. `unmerge` (and the other never-implemented
    // stubs) stay ungated: unmerging an installed package with a corrupt
    // EAPI must keep working, and there is no oracle for the rest.
    // Portage: doebuild.py:565-572, exception.py:228-230
    if let Some(refusal) = unsupported_eapi_refusal(ebuild_file, &commands) {
        eprintln!("{refusal}");
        return ExitCode::from(1);
    }

    // Real execution only when EVERY requested command is implemented
    // for real: no partial-real-execution ambiguity for mixed lists
    // (e.g. `compile clean`). A purely dry-run request keeps the stub.
    if commands.iter().all(|cmd| {
        ebuild_phases::is_phase_command(cmd)
            || ebuild_phases::is_standalone_phase_command(cmd)
            || ebuild_merge::is_merge_command(cmd)
            || ebuild_merge::is_qmerge_command(cmd)
            || ebuild_unmerge::is_unmerge_command(cmd)
            || ebuild_package::is_package_command(cmd)
    }) {
        return run_real_commands(&commands, ebuild_file, shell, debug, &force_test_env);
    }

    println!(
        "ebuild: dry run -- one or more of these commands is recognized but not \
         yet implemented in portuale"
    );
    println!("ebuild file: {ebuild_file:?}");
    println!("commands: {commands:?}");
    ExitCode::SUCCESS
}

/// Run the requested commands for real when every one of them is implemented.
pub(crate) fn run_real_commands(
    commands: &[&str],
    ebuild_file: &str,
    shell: ShellBackend,
    debug: bool,
    force_test_env: &[(String, String)],
) -> ExitCode {
    let root = portage_repo::root_from_env();
    let config_root = portage_repo::config_root_from_env();
    // One resolved `Config` per invocation: the tmpdir/distdir
    // defaults, the `FEATURES` gates and the binpkg scalars below all
    // read it instead of re-resolving per lookup. `None` outside any
    // repo checkout: every reader keeps its calling-env-only fallback.
    // Portage: config.py (settings); doebuild.py, doebuild_environment
    let canonical_pkg_dir = std::path::Path::new(ebuild_file)
        .canonicalize()
        .ok()
        .and_then(|p| p.parent().map(std::path::Path::to_path_buf));
    let standalone_config = canonical_pkg_dir
        .as_deref()
        .and_then(|pkg_dir| ebuild_phases::resolve_standalone_config(pkg_dir, &config_root, &root));
    // `PORTAGE_TMPDIR`/`DISTDIR` from the resolved config, calling
    // environment highest (Portage's top layer). Without a config the
    // old env-or-`make.globals`-default shortcut stands.
    // Portage: config.py; cnf/make.globals:30,35
    let portage_tmpdir_base = match &standalone_config {
        Some(config) => portage_repo::portage_tmpdir_from_env_or_config(config),
        None => portage_repo::portage_tmpdir_from_env(),
    };
    let distdir_base = match &standalone_config {
        Some(config) => portage_repo::distdir_from_env_or_config(config),
        None => std::env::var_os("DISTDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(portage_repo::DISTDIR_DEFAULT)),
    };
    // Per-package `PORTAGE_TMPDIR`: re-derive from the matched
    // `package.env` value when the calling environment is silent.
    // Skipped for `depend`/`clean`, which Portage exempts too.
    // A missing matched directory fails with Portage's message.
    // Portage: doebuild.py, _check_temp_dir
    let portage_tmpdir = if commands
        .iter()
        .any(|cmd| *cmd == "depend" || *cmd == "clean")
    {
        portage_tmpdir_base
    } else {
        let process_tmpdir = std::env::var_os("PORTAGE_TMPDIR").map(std::path::PathBuf::from);
        let ebuild_path = std::path::Path::new(ebuild_file);
        match ebuild_phases::resolve_standalone_portage_tmpdir(
            ebuild_path,
            &config_root,
            &root,
            &portage_tmpdir_base,
            process_tmpdir.as_deref(),
        ) {
            Ok(resolved) => resolved,
            Err(e) => {
                eprintln!("ebuild: {e}");
                return ExitCode::from(1);
            }
        }
    };
    let distdir = distdir_base;
    // Merge config like the `emerge` path's `merge_setup`: `from_env`
    // (shared, untouched), then the resolved config overrides — the
    // resolved `FEATURES` drive the gates (sandbox, protect-owned,
    // `unmerge-orphans`, `binpkg-*`), not the raw process value.
    // Portage: _emerge/actions.py; config.py (settings.features)
    let mut merge_options = ebuild_merge::MergeOptions::from_env(shell, debug);
    merge_options.distdir = distdir.clone();
    if let Some(config) = &standalone_config {
        merge_options.resolved_config = Some(std::sync::Arc::new(config.clone()));
        (
            merge_options.install_mask,
            merge_options.install_mask_prunes_usr_share,
        ) = crate::pretend::config_install_mask(config);
        merge_options.apply_uninstall_ignore(config);
        merge_options.set_resolved_features(&crate::pretend::config_features_string(config));
        merge_options.apply_config_protect(config);
        merge_options.package_env_vars = config.package_env_vars.clone();
    }
    // Per-package `FEATURES`: fold a matched `package.env` value
    // onto the run-wide list and re-derive the merge gates.
    if let Some(features) = ebuild_phases::resolve_standalone_resolved_features(
        std::path::Path::new(ebuild_file),
        &config_root,
        &root,
        &merge_options.features,
        &std::env::var("FEATURES").unwrap_or_default(),
    ) {
        merge_options.set_resolved_features(&features);
    }
    // `bin/ebuild` drops the scheduler-only tokens for the direct path
    // (the `emerge` Scheduler gates on `merge-wait`, so it keeps them).
    merge_options.features =
        ebuild_phases::discard_direct_ebuild_features(&merge_options.features.clone());
    // The direct `ebuild <file> merge|qmerge` path lists every merged entry
    // on stdout like Portage's foreground merge; the `emerge`-scheduler
    // paths keep `direct` off and stay silent.
    // Portage: dbapi/vartree.py, `dblink._display_merge`.
    merge_options.direct = true;
    // The resolved `FEATURES` ride into every phase through the build
    // env (like the merge path's run-wide env), so sandbox, isolation
    // and helper gating see the config value, not the raw process one.
    // `EBUILD_FORCE_TEST` still travels here too.
    let mut direct_build_env: Vec<(String, String)> = force_test_env.to_vec();
    direct_build_env.push(("FEATURES".to_string(), merge_options.features.clone()));
    direct_build_env.push((
        "PORTAGE_FEATURES".to_string(),
        merge_options.features.clone(),
    ));
    let UnmergeAndPackageOptionsOut {
        unmerge_options,
        package_options,
    } = unmerge_and_package_options(UnmergeAndPackageOptionsArgs {
        debug,
        shell,
        ebuild_file,
        root: &root,
        distdir: &distdir,
        standalone_config: standalone_config.as_ref(),
    });
    let ebuild_path = std::path::Path::new(ebuild_file);

    // `merge`/`qmerge`/`unmerge` write into a root-owned filesystem:
    // refuse unprivileged callers up front (Portage has no such
    // guard and just fails mid-phase). Non-root ownership of
    // `$ROOT` (a prefix/staging tree) is still allowed.
    if commands.iter().any(|&c| {
        ebuild_merge::is_merge_command(c)
            || ebuild_merge::is_qmerge_command(c)
            || ebuild_unmerge::is_unmerge_command(c)
    }) && !crate::privileges::is_privileged(&root)
    {
        crate::privileges::deny_superuser("ebuild");
        return ExitCode::from(1);
    }

    // Portage `doebuild()` validates the metadata (including the
    // REQUIRED_USE gate under `eapi_has_required_use`) for every
    // porttree command but `digest`/`help`/`manifest` — once per
    // invocation, before any phase runs. `clean` stays exempt here
    // (still a dry-run stub, unreachable in this branch); merge and
    // unmerge internals never re-gate (the resolver owns the merge
    // path, teardown is never gated).
    // Portage: doebuild.py, _validate_deps
    match ebuild_phases::compute_environment(ebuild_path, &portage_tmpdir) {
        Ok(gate_env) => {
            match ebuild_phases::required_use_refusal(&gate_env, &merge_options.config_root, &root)
            {
                Ok(Some(refusal)) => {
                    eprint!("{refusal}");
                    return ExitCode::from(1);
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("ebuild: {e}");
                    return ExitCode::from(1);
                }
            }
        }
        Err(e) => {
            eprintln!("ebuild: {e}");
            return ExitCode::from(1);
        }
    }

    // One command at a time: neither `merge`/`unmerge` nor `package`
    // is a phase, so a mixed list needs per-command routing (like
    // `bin/ebuild`'s own per-argument `doebuild` loop).
    //
    // Portage `bin/ebuild`'s stale-env notice, once per invocation before
    // the first command: `clean` absent, `noauto` off, a build-dir command
    // requested, and `${T}/environment` left from a previous run. stderr,
    // like `portage.writemsg` (which defaults there), with a literal `${T}`.
    // Portage: bin/ebuild:363-389 (stale_env_warning)
    if !commands.contains(&"clean")
        && !merge_options
            .features
            .split_whitespace()
            .any(|t| t == "noauto")
        && commands
            .iter()
            .any(|c| ebuild_phases::BUILD_DIR_PHASES.contains(c))
        && let Ok(notice_env) = ebuild_phases::compute_environment(ebuild_path, &portage_tmpdir)
        && notice_env.t().join("environment").is_file()
    {
        for line in ebuild_phases::stale_env_lines(&notice_env.split.pf) {
            eprintln!(">>> {line}");
        }
    }
    for &cmd in commands {
        let result = if ebuild_merge::is_merge_command(cmd) {
            ebuild_merge::run_merge(ebuild_path, &root, &portage_tmpdir, &merge_options, None)
        } else if ebuild_merge::is_qmerge_command(cmd) {
            ebuild_merge::run_qmerge(ebuild_path, &root, &portage_tmpdir, &merge_options)
                .map(|outcome| outcome.status)
        } else if ebuild_unmerge::is_unmerge_command(cmd) {
            ebuild_unmerge::run_unmerge(ebuild_path, &root, &portage_tmpdir, &unmerge_options)
        } else if ebuild_package::is_package_command(cmd) {
            ebuild_package::run_package(ebuild_package::RunPackageArgs {
                ebuild_path,
                root: &root,
                portage_tmpdir: &portage_tmpdir,
                options: &package_options,
                build_env: &[],
                use_flags: "",
                // Manual mode runs every phase like `doebuild()`; the
                // skip is scheduler-path only (backlog #458).
                skip_noop_src_phases: false,
                skip_pretend: false,
            })
        } else if ebuild_phases::is_standalone_phase_command(cmd) {
            ebuild_phases::run_single_phase(ebuild_phases::RunSinglePhaseArgs {
                ebuild_path,
                phase: cmd,
                root: &root,
                portage_tmpdir: &portage_tmpdir,
                debug,
                config_root: &merge_options.config_root,
                shell,
                build_env: &direct_build_env,
                log_file: ebuild_phases::PhaseLog::Foreground,
            })
        } else {
            ebuild_phases::run_commands(ebuild_phases::RunCommandsArgs {
                ebuild_path,
                commands: &[cmd],
                root: &root,
                portage_tmpdir: &portage_tmpdir,
                distdir: &distdir,
                debug,
                config_root: &merge_options.config_root,
                shell,
                log_file: ebuild_phases::PhaseLog::Foreground,
                build_env: &direct_build_env,
                // A direct `ebuild` run executes every phase like
                // `doebuild()`; the no-op src-phase skip is merge-path
                // only (backlog #456).
                skip_noop_src_phases: false,
                skip_pretend: false,
            })
        };
        match result {
            Ok(0) => {}
            Ok(_) => return ExitCode::from(1),
            Err(e) => {
                eprintln!("ebuild: {e}");
                return ExitCode::from(1);
            }
        }
    }
    ExitCode::SUCCESS
}

/// What [`unmerge_and_package_options`] hands back to its caller.
pub(crate) struct UnmergeAndPackageOptionsOut {
    pub(crate) unmerge_options: UnmergeOptions,
    pub(crate) package_options: PackageOptions,
}

/// Arguments of [`unmerge_and_package_options`].
pub(crate) struct UnmergeAndPackageOptionsArgs<'a> {
    pub(crate) debug: bool,
    pub(crate) shell: ShellBackend,
    pub(crate) ebuild_file: &'a str,
    pub(crate) root: &'a Path,
    pub(crate) distdir: &'a Path,
    pub(crate) standalone_config: Option<&'a portage_profile::Config>,
}

/// The unmerge and package options of the direct ebuild path.
/// `standalone_config` is the caller's once-resolved `Config` (cached
/// per invocation); `None` falls back to resolving the unmerge chain
/// here, and then to the calling-env-only defaults.
pub(crate) fn unmerge_and_package_options(
    args: UnmergeAndPackageOptionsArgs<'_>,
) -> UnmergeAndPackageOptionsOut {
    let UnmergeAndPackageOptionsArgs {
        debug,
        shell,
        ebuild_file,
        root,
        distdir,
        standalone_config,
    } = args;
    let default_unmerge_options = ebuild_unmerge::UnmergeOptions::default();
    // `unmerge-orphans` from the resolved `FEATURES` when a config
    // resolves (the same folded chain `emerge -C` reads, embedded make.globals
    // included); else the incremental fold of the calling env.
    // Portage: dbapi/vartree.py:2913, `"unmerge-orphans" in self.settings.features`
    let config_root = portage_repo::config_root_from_env();
    // Both unmerge options from the resolved chain when a config resolves
    // (the same folded chain `emerge -C` reads, embedded make.globals included);
    // else the incremental fold of the calling env and the `make.globals`
    // `UNINSTALL_IGNORE` default.
    // Portage: dbapi/vartree.py:2913, `"unmerge-orphans" in self.settings.features`
    let owned_unmerge_config;
    let unmerge_config = match standalone_config {
        Some(config) => Some(config),
        None => {
            owned_unmerge_config = ebuild_phases::resolve_unmerge_config(&config_root, root);
            owned_unmerge_config.as_ref()
        }
    };
    // `unmerge-orphans` from the resolved `FEATURES` when a config
    // resolves (the same folded chain `emerge -C` reads, embedded make.globals
    // included); else the incremental fold of the calling env.
    // Portage: dbapi/vartree.py:2913, `"unmerge-orphans" in self.settings.features`
    let unmerge_orphans = unmerge_config
        .as_ref()
        .map(|config| {
            crate::pretend::config_features_string(config)
                .split_whitespace()
                .any(|tok| tok == "unmerge-orphans")
        })
        .unwrap_or_else(ebuild_unmerge::unmerge_orphans_from_env);
    // `UNINSTALL_IGNORE` from the resolved scalar, else the calling env
    // over the `make.globals` default; packdebug appends its tarball dir.
    // Portage: dbapi/vartree.py, _unmerge_pkgfiles; const.py:113
    let (resolved_features, resolved_ignore) = unmerge_config.as_ref().map_or_else(
        || {
            (
                std::env::var("FEATURES").unwrap_or_default(),
                std::env::var("UNINSTALL_IGNORE").unwrap_or_else(|_| "/lib/modules/*".to_string()),
            )
        },
        |config| {
            (
                crate::pretend::config_features_string(config),
                config
                    .other_vars
                    .get("UNINSTALL_IGNORE")
                    .cloned()
                    .unwrap_or_else(|| "/lib/modules/*".to_string()),
            )
        },
    );
    let uninstall_ignore =
        ebuild_unmerge::uninstall_ignore_with_packdebug(&resolved_ignore, &resolved_features);
    // `preserve-libs` from the same resolved `FEATURES`; with no config, the calling env's
    // `FEATURES` when set, else the default token.
    // Portage: dbapi/vartree.py, `"preserve-libs" in mysettings.features`
    let preserve_libs = if unmerge_config.is_some() {
        resolved_features
            .split_whitespace()
            .any(|t| t == "preserve-libs")
    } else {
        std::env::var("FEATURES")
            .map(|f| f.split_whitespace().any(|t| t == "preserve-libs"))
            .unwrap_or(true)
    };
    let unmerge_options = ebuild_unmerge::UnmergeOptions {
        debug,
        shell,
        config_protect: std::env::var("CONFIG_PROTECT")
            .unwrap_or(default_unmerge_options.config_protect),
        config_protect_mask: std::env::var("CONFIG_PROTECT_MASK")
            .unwrap_or(default_unmerge_options.config_protect_mask),
        unmerge_orphans,
        preserve_libs,
        uninstall_ignore,
        config_root: portage_repo::config_root_from_env(),
        already_unmerged: Vec::new(),
        // The direct `ebuild <file> unmerge` path lists every entry like
        // Portage's foreground `dblink.unmerge`; `emerge -C` builds its
        // own options with `direct` off.
        // Portage: dbapi/vartree.py, `dblink._display_merge`.
        direct: true,
    };
    // Binpkg scalars from the resolved chain (calling env over
    // files), else the `make.globals` defaults. The gpkg pair reads
    // files-only below the env; `PORTAGE_BZIP2_COMMAND` and `PKGDIR`
    // keep the full chain.
    let ebuild_path_for_chain = std::path::Path::new(ebuild_file);
    let config_root_for_chain = portage_repo::config_root_from_env();
    let chain = |key: &str| {
        portage_profile::config_env_var(key).or_else(|| {
            ebuild_phases::resolve_standalone_chain_scalar(
                ebuild_path_for_chain,
                &config_root_for_chain,
                root,
                key,
            )
        })
    };
    let files = |key: &str| {
        ebuild_phases::resolve_standalone_chain_scalar(
            ebuild_path_for_chain,
            &config_root_for_chain,
            root,
            key,
        )
    };
    // `BINPKG_FORMAT` first: the compression lookup depends on it.
    let binpkg_format = portage_profile::config_env_var("BINPKG_FORMAT")
        .or_else(|| {
            ebuild_phases::resolve_standalone_binpkg_format(
                ebuild_path_for_chain,
                &config_root_for_chain,
                root,
            )
        })
        .unwrap_or_else(|| {
            ebuild_package::PackageOptions::default()
                .binpkg_format
                .clone()
        });
    let (binpkg_compress, binpkg_compress_flags) = if binpkg_format == "gpkg" {
        let compress = ebuild_package::resolve_binpkg_compress(files);
        let flags = ebuild_package::resolve_binpkg_compress_flags(files, &compress);
        (compress, flags)
    } else {
        let compress = ebuild_package::resolve_binpkg_compress(chain);
        let flags = ebuild_package::resolve_binpkg_compress_flags(chain, &compress);
        (compress, flags)
    };
    let mut package_options = ebuild_package::PackageOptions {
        debug,
        pkgdir: ebuild_package::resolve_pkgdir(chain),
        distdir: distdir.to_path_buf(),
        shell,
        binpkg_compress,
        binpkg_compress_flags,
        portage_bzip2_command: ebuild_package::resolve_portage_bzip2_command(chain),
        binpkg_format,
        config_root: portage_repo::config_root_from_env(),
        // The `FEATURES` tokens below follow the resolved list when a
        // config resolves (the whole emerge merge setup already does;
        // Q5: `binpkg-multi-instance` is no longer conservative here),
        // else the calling env with the `make.globals` defaults.
        // Portage: config.py (settings.features); cnf/make.globals
        buildpkg_live: unmerge_config
            .as_ref()
            .map(|c| {
                crate::pretend::config_features_string(c)
                    .split_whitespace()
                    .any(|t| t == "buildpkg-live")
            })
            .unwrap_or_else(|| {
                !std::env::var("FEATURES")
                    .map(|f| f.split_whitespace().any(|t| t == "-buildpkg-live"))
                    .unwrap_or(false)
            }),
        binpkg_multi_instance: unmerge_config
            .as_ref()
            .map(|c| {
                crate::pretend::config_features_string(c)
                    .split_whitespace()
                    .any(|t| t == "binpkg-multi-instance")
            })
            .unwrap_or_else(|| {
                std::env::var("FEATURES")
                    .map(|f| f.split_whitespace().any(|t| t == "binpkg-multi-instance"))
                    .unwrap_or(false)
            }),
        binpkg_signing: unmerge_config
            .as_ref()
            .map(|c| {
                crate::pretend::config_features_string(c)
                    .split_whitespace()
                    .any(|t| t == "binpkg-signing")
            })
            .unwrap_or_else(|| {
                std::env::var("FEATURES")
                    .map(|f| f.split_whitespace().any(|t| t == "binpkg-signing"))
                    .unwrap_or(false)
            }),
        binpkg_gpg_signing_base_command: std::env::var("BINPKG_GPG_SIGNING_BASE_COMMAND")
            .unwrap_or_default(),
        binpkg_gpg_signing_digest: std::env::var("BINPKG_GPG_SIGNING_DIGEST").unwrap_or_default(),
        binpkg_gpg_signing_gpg_home: std::env::var("BINPKG_GPG_SIGNING_GPG_HOME")
            .unwrap_or_default(),
        binpkg_gpg_signing_key: std::env::var("BINPKG_GPG_SIGNING_KEY").unwrap_or_default(),
        // Config-less standalone path: no resolved header snapshot.
        pkgindex_header: None,
    };
    // Per-package `BINPKG_COMPRESS*`: the match plays only when the
    // calling env is silent and only for `xpak` runs.
    if package_options.binpkg_format == "xpak"
        && std::env::var("BINPKG_COMPRESS").is_err()
        && let Some((compress, flags, bzip2)) = ebuild_phases::resolve_standalone_binpkg_compress(
            std::path::Path::new(ebuild_file),
            &portage_repo::config_root_from_env(),
            root,
        )
    {
        package_options.binpkg_compress = compress;
        let flags_name = format!(
            "BINPKG_COMPRESS_FLAGS_{}",
            package_options.binpkg_compress.to_uppercase()
        );
        if std::env::var(&flags_name).is_err()
            && std::env::var("BINPKG_COMPRESS_FLAGS").is_err()
            && let Some(matched_flags) = flags
        {
            package_options.binpkg_compress_flags = matched_flags;
        }
        if std::env::var("PORTAGE_BZIP2_COMMAND").is_err()
            && let Some(matched_bzip2) = bzip2
        {
            package_options.portage_bzip2_command = matched_bzip2;
        }
    }
    UnmergeAndPackageOptionsOut {
        unmerge_options,
        package_options,
    }
}

/// Portage's `UnsupportedAPIException` text for an ebuild whose EAPI this
/// portage does not know, wrapped the way `bin/ebuild` prints it: `!!! `
/// plus a greedy word-wrap at 70 columns (verified byte-for-byte against
/// Portage 3.0.82.2 for `EAPI=99` and `EAPI=7_alpha` below).
// Portage: exception.py:228-230
fn unsupported_eapi_message(cpv: &str, eapi: &str) -> String {
    // `exception.py` strips leading dashes from the echoed EAPI.
    let eapi = eapi.trim_start_matches('-');
    let msg = format!(
        "Unable to do any operations on '{cpv}', since its EAPI is higher than \
         this portage version's. Please upgrade to a portage version that \
         supports EAPI '{eapi}'."
    );
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in msg.split(' ') {
        if !current.is_empty() && current.len() + 1 + word.len() > 70 {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    format!("!!! {}", lines.join("\n!!! "))
}

/// The `ebuild <file> <commands>` refusal for an unsupported head EAPI:
/// `Some` with the exact Portage text when any requested command is one
/// Portage would run through `doebuild_environment` (the real phase chain,
/// the standalone phases, `merge`/`qmerge`/`package`, and `clean`, which
/// Portage refuses too) and the file's head EAPI is non-empty and
/// unsupported. `None` otherwise — unreadable files, supported EAPIs,
/// `unmerge` and the other dry-run stubs keep their existing behaviour.
fn unsupported_eapi_refusal(ebuild_file: &str, commands: &[&str]) -> Option<String> {
    if !commands.iter().any(|cmd| {
        ebuild_phases::is_phase_command(cmd)
            || ebuild_phases::is_standalone_phase_command(cmd)
            || ebuild_merge::is_merge_command(cmd)
            || ebuild_merge::is_qmerge_command(cmd)
            || ebuild_package::is_package_command(cmd)
            || *cmd == "clean"
    }) {
        return None;
    }
    let text = std::fs::read_to_string(ebuild_file).ok()?;
    let eapi = portage_dep::eapi::parse_eapi_head(&text);
    if eapi.is_empty() || portage_repo::md5_dict::eapi_is_supported(&eapi) {
        return None;
    }
    // Portage's `mycpv`: grandparent category plus the file stem.
    let path = std::path::Path::new(ebuild_file);
    let pf = path.file_stem()?.to_str()?;
    let category = path.parent()?.parent()?.file_name()?.to_str()?;
    Some(unsupported_eapi_message(&format!("{category}/{pf}"), &eapi))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn accepts_a_real_command_and_still_prints_the_stub_marker() {
        // "clean" is a Portage command portuale still only dry-runs.
        let code = run(&args(&["foo-1.0.ebuild", "clean"]));
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn accepts_multiple_real_commands() {
        let code = run(&args(&["foo-1.0.ebuild", "clean", "compile", "install"]));
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn accepts_a_real_boolean_option() {
        let code = run(&args(&["--force", "foo-1.0.ebuild", "clean"]));
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn accepts_a_real_value_option_without_misreading_its_value() {
        let code = run(&args(&["--color", "y", "foo-1.0.ebuild", "clean"]));
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn accepts_the_inline_equals_form_of_a_value_option() {
        let code = run(&args(&["--color=y", "foo-1.0.ebuild", "clean"]));
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn accepts_shell_bash_and_shell_brush() {
        // "clean" is dry-run-only, so this only exercises `--shell`'s
        // own CLI parsing.
        let code = run(&args(&["--shell", "brush", "foo-1.0.ebuild", "clean"]));
        assert_eq!(code, ExitCode::SUCCESS);
        let code = run(&args(&["--shell", "bash", "foo-1.0.ebuild", "clean"]));
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn accepts_the_inline_equals_form_of_shell() {
        let code = run(&args(&["--shell=bash", "foo-1.0.ebuild", "clean"]));
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn rejects_an_invalid_shell_value() {
        let code = run(&args(&["--shell", "zsh", "foo-1.0.ebuild", "clean"]));
        assert_eq!(code, ExitCode::from(1));
    }

    #[test]
    fn rejects_a_missing_shell_value() {
        let code = run(&args(&["--shell"]));
        assert_eq!(code, ExitCode::from(2));
    }

    #[test]
    fn rejects_an_unrecognized_option() {
        let code = run(&args(&["--not-a-real-option", "foo-1.0.ebuild", "clean"]));
        assert_eq!(code, ExitCode::from(1));
    }

    #[test]
    fn rejects_a_filename_not_ending_in_dot_ebuild() {
        let code = run(&args(&["foo-1.0.tar.gz", "merge"]));
        assert_eq!(code, ExitCode::from(1));
    }

    #[test]
    fn rejects_an_unrecognized_command() {
        let code = run(&args(&["foo-1.0.ebuild", "not-a-real-phase"]));
        assert_eq!(code, ExitCode::from(1));
    }

    #[test]
    fn rejects_a_missing_ebuild_file() {
        let code = run(&args(&[]));
        assert_eq!(code, ExitCode::from(2));
    }

    #[test]
    fn rejects_a_missing_command() {
        let code = run(&args(&["foo-1.0.ebuild"]));
        assert_eq!(code, ExitCode::from(2));
    }

    #[test]
    fn unsupported_eapi_message_matches_portage_byte_for_byte() {
        // Portage 3.0.82.2, host cells `d-eapi99-clean` / `d-eapi7a-clean`:
        // `ebuild <f> clean` on an unsupported EAPI, rc 1, stderr exactly this.
        assert_eq!(
            unsupported_eapi_message("app-misc/t348-d-eapi99-1", "99"),
            "!!! Unable to do any operations on 'app-misc/t348-d-eapi99-1', since its\n\
             !!! EAPI is higher than this portage version's. Please upgrade to a\n\
             !!! portage version that supports EAPI '99'."
        );
        assert_eq!(
            unsupported_eapi_message("app-misc/t348-d-eapi7a-1", "7_alpha"),
            "!!! Unable to do any operations on 'app-misc/t348-d-eapi7a-1', since its\n\
             !!! EAPI is higher than this portage version's. Please upgrade to a\n\
             !!! portage version that supports EAPI '7_alpha'."
        );
    }

    #[test]
    fn help_is_implemented_and_exits_success_with_no_other_args() {
        let code = run(&args(&["--help"]));
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn short_help_alias_is_implemented() {
        let code = run(&args(&["-h"]));
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn help_wins_unconditionally_regardless_of_position_or_other_args() {
        // Help is checked before anything else is parsed at all.
        let code = run(&args(&["foo-1.0.ebuild", "merge", "--help"]));
        assert_eq!(code, ExitCode::SUCCESS);
        let code = run(&args(&["--not-a-real-option", "--help"]));
        assert_eq!(code, ExitCode::SUCCESS);
    }
}
