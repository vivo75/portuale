// Rust port of the comparison semantics implemented by
// `lib/portage/versions.py` (functions `ververify` and `vercmp`) in the
// Python codebase. Ported by hand, structurally close to the Python
// original so it stays easy to diff against on future changes -- this is
// a portuale artifact, not yet idiomatic-first Rust; see docs/agent-context.md.
//
// Version components are parsed as `i128`, but any component too wide for
// `i128` is compared as an arbitrary-length decimal string instead of
// panicking. That mirrors the Python original's unbounded `int` exactly, so
// no width of numeric component can crash or miscompare a version.

use regex::Regex;
use std::cmp::Ordering;
use std::sync::OnceLock;

fn ver_regexp() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(\d+)((?:\.\d+)*)([a-z]?)((?:_(?:pre|p|beta|alpha|rc)\d*)*)(?:-r(\d+))?$")
            .unwrap()
    })
}

fn suffix_regexp() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(alpha|beta|rc|pre|p)(\d*)$").unwrap())
}

/// Sentinel for an implicit missing dotted-version component (e.g. the
/// missing third component of "1.0" compared against "1.0.0"), comparing
/// less than any explicit component so that "1.0.0" > "1.0".
///
/// A single numeric component is either an in-range `i128` (`Num`) or, when
/// the digit string is wider than `i128` can hold, a `BigNum` decimal
/// string compared by length-then-digits. Components are non-negative
/// (`\d+` in the grammar) and `BigNum` only ever arises from genuine
/// overflow (verified: Rust's `parse` accepts arbitrary leading zeros), so
/// any `BigNum` exceeds `i128::MAX` and therefore any `Num`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    ImplicitZero,
    Num(i128),
    BigNum(String),
}

fn cmp_bignum(a: &str, b: &str) -> Ordering {
    let a = a.trim_start_matches('0');
    let b = b.trim_start_matches('0');
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

impl Ord for Part {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Part::ImplicitZero, Part::ImplicitZero) => Ordering::Equal,
            (Part::ImplicitZero, _) => Ordering::Less,
            (_, Part::ImplicitZero) => Ordering::Greater,
            (Part::Num(a), Part::Num(b)) => a.cmp(b),
            (Part::BigNum(a), Part::BigNum(b)) => cmp_bignum(a, b),
            (Part::BigNum(_), Part::Num(_)) => Ordering::Greater,
            (Part::Num(_), Part::BigNum(_)) => Ordering::Less,
        }
    }
}

impl PartialOrd for Part {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn ververify(myver: &str) -> bool {
    ver_regexp().is_match(myver)
}

fn suffix_value(name: &str) -> i32 {
    match name {
        "pre" => -2,
        "p" => 0,
        "alpha" => -4,
        "beta" => -3,
        "rc" => -1,
        _ => unreachable!("invalid version suffix {name:?}"),
    }
}

fn parse_component(s: &str) -> Part {
    match s.parse::<i128>() {
        Ok(v) => Part::Num(v),
        Err(_) => Part::BigNum(s.to_string()),
    }
}

/// Revision and suffix numbers may be absent, in which case Python treats
/// the implicit value as zero (`int("")` would raise, so it fudges to 0).
fn parse_uint_or_zero(s: &str) -> Part {
    if s.is_empty() {
        Part::Num(0)
    } else {
        parse_component(s)
    }
}

/// Builds the paired dotted-component lists (the parts after the first
/// integer, e.g. the ".2.3" in "1.2.3"), applying Portage's implicit-zero
/// and leading-zero ("float-like") comparison rules. Must be built jointly
/// because both rules compare same-index components across both versions.
fn build_dotted_lists(dotted1: &str, dotted2: &str) -> (Vec<Part>, Vec<Part>) {
    let vlist1: Vec<&str> = if dotted1.is_empty() {
        Vec::new()
    } else {
        dotted1[1..].split('.').collect()
    };
    let vlist2: Vec<&str> = if dotted2.is_empty() {
        Vec::new()
    } else {
        dotted2[1..].split('.').collect()
    };

    let mut list1 = Vec::new();
    let mut list2 = Vec::new();
    for i in 0..vlist1.len().max(vlist2.len()) {
        let a = vlist1.get(i).copied().unwrap_or("");
        let b = vlist2.get(i).copied().unwrap_or("");
        if a.is_empty() {
            list1.push(Part::ImplicitZero);
            list2.push(parse_component(b));
        } else if b.is_empty() {
            list1.push(parse_component(a));
            list2.push(Part::ImplicitZero);
        } else if !a.starts_with('0') && !b.starts_with('0') {
            list1.push(parse_component(a));
            list2.push(parse_component(b));
        } else {
            // At least one side has a leading zero, so plain integer
            // comparison would be wrong (e.g. "01" vs "1"): pad both with
            // trailing zeros to the same width and compare as integers,
            // matching Python's float-like comparison.
            let width = a.len().max(b.len());
            list1.push(parse_component(&format!("{a:0<width$}")));
            list2.push(parse_component(&format!("{b:0<width$}")));
        }
    }
    (list1, list2)
}

fn cmp_parts(list1: &[Part], list2: &[Part]) -> Ordering {
    let max_len = list1.len().max(list2.len());
    for i in 0..max_len {
        match (list1.get(i), list2.get(i)) {
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(a), Some(b)) => {
                let ord = a.cmp(b);
                if ord != Ordering::Equal {
                    return ord;
                }
            }
        }
    }
    Ordering::Equal
}

