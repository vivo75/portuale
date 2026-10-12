//! Command-line atom validation: package arguments and the `--exclude`-style
//! option families (plan #341 S7).

use crate::pretend::EmergeOpts;
use portage_dep::{AtomEapi, AtomOptions, Blocker, Operator, parse_atom_with};
use std::borrow::Cow;
use std::process::ExitCode;

/// Portage `_emerge/is_valid_package_atom.py:19-27`
/// (`insert_category_into_atom`): splice `category/` before the first
/// alphanumeric/`_`/`*` character (ASCII). `None` when the string holds
/// none (the caller then validates the string as-is, which fails).
pub(crate) fn insert_category_into_atom(atom: &str, category: &str) -> Option<String> {
    let pos = atom
        .char_indices()
        .find(|(_, c)| *c == '*' || c.is_ascii_alphanumeric() || *c == '_')
        .map(|(i, _)| i)?;
    Some(format!("{}{}/{}", &atom[..pos], category, &atom[pos..]))
}

/// Portage `_emerge/is_valid_package_atom.py:is_valid_package_atom`
/// (`x` only; the `Atom` input form never occurs on this path): a short
/// name gets `cat/` spliced in, then the atom must parse with
/// `allow_repo=True` (build-ids on via the `eapi=None` default, no
/// wildcards) and carry no blocker. This is the `run_action` gate
/// (`_emerge/actions.py:4244-4262`); the `depgraph._select_files`
/// three-line variant (depgraph.py:5261-5269) is unreachable behind it
/// and is not ported, and `action_config` (`emerge --config`, three
/// lines, actions.py:717-724) stays as `config_action.rs` already
/// renders it.
pub(crate) fn is_valid_package_atom(x: &str) -> bool {
    let probe: Cow<'_, str> = if x.split(':').next().unwrap_or(x).contains('/') {
        Cow::Borrowed(x)
    } else {
        match insert_category_into_atom(x, "cat") {
            Some(inserted) => Cow::Owned(inserted),
            None => Cow::Borrowed(x),
        }
    };
    let opts = AtomOptions {
        eapi: AtomEapi::None,
        allow_wildcard: false,
        allow_repo: Some(true),
        allow_build_id: None,
    };
    match parse_atom_with(&probe, &opts, "") {
        Ok(atom) => atom.blocker == Blocker::None,
        Err(_) => false,
    }
}

/// Portage `portage/repository/config.py:91-116` (`_find_bad_atoms`):
/// the bad entries of an `--exclude`-style value, in order. The value is
/// re-split on whitespace exactly like Portage's `" ".join(atoms).split()`;
/// a category-less entry gets `dummy-category/` spliced in for the parse.
/// Strict mode (`allow_repo=False`) backs every family except
/// `--buildpkg-exclude`: any operator, blocker, use dep, repo or build-id
/// form is bad (build-ids only ever parse on a `=` operator, so the
/// operator rule covers them). `less_strict` (only `--buildpkg-exclude`,
/// `_emerge/main.py:839-845`) parses with `allow_repo=True` and accepts
/// operators, repos and build-ids; only parse failures, blockers and use
/// deps stay bad.
pub(crate) fn find_bad_atoms(atoms: &[String], less_strict: bool) -> Vec<&str> {
    let mut bad = Vec::new();
    for x in atoms.iter().flat_map(|s| s.split_whitespace()) {
        let probe: Cow<'_, str> = if x.split(':').next().unwrap_or(x).contains('/') {
            Cow::Borrowed(x)
        } else {
            match insert_category_into_atom(x, "dummy-category") {
                Some(inserted) => Cow::Owned(inserted),
                None => Cow::Borrowed(x),
            }
        };
        let opts = AtomOptions {
            eapi: AtomEapi::None,
            allow_wildcard: true,
            allow_repo: Some(less_strict),
            allow_build_id: None,
        };
        let is_bad = match parse_atom_with(&probe, &opts, "") {
            Err(_) => true,
            Ok(atom) => {
                (atom.operator != Operator::None && !less_strict)
                    || atom.blocker != Blocker::None
                    || atom.use_deps.is_some()
            }
        };
        if is_bad {
            bad.push(x);
        }
    }
    bad
}

