# crates/portage-repo/src/tests/tests_p573.rs
Context moved out of the code comments.

## module
- Backlog numbers removed from the comment (original text): #573: the skipped-update block names every installed consumer Portage lists, not just the first in-graph consumer that shares the withheld pin atom.  Expected values come from host Portage 3.0.83 on the same scratch ROOT (`LLM/evidence/2026-10-28-573/captures/portage-p569prov.stderr` for the in-graph cell, `portage-p573up.stderr` for the complete-mode cell), never from a second portuale run.

## fn p573_root
- Backlog numbers removed from the comment (original text): The scratch ROOT of `LLM/evidence/2026-10-28-573/probe-p573.sh`: the shared fixture vdb plus the #573 recorded installed consumers (`extra`), built beside the shared tree so the fixture vdb itself stays untouched.
