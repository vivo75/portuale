//! Expanding command-line tokens to atoms (dep_expand).

use crate::pretend::*;

/// `emerge --unmerge` / `-C <atoms>` (with or without `--pretend`): real
/// `_emerge/unmerge.py::_unmerge_display` for `unmerge_action ==
/// "unmerge"`. With `pretend` this is a preview only; **without** it,
/// after the display, `execute_unmerge` really removes each `selected`
/// package (real `unmerge()`'s own removal loop -- see its own doc
/// comment). The `>>> These are the packages that would be unmerged:`
/// header is `--pretend`/`--ask`-gated in real portage
/// (`unmerge.py:195`); everything else in the block prints either way.
/// `--depclean`/`--prune` reuse this with their computed cleanlist and
/// their own real `pretend` flag -- so without `--pretend` this same
/// path removes their cleanlist too (real `action_depclean` feeds it to
/// the identical `unmerge()`). Each target atom is
/// matched against the vdb (`installed_candidates` + `match_from_list`,
/// exactly real `vartree.dbapi.match`); every match goes into
/// `selected`, and every *other* installed version of the same
/// `category/package` becomes `omitted` (real `vartree.dep_match(cp)`
/// minus the selected/protected ones). `sys-apps/portage` itself is
/// force-`protected` with real portage's own "no valid reason for
/// Portage to unmerge itself" note (real `PORTAGE_PACKAGE_ATOM`). A
/// `@world`/`@system`/`@customset` target expands to its own atom list
/// first (the same machinery `run()` and `run_deselect` already use).
///
/// The "still listed in the following package sets" set-protection
/// warning (real `unmerge.py:355-447`, `EditablePackageSet` members
/// reached via `world_sets`) and the "is part of your system profile"
/// warning (real `cp in syslist`) are both real -- including the
/// higher-slot refinement (`unmerge.py:421-441`: an installed
/// higher-versioned instance of the same cp *in a different slot* that
/// also matches the set atom suppresses the warning for that set).
///
/// **Documented cuts** (real `_unmerge_display` still does these): the
/// "currently used Python interpreter" self-skip (real
/// `_dblink(cpv).isowner(portage._python_interpreter)`) -- a non-gap
/// for portuale, whose `emerge` is a Rust binary with no Python
/// interpreter of its own to protect. The `--prune`/`--depclean`
/// variants have their own dedicated `run_prune_pretend`/
/// `run_depclean_pretend` and never really remove.
/// Real `unmerge.py:137-182`'s own installed-ebuild-path handling: an
/// `--unmerge`/`-C` argument that starts with `.` or `/`, or ends with
/// `.ebuild`, is a path into the vdb, not an atom. Returns `Ok(None)` if
/// `arg` isn't path-shaped, `Ok(Some("=cat/pkg-ver"))` for a valid vdb
/// entry (real portage also echoes that `=atom` to stdout, reproduced
/// here), or `Err(code)` after printing the matching diagnostic for a
/// bad path.
///
/// The path is resolved with `canonicalize` (real portage uses
/// `os.path.abspath`, which doesn't follow symlinks -- `canonicalize`
/// does, but it resolves the vdb root the same way, so `strip_prefix`
/// still works, and it is what actually keeps a symlinked test `ROOT`
/// working). Real portage's own stray `print(sp_absx)` / `print(absx)`
/// debug lines before the "not inside …; aborting" message (a raw
/// list repr -- clearly unintended output) are deliberately omitted.
/// Outcome of qualifying a bare command-line name against the repo tree.
pub(crate) enum BareName {
    Qualified(String),
    Ambiguous(Vec<String>),
    NoMatch,
}