fn split_suffix(s: &str) -> (String, String) {
    let caps = suffix_regexp()
        .captures(s)
        .unwrap_or_else(|| panic!("suffix chunk {s:?} should already be validated by ver_regexp"));
    let name = caps.get(1).unwrap().as_str().to_string();
    let num = caps.get(2).map(|m| m.as_str()).unwrap_or("").to_string();
    (name, num)
}

fn parse_suffix_chain(chain: &str) -> Vec<&str> {
    if chain.is_empty() {
        Vec::new()
    } else {
        // chain looks like "_pre1_p2"; split('_') yields ["", "pre1", "p2"].
        chain.split('_').skip(1).collect()
    }
}

fn cmp_suffixes(chain1: &str, chain2: &str) -> Ordering {
    let list1 = parse_suffix_chain(chain1);
    let list2 = parse_suffix_chain(chain2);
    let max_len = list1.len().max(list2.len());
    for i in 0..max_len {
        // Implicit "_p0" is less than any explicit suffix, so "1" < "1_p0".
        let (name1, num1) = match list1.get(i) {
            Some(s) => split_suffix(s),
            None => ("p".to_string(), "-1".to_string()),
        };
        let (name2, num2) = match list2.get(i) {
            Some(s) => split_suffix(s),
            None => ("p".to_string(), "-1".to_string()),
        };
        if name1 != name2 {
            return suffix_value(&name1).cmp(&suffix_value(&name2));
        }
        let r1 = parse_uint_or_zero(&num1);
        let r2 = parse_uint_or_zero(&num2);
        let ord = r1.cmp(&r2);
        if ord != Ordering::Equal {
            return ord;
        }
    }
    Ordering::Equal
}

