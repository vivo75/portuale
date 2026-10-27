# crates/portuale/src/pretend/conflicts.rs
Slot-conflict text helpers. Context moved out of the code comments (#336 Phase 7); the code keeps only the short why.

## enum SlotConflictReason
- Mirrors Portage's `_slot_conflict_handler._prepare_conflict_msg_and_check_for_specificity` `collision_reasons` keys (`dep/slot_collision.py`).
- Cuts vs. Portage: the `soname` reason key (no soname atom can reach this renderer -- `portage-dep` cannot parse soname atoms and dep flattening drops unparseable tokens, so the key is unreachable, not merely unimplemented), an `=*` operator's `("version", None)` key (portuale files it under `"eq"`), operator/USE colorization, and the `need_rebuild` "cannot be rebuilt" trailer (wired, not cut).

## fn slot_conflict_atom_string
- The two forms are what Portage builds as `atom.without_use.without_slot` and `atom.without_use` for its `findAtomForPackage` collision-reason probes.

## struct ConflictOther
- The flag sets are what Portage matches USE-deps against (`other_pkg.iuse`, `_pkg_use_enabled(other_pkg)`).

## fn slot_conflict_reasons
- Version mismatch wins over slot mismatch wins over USE mismatch, matching Portage's `elif` ordering (`slot_collision.py:307-397`): a version-mismatched pair skips the slot and USE checks even when the operator is `None` and nothing is recorded for that pair; the USE check runs only when the slot half matched.
- `match_from_list` skips USE evaluation against plain-string candidates (Portage's own `hasattr(x, "use")` guard), so a version+slot match always "matches" there -- the USE verdict comes from `use_mismatch_flags` alone, exactly partitioning the cases where Portage's full-atom `findAtomForPackage` fails.

## fn slot_conflict_caret_idx
- The render site wraps these spans in `BAD` red (`colorize_marked_spans`) and shifts the markers onto the displayed string; Portage marks the pre-color indices and drifts (upstream bug -- and Portage's own red varies by terminal colormap, so byte-parity with the drift is unachievable anyway).
- USE-token branch: Portage walks its own re-rendered tokens with a running cursor (no spaces assumed); portuale locates each comma-separated piece from the cursor instead, so spaced atoms (`[x, y]`) still mark correctly. The strip rule is Portage's own (`lstrip("-!")` / `rstrip("=?")`) -- including its quirk that `(+)`/`(-)`-suffixed tokens never match.

## fn colorize_marked_spans
- The marked char's displayed position is recorded after the ANSI open codes are pushed. Markers track the displayed string rather than Portage's pre-color indices (see above).

## fn slot_conflict_need_rebuild
- Ports Portage's `slot_collision.py` `need_rebuild` scan (lines 407-458). A *built* slot-operator atom is Portage's `Atom.slot_operator_built`; soname atoms can't reach this renderer. Unavailable means Portage's `_equiv_ebuild_visible` fails. Under `--usepkgonly` Portage stays quiet to avoid false positives.

## fn render_pkg_use_display
- The pairs come from `portage_repo::pkg_use_display_for`. Portage always renders a `USE=` field even with no flags (`var_order.insert(0, "USE")` + the unconditional `flag_displays.append`), so an empty slice still yields `USE=""`. Colorization of the flag tokens (Portage `UseFlagDisplay.__str__`'s `red`/`blue`) is cut -- it is a no-op under `emerge -p` without `--color=y` anyway.

## fn skipped_node_suffix
- Backlog #242 Slice D: `running` is the row's `missed_targets_running_root` / `consumer_targets_running_root`; the path comes from the display roots (target `root`, `running_root` when the row rides it). Portage's bare `/`-rooted missed line stays bare.
- Backlog #242 Slice D: Portage `Package.__str__`'s own quoted destination (`Package.py:568-608`) for a scheduled-for-merge skipped-block node whose OWN root is not `/` — ` to '<ROOT>'`, inside the paren, or empty when that root is `/` (Portage's bare `/`-rooted missed line). `running` is the row's `missed_targets_running_root` / `consumer_targets_running_root`; the path comes from the display roots (target `root`, `running_root` when the row rides it).

## fn skip_conflict_caret_line
- Backlog #90 (S2), extended by #230: Portage's `format_unmatched_atom` (`_emerge/resolver/output.py:892`) marks the leading operator chars plus the version token (`~` + `1.0` in `~dev-libs/whblocker-1.0`, staged oracle), plus a `:slot[/sub-slot][op]` span when the atom carries one that mismatches the missed package (`:967-975`). Spans are computed on the raw atom text with no color realignment -- the same deliberate divergence as the slot block's markers. No grounded case carries USE-deps on a skipped row, so USE-token spans stay cut. `highlight_version` is Portage's `highlight_version` (#270).
- A trailing `-rN` revision is left out of the version span (no oracle covers it).
- Portage's `highlight_slot`: the atom names a slot (or sub-slot) the missed package does not have. The span covers `:slot`, the `/sub-slot` half when present, and the trailing slot operator (`=`/`*`), anchored on its occurrence in the raw atom text.

## struct SlotConflictNeedRebuildArgs (allow-list, #571)
- Carries `config_root`; the `ebuild_visible_at` check builds its gate once per report.

## fn slot_conflict_explanation (#597)
- Port of `get_explanation` (`resolver/slot_collision.py:728-784`). Returns `None` for a version or slot conflict, `None` without solutions, the `--newuse and --update` text for an unspecific conflict unless both options are set (that text needs no solution). The header line counts solutions while the body prints the minimised changes (quirk Q22). Flags print red for `+` and blue for `-` (`colorize("red"/"blue")`).
