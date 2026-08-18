// Multicall binary proving the emerge/ebuild dispatch mechanism described
// in PORTING/PROMPT.md ("emerge/ebuild binary shape"): a single static
// binary that behaves differently depending on how it is invoked,
// busybox-style. Real dispatch installs this binary once and creates
// `emerge` / `ebuild` symlinks (or hardlinks) pointing at it; argv[0] tells
// it which applet to run.
//
// `emerge` implements one real slice: `--pretend <category/package>` (see
// pretend.rs). Everything else -- real merges, phase execution via
// `ebuild`, and anything about `emerge` beyond that one slice -- is still
// a dry-run/read-only stub (see PROMPT.md, "Scope of the first port").
// Both applets recognize their real CLI surface by name (see
// emerge_options.rs/ebuild_options.rs) even where the underlying
// behavior isn't implemented.

mod ebuild;
mod ebuild_merge;
mod ebuild_options;
mod ebuild_package;
mod ebuild_phases;
mod ebuild_unmerge;
mod emerge_build;
mod emerge_options;
mod pretend;

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
    // us). Fallback: an explicit first argument, e.g. `multicall emerge
    // --pretend ...`, matching busybox's own dual invocation style so the
    // binary is still exercisable without symlinks set up.
    if let Some(applet) = Applet::from_name(invoked_as) {
        return run(applet, &argv[1..]);
    }

    match argv.get(1).and_then(|a| Applet::from_name(a)) {
        Some(applet) => run(applet, &argv[2..]),
        None => {
            eprintln!(
                "multicall: unrecognized applet (invoked as {invoked_as:?}); \
                 expected a symlink named 'emerge' or 'ebuild', or \
                 `multicall <emerge|ebuild> ...`"
            );
            ExitCode::from(1)
        }
    }
}