/// Outcome of running real `dep_expand` over one command-line token.
pub(crate) enum DepExpand {
    /// Already category-qualified, a `@set`, or not a package token --
    /// left as-is.
    Unchanged,
    /// The token with its category spliced in (`eix-1.2` ->
    /// `=app-portage/eix-1.2`, `eix:0` -> `app-portage/eix:0`).
    Qualified(String),
    /// The bare name matches more than one `cat/pkg` (real
    /// `AmbiguousPackageName`) -- carries the sorted `cat/pkg` list.
    Ambiguous(Vec<String>),
    /// The bare name matches no `cat/pkg` anywhere.
    NoMatch,
}

/// Real `dep_expand` (`lib/portage/dbapi/dep_expand.py`): a command-line
/// target with no category is qualified against the tree. Covers the
/// pure-bare name (`emerge eix`) **and** one that carries an operator /
/// bare version / slot / use-deps / repo (`emerge eix-1.2`,
/// `emerge '>=eix-1.2'`, `emerge eix:0`): real inserts `null/` before the
/// first word character, parses the result (retrying with a leading `=`
/// for the "missing `=`" backward-compat shape), pulls the package name
/// back out of `mydep.cp`, `cpv_expand`s just that name, and splices the
/// category into the original string with a single `str.replace`.
///
/// Documented cut vs real: old-style PROVIDE-virtual expansion for an
/// explicit `virtual/` category (`dep_expand`'s `has_cat` branch) -- dead
/// since 2011 and unused by this fork.
pub(crate) fn dep_expand_token(token: &str, all_cp: &[String]) -> DepExpand {
    if token.is_empty() || token.starts_with('@') {
        return DepExpand::Unchanged;
    }
    // Real: a leading `*` (set-file atom form) is stripped and does not
    // survive into `orig_dep`.
    let deref = token.strip_prefix('*').unwrap_or(token);
    // `has_cat = "/" in orig_dep.split(":")[0]`
    if deref.split(':').next().unwrap_or(deref).contains('/') {
        return DepExpand::Unchanged;
    }
    // `alphanum = re.search(r"\w", orig_dep)`, then insert `null/` there.
    let Some((wpos, _)) = deref
        .char_indices()
        .find(|(_, c)| c.is_alphanumeric() || *c == '_')
    else {
        return DepExpand::Unchanged;
    };
    let with_null = format!("{}null/{}", &deref[..wpos], &deref[wpos..]);
    let (atom, added_eq) = match parse_atom(&with_null) {
        Some(a) => (a, false),
        // "Missing '=' prefix is allowed for backward compatibility."
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
            // `Atom(str(orig_dep).replace(mydep, expanded, 1))`, where
            // `orig_dep` has gained the `=` iff we did above.
            let orig = if added_eq {
                format!("={deref}")
            } else {
                deref.to_string()
            };
            DepExpand::Qualified(orig.replacen(&pn, &cp, 1))
        }
    }
}

/// Real `cpv_expand` (`lib/portage/dbapi/cpv_expand.py`): the `cat/pkg`
/// keys under which a package named `name` has an ebuild. Zero ->
/// `NoMatch`; one -> `Qualified`; more than one -> `Qualified` if exactly
/// one is not a `virtual/` / `acct-group/` / `acct-user/` key (real
/// "assume that the non-virtual is desired"), else `Ambiguous` (sorted).
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

pub(crate) fn resolve_vdb_path_arg(arg: &str, root: &Path) -> Result<Option<String>, ExitCode> {
    let path_shaped = arg.starts_with('.') || arg.starts_with('/') || arg.ends_with(".ebuild");
    if !path_shaped {
        return Ok(None);
    }
    let Ok(mut absx) = std::fs::canonicalize(arg) else {
        println!("\n!!! The path '{arg}' doesn't exist.\n");
        return Err(ExitCode::from(1));
    };
    // Real: `if sp_absx[-1][-7:] == ".ebuild": del sp_absx[-1]`.
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
    // The directory-backed VDB's own path (`files`: `<root>/var/db/pkg`);
    // a database backend has no directory a path argument could name.
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
    // Real: `sp_absx_len <= sp_vdb_len` -> "cannot be inside …".
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
