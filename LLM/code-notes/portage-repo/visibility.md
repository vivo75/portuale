# crates/portage-repo/src/visibility.rs
Keyword, mask and license visibility of a candidate.

## fn metadata_key_accepted
- Portage `_getMissingProperties`/`_getMissingRestrict`, ported as a bool. Unlike `LICENSE` (which needs `||`-group structure -- see `has_masked_license`'s doc), `PROPERTIES`/`RESTRICT` have no any-of semantics: Portage config.py's own comment says it plainly, "ACCEPT_PROPERTIES works like ACCEPT_LICENSE, without groups" -- so this reuses `use_reduce_flat` directly instead of the bespoke `LicenseNode` tree (no `||`-structure to lose).
- Memo body runs `use_flags_if_conditional` (itself memoised), `resolve_accept_tokens`, `all_mentioned_tokens` (a `MatchMode::All` use-reduce), `resolve_acceptable_tokens` and `use_reduce_flat`; it was the last remaining hot function of that shape after #102-#108 (23.30% of the reference workload with children in S0's re-profile). Pure in its arguments; the `RUMF_CACHE` shape (backlog #104).

## enum MetadataKey
- `PROPERTIES` uses `accept_properties`/`package_properties`, `RESTRICT` uses `accept_restrict`/`package_accept_restrict`. Every call site passes exactly one of the two pairs (audited for #110).

## fn accept_lists_fingerprint
- The `MKA_CACHE` key hashes the accept lists because `use_context_fingerprint` covers only USE-relevant fields; the crate's masking unit tests build exactly such differing pairs in one process -- without this they would collide and become test-order dependent. Production has one config per process; hashed fully (not sampled) to keep the key exact.

## fn is_visible_fingerprint
- Backlog #114: the frozen visibility base (`portage_profile::is_visible_base_fingerprint`: masks, accept lists, `package.accept_keywords`, `license_groups`, plus the USE-context base) and the live `autounmask_use`, which the visibility check reaches through `use_flags_if_conditional`. A hand-built config without a base falls back to the live digest.
- Backlog #121: same frozen-base `debug_assert_eq!` as `use_context_fingerprint`.

## fn is_visible
- Portage `Package.py`'s `_masks` dict collects `package.mask`, `LICENSE`, `PROPERTIES`, and `RESTRICT` as four independent masking reasons the same way.
- Memo: the reference workload calls it ~40k times against 1,503 distinct inputs (backlog #114). Backlog #122: the literal `candidate_str` bounds any 64-bit digest collision to same-candidate inputs (`EUF_CACHE`'s shape).
- Backlog #153: one version is one ebuild is one DEPEND for ebuild candidates, so nothing extra is needed there.

## fn is_visible_uncached
- Backlog #153: Portage masks a candidate whose own dep conditionals reference flags outside its effective IUSE (`invalid`), after every `_getmaskingstatus` reason -- so this check sits last here too.

## fn visible_with_relax
- Portage `_pkg_visibility_check(pkg, autounmask_level)`. `relax_license` / `relax_keywords` / `relax_masks` mirror `_AutounmaskLevel`'s `allow_license_changes` / `allow_unstable_keywords` / `allow_unmasks`. The single-flag `*_masked_only` fns are the "and only this category is wrong" special cases (kept for their other callers). `resolve_pretend` walks the `_autounmask_levels` sequence, re-scanning every version at each level.
- Backlog #153: enforced at every autounmask level exactly like the unrelaxed `is_visible`.

## fn keyword_masked_only
- Portage `--autounmask-keep-keywords=n`; see `resolve_pretend_graph`'s doc for the full on/off default-resolution logic portuale ported.
- Duplicates `is_visible`'s body rather than refactoring it to return a reason enum -- Portage's own `_get_masking_status` is considerably more elaborate (distinguishing package.mask/license/keyword/REQUIRED_USE/etc. reasons, each with its own "unmask hint"), and portuale only needs the single "keywords, and only keywords" question for its deliberately narrow v1 (see `resolve_pretend_graph`'s doc for the full scope writeup, including what's deliberately still out: package.mask/license/USE suggestions, Portage's exact suggested-atom syntax and dependency-chain-comment formatting).

## fn mask_masked_only
- Portage keeps masks by default (`autounmask_keep_masks` defaults `True`); only `--autounmask-keep-masks=n` unmasks. Portuale's v1 doesn't reproduce Portage's `# <filename>:` + masking-comment lines (the `package_mask` list carries no source-file / comment provenance), only the `#required by` dep chain + `=<cpv>`.

## fn suggested_keyword
- Deliberately simpler than Portage's own `_get_masking_status` (which picks between the unstable (`~arch`) form, a different arch entirely, or `**`, based on exactly what's already accepted) -- see `keyword_masked_only`'s doc for the shared scope writeup. `None` only if `KEYWORDS` is empty or every token is `-`-prefixed (unusual, but not impossible).

## fn suggested_keyword_candidate
- `None` if `category/package` isn't listable at all, or no candidate is masked by `KEYWORDS` alone. A top-level atom's own fatal `NoVisibleCandidate` turns it into part of an `Err` message; a dependency's own attaches it to that `GraphEntry` instead (see `GraphEntry::keyword_suggestion`'s doc). Unlike `is_visible`/`keyword_masked_only`'s deliberate duplication (which trade off different bools), these two calls want the exact same "best near-miss" computation, so factoring it out is right here.

## fn suggested_license_candidate
- Paired with its space-joined sorted missing-license names. `None` if none is license-masked-only.

## fn use_masked_only
- Portage `--autounmask-use`'s v1 slice. Unlike `keyword_masked_only`, which explicitly skips the keywords check, this one requires it. KEYWORDS and USE-deps are two genuinely independent rejection reasons; Portage's "only suggest a change that would actually fix it" spirit (already established at `keyword_masked_only`).

## fn keywords_accepted
- Portage `KeywordsManager.getMissingKeywords`/`_getEgroups` (`lib/portage/package/ebuild/_config/KeywordsManager.py`). Portage shares this exact matching logic between `isStable`/`getMissingKeywords` and visibility checking, just against a different input keyword set, not a separate algorithm.
- History: the previous port just unioned everything a matching entry ever mentioned and ignored any `-` prefix. `"**"` used to be a separate unconditional-accept pre-scan that ignored fold order entirely; now it folds like any other token (removable by a later `-*`/`-**`), while its presence in the final set still means "accept any KEYWORDS state, even empty" (Portage's `"**" in pgroups` rule).
- Previously unhandled: a plain keyword name would sit in the accepted set as an inert string that can never equal a Portage `KEYWORDS` entry via `apply_incremental`.
- Per-keyword loop: Portage `_getMissingKeywords`'s loop (`KeywordsManager.py`, lines ~273-300) -- direct match short-circuits (`match = True; break`); a `-`-prefixed keyword is excluded from every branch of Portage's elif chain; the fallback matches Portage `(hastesting and "~*" in pgroups) or (hasstable and "*" in pgroups)` exactly (the third disjunct, `"**" in pgroups`, is the unconditional check above).

## fn is_stable
- Portage `KeywordsManager.isStable` (`lib/portage/package/ebuild/_config/KeywordsManager.py`): not a raw "no `~` prefix" check. Portage's own comment: "this guarantees that the effective use.force/mask settings for a particular ebuild do not change when that ebuild is stabilized."

## fn keyword_masked_only / mask_masked_only / license_masked_only
- Backlog #153: an invalid package is never visible-except-X.
