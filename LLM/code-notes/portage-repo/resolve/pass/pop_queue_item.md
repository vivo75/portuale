# crates/portage-repo/src/resolve/pass/pop_queue_item.rs
Pop one item off the walk queue, holding back later seeds' blocks.

## fn pop_queue_item
- Backlog #298: Portage `_create_graph` (`depgraph.py:3254-3271`) adds every argument's package first, then pops LIFO. The interleaving showed as `slotconflictnewconsumer slotconflictunsolvable` selecting the bare dep's 2.0 before the pin pair's 1.0; Portage adds 1.0 first.

