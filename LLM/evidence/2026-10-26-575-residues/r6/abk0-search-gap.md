# R6 residue: the abk0 world-B search gap (#613)

`--pretend --autounmask-backtrack=y --backtrack=2 dev-libs/abk0d` with
`abk0b` in world. Host Portage 3.0.83 (r6trace on a scratch copy of the
fixtures, `/tmp/abkworld` vdb + world from the pin's `_abkworld_worldb_root`):

```
PASS bt_before=0 infos=['config', 'slot conflict'] need_restart=True
  ARM restart -> 1; feedback keys=['config', 'slot conflict']; children:
    config (mask_steps 0, unexplored), mask {A-3}, mask {A-1, A-1 installed}
PASS bt_before=1 mask={A-1...} infos=['slot conflict'] need_restart=True
  ARM restart -> 2; both children REJECTED (mask_steps 2/1 > max_depth)
PASS bt_before=2 mask={A-3} infos=['config'] need_restart=False
  ARM break (need_config_change, at limit); RETURN backtracked=2/2
```

portuale (`PORTUALE_BT_TRACE=1` on the same scratch root):

```
try=Settle                                             (first walk)
try=Feedback:RevDep  (A-3 mask, x-flip folded)         (complete walk)
try=Feedback:Config  (grow y on abk0c)
try=Settle                                             -> 1/2
```

The R6 counting rule is faithful here: Portage's third pass folds the same
`y` change on `abk0c` with `need_restart=False` (`_pkg_use_enabled` records
no `config` restart: the flattened `RDEPEND` is empty under either USE and
no parent USE dep breaks), so the grow restart is rightly not counted. The
missing `2` is Portage's second pass: a slot-conflict restart on the
`{A-1}`-masked node, which portuale never visits — its reverse-dependency
feedback adds a single `{A-3}`-masked child where Portage's
`_slot_confict_backtrack` adds two mask choices.

Why one child: `consumer_pin_conflict_may_feed_back`
(`resolve/feedback/reverse_dep_pins.rs`) refuses the consumer-pin record
when the pass grew a USE change (`pass.use_overlay` non-empty), so the
conflict never reaches the multi-choice `step_slot_conflict_backtrack`;
the `x` flip folded on the same pass then routes the single mask through
`step_reverse_dep_pins` instead. Lifting that exclusion is a 10a-P3-scope
search change (the `t8x` counter-shape relies on the grow-first order),
so it needs its own S0 and slice — filed as #613, not done here.

Effect: rows, rc (1) and blocks are equal; only the count line differs
(`1/2` vs `2/2`). Pins recording `2/2`
(`test_autounmask_use_backtrack_world_bound_falls_back_to_the_older`,
`enforced_pin_miss_renders_the_installed_enforcing_consumer`) stay red
until #613 lands.
