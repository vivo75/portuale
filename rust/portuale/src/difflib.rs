// A faithful, minimal port of CPython's `difflib.SequenceMatcher.ratio()`
// (`Lib/difflib.py`), just enough for `emerge --search`'s own real fuzzy
// matching (`_emerge/search.py`: `difflib.SequenceMatcher().ratio() >=
// cutoff`). Real portage gates on all three of `real_quick_ratio()`,
// `quick_ratio()` and `ratio()` -- but the first two are upper bounds on
// the third (`ratio <= quick_ratio <= real_quick_ratio` always), so
// `ratio() >= cutoff` alone is the exact combined condition.
//
// Scope match with CPython: `isjunk` is always `None` (search.py never
// passes one), so there is no junk set at all; and `autojunk` only fires
// for a `b` of length >= 200, which a search key / package-name part
// never reaches -- so both the junk sets and the autojunk pruning are
// omitted here. Everything else (`find_longest_match`'s `j2len` DP, the
// earliest-match tie-break, the recursive `get_matching_blocks` split,
// `2.0 * matches / total`) is ported literally so a Rust search produces
// byte-identical results to `emerge_pretend_reference.py`'s own
// `difflib`-backed one, verified by the shared contract suite.

use std::collections::HashMap;

/// Real `SequenceMatcher(None, a, b).ratio()`: `2.0 * M / (len(a) +
/// len(b))` where `M` is the total size of the non-overlapping matching
/// blocks `get_matching_blocks()` finds. Operates on `char`s (real
/// `difflib` compares Python string elements, i.e. code points).
pub fn ratio(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let total = a.len() + b.len();
    if total == 0 {
        // Real `_calculate_ratio(0, 0)` -> `1.0`.
        return 1.0;
    }
    // Real `__chain_b`: map each element of `b` to its ascending list of
    // indices.
    let mut b2j: HashMap<char, Vec<usize>> = HashMap::new();
    for (j, &c) in b.iter().enumerate() {
        b2j.entry(c).or_default().push(j);
    }
    let matches = matching_char_count(&a, &b, &b2j, 0, a.len(), 0, b.len());
    2.0 * matches as f64 / total as f64
}

/// Real `difflib.get_close_matches(word, possibilities, n, cutoff)`: the
/// up-to-`n` entries of `possibilities` whose `ratio()` against `word`
/// reaches `cutoff`, best first. Real `get_close_matches` sets `word` as
/// seq2 and each possibility as seq1, so the score is `ratio(possibility,
/// word)`; its `_nlargest(n, [(score, x), ...])` orders by `(score, x)`
/// descending, so a score tie breaks toward the lexicographically larger
/// string -- reproduced here. (Real portage calls this with the default
/// `n=3`, `cutoff=0.6` via `_similar_name_search`.)
pub fn get_close_matches(
    word: &str,
    possibilities: &[String],
    n: usize,
    cutoff: f64,
) -> Vec<String> {
    let mut scored: Vec<(f64, &str)> = possibilities
        .iter()
        .map(|p| (ratio(p, word), p.as_str()))
        .filter(|(r, _)| *r >= cutoff)
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| b.1.cmp(a.1)));
    scored
        .into_iter()
        .take(n)
        .map(|(_, s)| s.to_string())
        .collect()
}

/// Real `get_matching_blocks()`'s recursion, summed: `find_longest_match`
/// on the window, then recurse into the slice before it and the slice
/// after it. Block *order* doesn't matter for the sum, so the explicit
/// LIFO queue is just a recursion here.
fn matching_char_count(
    a: &[char],
    b: &[char],
    b2j: &HashMap<char, Vec<usize>>,
    alo: usize,
    ahi: usize,
    blo: usize,
    bhi: usize,
) -> usize {
    let (i, j, k) = find_longest_match(a, b, b2j, alo, ahi, blo, bhi);
    if k == 0 {
        return 0;
    }
    let mut total = k;
    if alo < i && blo < j {
        total += matching_char_count(a, b, b2j, alo, i, blo, j);
    }
    if i + k < ahi && j + k < bhi {
        total += matching_char_count(a, b, b2j, i + k, ahi, j + k, bhi);
    }
    total
}

