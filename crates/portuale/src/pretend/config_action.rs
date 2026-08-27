//! `emerge --config`.

use crate::pretend::*;

/// Arguments of [`run_config_action`].
pub(crate) struct RunConfigActionArgs<'a> {
    pub(crate) atom_args: &'a [&'a str],
    pub(crate) root: &'a Path,
    pub(crate) shell: ebuild_phases::ShellBackend,
    pub(crate) debug: bool,
    pub(crate) ask: bool,
    pub(crate) color: &'a Colorizer,
}
