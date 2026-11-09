//! Experiment front end of `portage-world`: prints, for every package slot
//! `@world` and its dependencies reach, the versions in final order.
//! See that crate for what is followed and what is cut.

use portage_world::Options;
use std::path::{Path, PathBuf};

/// `ROOT` and `PORTAGE_CONFIGROOT` the `portuale` child runs its phase with.
static PHASE_ROOTS: std::sync::OnceLock<(PathBuf, PathBuf)> = std::sync::OnceLock::new();

/// Metadata of an ebuild the repository has no md5-cache entry for: the
/// depcachedir first, then `portuale __aux-metadata`, which runs the
/// `depend` phase (this program cannot) and fills the depcachedir.
/// The binary is `$PORTUALE_BIN`, else `portuale` beside this executable.
///
/// # Errors
/// No `portuale` binary found, or it could not read the ebuild.
fn aux_metadata(
    repo: &Path,
    category: &str,
    pf: &str,
) -> Result<std::collections::HashMap<String, String>, String> {
    if let Ok(md) = portage_repo::depcache_aux_metadata(repo, category, pf) {
        return Ok(md);
    }
    let bin = std::env::var_os("PORTUALE_BIN")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_exe()
                .ok()?
                .parent()
                .map(|d| d.join("portuale"))
        })
        .filter(|p| p.is_file())
        .ok_or("no portuale binary (set PORTUALE_BIN) to run the depend phase")?;
    let mut cmd = std::process::Command::new(bin);
    cmd.arg("__aux-metadata").arg(repo).arg(category).arg(pf);
    if let Some((root, config_root)) = PHASE_ROOTS.get() {
        cmd.env("ROOT", root).env("PORTAGE_CONFIGROOT", config_root);
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(portage_repo::parse_aux_entry(&String::from_utf8_lossy(
        &out.stdout,
    )))
}

struct Cli {
    opts: Options,
    only_best: bool,
    /// Print the seed atoms and stop (`--show-seeds`).
    show_seeds: bool,
}

fn main() {
    portage_repo::register_aux_metadata_provider(aux_metadata);
    let cli = match parse_args() {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };
    let _ = PHASE_ROOTS.set((cli.opts.root.clone(), cli.opts.config_root.clone()));
    if let Err(msg) = run(&cli) {
        eprintln!("experiment-pre-sort-atoms: {msg}");
        std::process::exit(1);
    }
}

fn run(cli: &Cli) -> Result<(), String> {
    if cli.show_seeds {
        for seed in portage_world::seeds(&cli.opts)? {
            println!("{seed}");
        }
        return Ok(());
    }
    let world = portage_world::analyse(&cli.opts)?;
    let (mut invalid, mut unneeded) = (0, 0);
    for set in &world.slots {
        invalid += usize::from(set.invalid);
        unneeded += usize::from(set.unneeded);
        set.print(cli.only_best);
    }
    eprintln!(
        "{} atoms, {} package slots, {invalid} invalid, {unneeded} unneeded",
        world.atoms,
        world.slots.len()
    );
    Ok(())
}

fn parse_args() -> Result<Cli, String> {
    let mut cli = Cli {
        opts: Options::new(PathBuf::from("/"), PathBuf::from("/")),
        only_best: false,
        show_seeds: false,
    };
    let opts = &mut cli.opts;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match a.as_str() {
            "--root" => opts.root = value("--root")?.into(),
            "--config-root" => opts.config_root = value("--config-root")?.into(),
            "--threads" => {
                opts.threads = value("--threads")?
                    .parse::<usize>()
                    .map_err(|e| format!("--threads: {e}"))?
                    .max(1);
            }
            "--best-only" => cli.only_best = true,
            "--unique" => opts.unique = true,
            "--with-bdeps" => opts.with_bdeps = true,
            "--show-seeds" => cli.show_seeds = true,
            "--set" => opts
                .extra_sets
                .push(value("--set")?.trim_start_matches('@').to_string()),
            "-h" | "--help" => {
                return Err(
                    "usage: experiment-pre-sort-atoms [--root DIR] [--config-root DIR] \
                            [--threads N] [--best-only] [--unique] [--with-bdeps] [--set NAME]... [--show-seeds]"
                        .into(),
                );
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(cli)
}
