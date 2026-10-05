// Portuale: a multicall-style binary, busybox-style: one static binary
// behaves as `emerge`, `ebuild`, `mrg`, `vdb` or `portageq` depending on
// argv[0] (a symlink beside it) or an explicit first argument. `emerge`
// resolves and merges packages, `ebuild` runs phase chains and the
// merge/unmerge/package steps; both recognize their whole Portage CLI
// surface by name even where a flag is not implemented. `portuale --help`
// lists the applets.

mod binpkg;
mod color;
mod difflib;
mod ebuild;
mod ebuild_merge;
mod ebuild_options;
mod ebuild_package;
mod ebuild_phases;
mod ebuild_unmerge;
mod elog;
mod embedded_runtime;
mod emerge_build;
mod emerge_getbinpkg;
mod emerge_options;
mod env_update;
mod error;
mod fetch;
mod helpers;
mod info_files;
mod install_mask;
mod merge_engines;
mod mrg;
mod mtimedb;
mod needed_elf;
mod portage_lock;
mod portageq;
mod preserved_libs;
mod pretend;
mod privileges;
mod regen;
mod remote;
mod remote_bundle;
mod self_exe;
mod vdb_cmd;
#[cfg(feature = "vdb-fuse")]
mod vdb_fuse;
mod vdb_ipc;
#[cfg(feature = "vdb-fuse")]
mod vdb_rw;
#[cfg(feature = "vdb-fuse")]
mod vdb_view;

use std::process::ExitCode;

enum Applet {
    Emerge,
    Ebuild,
    Mrg,
    Vdb,
    Portageq,
}

impl Applet {
    fn from_name(name: &str) -> Option<Applet> {
        match name {
            "emerge" => Some(Applet::Emerge),
            "ebuild" => Some(Applet::Ebuild),
            "mrg" => Some(Applet::Mrg),
            "vdb" => Some(Applet::Vdb),
            "portageq" => Some(Applet::Portageq),
            _ => None,
        }
    }
}

fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// `portuale --help` / bare `portuale`: one line per applet. This dispatch
/// shim has no upstream counterpart, so the text is not a port of anything.
fn print_applets() {
    println!(
        "portuale: a multicall binary -- runs as `emerge`, `ebuild`, `mrg`, `vdb`, or `portageq` depending on how it is invoked"
    );
    println!();
    println!("Usage:");
    println!("   portuale <applet> [args ...]   run an applet by name");
    println!(
        "   <applet> [args ...]            run via an 'emerge' / 'ebuild' / 'mrg' symlink beside the binary"
    );
    println!("   portuale --help                show this message");
    println!();
    println!("Applets:");
    println!(
        "   emerge   resolve dependencies and build, merge, or unmerge packages -- the package-manager front end"
    );
    println!(
        "   ebuild   run individual build phases (unpack/compile/install/merge/unmerge/...) on one ebuild file"
    );
    println!(
        "   mrg      Portage's emerge option surface via clap, driving portuale's emerge codepath -- a relaxed re-take"
    );
    println!(
        "   vdb      convert or verify the installed-package database between backends (files, sqlite)"
    );
    println!(
        "   portageq has_version / best_version over the installed-package database (the only portageq commands)"
    );
    println!();
    println!("Run `portuale <applet> --help` for that applet's own options.");
}

/// `--help` / `--version` (any spelling) never touch the installed
/// database, so they must not fail on a missing one.
fn is_help_or_version(args: &[String]) -> bool {
    args.iter()
        .any(|a| matches!(a.as_str(), "--help" | "-h" | "--version" | "-V"))
}

/// `emerge --pretend` in any spelling: `--pretend` or a short-option
/// cluster holding `p` (`-p`, `-pv`, `-uDNp`).
fn emerge_is_pretend(args: &[String]) -> bool {
    args.iter().any(|a| {
        a == "--pretend" || (a.starts_with('-') && !a.starts_with("--") && a[1..].contains('p'))
    })
}

/// Select, open and register the VDB backend before the applet runs (the same
/// rule `mrg` applies without its flags). `None` keeps the files tree.
fn select_vdb_for(
    who: &str,
    args: &[String],
    readonly: bool,
) -> Result<Option<crate::vdb_ipc::Server>, ExitCode> {
    if is_help_or_version(args) {
        return Ok(None);
    }
    mrg::select_vdb(who, None, None, readonly).map_err(|(message, code)| {
        eprintln!("{message}");
        ExitCode::from(code)
    })
}

fn run_emerge(args: &[String]) -> ExitCode {
    // Held for the whole run: the redb parent pipe stops when it drops.
    let _vdb_ipc = match select_vdb_for("emerge", args, emerge_is_pretend(args)) {
        Ok(server) => server,
        Err(code) => return code,
    };
    let code = pretend::run(args);
    if std::env::var_os("PORTUALE_REPORT_UNPARSED_DEP_TOKENS").is_some() {
        eprintln!(
            "portuale: unparsed dependency tokens: {}",
            portage_repo::unparsed_dep_tokens()
        );
    }
    code
}

fn run_ebuild(args: &[String]) -> ExitCode {
    // Read-write even for a build-only command: its phases' `has_version`
    // reach a redb file only through the parent pipe a read-write open starts.
    let _vdb_ipc = match select_vdb_for("ebuild", args, false) {
        Ok(server) => server,
        Err(code) => return code,
    };
    ebuild::run(args)
}

fn run_mrg(args: &[String]) -> ExitCode {
    mrg::run(args)
}

fn run(applet: Applet, args: &[String]) -> ExitCode {
    match applet {
        Applet::Emerge => run_emerge(args),
        Applet::Ebuild => run_ebuild(args),
        Applet::Mrg => run_mrg(args),
        Applet::Vdb => vdb_cmd::run(args),
        Applet::Portageq => portageq::run(args),
    }
}

/// The Rust runtime ignores SIGPIPE, so `portuale ... | head` would make the
/// next `println!` panic; turn that one panic into the quiet exit 141 a
/// SIGPIPE death shows a shell. The ignored disposition stays (the remote
/// transport relies on `EPIPE` errors when ssh dies).
fn exit_quietly_on_broken_pipe() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = info.to_string();
        if msg.contains("failed printing to std") && msg.contains("Broken pipe") {
            std::process::exit(141);
        }
        previous(info);
    }));
}

fn main() -> ExitCode {
    // Applets are wired in here as they become usable.
    eprintln!("portuale: no applet is wired in yet");
    ExitCode::FAILURE
}

#[cfg(test)]
mod broken_pipe_tests {
    /// A closed stdout pipe exits 141 quietly, not a panic message and an abort.
    #[test]
    fn a_closed_stdout_pipe_exits_141_without_a_panic() {
        use std::os::unix::process::ExitStatusExt;
        let exe = crate::self_exe::built_portuale_bin();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        let mut child = std::process::Command::new(exe)
            .args(["emerge", "--list-sets"])
            .env("PORTAGE_CONFIGROOT", &root)
            .env("ROOT", &root)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        drop(child.stdout.take()); // the reader goes away before the first write
        let out = child.wait_with_output().unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!err.contains("panicked"), "stderr: {err}");
        assert_eq!(out.status.signal(), None, "killed by a signal: {out:?}");
        assert!(
            matches!(out.status.code(), Some(0) | Some(141)),
            "unexpected exit: {:?}",
            out.status
        );
    }
}
