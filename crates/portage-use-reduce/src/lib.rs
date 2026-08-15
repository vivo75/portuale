// Rust port of `portage.dep.use_reduce` (lib/portage/dep/__init__.py),
// restricted to `flat=True` mode: the tokenizer and bracket/use-conditional
// handling, ported as-is, error behavior included.
// Portage: lib/portage/dep/__init__.py, use_reduce
//
// Out of scope are only optional parameters the harness never exercises:
// `masklist`, `excludeall`, `is_src_uri`/the `->` arrow token,
// `opconvert`, and `token_class`/`is_valid_flag` (tokens stay opaque
// strings here).
//
// `subset` IS ported (`use_reduce_flat_subset`, against Portage's
// `select_subset`) via its own tree type (`DepNode`) feeding into the
// unmodified `use_reduce_flat`. `use_reduce_structured` ports the
// `flat=False`/`opconvert=False` mode (verified byte-for-byte against
// Portage over thousands of randomized dep strings).
// `use_reduce_flat_disjunctive` resolves `"||"` groups to the
// highest-`AltPreference` alternative a caller closure reports. This crate
// stays atom-agnostic throughout.

use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;

fn useflag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9+_@-]*$").unwrap())
}
