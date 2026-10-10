# crates/portuale/src/difflib.rs
Minimal `difflib.SequenceMatcher` port for `emerge --search` fuzzy matching.

## module
- A faithful, minimal port of CPython's `difflib.SequenceMatcher.ratio()` (`Lib/difflib.py`), just enough for `emerge --search`'s fuzzy matching (`_emerge/search.py`: `difflib.SequenceMatcher().ratio() >= cutoff`). Portage gates on all three of `real_quick_ratio()`, `quick_ratio()` and `ratio()`, but the first two are upper bounds on the third (`ratio <= quick_ratio <= real_quick_ratio` always), so `ratio() >= cutoff` alone is the exact combined condition.
- Scope match with CPython: `isjunk` is always `None` (search.py never passes one), so there is no junk set at all; `autojunk` only fires for a `b` of length >= 200, which a search key / package-name part never reaches, so both the junk sets and the autojunk pruning are omitted. Everything else (`find_longest_match`'s `j2len` DP, the earliest-match tie-break, the recursive `get_matching_blocks` split, `2.0 * matches / total`) is ported literally so a Rust search produces byte-identical results to Portage's `emerge --search` `difflib`-backed one.

## fn ratio
- Ports `SequenceMatcher(None, a, b).ratio()`: `2.0 * M / (len(a) + len(b))` where `M` is the total size of the non-overlapping matching blocks `get_matching_blocks()` finds. Operates on `char`s (CPython compares string elements, i.e. code points). Empty/empty returns 1.0 (CPython's `_calculate_ratio(0, 0)`). Chains `b` as `__chain_b` does.

## fn get_close_matches
- Ports `difflib.get_close_matches(word, possibilities, n, cutoff)`: sets `word` as seq2 and each possibility as seq1, so the score is `ratio(possibility, word)`; its `_nlargest(n, [(score, x), ...])` orders by `(score, x)` descending, so a score tie breaks toward the lexicographically larger string. Portage calls this with the default `n=3`, `cutoff=0.6` via `_similar_name_search`.

## tests
- Expected values come from CPython's own `difflib` (see the test bodies for the exact calls).
