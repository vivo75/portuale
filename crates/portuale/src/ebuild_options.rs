//! Portage's `ebuild` CLI option and command surface: the six options from
//! `bin/ebuild`'s `argparse` setup and the command list `doebuild()`
//! accepts. Recognizing (not executing) them lets `ebuild.rs` tell "a
//! Portage option/command portuale doesn't implement yet" apart from
//! invalid syntax, the same distinction `emerge_options.rs` draws.
//!
//! `-h`/`--help` is implemented but excluded from `OPTIONS`, like
//! `emerge_options.rs` excludes its own `--help`; `--version` is
//! deliberately not implemented (see notes).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Boolean,
    Value,
}
