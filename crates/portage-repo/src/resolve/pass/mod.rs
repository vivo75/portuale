//! One resolver pass over the graph.

use crate::*;
use std::ops::ControlFlow;
mod unresolved_dep;
pub(crate) use unresolved_dep::*;
