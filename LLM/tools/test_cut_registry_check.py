#!/usr/bin/env python3
"""Regression checks for cut_registry_check.py on scratch files (no cargo).
Run: python3 -m pytest LLM/tools/test_cut_registry_check.py

Each case pins one rule: (a) a registry cut still open against a DONE backlog
entry, (b) a deferred-cut marker in what-this-proves.md with no CUT-n
acknowledgment, (c) a CUT-n pointing at a backlog number that does not exist,
and the clean case. Malformed registry lines and duplicate CUT numbers are
covered too.
"""

import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(__file__))
import cut_registry_check as crc  # noqa: E402

BACKLOG = (
    "# Backlog\n\n"
    "5. **DONE 2026-10-01 — a finished follow-up.** body\n"
    "6. **OPEN (filed 2026-10-02).** body\n"
    "7. **CLOSED 2026-10-03 — not a fixed behaviour.** body\n"
)

PROOF = (
    "# What this proves\n\n"
    "Some paragraph.\n"
    "**Still deferred:** the thing nobody did.\n"
    "More prose.\n"
)


def scratch(registry, backlog=BACKLOG, proof=PROOF):
    """Write the three inputs to a temp dir and run the checker on them."""
    d = tempfile.mkdtemp()
    paths = {}
    for name, text in (("scope-cuts.md", registry), ("backlog-tasks.md", backlog),
                       ("what-this-proves.md", proof)):
        p = os.path.join(d, name)
        with open(p, "w") as f:
            f.write(text)
        paths[name] = p
    return crc.check(paths["scope-cuts.md"], paths["backlog-tasks.md"],
                     paths["what-this-proves.md"])


def scratch_two_backlogs(registry, second):
    """Run the checker with the backlog split over two files."""
    d = tempfile.mkdtemp()
    paths = []
    for name, text in (("scope-cuts.md", registry), ("backlog-tasks.md", BACKLOG),
                       ("backlog-tasks-2026-10.md", second), ("what-this-proves.md", PROOF)):
        paths.append(os.path.join(d, name))
        with open(paths[-1], "w") as f:
            f.write(text)
    return crc.check(paths[0], paths[1:3], paths[3])


ACK_760 = "- CUT-1: the thing nobody did (what-this-proves.md:4) → none (accepted): tiny.\n"


class CutRegistryCheck(unittest.TestCase):
    def test_clean_registry_passes(self):
        self.assertEqual(scratch(ACK_760), [])

    def test_a_open_cut_against_done_backlog_fails(self):
        reg = ACK_760 + "- CUT-2: follow-up shipped (what-this-proves.md:4) → #5\n"
        problems = scratch(reg)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("CUT-2", problems[0])
        self.assertIn("#5", problems[0])
        self.assertIn("DONE", problems[0])

    def test_a_closed_cut_against_done_backlog_passes(self):
        reg = ACK_760 + "- CUT-2: follow-up shipped (what-this-proves.md:4) → closed #5\n"
        self.assertEqual(scratch(reg), [])

    def test_a_open_cut_against_open_backlog_passes(self):
        reg = ACK_760 + "- CUT-2: follow-up pending (what-this-proves.md:4) → #6\n"
        self.assertEqual(scratch(reg), [])

    def test_b_unacknowledged_marker_fails(self):
        problems = scratch("")
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("what-this-proves.md:4", problems[0])
        self.assertIn("Still deferred", problems[0])

    def test_b_cut_reference_within_three_lines_in_proof_passes(self):
        proof = PROOF.replace("More prose.", "More prose. CUT-9 notes it.")
        self.assertEqual(scratch("", proof=proof), [])

    def test_b_reference_for_another_line_does_not_acknowledge(self):
        reg = "- CUT-1: wrong line (what-this-proves.md:50) → none (accepted): tiny.\n"
        problems = scratch(reg)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("what-this-proves.md:4", problems[0])

    def test_entry_without_status_word_exists_and_is_not_done(self):
        backlog = BACKLOG + "8. **brush PRs: submit staged fixes** — no status word.\n"
        reg = ACK_760 + "- CUT-2: upstream PRs unopened (what-this-proves.md:4) → #8\n"
        self.assertEqual(scratch(reg, backlog=backlog), [])

    def test_c_missing_backlog_number_fails(self):
        reg = ACK_760 + "- CUT-3: points nowhere → #99\n"
        problems = scratch(reg)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("CUT-3", problems[0])
        self.assertIn("#99", problems[0])

    def test_c_missing_backlog_number_fails_for_closed_too(self):
        reg = ACK_760 + "- CUT-3: points nowhere → closed #99\n"
        problems = scratch(reg)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("#99", problems[0])

    def test_malformed_registry_line_fails(self):
        problems = scratch(ACK_760 + "- CUT-4: no target at all\n")
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("CUT-4", problems[0])
        self.assertIn("target", problems[0])

    def test_duplicate_cut_number_fails(self):
        reg = ACK_760 + "- CUT-1: again (what-this-proves.md:9) → none (accepted): x.\n"
        problems = scratch(reg)
        self.assertTrue(any("CUT-1" in p and "duplicate" in p for p in problems), problems)

    def test_c_number_defined_only_in_second_backlog_file_exists(self):
        reg = ACK_760 + "- CUT-2: lives in the second file (what-this-proves.md:4) → #300\n"
        self.assertEqual(scratch_two_backlogs(reg, "300. **OPEN (filed 2026-10-02).** body\n"), [])
        problems = scratch_two_backlogs(reg, "")
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("#300", problems[0])

    def test_a_done_entry_in_second_backlog_file_closes_the_cut(self):
        reg = ACK_760 + "- CUT-2: shipped (what-this-proves.md:4) → #300\n"
        problems = scratch_two_backlogs(reg, "300. **DONE 2026-10-02.** body\n")
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("DONE", problems[0])

    def test_prose_lines_are_not_registry_entries(self):
        reg = ("# Scope cuts\n\nSee `CUT-n` in the header; `→ #N` links a cut.\n"
               + ACK_760)
        self.assertEqual(scratch(reg), [])


if __name__ == "__main__":
    unittest.main()
