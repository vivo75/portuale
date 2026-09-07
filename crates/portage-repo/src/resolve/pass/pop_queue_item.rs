//! Pop one item off the walk queue, holding back later seeds' blocks.

use crate::resolve::pass::*;

/// Pop one item off the walk queue, holding back later seeds' blocks.
///
/// Each seed's whole subtree drains as one block, in seed order, so the last
/// argument's subtree walks before the previous argument's deps are looked
/// at. `Break` carries the empty queue.
pub(crate) fn pop_queue_item(state: &mut PassState) -> ControlFlow<(), QueueItem> {
    if let Some(n) = state.seed_split_at.take()
        && state.queue.len() > n
    {
        state.held_blocks.push_back(state.queue.split_off(n));
    }
    if state.queue.is_empty() {
        match state.held_blocks.pop_front() {
            Some(block) => state.queue = block,
            None => return ControlFlow::Break(()),
        }
    }
    let Some(item) = state.queue.pop_front() else {
        return ControlFlow::Break(());
    };
    if state.seeds_left > 0 {
        state.seeds_left -= 1;
        state.seed_split_at = Some(state.queue.len());
    }
    ControlFlow::Continue(item)
}
