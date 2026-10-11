//! Package removal actions: unmerge, prune, clean, depclean.

mod depclean;
mod prune_clean;
mod unmerge;

pub(crate) use depclean::*;
pub(crate) use prune_clean::*;
pub(crate) use unmerge::*;
