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
mod emerge_build;
mod emerge_getbinpkg;
mod emerge_options;
mod env_update;
mod fetch;
mod mtimedb;
mod needed_elf;
mod portage_lock;
mod pretend;
mod regen;

use std::process::ExitCode;

enum Applet {
    Emerge,
    Ebuild,
}

impl Applet {
    fn from_name(name: &str) -> Option<Applet> {
        match name {
            "emerge" => Some(Applet::Emerge),
            "ebuild" => Some(Applet::Ebuild),
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
        "portuale: a multicall binary -- runs as `emerge` or `ebuild` depending on how it is invoked"
    );
    println!();
    println!("Usage:");
    println!("   portuale <applet> [args ...]   run an applet by name");
    println!("   <applet> [args ...]            run via an 'emerge' / 'ebuild' symlink beside the binary");
    println!("   portuale --help                show this message");
    println!();
    println!("Applets:");
    println!(
        "   emerge   resolve dependencies and build, merge, or unmerge packages -- the package-manager front end"
    );
    println!(
        "   ebuild   run individual build phases (unpack/compile/install/merge/unmerge/...) on one ebuild file"
    );
    println!();
    println!("Run `portuale <applet> --help` for that applet's own options.");
}

fn run_emerge(args: &[String]) -> ExitCode {
    pretend::run(args)
}

fn run_ebuild(args: &[String]) -> ExitCode {
    ebuild::run(args)
}

fn run(applet: Applet, args: &[String]) -> ExitCode {
    match applet {
        Applet::Emerge => run_emerge(args),
        Applet::Ebuild => run_ebuild(args),
    }
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    let invoked_as = basename(&argv[0]);

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
                 expected a symlink named 'emerge' or 'ebuild', or \
                 `portuale <emerge|ebuild> ...` -- run `portuale --help` for the applet list"
            );
            ExitCode::from(1)
        }
    }
}
