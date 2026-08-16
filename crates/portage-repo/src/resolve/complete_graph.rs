//! Decides when complete mode switches itself on.

use crate::*;

/// Arguments of [`complete_graph_auto_enable`].
pub struct CompleteGraphAutoEnableArgs<'a> {
    pub entries: &'a [GraphEntry],
    pub if_new_use: bool,
    pub if_new_ver: bool,
    pub if_new_slot: bool,
    pub root: &'a Path,
    pub repos: &'a [RepoConfig],
    pub config: &'a portage_profile::Config,
}
