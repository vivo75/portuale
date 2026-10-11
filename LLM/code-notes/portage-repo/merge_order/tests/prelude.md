# crates/portage-repo/src/merge_order/tests/prelude.rs
`select_dep_target` narrowing/ranking and `rank_best` pins.

## fn select_dep_target_keeps_the_flag_carrier_a_later_flip_broke
- Backlog #244 (`aub0`, argv `c a b`): B's `d[foo]` landed on d-0 (foo on by default); A's autounmask `-foo` then flipped d-0, so no instance satisfies `d[foo]` any more. Portage keeps B's edge on d-0 -- the conflict block lists B under it -- so the USE narrowing falls back to the instances whose IUSE carries `foo`, not to the vercmp-highest d-1 (which lacks the flag).

## fn select_dep_target_elimination_keeps_the_last_on_a_version_tie
- The single-match shortcut keeps the vercmp tie on the first; the elimination path keeps the last.

## fn select_dep_target_keeps_nvc_out_of_the_elimination
- The NVC candidate stays alive and wins the ranking (its empty version ties, and it is first); the elimination would otherwise remove it and leave an installed candidate.
