//! Expanding command-line tokens to atoms (dep_expand).

use crate::pretend::*;

/// An installed-ebuild path argument (`./…`, `/…`, `*.ebuild`) resolved to
/// its echoed `=cat/pkg-ver` atom, or `Ok(None)` if `arg` isn't path-shaped.
/// Portage: _emerge/unmerge.py
pub(crate) fn resolve_vdb_path_arg(arg: &str, root: &Path) -> Result<Option<String>, ExitCode> {
    let path_shaped = arg.starts_with('.') || arg.starts_with('/') || arg.ends_with(".ebuild");
    if !path_shaped {
        return Ok(None);
    }
    let Ok(mut absx) = std::fs::canonicalize(arg) else {
        println!("\n!!! The path '{arg}' doesn't exist.\n");
        return Err(ExitCode::from(1));
    };
    // Drop a trailing `.ebuild` before the db-dir check.
    if absx
        .file_name()
        .is_some_and(|n| n.to_string_lossy().ends_with(".ebuild"))
    {
        absx.pop();
    }
    if !absx.join("CONTENTS").exists() {
        println!("!!! Not a valid db dir: {}", absx.display());
        return Err(ExitCode::from(1));
    }
    // A database backend has no directory a path argument could name.
    let Some(vdb_dir) = portage_vdb::for_root(root).vdb_dir() else {
        println!("\n!!! {arg} is not inside the installed-package database; aborting.\n");
        return Err(ExitCode::from(1));
    };
    let vdb = std::fs::canonicalize(&vdb_dir).unwrap_or(vdb_dir);
    let Ok(rel) = absx.strip_prefix(&vdb) else {
        println!("\n!!! {arg} is not inside {}; aborting.\n", vdb.display());
        return Err(ExitCode::from(1));
    };
    let rel = rel.to_string_lossy();
    if rel.is_empty() || !rel.contains('/') {
        println!(
            "\n!!! {arg} cannot be inside {}; aborting.\n",
            vdb.display()
        );
        return Err(ExitCode::from(1));
    }
    let atom = format!("={rel}");
    println!("{atom}");
    Ok(Some(atom))
}
