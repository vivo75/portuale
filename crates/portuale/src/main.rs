// Portuale: a multicall-style binary proving the emerge/ebuild dispatch
// mechanism described in docs/agent-context.md ("emerge/ebuild binary
// shape"): a single static binary that behaves differently depending on
// how it is invoked, busybox-style. Real dispatch installs this binary
// once and creates `emerge` / `ebuild` symlinks (or hardlinks) pointing
// at it; argv[0] tells it which applet to run.
//
// Both applets now do real work: `emerge` resolves dependencies and
// builds / merges / unmerges packages (see pretend.rs and its
// `emerge_*` siblings), `ebuild` runs real phase chains and the real
// merge / unmerge / package steps (see ebuild.rs). Both still recognize
// their whole real CLI surface by name (see emerge_options.rs /
// ebuild_options.rs) even where a given flag or action isn't
// implemented, reporting which one it is rather than a generic error.
// `portuale --help` (or a bare `portuale`) lists the applets.

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

/// `portuale --help` / `portuale` with no applet: one line per applet,
/// name plus a short (< 120 char) description. busybox lists its applets
/// the same way. This dispatch shim has no upstream counterpart, so the
/// text is not a port of anything.
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
        "   mrg      the real emerge option surface via clap, driving portuale's emerge codepath -- a relaxed re-take"
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

/// #318: the backend `PORTUALE_VDB_BACKEND` selects (environment, then
/// make.conf), opened and registered before the applet runs, the same
/// rule `mrg` applies without its flags. `None` keeps the run going on
/// the files tree; `Err` is the exit status to stop with.
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
    // `docs/history/second_python_copy_removal.md` §4: the L0 bed and the
    // contract suite ask for the count of dependency tokens the resolver
    // dropped as unparseable, and assert it is zero.
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
    // reach a redb file only through the parent pipe a read-write open
    // starts (redb allows one process per file).
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

/// The Rust runtime ignores SIGPIPE, so `portuale ... | head` makes the next
/// `println!` panic ("failed printing to stdout: Broken pipe"), and with
/// `panic = "abort"` that is a core dump. Leave SIGPIPE ignored (the remote
/// transport relies on `EPIPE` errors when ssh dies) and instead turn that one
/// panic into the exit a SIGPIPE death would have shown a shell: 141, silently.
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
    exit_quietly_on_broken_pipe();
    // #326 D7: the helper entry runs first, from `args_os()` (helpers
    // receive raw filenames that may not be UTF-8), before anything else:
    // no config, no `bin_dir()`, no extraction, no atexit.
    let argv_os: Vec<std::ffi::OsString> = std::env::args_os().collect();
    if argv_os.get(1).is_some_and(|s| s == "__helper") {
        return ExitCode::from(helpers::run(&argv_os[2..]) as u8);
    }
    // #331: pin the binary's path before any phase can run, so a file
    // replaced mid-run never surfaces as `<path> (deleted)`.
    self_exe::self_exe();
    let argv: Vec<String> = std::env::args().collect();
    let invoked_as = basename(&argv[0]);

    // C2: the binary that owns real ebuild-phase execution registers the
    // cache-miss metadata provider `portage-repo` asks from
    // `repo_aux_metadata` (the ebuild fallback real `porttree.py` runs
    // when `metadata/md5-cache` lacks the entry). `portage-repo` itself
    // carries no phase runner; unregistered (unit tests), a miss keeps
    // the old read error.
    portage_repo::register_aux_metadata_provider(ebuild_phases::depend_phase_metadata);

    // Primary dispatch: argv[0] (how a real emerge/ebuild symlink invokes
    // us). Fallback: an explicit first argument, e.g. `portuale emerge
    // --pretend ...`, matching busybox's own dual invocation style so the
    // binary is still exercisable without symlinks set up.
    if let Some(applet) = Applet::from_name(invoked_as) {
        return run(applet, &argv[1..]);
    }

    let sub = argv.get(1).map(String::as_str);
    if let Some(applet) = sub.and_then(Applet::from_name) {
        return run(applet, &argv[2..]);
    }
    match sub {
        None | Some("-h") | Some("--help") => {
            print_applets();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!(
                "portuale: unrecognized applet {other:?} (invoked as {invoked_as:?}); \
                 expected a symlink named 'emerge', 'ebuild', or 'mrg', or \
                 `portuale <emerge|ebuild|mrg|vdb|portageq> ...` -- run `portuale --help` for the applet list"
            );
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod broken_pipe_tests {
    /// Backlog #323: output into a closed pipe is a quiet 141, not a panic
    /// message and an abort (SIGABRT, 134).
    #[test]
    fn a_closed_stdout_pipe_exits_141_without_a_panic() {
        use std::os::unix::process::ExitStatusExt;
        let mut exe = std::env::current_exe().unwrap();
        exe.pop();
        if exe.ends_with("deps") {
            exe.pop();
        }
        exe.push("portuale");
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
