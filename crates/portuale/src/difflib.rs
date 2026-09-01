// A minimal port of CPython's `difflib.SequenceMatcher.ratio()`, just
// enough for `emerge --search` fuzzy matching: `ratio() >= cutoff` is
// the exact combined condition for the three gated ratios, since
// `ratio <= quick_ratio <= real_quick_ratio` always.
//
// `isjunk` is always `None` and `autojunk` never fires for short search
// keys, so both are omitted. Everything else is ported literally for
// byte-identical results.
// Portage: _emerge/search.py; CPython: Lib/difflib.py

use std::collections::HashMap;

/// Arguments of [`matching_char_count`].
struct MatchingCharCountArgs<'a> {
    a: &'a [char],
    b: &'a [char],
    b2j: &'a HashMap<char, Vec<usize>>,
    alo: usize,
    ahi: usize,
    blo: usize,
    bhi: usize,
}

/// Arguments of [`find_longest_match`].
struct FindLongestMatchArgs<'a> {
    a: &'a [char],
    b: &'a [char],
    b2j: &'a HashMap<char, Vec<usize>>,
    alo: usize,
    ahi: usize,
    blo: usize,
    bhi: usize,
}
