# crates/portage-repo/src/provenance.rs
Where a candidate's mask or keyword decision comes from.

## struct VisibilityProvenance
- Portuale's own `--json` "state-change trace" (not a port of any Portage `emerge` output). Same "duplicate a small, stable chunk of `is_visible` rather than thread a reason out of its own filtering fast path" precedent `keyword_masked_only` already set.
- Computed once per finally-chosen candidate in `resolve_pretend_graph`, not in `is_visible`'s own hot filtering loop over every candidate.

## fn visibility_provenance
- An invisible candidate would just report "no entry helped," which isn't a useful trace of anything.
- S9 (#341): the `package.mask`/`unmask` entry search passes the candidate's binary build-id, so an `=`+build-id entry provenances to the equal instance.

## fn keyword_provenance
- Checks the global set alone by calling `keywords_accepted` with no package entries at all -- reuses its exact matching logic rather than a second implementation. Walks `package_accept_keywords` in the same least-to-most-specific order `specificity_ordered_flags` applies them in, accumulating onto a copy of the global set exactly like `specificity_ordered_flags` does. A less-specific entry earlier in the accumulation may already have been enough, e.g. a `*` wildcard reached before a slot-scoped `~arch` grant. `None` if even every matching entry together somehow isn't enough -- shouldn't happen for a candidate already confirmed `is_visible`, but a safe fallback rather than a panic.

- S3 (#346): new `build_id` field; the load-bearing-entry walk matches with it, so an `=`+build-id accept line provenances to the equal instance.
## fn keyword_mask_marker
- Portage `isHardMasked` / `_getRawMaskAtom` deliberately ignores `package.unmask`; Portage `get_keyword_mask`'s `missing` empty = globally accepted; Portage `keyword.lstrip("~") in global_accept_keywords` -> `"unstable"`; Portage `"missing"`. Deliberately narrower than Portage's `getRawMissingKeywords`: the split is decided straight off the candidate's own `KEYWORDS` tokens rather than reconstructing the exact `missing` list. Present at plain `-p` and `-pv` alike (see `GraphEntry::keyword_mask`).
- Portage `output.py::gen_mask_str` + `Package.get_keyword_mask` / `isHardMasked`, for the one-character bracket-mask column. `Some('#')` -- Portage `if pkg.isHardMasked(): "#"`, checked first: `mask_entry` was set (some `package.mask` matched), even though a `package.unmask` went on to cancel it. `Some('*')` -- not globally accepted and no such `~<our-arch>` keyword: visible only via `**` or a different arch's keyword (Portage `"missing"`).
- #346 S2: `visibility_provenance` and `keyword_mask_marker` read the effective KEYWORDS (`effective_keywords`): a profile-added stable keyword counts as globally accepted (no `~` marker, S0 k02/k03).
