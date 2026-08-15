//! Recognition tables for Portage's full `emerge` CLI option surface
//! (`lib/_emerge/main.py`: the `options` list, the `shortmapping` and
//! `argument_options` dicts, and the `actions` frozenset).
//!
//! These tables exist purely for recognition, not behavior: using a
//! Portage flag portuale doesn't implement yet reports "recognized, but
//! not implemented", distinct from a genuinely unknown flag. Every flag
//! portuale implements is excluded here and handled directly by the caller
//! (the implemented set is what `emerge --help` documents, in
//! `pretend/help.txt`); the contract suite pins the text each table
//! produces. Per-flag grounding for each exclusion lives in the notes.
//!
//! Two parsing facts the caller relies on: short boolean flags bundle
//! (`-pv` is argparse-native bundling for plain boolean shorts, not
//! `insert_optional_args`, which separately lets `-v` take an optional
//! value); and `Category::Value` marks which options take an argument,
//! but lookups report-and-exit immediately, so the argument is never
//! parsed here.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Boolean,
    Value,
    Action,
}
