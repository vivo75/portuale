//! Package removal actions: unmerge, prune, clean, depclean.

mod depclean;
mod unmerge;

pub(crate) use depclean::*;
pub(crate) use unmerge::*;
