# crates/portuale/src/pretend/tests/resume.rs
Resume news notices, pretend write isolation, and the not-merged tail.

## fn resume_pretend_end_notice_prints_once_like_portage
- Backlog #231 (a): Portage reaches `post_emerge` after the `--resume --pretend` display too (`actions.py:4289-4297`), whose `not _pkgs_changed` arm prints the notice under `--pretend` (`post_emerge.py:112-117`) -- exactly once (the pre-resolution notice is itself `--pretend`-gated, `actions.py:4266`). A stale list + `FEATURES="news"` on an isolated ROOT; no merge runs.

## fn resume_skipfirst_empty_list_prints_only_the_pre_notice_like_portage
- Backlog #231 (a): Portage's pre-resolution notice precedes all of `action_build`'s resume handling (`actions.py:4266` before `:4289`), so even a `--resume --skipfirst` that empties the list prints it -- while the post notice stays silent (the vdb never changed; Portage's `post_emerge.py:112-117` early-returns on a non-`--pretend` run). Fast: a single-item stale list, no merge runs.

## fn pretend_writes_no_resume_list_even_with_a_stale_list_present
- #179: Portage's `_emerge/actions.py` returns from the `--pretend` branch (`display(...)`, `return os.EX_OK`) before `Scheduler` exists -- and before the `resume_backup` rotation -- so a preview never touches `mtimedb`. A stale list must come back byte-identical, and no fresh list may appear on a bare root.
- Bare root first: `--pretend` resolves and prints, and leaves no `mtimedb` behind at all. Stale list present: byte-identical afterwards (neither a fresh save nor the rotation may run).
- Backlog #562 had narrowed both halves (the pre-resolve updates check, `actions.py:3944` with no `--pretend` exemption, still committed the `updates` record); backlog #575 Task 1 reverted the check to read-only, so the narrowing is gone and the byte-identical/absent pins are back.

## fn entries_not_merged_tags_and_includes_both_source_and_binary_entries
- Portage's own resume mergelist can hold both a source and a binary entry together -- `source_entries_not_merged` (this function's own former name) used to silently drop every `Binary` entry instead of tagging and including it.
- Neither has a real vdb CONTENTS under `root` -- both count as "not merged".

## fn up_front_save_covers_exactly_the_failure_path_tail_when_nothing_merged
- #168: the up-front save (`emerge_build::resume_cpv` over the whole plan) and the failure-path tail (`entries_not_merged`) agree when no merge has landed yet -- so keeping the failure writes alongside the new up-front save + per-merge shrink is provably a harmless redundancy, and failure behaviour is unchanged.
