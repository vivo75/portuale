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

/// `SequenceMatcher(None, a, b).ratio()`: `2.0 * M / (len(a) + len(b))`
/// where `M` is the total non-overlapping matching-block size.
/// Operates on `char`s (Python string elements).
pub fn ratio(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let total = a.len() + b.len();
    if total == 0 {
        // Real `_calculate_ratio(0, 0)` -> `1.0`.
        return 1.0;
    }
    // `__chain_b`: map each element of `b` to its ascending index list.
    let mut b2j: HashMap<char, Vec<usize>> = HashMap::new();
    for (j, &c) in b.iter().enumerate() {
        b2j.entry(c).or_default().push(j);
    }
    let matches = matching_char_count(MatchingCharCountArgs {
        a: &a,
        b: &b,
        b2j: &b2j,
        alo: 0,
        ahi: a.len(),
        blo: 0,
        bhi: b.len(),
    });
    2.0 * matches as f64 / total as f64
}

/// `get_close_matches(word, possibilities, n, cutoff)`: the up-to-`n`
/// entries reaching `cutoff`, best first. Score ties break toward the
/// lexicographically larger string.
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

/// `get_matching_blocks()`'s recursion, summed. Block order doesn't
/// matter for the sum, so the LIFO queue is just recursion here.
fn matching_char_count(args: MatchingCharCountArgs<'_>) -> usize {
    let MatchingCharCountArgs {
        a,
        b,
        b2j,
        alo,
        ahi,
        blo,
        bhi,
    } = args;
    let (i, j, k) = find_longest_match(FindLongestMatchArgs {
        a,
        b,
        b2j,
        alo,
        ahi,
        blo,
        bhi,
    });
    if k == 0 {
        return 0;
    }
    let mut total = k;
    if alo < i && blo < j {
        total += matching_char_count(MatchingCharCountArgs {
            a,
            b,
            b2j,
            alo,
            ahi: i,
            blo,
            bhi: j,
        });
    }
    if i + k < ahi && j + k < bhi {
        total += matching_char_count(MatchingCharCountArgs {
            a,
            b,
            b2j,
            alo: i + k,
            ahi,
            blo: j + k,
            bhi,
        });
    }
    total
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

/// `find_longest_match(alo, ahi, blo, bhi)` with no junk: the `j2len`
/// scan for the longest common run, ties toward the earliest `i`
/// (then earliest `j`), then extended maximally on both ends.
fn find_longest_match(args: FindLongestMatchArgs<'_>) -> (usize, usize, usize) {
    let FindLongestMatchArgs {
        a,
        b,
        b2j,
        alo,
        ahi,
        blo,
        bhi,
    } = args;
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
    // Extend the match while elements stay equal (the junk-aware loops
    // never fire: there is no junk).
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
    // `SequenceMatcher(None, a, b).ratio()`.
    fn approx(x: f64, y: f64) {
        assert!((x - y).abs() < 1e-9, "{x} != {y}");
    }

    #[test]
    fn get_close_matches_matches_cpython() {
        let words: Vec<String> = ["ape", "apple", "peach", "puppy"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        // `get_close_matches("appel", words)` -> `['apple', 'ape']`.
        assert_eq!(
            get_close_matches("appel", &words, 3, 0.6),
            vec!["apple", "ape"]
        );
        // Both clear 0.6; newpkg scores higher.
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
        // `ratio()` is NOT symmetric in general; these pairs happen to
        // be, and match CPython.
        approx(ratio("abc", "cba"), ratio("cba", "abc"));
    }
}