/// Real `find_longest_match(alo, ahi, blo, bhi)` with no junk: the
/// `j2len` dynamic-programming scan for the longest common contiguous
/// run, ties broken toward the earliest `i` (then earliest `j`), then
/// extended maximally on both ends.
fn find_longest_match(
    a: &[char],
    b: &[char],
    b2j: &HashMap<char, Vec<usize>>,
    alo: usize,
    ahi: usize,
    blo: usize,
    bhi: usize,
) -> (usize, usize, usize) {
    let (mut besti, mut bestj, mut bestsize) = (alo, blo, 0usize);
    let mut j2len: HashMap<usize, usize> = HashMap::new();
    for (i, ai) in a.iter().enumerate().take(ahi).skip(alo) {
        let mut newj2len: HashMap<usize, usize> = HashMap::new();
        if let Some(js) = b2j.get(ai) {
            for &j in js {
                if j < blo {
                    continue;
                }
                if j >= bhi {
                    break;
                }
                let prev = if j > 0 {
                    j2len.get(&(j - 1)).copied()
                } else {
                    None
                };
                let k = prev.unwrap_or(0) + 1;
                newj2len.insert(j, k);
                if k > bestsize {
                    besti = i + 1 - k;
                    bestj = j + 1 - k;
                    bestsize = k;
                }
            }
        }
        j2len = newj2len;
    }
    // Real: extend the match on both sides for as long as the elements
    // stay equal (the junk-aware second pair of loops never fires here --
    // there is no junk).
    while besti > alo && bestj > blo && a[besti - 1] == b[bestj - 1] {
        besti -= 1;
        bestj -= 1;
        bestsize += 1;
    }
    while besti + bestsize < ahi
        && bestj + bestsize < bhi
        && a[besti + bestsize] == b[bestj + bestsize]
    {
        bestsize += 1;
    }
    (besti, bestj, bestsize)
}

#[cfg(test)]
mod tests {
    use super::{get_close_matches, ratio};

    // Values below were produced by CPython's own
    // `difflib.SequenceMatcher(None, a, b).ratio()`.
    fn approx(x: f64, y: f64) {
        assert!((x - y).abs() < 1e-9, "{x} != {y}");
    }

    #[test]
    fn get_close_matches_matches_cpython() {
        let words: Vec<String> = ["ape", "apple", "peach", "puppy"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        // CPython: difflib.get_close_matches("appel", words) -> ['apple', 'ape']
        assert_eq!(
            get_close_matches("appel", &words, 3, 0.6),
            vec!["apple", "ape"]
        );
        // difflib.get_close_matches("dev-libs/newpgk",
        //   ["dev-libs/newpkg", "dev-libs/oldpkg"])
        //   -> ['dev-libs/newpkg', 'dev-libs/oldpkg'] (both clear 0.6;
        //   newpkg scores higher).
        let cps: Vec<String> = ["dev-libs/oldpkg", "dev-libs/newpkg"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            get_close_matches("dev-libs/newpgk", &cps, 3, 0.6),
            vec!["dev-libs/newpkg", "dev-libs/oldpkg"]
        );
        assert!(get_close_matches("zzzzzz", &words, 3, 0.6).is_empty());
    }

    #[test]
    fn matches_cpython_reference_values() {
        approx(ratio("newpkg", "newpkg"), 1.0);
        approx(ratio("", ""), 1.0);
        approx(ratio("abcd", "bcde"), 0.75);
        approx(ratio("useflagpkg", "useflag"), 0.8235294117647058);
        approx(ratio("dev-libs/newpkg", "newpkg"), 0.5714285714285714);
        approx(ratio("kitten", "sitting"), 0.6153846153846154);
        approx(ratio("newpkg", "nomatchanywhere"), 0.19047619047619047);
        approx(ratio("a", "b"), 0.0);
    }

    #[test]
    fn is_symmetric_in_length_but_order_sensitive_like_cpython() {
        // difflib.ratio() is NOT symmetric in general; these particular
        // pairs happen to be, and match CPython.
        approx(ratio("abc", "cba"), ratio("cba", "abc"));
    }
}
