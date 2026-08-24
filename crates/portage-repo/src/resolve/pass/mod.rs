//! One resolver pass over the graph.

use crate::*;
use std::ops::ControlFlow;
mod enqueue_deps;
mod unresolved_dep;
pub(crate) use enqueue_deps::*;
pub(crate) use unresolved_dep::*;
