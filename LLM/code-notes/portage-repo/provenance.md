# crates/portage-repo/src/provenance.rs
Where a candidate's mask or keyword decision comes from.

## struct VisibilityProvenance
- Portuale's own `--json` "state-change trace" (not a port of any Portage `emerge` output). Same "duplicate a small, stable chunk of `is_visible` rather than thread a reason out of its own filtering fast path" precedent `keyword_masked_only` already set.

## fn visibility_provenance
- An invisible candidate would just report "no entry helped," which isn't a useful trace of anything.

## fn keyword_provenance
- Checks the global set alone by calling `keywords_accepted` with no package entries at all -- reuses its exact matching logic rather than a second implementation. Walks `package_accept_keywords` in the same least-to-most-specific order `specificity_ordered_flags` applies them in, accumulating onto a copy of the global set exactly like `specificity_ordered_flags` does. A less-specific entry earlier in the accumulation may already have been enough, e.g. a `*` wildcard reached before a slot-scoped `~arch` grant. `None` if even every matching entry together somehow isn't enough -- shouldn't happen for a candidate already confirmed `is_visible`, but a safe fallback rather than a panic.

## fn keyword_mask_marker
- Portage `isHardMasked` / `_getRawMaskAtom` deliberately ignores `package.unmask`; Portage `get_keyword_mask`'s `missing` empty = globally accepted; Portage `keyword.lstrip("~") in global_accept_keywords` -> `"unstable"`; Portage `"missing"`. Deliberately narrower than Portage's `getRawMissingKeywords`: the split is decided straight off the candidate's own `KEYWORDS` tokens rather than reconstructing the exact `missing` list. Present at plain `-p` and `-pv` alike (see `GraphEntry::keyword_mask`).
