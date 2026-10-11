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
use crate::ebuild_phases;
use crate::ebuild_unmerge;
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
        let root = portage_repo::root_from_env();
        // `make.globals` default is `/var/tmp` (the builddir adds the
        // rest itself).
        let portage_tmpdir = portage_repo::portage_tmpdir_from_env();
        // Per-package `PORTAGE_TMPDIR`: re-derive from the matched
        // `package.env` value when the calling environment is silent.
        // Skipped for `depend`/`clean`, which Portage exempts too.
        // A missing matched directory fails with Portage's message.
        // Portage: doebuild.py, _check_temp_dir
        let portage_tmpdir = if commands
            .iter()
            .any(|cmd| *cmd == "depend" || *cmd == "clean")
        {
            portage_tmpdir
        } else {
            let process_tmpdir = std::env::var_os("PORTAGE_TMPDIR").map(std::path::PathBuf::from);
            let ebuild_path = std::path::Path::new(ebuild_file);
            match ebuild_phases::resolve_standalone_portage_tmpdir(
                ebuild_path,
                &portage_repo::config_root_from_env(),
                &root,
                &portage_tmpdir,
                process_tmpdir.as_deref(),
            ) {
                Ok(resolved) => resolved,
                Err(e) => {
                    eprintln!("ebuild: {e}");
                    return ExitCode::from(1);
                }
            }
        };
        // Same env-var-not-full-config-resolution shortcut: the
        // `make.globals` defaults apply when unset.
        // Real `make.globals` `DISTDIR` default, same shortcut.
        let distdir = std::env::var_os("DISTDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("/var/cache/distfiles"));
        // Merge config via the same env-var shortcut, factored into
        // `MergeOptions::from_env` (shared with `emerge <atom>`).
        let mut merge_options = ebuild_merge::MergeOptions::from_env(shell, debug);
        // Per-package `FEATURES`: fold a matched `package.env` value
        // onto the raw process value and re-derive the merge gates.
        if let Some(features) = ebuild_phases::resolve_standalone_resolved_features(
            std::path::Path::new(ebuild_file),
            &portage_repo::config_root_from_env(),
            &root,
            &merge_options.features,
            &std::env::var("FEATURES").unwrap_or_default(),
        ) {
            merge_options.set_resolved_features(&features);
        }
        // `unmerge-orphans` from `FEATURES`, same shortcut.
        let default_unmerge_options = ebuild_unmerge::UnmergeOptions::default();
        let unmerge_orphans = std::env::var("FEATURES")
            .map(|features| {
                features
                    .split_whitespace()
                    .any(|tok| tok == "unmerge-orphans")
            })
            .unwrap_or(default_unmerge_options.unmerge_orphans);
        let unmerge_options = ebuild_unmerge::UnmergeOptions {
            debug,
            shell,
            config_protect: std::env::var("CONFIG_PROTECT")
                .unwrap_or(default_unmerge_options.config_protect),
            config_protect_mask: std::env::var("CONFIG_PROTECT_MASK")
                .unwrap_or(default_unmerge_options.config_protect_mask),
            unmerge_orphans,
            config_root: portage_repo::config_root_from_env(),
            already_unmerged: Vec::new(),
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
                    &root,
                    key,
                )
            })
        };
        let files = |key: &str| {
            ebuild_phases::resolve_standalone_chain_scalar(
                ebuild_path_for_chain,
                &config_root_for_chain,
                &root,
                key,
            )
        };
        // `BINPKG_FORMAT` first: the compression lookup depends on it.
        let binpkg_format = portage_profile::config_env_var("BINPKG_FORMAT")
            .or_else(|| {
                ebuild_phases::resolve_standalone_binpkg_format(
                    ebuild_path_for_chain,
                    &config_root_for_chain,
                    &root,
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
            distdir: distdir.clone(),
            shell,
            binpkg_compress,
            binpkg_compress_flags,
            portage_bzip2_command: ebuild_package::resolve_portage_bzip2_command(chain),
            binpkg_format,
            config_root: portage_repo::config_root_from_env(),
            // Real default-on `FEATURES` token; see
            // `PackageOptions::buildpkg_live`.
            buildpkg_live: !std::env::var("FEATURES")
                .map(|f| f.split_whitespace().any(|t| t == "-buildpkg-live"))
                .unwrap_or(false),
            // Real default is on, but this defaults off; see
            // `PackageOptions::binpkg_multi_instance`.
            binpkg_multi_instance: std::env::var("FEATURES")
                .map(|f| f.split_whitespace().any(|t| t == "binpkg-multi-instance"))
                .unwrap_or(false),
            binpkg_signing: std::env::var("FEATURES")
                .map(|f| f.split_whitespace().any(|t| t == "binpkg-signing"))
                .unwrap_or(false),
            binpkg_gpg_signing_base_command: std::env::var("BINPKG_GPG_SIGNING_BASE_COMMAND")
                .unwrap_or_default(),
            binpkg_gpg_signing_digest: std::env::var("BINPKG_GPG_SIGNING_DIGEST")
                .unwrap_or_default(),
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
            && let Some((compress, flags, bzip2)) =
                ebuild_phases::resolve_standalone_binpkg_compress(
                    std::path::Path::new(ebuild_file),
                    &portage_repo::config_root_from_env(),
                    &root,
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

        // One command at a time: neither `merge`/`unmerge` nor `package`
        // is a phase, so a mixed list needs per-command routing (like
        // `bin/ebuild`'s own per-argument `doebuild` loop).
        for &cmd in &commands {
            let result = if ebuild_merge::is_merge_command(cmd) {
                ebuild_merge::run_merge(ebuild_path, &root, &portage_tmpdir, &merge_options, None)
            } else if ebuild_merge::is_qmerge_command(cmd) {
                ebuild_merge::run_qmerge(ebuild_path, &root, &portage_tmpdir, &merge_options)
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
                    build_env: &[],
                    log_file: None,
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
                    build_env: &[],
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
        return ExitCode::SUCCESS;
    }

    println!(
        "ebuild: dry run -- one or more of these commands is recognized but not \
         yet implemented in portuale"
    );
    println!("ebuild file: {ebuild_file:?}");
    println!("commands: {commands:?}");
    ExitCode::SUCCESS
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
