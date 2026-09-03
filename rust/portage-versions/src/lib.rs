// Rust port of the comparison semantics implemented by
// `lib/portage/versions.py` (functions `ververify` and `vercmp`) in the
// Python codebase. Ported by hand, structurally close to the Python
// original so it stays easy to diff against on future changes -- this is
// a portuale artifact, not yet idiomatic-first Rust; see docs/agent-context.md.
//
// Known simplification vs. Python: version components are parsed as
// `i128` rather than arbitrary-precision integers. This covers every
// practical Gentoo version string (and the >=30-digit case in Python's own
// test suite) but will panic on a component wider than ~38 decimal digits.
// A real port should use a bignum type or string-based comparison instead.

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
/// missing third component of "1.0" compared against "1.0.0"), defined to
/// be less than any explicit component so that "1.0.0" > "1.0".
const IMPLICIT_ZERO: i128 = -1;

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

fn parse_component(s: &str) -> i128 {
    s.parse()
        .unwrap_or_else(|_| panic!("version component too wide for i128: {s:?}"))
}

/// Builds the paired dotted-component lists (the parts after the first
/// integer, e.g. the ".2.3" in "1.2.3"), applying Portage's implicit-zero
/// and leading-zero ("float-like") comparison rules. Must be built jointly
/// because both rules compare same-index components across both versions.
fn build_dotted_lists(dotted1: &str, dotted2: &str) -> (Vec<i128>, Vec<i128>) {
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
            list1.push(IMPLICIT_ZERO);
            list2.push(parse_component(b));
        } else if b.is_empty() {
            list1.push(parse_component(a));
            list2.push(IMPLICIT_ZERO);
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

fn cmp_lists(list1: &[i128], list2: &[i128]) -> Ordering {
    let max_len = list1.len().max(list2.len());
    for i in 0..max_len {
        match (list1.get(i), list2.get(i)) {
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(a), Some(b)) if a != b => return a.cmp(b),
            _ => {}
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
        let r1: i64 = num1.parse().unwrap_or(0);
        let r2: i64 = num2.parse().unwrap_or(0);
        if r1 != r2 {
            return r1.cmp(&r2);
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
        list1.push(c as i128);
    }
    if let Some(c) = letter2.chars().next() {
        list2.push(c as i128);
    }

    let ord = cmp_lists(&list1, &list2);
    if ord != Ordering::Equal {
        return Some(ordering_to_i32(ord));
    }

    let ord = cmp_suffixes(suffix1, suffix2);
    if ord != Ordering::Equal {
        return Some(ordering_to_i32(ord));
    }

    let r1: i64 = if rev1.is_empty() {
        0
    } else {
        rev1.parse().unwrap()
    };
    let r2: i64 = if rev2.is_empty() {
        0
    } else {
        rev2.parse().unwrap()
    };
    Some(ordering_to_i32(r1.cmp(&r2)))
}
