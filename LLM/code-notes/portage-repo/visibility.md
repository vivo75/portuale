# crates/portage-repo/src/visibility.rs
Keyword, mask and license visibility of a candidate.

## fn metadata_key_accepted
- Portage `_getMissingProperties`/`_getMissingRestrict`, ported as a bool. Unlike `LICENSE` (which needs `||`-group structure -- see `has_masked_license`'s doc), `PROPERTIES`/`RESTRICT` have no any-of semantics: Portage config.py's own comment says it plainly, "ACCEPT_PROPERTIES works like ACCEPT_LICENSE, without groups" -- so this reuses `use_reduce_flat` directly instead of the bespoke `LicenseNode` tree (no `||`-structure to lose).
- Memo body runs `use_flags_if_conditional` (itself memoised), `resolve_accept_tokens`, `all_mentioned_tokens` (a `MatchMode::All` use-reduce), `resolve_acceptable_tokens` and `use_reduce_flat`; it was the last remaining hot function of that shape after #102-#108 (23.30% of the reference workload with children in S0's re-profile). Pure in its arguments; the `RUMF_CACHE` shape (backlog #104).
- `iuse`/`keywords` go in the memo key for the same reason `EUF_CACHE` hashes them: `candidate_str` does not encode them, and a binary candidate from a `Packages` index shares it with the ebuild for the same CPV while carrying its build-time IUSE/KEYWORDS.

## enum MetadataKey
- `PROPERTIES` uses `accept_properties`/`package_properties`, `RESTRICT` uses `accept_restrict`/`package_accept_restrict`. Every call site passes exactly one of the two pairs (audited for #110).

## fn accept_lists_fingerprint
- The `MKA_CACHE` key hashes the accept lists because `use_context_fingerprint` covers only USE-relevant fields; the crate's masking unit tests build exactly such differing pairs in one process -- without this they would collide and become test-order dependent. Production has one config per process; hashed fully (not sampled) to keep the key exact.

## fn is_visible_fingerprint
- Backlog #114: the frozen visibility base (`portage_profile::is_visible_base_fingerprint`: masks, accept lists, `package.accept_keywords`, `license_groups`, plus the USE-context base) and the live `autounmask_use`, which the visibility check reaches through `use_flags_if_conditional`. A hand-built config without a base falls back to the live digest.
- Backlog #121: same frozen-base `debug_assert_eq!` as `use_context_fingerprint`.

## fn is_visible
- A candidate is visible if it isn't masked (matches a `package.mask` entry and no `package.unmask` entry), its KEYWORDS intersect the accepted set -- the global `config.accept_keywords`, plus any extra keywords contributed by a matching `package.accept_keywords` entry, with a `"**"` token in such an entry meaning "accept unconditionally" for matching candidates (even ones with empty/no KEYWORDS) -- and its own declared `LICENSE`/`PROPERTIES`/`RESTRICT` are all fully accepted (see `license_accepted`/`metadata_key_accepted`).
- Portage `Package.py`'s `_masks` dict collects `package.mask`, `LICENSE`, `PROPERTIES`, and `RESTRICT` as four independent masking reasons the same way.
- Memo: the reference workload calls it ~40k times against 1,503 distinct inputs (backlog #114). Backlog #122: the literal `candidate_str` bounds any 64-bit digest collision to same-candidate inputs (`EUF_CACHE`'s shape).
- Backlog #153: one version is one ebuild is one DEPEND for ebuild candidates, so nothing extra is needed there.
- S4 (#341): the memo key also hashes `source`, `repo_location`, `eapi` and the sorted `binary_deps` -- the `invalid` verdict reads the instance's own dep keys under its own EAPI, so without them a masked binary poisons its ebuild twin's lookup (found via S0 `F-binary-rewritten`: same version/slot/repo/IUSE, different `RDEPEND`).

## fn is_visible_uncached
- Backlog #153: Portage masks a candidate whose own dep conditionals reference flags outside its effective IUSE (`invalid`), after every `_getmaskingstatus` reason -- so this check sits last here too.
- S9 (#341): the `package.mask`/`unmask` check passes the candidate's binary build-id (`any_config_entry_matches_with_build_id`), so an `=`+build-id mask line masks only the equal instance (dep:2994-3008). Same in `visible_with_relax` and the three `*_masked_only` fns below, which duplicate this check.

## fn visible_with_relax
- Portage `_pkg_visibility_check(pkg, autounmask_level)`. `relax_license` / `relax_keywords` / `relax_masks` mirror `_AutounmaskLevel`'s `allow_license_changes` / `allow_unstable_keywords` / `allow_unmasks`. The single-flag `*_masked_only` fns are the "and *only* this category is wrong" special cases (kept for their other callers). `resolve_pretend` walks the `_autounmask_levels` sequence, re-scanning every version at each level.
- Backlog #153: enforced at every autounmask level exactly like the unrelaxed `is_visible`.

## fn keyword_masked_only
- Portage `--autounmask-keep-keywords=n`; see `resolve_pretend_graph`'s doc for the full on/off default-resolution logic portuale ported.
- Duplicates `is_visible`'s body rather than refactoring it to return a reason enum -- Portage's own `_get_masking_status` is considerably more elaborate (distinguishing package.mask/license/keyword/REQUIRED_USE/etc. reasons, each with its own "unmask hint"), and portuale only needs the single "keywords, and only keywords" question for its deliberately narrow v1 (see `resolve_pretend_graph`'s doc for the full scope writeup, including what's deliberately still out: package.mask/license/USE suggestions, Portage's exact suggested-atom syntax and dependency-chain-comment formatting).

## fn mask_masked_only
- Portage keeps masks by default (`autounmask_keep_masks` defaults `True`); only `--autounmask-keep-masks=n` unmasks. Portuale's v1 doesn't reproduce Portage's `# <filename>:` + masking-comment lines (the `package_mask` list carries no source-file / comment provenance), only the `#required by` dep chain + `=<cpv>`.

## fn suggested_keyword
- The keyword portuale's own `--autounmask` v1 would suggest adding to `package.accept_keywords` for `candidate` -- the first of its own (non-`-`-prefixed; a `-`-prefixed KEYWORDS token means "explicitly unsupported here", never a valid suggestion) `KEYWORDS` tokens.
- Deliberately simpler than Portage's own `_get_masking_status` (which picks between the unstable (`~arch`) form, a different arch entirely, or `**`, based on exactly what's already accepted) -- see `keyword_masked_only`'s doc for the shared scope writeup. `None` only if `KEYWORDS` is empty or every token is `-`-prefixed (unusual, but not impossible).

## fn suggested_keyword_candidate
- The best `--autounmask` keyword suggestion for `category/package`, if any: among every candidate masked by `KEYWORDS` alone (`keyword_masked_only`), the highest-versioned one (repo priority breaking a tie), paired with its own `suggested_keyword`. Shared by both call sites that need this exact "what would I suggest here" question -- a top-level atom's own fatal `NoVisibleCandidate`, and a *dependency's* own `NoVisibleCandidate`.
- `None` if `category/package` isn't listable at all, or no candidate is masked by `KEYWORDS` alone. A top-level atom's own fatal `NoVisibleCandidate` turns it into part of an `Err` message; a dependency's own attaches it to that `GraphEntry` instead (see `GraphEntry::keyword_suggestion`'s doc). Unlike `is_visible`/`keyword_masked_only`'s deliberate duplication (which trade off different bools), these two calls want the exact same "best near-miss" computation, so factoring it out is right here.

## fn suggested_license_candidate
- Paired with its space-joined sorted missing-license names. `None` if none is license-masked-only.

## fn use_masked_only
- Portage `--autounmask-use`'s v1 slice. Unlike `keyword_masked_only`, which explicitly skips the keywords check, this one requires it. KEYWORDS and USE-deps are two genuinely independent rejection reasons; Portage's "only suggest a change that would actually fix it" spirit (already established at `keyword_masked_only`).

## fn keywords_accepted
- Grounded against Portage `KeywordsManager.getMissingKeywords`/`_getEgroups` (`lib/portage/package/ebuild/_config/KeywordsManager.py`): a `package.accept_keywords` entry doesn't just *add* keywords on top of the global `ACCEPT_KEYWORDS` set -- Portage `_getEgroups` folds `-token`/`-*` removals too, over the *combined* list (global keywords first, then each matching entry's own tokens, in atom-specificity order), so a more-specific `package.accept_keywords` line can revoke a keyword the global set already granted, not just add new ones. Ported via `specificity_ordered_flags` (already established for `package.use.mask`/`.force`'s own identical "specificity-ordered incremental fold" shape) seeded with `accept_keywords` itself, rather than the previous, incorrect "union everything a matching entry ever mentions, ignore any `-` prefix" accumulation. `"**"` is folded in exactly like any other token now (removable by a later `-*`/`-**`), rather than a separate unconditional-accept pre-scan that ignored fold order entirely -- once folded, its presence in the final accepted set still means "accept any KEYWORDS state, even empty," the same Portage `"**" in pgroups` unconditional-match rule. A bare `package.accept_keywords` atom with no keyword list at all no longer reaches this function empty: `resolve_config` (`portage-profile`) already substitutes Portage `accept_keywords_defaults`'s own implicit meaning -- the `~`-prefixed unstable form of every currently-accepted keyword -- before this function ever sees it, so it folds in through `specificity_ordered_flags` exactly like any other entry's own explicit tokens (see `parse_package_accept_keywords_lines`'s doc, portage-profile).
- A literal `"*"`/`"~*"` token in the accepted set means "accept any stable keyword"/"accept any testing keyword" respectively -- distinct from `"**"` (accept even an *empty* `KEYWORDS`) and from a plain keyword name, which `apply_incremental` would otherwise insert as an inert string that can never equal a Portage `KEYWORDS` entry.
- Per-keyword loop (Portage `_getMissingKeywords`): each of the candidate's own `keywords` is checked for a direct match first (short-circuiting immediately, same as Portage `match = True; break`); a `-`-prefixed one (explicit "not supported here", distinct from simply absent) never matches and is excluded from classification entirely, matching Portage's own elif chain; anything else is classified stable or testing (`~`-prefixed) for the final fallback -- `"*"` grants acceptance if *any* declared keyword was stable-classified, `"~*"` if any was testing-classified, matching Portage `(hastesting and "~*" in pgroups) or (hasstable and "*" in pgroups)` exactly (the third real disjunct, `"**" in pgroups`, is the unconditional check handled earlier). A direct match (the accepted set literally contains this exact keyword) wins immediately; otherwise the keyword only contributes to the has_stable/has_testing tally used by the `"*"`/`"~*"` fallback.
- Portage `KeywordsManager.getMissingKeywords`/`_getEgroups` (`lib/portage/package/ebuild/_config/KeywordsManager.py`). Portage shares this exact matching logic between `isStable`/`getMissingKeywords` and visibility checking, just against a different input keyword set, not a separate algorithm.
- History: the previous port just unioned everything a matching entry ever mentioned and ignored any `-` prefix. `"**"` used to be a separate unconditional-accept pre-scan that ignored fold order entirely; now it folds like any other token (removable by a later `-*`/`-**`), while its presence in the final set still means "accept any KEYWORDS state, even empty" (Portage's `"**" in pgroups` rule).
- Previously unhandled: a plain keyword name would sit in the accepted set as an inert string that can never equal a Portage `KEYWORDS` entry via `apply_incremental`.
- Per-keyword loop: Portage `_getMissingKeywords`'s loop (`KeywordsManager.py`, lines ~273-300) -- direct match short-circuits (`match = True; break`); a `-`-prefixed keyword is excluded from every branch of Portage's elif chain; the fallback matches Portage `(hastesting and "~*" in pgroups) or (hasstable and "*" in pgroups)` exactly (the third disjunct, `"**" in pgroups`, is the unconditional check above).

- S3 (#346): new `build_id` field; the folded `package.accept_keywords` entries match with it, so an `=`+build-id line accepts only the equal instance (Portage `_pkg_str` comparison, dep:2994-3008).
## fn is_stable
- Portage `KeywordsManager.isStable` (`lib/portage/package/ebuild/_config/KeywordsManager.py`): not a raw "no `~` prefix" check. Portage's own comment: "this guarantees that the effective use.force/mask settings for a particular ebuild do not change when that ebuild is stabilized."

- S3 (#346): new `build_id` field, passed through to `keywords_accepted` (the unstabilized-keyword probe matches the same instances the visibility check sees).
## fn keyword_masked_only / mask_masked_only / license_masked_only
- Backlog #153: an invalid package is never visible-except-X.

## fn is_visible_uncached
- #348 S5: an unsupported `candidate.eapi` masks outright, before every other check (ebuild and binary alike — a binpkg with unknown index `EAPI` never merges). Portage: `getmaskingstatus.py` (EAPI gate first).

## fn visible_with_relax
- #348 S5: the EAPI mask never relaxes (Portage offers no unmask for it).

## fn keyword_masked_only / mask_masked_only / license_masked_only
- #348 S5: an EAPI-masked candidate is never "masked by X alone" (its reason is the lone `EAPI <eapi>`); `use_masked_only` needs no guard (it goes through `is_visible`).
