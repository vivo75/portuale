//! Expanding command-line tokens to atoms (dep_expand).

use crate::pretend::*;

/// Selection behind `emerge --unmerge` / `-C`: each target atom is matched
/// against the vdb; every match is `selected`, other installed versions of
/// the same `cat/pkg` are `omitted`. Full behavior in the notes.
/// Portage: _emerge/unmerge.py, _unmerge_display
/// Outcome of qualifying a bare command-line name against the repo tree.
pub(crate) enum BareName {
    Qualified(String),
    Ambiguous(Vec<String>),
    NoMatch,
}

/// Outcome of running Portage's `dep_expand` over one command-line token.
pub(crate) enum DepExpand {
    /// Already category-qualified, a `@set`, or not a package token --
    /// left as-is.
    Unchanged,
    /// The token with its category spliced in (`eix-1.2` ->
    /// `=app-portage/eix-1.2`, `eix:0` -> `app-portage/eix:0`).
    Qualified(String),
    /// The bare name matches more than one `cat/pkg` (Portage's
    /// `AmbiguousPackageName`) -- carries the sorted `cat/pkg` list.
    Ambiguous(Vec<String>),
    /// The bare name matches no `cat/pkg` anywhere.
    NoMatch,
}

/// Portage's `dep_expand` (`dbapi/dep_expand.py`): qualify a category-less
/// command-line target against the tree, via a `null/` placeholder parse.
/// Cuts: old-style PROVIDE-virtual expansion (dead since 2011).
/// Portage: dbapi/dep_expand.py, dep_expand
pub(crate) fn dep_expand_token(token: &str, all_cp: &[String]) -> DepExpand {
    if token.is_empty() || token.starts_with('@') {
        return DepExpand::Unchanged;
    }
    // A leading `*` (set-file atom form) is stripped.
    let deref = token.strip_prefix('*').unwrap_or(token);
    if deref.split(':').next().unwrap_or(deref).contains('/') {
        return DepExpand::Unchanged;
    }
    let Some((wpos, _)) = deref
        .char_indices()
        .find(|(_, c)| c.is_alphanumeric() || *c == '_')
    else {
        return DepExpand::Unchanged;
    };
    let with_null = format!("{}null/{}", &deref[..wpos], &deref[wpos..]);
    let (atom, added_eq) = match parse_atom(&with_null) {
        Some(a) => (a, false),
        // A missing `=` prefix is allowed for backward compatibility.
        None => match parse_atom(&format!("={with_null}")) {
            Some(a) => (a, true),
            None => return DepExpand::Unchanged,
        },
    };
    let pn = atom.package;
    match qualify_bare_name(&pn, all_cp) {
        BareName::NoMatch => DepExpand::NoMatch,
        BareName::Ambiguous(m) => DepExpand::Ambiguous(m),
        BareName::Qualified(cp) => {
            let orig = if added_eq {
                format!("={deref}")
            } else {
                deref.to_string()
            };
            DepExpand::Qualified(orig.replacen(&pn, &cp, 1))
        }
    }
}

/// Portage's `cpv_expand` (`dbapi/cpv_expand.py`): the `cat/pkg` keys with
/// an ebuild named `name`. Zero -> `NoMatch`; one -> `Qualified`; several
/// -> `Qualified` if exactly one is non-virtual, else `Ambiguous` (sorted).
/// Portage: dbapi/cpv_expand.py, cpv_expand
pub(crate) fn qualify_bare_name(name: &str, all_cp: &[String]) -> BareName {
    let mut matches: Vec<String> = all_cp
        .iter()
        .filter(|cp| cp.rsplit_once('/').is_some_and(|(_, p)| p == name))
        .cloned()
        .collect();
    matches.sort();
    matches.dedup();
    match matches.len() {
        0 => BareName::NoMatch,
        1 => BareName::Qualified(matches.pop().unwrap()),
        _ => {
            let non_virtual: Vec<&String> = matches
                .iter()
                .filter(|cp| {
                    !["virtual/", "acct-group/", "acct-user/"]
                        .iter()
                        .any(|p| cp.starts_with(p))
                })
                .collect();
            if non_virtual.len() == 1 {
                BareName::Qualified(non_virtual[0].clone())
            } else {
                BareName::Ambiguous(matches)
            }
        }
    }
}

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
