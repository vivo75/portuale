//! SRC_URI and RESTRICT=fetch helpers.

use crate::*;

/// Arguments of [`use_flags_if_conditional`].
pub(crate) struct UseFlagsIfConditionalArgs<'a> {
    pub(crate) value_str: &'a str,
    pub(crate) candidate: &'a Candidate,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) candidate_str: &'a str,
    pub(crate) config: &'a portage_profile::Config,
}
