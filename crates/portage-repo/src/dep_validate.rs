//! Ebuild and binary `*DEPEND` validation (Portage `Package._validate_deps`).
//!
//! Each dep key is reduced like `use_reduce(matchall=True, flat=True,
//! token_class=Atom, is_valid_flag=…)` with `eapi=pkg.eapi`: the first bad
//! token per key masks the package `invalid: <KEY>: <text>`. Categorized
//! failures (`IUSE.missing`, `EAPI.incompatible`) print the bare message;
//! anything else prints `Invalid atom (…), token N` or the structural text
//! the walk raises. Reasons file under their category in first-insertion
//! order (Package.py:534-557 groups by category; depgraph.py:12560-12563
//! iterates the buckets in that order). Unbuilt (ebuild) candidates also
//! reject `:slot/sub=` atoms, one reason per built atom, but only for keys
//! that parsed cleanly (Package.py:330-356 runs the loop in the `else:`
//! branch). Only repo/binary candidates are validated, never vdb ones
//! (Portage passes `dep_eapi=None, dep_valid_flag=None` for installed
//! packages); binaries skip the built-atom loop (`if not self.built`).

use crate::*;
use portage_dep::{
    Atom, AtomEapi, AtomErrorKind, AtomOptions, atom_to_string, check_conditional_flags,
    parse_atom_with,
};
use portage_use_reduce::{TokenError, TokenErrorCategory, use_reduce_flat_validated};

/// Which `_invalid` bucket a dep failure files under.
/// Portage: lib/_emerge/Package.py:534-557 (`_metadata_exception`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DepErrorCategory {
    Syntax,
    IuseMissing,
    EapiIncompatible,
}

/// One dep string's first failure: Portage's `InvalidDependString` text for
/// the token, filed under its category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DepError {
    pub category: DepErrorCategory,
    pub text: String,
}

/// One dep string, like `use_reduce(matchall=True, flat=True,
/// token_class=Atom, is_valid_flag=…)`: the parsed atoms, or Portage's error
/// text for the first failure. `eapi` is the owning package's EAPI;
/// `valid_flag` is its `is_valid_flag` (the implicit IUSE set).
/// Portage: lib/portage/dep/__init__.py, `use_reduce` + `Atom.__init__`.
pub(crate) fn validate_dep_string(
    dep: &str,
    eapi: &str,
    valid_flag: &dyn Fn(&str) -> bool,
) -> Result<Vec<Atom>, DepError> {
    // An EAPI Portage itself cannot parse has no validation behaviour
    // (eapi.py:224-229 raises there); report it as the first token's
    // syntax failure with the S1 cut text.
    let atom_eapi = AtomEapi::parse(Some(eapi)).map_err(|_| DepError {
        category: DepErrorCategory::Syntax,
        text: format!("Invalid atom (invalid EAPI '{eapi}'), token 1"),
    })?;
    let opts = AtomOptions {
        eapi: atom_eapi,
        ..Default::default()
    };
    let mut atoms = Vec::new();
    let mut hook = |token: &str, _pos: usize| -> Result<(), TokenError> {
        let atom = parse_atom_with(token, &opts, eapi).map_err(|err| TokenError {
            category: match err.kind {
                AtomErrorKind::Invalid => None,
                AtomErrorKind::IuseMissing => Some(TokenErrorCategory::IuseMissing),
                AtomErrorKind::EapiIncompatible => Some(TokenErrorCategory::EapiIncompatible),
            },
            message: err.message,
        })?;
        // Portage runs this check inside `Atom.__init__`, before the
        // strong-blocks result surfaces (dep:2125-2144); the order is not
        // observable (strong blocks are off only in EAPI 0/1, where use deps
        // already fail first), but the error itself is.
        check_conditional_flags(&atom, atom_eapi, valid_flag).map_err(|err| TokenError {
            category: Some(TokenErrorCategory::IuseMissing),
            message: err.message,
        })?;
        atoms.push(atom);
        Ok(())
    };
    let tokens: Vec<String> = dep.split_whitespace().map(String::from).collect();
    use_reduce_flat_validated(&tokens, valid_flag, &mut hook).map_err(|err| match err {
        portage_use_reduce::Error::Categorized { message, category } => DepError {
            category: match category {
                TokenErrorCategory::IuseMissing => DepErrorCategory::IuseMissing,
                TokenErrorCategory::EapiIncompatible => DepErrorCategory::EapiIncompatible,
            },
            text: message,
        },
        other => DepError {
            category: DepErrorCategory::Syntax,
            text: other.to_string(),
        },
    })?;
    Ok(atoms)
}

/// Portage `Package._validate_deps` + `_metadata_exception` for a repo/binary
/// candidate, rendered `invalid: <KEY>: <text>`, grouped by category in
/// first-insertion order. `category`/`package` name the owner for the metadata
/// re-read (ebuilds) and the messages.
pub(crate) fn invalid_dep_reasons(
    candidate: &Candidate,
    category: &str,
    package: &str,
    config: &portage_profile::Config,
) -> Vec<String> {
    let ebuild_map;
    let (deps, eapi): (&HashMap<String, String>, &str) = match candidate.source {
        CandidateSource::Ebuild => {
            let stem = format!("{package}-{}", candidate.version);
            let Ok(map) = repo_aux_metadata(&candidate.repo_location, category, &stem) else {
                return Vec::new();
            };
            ebuild_map = map;
            (&ebuild_map, candidate.eapi.as_str())
        }
        CandidateSource::Binary => (&candidate.binary_deps, candidate.eapi.as_str()),
    };
    let valid = implicit_iuse_set(&candidate.iuse, config);
    // Buckets in first-insertion order: (category, reasons).
    let mut buckets: Vec<(DepErrorCategory, Vec<String>)> = Vec::new();
    let mut push = |category: DepErrorCategory, reason: String| match buckets
        .iter_mut()
        .find(|(c, _)| *c == category)
    {
        Some((_, reasons)) => reasons.push(reason),
        None => buckets.push((category, vec![reason])),
    };
    // Portage's `_dep_keys` order (Package.py:93).
    for key in ["BDEPEND", "DEPEND", "IDEPEND", "PDEPEND", "RDEPEND"] {
        let Some(dep) = deps.get(key) else {
            continue;
        };
        if dep.trim().is_empty() {
            continue;
        }
        match validate_dep_string(dep, eapi, &|flag| valid.contains(flag)) {
            Err(err) => push(err.category, format!("invalid: {key}: {}", err.text)),
            Ok(atoms) => {
                // Unbuilt candidates only (Package.py:347-356
                // `if not self.built`): one syntax reason per built atom.
                if candidate.source != CandidateSource::Binary {
                    for atom in &atoms {
                        if atom.slot_operator == Some(portage_dep::SlotOperator::Equals)
                            && atom.sub_slot.is_some()
                        {
                            push(
                                DepErrorCategory::Syntax,
                                format!(
                                    "invalid: {key}: Improper context for slot-operator \
                                     \"built\" atom syntax: {}",
                                    atom_to_string(atom)
                                ),
                            );
                        }
                    }
                }
            }
        }
    }
    buckets
        .into_iter()
        .flat_map(|(_, reasons)| reasons)
        .collect()
}
