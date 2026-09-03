//! The circular-dependency block.

use crate::pretend::*;

/// Arguments of [`circular_node_text`].
pub(crate) struct CircularNodeTextArgs<'a> {
    pub(crate) cpv: &'a str,
    pub(crate) slot: &'a str,
    pub(crate) sub_slot: &'a str,
    pub(crate) repo: &'a str,
    pub(crate) type_name: &'a str,
    pub(crate) color: &'a Colorizer,
}