/// Portage `_emerge/main.py:839-845,885-906` (option post-pass):
/// `--buildpkg-exclude` first (its own message, no "(only …)" suffix),
/// then the nine strict families in tuple order. Failures print Portage's
/// `parser.error` line (the ~4.7 KB argparse `usage:` block Portage
/// prints above it is not reproduced) and exit 2.
pub(crate) fn validate_exclude_options(opts: &EmergeOpts<'_>) -> Option<ExitCode> {
    if !opts.buildpkg_exclude.is_empty() {
        let bad = find_bad_atoms(&opts.buildpkg_exclude, true);
        if !bad.is_empty() {
            eprintln!(
                "emerge: error: Invalid Atom(s) in --buildpkg-exclude parameter: '{}'",
                bad.join(",")
            );
            return Some(ExitCode::from(2));
        }
    }
    let families: &[(&[String], &str)] = &[
        (&opts.excluded, "exclude"),
        (&opts.getbinpkg_exclude, "getbinpkg-exclude"),
        (&opts.getbinpkg_include, "getbinpkg-include"),
        (&opts.reinstall_atoms, "reinstall-atoms"),
        (&opts.rebuild_exclude, "rebuild-exclude"),
        (&opts.rebuild_ignore, "rebuild-ignore"),
        (&opts.usepkg_exclude, "usepkg-exclude"),
        (&opts.usepkg_include, "usepkg-include"),
        (&opts.useoldpkg_atoms, "useoldpkg-atoms"),
    ];
    for (atoms, flag) in families {
        if atoms.is_empty() {
            continue;
        }
        let bad = find_bad_atoms(atoms, false);
        if !bad.is_empty() {
            eprintln!(
                "emerge: error: Invalid Atom(s) in --{flag} parameter: '{}' \
                 (only package names and slot atoms (with wildcards) allowed)",
                bad.join(",")
            );
            return Some(ExitCode::from(2));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // Portage 3.0.82.2, `python3 -c` on this host (2026-10-12):
    // `from _emerge.is_valid_package_atom import is_valid_package_atom` /
    // `from portage.repository.config import _find_bad_atoms`, one call
    // per row below.
    #[test]
    fn package_args_match_portages_run_action_gate() {
        let valid = [
            "dev-libs/t341-dep",
            "dev-libs/t341-dep[x?]",
            "dev-libs/t341-dep[x]",
            "t341-dep::t341",
            "t341-dep",
            "t341-dep:0",
            "=dev-libs/t341-dep-1-2",
            "=t341-dep-1.0",
            "dev-libs/t341-dep:0/1",
            "dev-libs/t341-dep:*",
            "dev-libs/t341-dep:0=",
            "dev-libs/t341-dep:=",
            ">=dev-libs/t341-dep-1",
        ];
        for x in valid {
            assert!(
                is_valid_package_atom(x),
                "{x:?} must be a valid package atom"
            );
        }
        let invalid = [
            "*/t341-dep",
            "!dev-libs/t341-dep",
            "!!dev-libs/t341-dep",
            "dev-libs/t341-dep-1",
            "=dev-libs/t341-dep-1-02",
            "dev-libs/t341-dep:0*",
            "*/*",
            "dev-libs/*",
            "*/t341-dep:0",
            "dev-libs/t341-*",
            "dev-libs/foo:",
            "",
            "not an atom!",
        ];
        for x in invalid {
            assert!(
                !is_valid_package_atom(x),
                "{x:?} must not be a valid package atom"
            );
        }
    }

    #[test]
    fn category_insertion_splices_before_the_first_word_char() {
        assert_eq!(
            insert_category_into_atom("t341-dep::t341", "cat").as_deref(),
            Some("cat/t341-dep::t341")
        );
        // An operator/blocker prefix stays outside the spliced category
        // (Portage inserts before the first `[\*\w]`, not at index 0).
        assert_eq!(
            insert_category_into_atom("=t341-dep-1.0", "cat").as_deref(),
            Some("=cat/t341-dep-1.0")
        );
        assert_eq!(insert_category_into_atom("!!!", "cat"), None);
        assert_eq!(insert_category_into_atom("", "cat"), None);
    }

    #[test]
    fn exclude_family_rejects_operators_blockers_use_repo_and_versions() {
        // Same live session as above; each row is
        // `_find_bad_atoms([x])` (strict) and `(…, less_strict=True)`.
        let strict_bad = [
            ">=dev-libs/t341-dep-1",
            "=dev-libs/t341-dep-1-2",
            "dev-libs/t341-dep::t341",
            "!dev-libs/t341-dep",
            "dev-libs/t341-dep[x]",
            "dev-libs/t341-dep-1",
            "dev-libs/t341-dep:0*",
        ];
        for x in strict_bad {
            assert_eq!(find_bad_atoms(&[x.to_string()], false), vec![x], "{x:?}");
        }
        let strict_ok = [
            "dev-libs/t341-dep",
            "t341-dep",
            "t341-dep:0",
            "dev-libs/t341-dep:0/1",
            "dev-libs/t341-dep:*",
            "dev-libs/t341-dep:0=",
            "dev-libs/t341-dep:=",
            "*/*",
            "dev-libs/*",
            "dev-libs/t341-*",
        ];
        for x in strict_ok {
            assert!(
                find_bad_atoms(&[x.to_string()], false).is_empty(),
                "{x:?} must pass the strict families"
            );
        }
        // `less_strict` (only `--buildpkg-exclude`) additionally accepts
        // operators, repos and build-ids; blockers, use deps and
        // structurally invalid atoms stay bad.
        for x in [
            ">=dev-libs/t341-dep-1",
            "=dev-libs/t341-dep-1-2",
            "dev-libs/t341-dep::t341",
        ] {
            assert!(
                find_bad_atoms(&[x.to_string()], true).is_empty(),
                "{x:?} must pass --buildpkg-exclude"
            );
        }
        for x in [
            "!dev-libs/t341-dep",
            "dev-libs/t341-dep[x]",
            "dev-libs/t341-dep-1",
            "dev-libs/t341-dep:0*",
        ] {
            assert_eq!(find_bad_atoms(&[x.to_string()], true), vec![x], "{x:?}");
        }
    }

    #[test]
    fn bad_atoms_join_in_order_across_occurrences() {
        let atoms = vec![
            "dev-libs/ok".to_string(),
            "dev-libs/bad::repo >=dev-libs/other-1".to_string(),
        ];
        assert_eq!(
            find_bad_atoms(&atoms, false),
            vec!["dev-libs/bad::repo", ">=dev-libs/other-1"]
        );
    }
}
