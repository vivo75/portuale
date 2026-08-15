// Portuale: a multicall-style binary, busybox-style: one static binary
// behaves as `emerge`, `ebuild`, `mrg`, `vdb` or `portageq` depending on
// argv[0] (a symlink beside it) or an explicit first argument. `emerge`
// resolves and merges packages, `ebuild` runs phase chains and the
// merge/unmerge/package steps; both recognize their whole Portage CLI
// surface by name even where a flag is not implemented. `portuale --help`
// lists the applets.

mod ebuild_merge;
mod ebuild_options;
mod ebuild_phases;
mod emerge_options;
mod error;
mod helpers;
mod info_files;
mod mrg;
mod mtimedb;
mod pretend;
mod remote;
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

fn main() -> ExitCode {
    // Applets are wired in here as they become usable.
    eprintln!("portuale: no applet is wired in yet");
    ExitCode::FAILURE
}