fn ordering_to_i32(o: Ordering) -> i32 {
    match o {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// Mirrors `portage.versions.vercmp(ver1, ver2, silent=1)`: `None` means at
/// least one input failed `ververify`.
pub fn vercmp(ver1: &str, ver2: &str) -> Option<i32> {
    if ver1 == ver2 {
        return Some(0);
    }

    let re = ver_regexp();
    let caps1 = re.captures(ver1)?;
    let caps2 = re.captures(ver2)?;

    let main1 = caps1.get(1).unwrap().as_str();
    let dotted1 = caps1.get(2).unwrap().as_str();
    let letter1 = caps1.get(3).unwrap().as_str();
    let suffix1 = caps1.get(4).unwrap().as_str();
    let rev1 = caps1.get(5).map(|m| m.as_str()).unwrap_or("");

    let main2 = caps2.get(1).unwrap().as_str();
    let dotted2 = caps2.get(2).unwrap().as_str();
    let letter2 = caps2.get(3).unwrap().as_str();
    let suffix2 = caps2.get(4).unwrap().as_str();
    let rev2 = caps2.get(5).map(|m| m.as_str()).unwrap_or("");

    let mut list1 = vec![parse_component(main1)];
    let mut list2 = vec![parse_component(main2)];
    let (d1, d2) = build_dotted_lists(dotted1, dotted2);
    list1.extend(d1);
    list2.extend(d2);

    // NOTE: behavior changed between portage-2.0.x and portage-2.1: a bare
    // letter suffix now sorts *after* the same version without one (e.g.
    // "12.2.5" > "12.2b"), because it's appended to the same comparison
    // list rather than compared as its own component.
    if let Some(c) = letter1.chars().next() {
        list1.push(Part::Num(c as i128));
    }
    if let Some(c) = letter2.chars().next() {
        list2.push(Part::Num(c as i128));
    }

    let ord = cmp_parts(&list1, &list2);
    if ord != Ordering::Equal {
        return Some(ordering_to_i32(ord));
    }

    let ord = cmp_suffixes(suffix1, suffix2);
    if ord != Ordering::Equal {
        return Some(ordering_to_i32(ord));
    }

    let r1 = parse_uint_or_zero(rev1);
    let r2 = parse_uint_or_zero(rev2);
    Some(ordering_to_i32(r1.cmp(&r2)))
}

#[cfg(test)]
mod tests {
    use super::vercmp;

    fn cmp(a: &str, b: &str) -> i32 {
        vercmp(a, b).expect("valid versions")
    }

    #[test]
    fn oversized_main_components_do_not_panic() {
        assert_eq!(cmp(&"9".repeat(39), &"9".repeat(39)), 0);
        assert_eq!(cmp(&"9".repeat(39), &format!("{}8", "9".repeat(38))), 1);
        // 1e39 (40 digits) beats the 39-digit ~9.99e38.
        assert_eq!(cmp(&format!("1{}", "0".repeat(39)), &"9".repeat(39)), 1);
    }

    #[test]
    fn oversized_leading_zero_components_still_compare_by_value() {
        // Leading zeros must not count toward width: value is 1, so this is
        // `BigNum` on the overflowing side but still equal to "1".
        assert_eq!(
            cmp(
                &format!("0{}", "9".repeat(39)),
                &format!("0{}", "9".repeat(39))
            ),
            0
        );
        assert_eq!(cmp(&"9".repeat(39), &"1".to_string()), 1);
    }

    #[test]
    fn oversized_revisions_do_not_panic() {
        assert_eq!(
            cmp(
                &format!("1.0-r{}", "9".repeat(30)),
                &format!("1.0-r{}", "9".repeat(30))
            ),
            0
        );
        assert_eq!(
            cmp(
                &format!("1.0-r{}", "9".repeat(30)),
                &format!("1.0-r{}", &format!("{}8", "9".repeat(29)))
            ),
            1
        );
        assert_eq!(cmp(&format!("1.0-r{}", "9".repeat(30)), "1.0"), 1);
    }

    #[test]
    fn oversized_suffix_numbers_match_python_bignum() {
        // Python wraps these in `int()` (unbounded) when the suffix numeral
        // is not empty, so a huge suffix digit outranks the smaller one.
        assert_eq!(
            cmp(
                &format!("1.0_p{}", "9".repeat(30)),
                &format!("1.0_p{}", &format!("{}8", "9".repeat(29)))
            ),
            1
        );
        assert_eq!(cmp(&format!("1.0_p{}", "9".repeat(30)), "1.0"), 1);
    }

    #[test]
    fn ordinary_pairs_unchanged() {
        assert_eq!(cmp("6.0", "5.0"), 1);
        assert_eq!(cmp("1.0-r1", "1.0"), 1);
        assert_eq!(cmp("1.0.0", "1.0"), 1);
        assert_eq!(cmp("12.2.5", "12.2b"), 1);
        assert_eq!(cmp("1.0", "1.0-r0"), 0);
    }
}
