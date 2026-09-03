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

/// `emerge --pretend` in any spelling: `--pretend` or a short-option
/// cluster holding `p` (`-p`, `-pv`, `-uDNp`).
fn emerge_is_pretend(args: &[String]) -> bool {
    args.iter().any(|a| {
        a == "--pretend" || (a.starts_with('-') && !a.starts_with("--") && a[1..].contains('p'))
    })
}

fn main() -> ExitCode {
    // Applets are wired in here as they become usable.
    eprintln!("portuale: no applet is wired in yet");
    ExitCode::FAILURE
}
