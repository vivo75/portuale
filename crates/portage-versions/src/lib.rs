// Port of `lib/portage/versions.py` (`ververify`, `vercmp`), kept
// structurally close to the original for diffing.
//
// Components wider than `i128` compare as decimal strings, mirroring
// Python's unbounded `int`, so no width of component can miscompare.

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

/// Sentinel for a missing dotted-version component (e.g. the third of
/// "1.0" against "1.0.0"): sorts less than any explicit component.
///
/// A single numeric component is either an in-range `i128` (`Num`) or,
/// past its width, a decimal string compared by length-then-digits
/// (`BigNum`). `BigNum` only arises from genuine overflow, so it always
/// exceeds any `Num`.
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
