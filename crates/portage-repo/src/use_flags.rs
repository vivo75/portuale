//! Effective USE and USE mask/force resolution.

use crate::*;

pub struct EffectiveUseFlagsArgs<'a> {
    pub config: &'a portage_profile::Config,
    pub iuse: &'a str,
    pub keywords: &'a [String],
    pub candidate_str: &'a str,
    pub category: &'a str,
    pub package: &'a str,
}

pub struct ForcedOrMaskedFlagsArgs<'a> {
    pub iuse: &'a str,
    pub keywords: &'a [String],
    pub candidate_str: &'a str,
    pub category: &'a str,
    pub package: &'a str,
    pub config: &'a portage_profile::Config,
}
