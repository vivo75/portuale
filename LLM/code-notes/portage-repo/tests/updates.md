# crates/portage-repo/src/tests/updates.rs
Update-command parsing and dep-string rewrite tests.

## fn installed_cp_sources_map_matches_the_command_loop
- Backlog #102 S1: the inverted map answers exactly what the old per-call command loop did, on whatever `profiles/updates/` the ambient repos carry (possibly none).

## fn installed_cp_sources_cycle_emits_no_duplicate
- Backlog #120 (audit O1): the target is already `out[0]`, and emitting it again would scan the same vdb entry twice.

## fn apply_updates_to_dep_string_reports_unchanged_and_rewrites_only_moved_tokens
- Backlog #119: `None` when no `move`/`slotmove` touched a token (so the caller can skip its insert and no output is allocated), `Some(rewritten)` otherwise, with the surrounding whitespace, `||`/`(`/`)` and `use?` tokens preserved byte-for-byte.

## fn slotmove_rewrites_versioned_blocker_and_usedeps_tokens
- Each expectation was generated with Portage 3.0.83 (`update_dbentry(["slotmove", Atom("dev-libs/foo"), "0", "1"], token, eapi="8")`). Portage: `lib/portage/update.py:69-102`.
