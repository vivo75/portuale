//! `resolve_config`: building the full `Config` from disk and the environment.

use crate::*;

mod licenses;
mod package_env;
mod package_masks;
mod package_sets;
mod package_use;
mod use_expand;
mod use_force_mask;
mod use_levels;
mod use_masks;
pub(crate) use licenses::*;
pub(crate) use package_env::*;
pub(crate) use package_masks::*;
pub(crate) use package_sets::*;
pub(crate) use package_use::*;
pub(crate) use use_expand::*;
pub(crate) use use_force_mask::*;
pub(crate) use use_levels::*;
pub(crate) use use_masks::*;
/// Arguments of [`resolve_config`].
pub struct ResolveConfigArgs<'a> {
    pub config_root: &'a Path,
    pub main_repo_location: &'a Path,
    pub overlay_repos: &'a [(String, PathBuf)],
    pub repo_aliases: &'a [(String, PathBuf)],
    pub main_repo_name: &'a str,
    pub repo_masters: &'a HashMap<String, Vec<PathBuf>>,
    pub eroot: &'a Path,
}
