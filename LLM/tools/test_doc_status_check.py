#!/usr/bin/env python3
"""Regression checks for doc_status_check.py on scratch trees (no cargo).
Run: python3 -m pytest LLM/tools/test_doc_status_check.py

Each case pins one rule: (a) plan Status vs backlog entry status, (b) a number
defined twice, (c) entries above FIRST_ENFORCED need Oracle:/Premise:, (d) tier
prefix, plus malformed `N. N.` lines, UNKNOWN status, ALLOW shrink-only and the
clean case.
"""

import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(__file__))
import doc_status_check as dsc  # noqa: E402

# Pinned to the count written by the first run on the real tree; shrink only.
ALLOW_COUNT = 3  # first real-tree run: (a,447) (d,443) (d,452)


def tree(backlogs, plans):
    """Write backlog files {name: text} and plans {name: text}; return paths."""
    d = tempfile.mkdtemp()
    bl, pl = [], []
    for name, text in backlogs.items():
        p = os.path.join(d, name)
        with open(p, "w") as f:
            f.write(text)
        bl.append(p)
    for name, text in plans.items():
        p = os.path.join(d, name)
        with open(p, "w") as f:
            f.write(text)
        pl.append(p)
    return bl, pl


def run(backlogs, plans, allow=None):
    bl, pl = tree(backlogs, plans)
    return dsc.check(bl, pl, allow={} if allow is None else allow)


T1 = "## Tier 1 — focused\n\n"
T2 = "## Tier 2 — larger\n\n"


def entry(n, status="DONE", extra=""):
    return f"{n}. **{status} 2026-10-01 — thing.** body {extra}\n"


def plan(status, tier="01"):
    return f"# Plan\n\nPreamble line\nStatus: {status}\n"


class DocStatusCheck(unittest.TestCase):
    def test_clean_tree_passes(self):
        self.assertEqual(run({"b.md": T1 + entry(10)},
                             {"01.10-x.opus.md": plan("done 2026-10-02")}), [])

    def test_a_plan_done_entry_open_fails(self):
        p = run({"b.md": T1 + entry(10, "OPEN")}, {"01.10-x.opus.md": plan("done 2026-10-02")})
        self.assertEqual(len(p), 1, p)
        self.assertIn("#10", p[0])
        self.assertIn("(a)", p[0])

    def test_a_plan_done_entry_closed_passes(self):
        self.assertEqual(run({"b.md": T1 + entry(10, "CLOSED")},
                             {"01.10-x.opus.md": plan("**done 2026-10-02**")}), [])

    def test_a_plan_proposed_entry_done_fails(self):
        p = run({"b.md": T1 + entry(10)}, {"01.10-x.opus.md": plan("proposed (2026-10-01)")})
        self.assertEqual(len(p), 1, p)
        self.assertIn("(a)", p[0])

    def test_a_plan_proposed_entry_open_passes(self):
        self.assertEqual(run({"b.md": T1 + entry(10, "OPEN")},
                             {"01.10-x.opus.md": plan("proposed (2026-10-01)")}), [])

    def test_a_in_progress_naming_residues_passes(self):
        self.assertEqual(run({"b.md": T1 + entry(10)},
                             {"01.10-x.opus.md": plan("in progress; open residues #11")}), [])

    def test_a_missing_entry_fails(self):
        p = run({"b.md": T1 + entry(11)}, {"01.10-x.opus.md": plan("done 2026-10-02")})
        self.assertEqual(len(p), 1, p)
        self.assertIn("no backlog entry", p[0])

    def test_a_unknown_status_fails(self):
        p = run({"b.md": T1 + "10. **plain title** body\n"},
                {"01.10-x.opus.md": plan("done 2026-10-02")})
        self.assertEqual(len(p), 1, p)
        self.assertIn("UNKNOWN", p[0])

    def test_status_header_found_past_preamble(self):
        text = "# t\n" + "\n" * 15 + "Status: **done 2026-10-12 (x)**\n"
        self.assertEqual(run({"b.md": T1 + entry(10)}, {"01.10-x.opus.md": text}), [])

    def test_b_duplicate_in_one_file_fails(self):
        p = run({"b.md": T1 + entry(10) + entry(10)}, {})
        self.assertEqual(len(p), 1, p)
        self.assertIn("(b)", p[0])

    def test_b_duplicate_across_two_files_fails(self):
        p = run({"a.md": T1 + entry(10), "b.md": T1 + entry(10)}, {})
        self.assertEqual(len(p), 1, p)
        self.assertIn("(b)", p[0])

    def test_malformed_item_line_fails(self):
        p = run({"b.md": T1 + "10. 10. **DONE 2026-10-01 — x.**\n"}, {})
        self.assertEqual(len(p), 1, p)
        self.assertIn("malformed", p[0])

    def test_c_entry_above_first_enforced_needs_oracle(self):
        p = run({"b.md": T1 + entry(494, "OPEN")}, {})
        self.assertEqual(len(p), 1, p)
        self.assertIn("(c)", p[0])

    def test_c_oracle_or_premise_passes(self):
        self.assertEqual(run({"b.md": T1 + entry(494, "OPEN", "Oracle: depgraph.py:1")}, {}), [])
        self.assertEqual(run({"b.md": T1 + entry(494, "OPEN", "Premise: PM source read")}, {}), [])

    def test_c_premise_na_with_reason_passes(self):
        self.assertEqual(run({"b.md": T1 + entry(494, "OPEN", "Premise: n/a — doc only")}, {}), [])

    def test_c_premise_na_without_reason_fails(self):
        p = run({"b.md": T1 + entry(494, "OPEN", "Premise: n/a")}, {})
        self.assertEqual(len(p), 1, p)

    def test_c_entry_at_first_enforced_is_exempt(self):
        self.assertEqual(run({"b.md": T1 + entry(dsc.FIRST_ENFORCED, "OPEN")}, {}), [])

    def test_d_tier_mismatch_fails(self):
        p = run({"b.md": T2 + entry(10)}, {"01.10-x.opus.md": plan("done 2026-10-02")})
        self.assertEqual(len(p), 1, p)
        self.assertIn("(d)", p[0])

    def test_allow_suppresses_exactly_the_keyed_violation(self):
        bl = {"b.md": T1 + entry(10, "OPEN")}
        pl = {"01.10-x.opus.md": plan("done 2026-10-02")}
        self.assertEqual(run(bl, pl, allow={("a", 10): "legacy"}), [])
        p = run(bl, pl, allow={("a", 11): "other"})
        self.assertEqual(len(p), 2, p)  # the violation stays, the unused row is stale

    def test_stale_allow_row_is_reported(self):
        p = run({"b.md": T1 + entry(10)}, {}, allow={("a", 10): "legacy"})
        self.assertEqual(len(p), 1, p)
        self.assertIn("stale ALLOW", p[0])

    def test_allow_rows_carry_a_reason(self):
        self.assertTrue(all(isinstance(k, tuple) and len(k) == 2 and r.strip()
                            for k, r in dsc.ALLOW.items()))

    def test_allow_count_is_pinned_shrink_only(self):
        self.assertLessEqual(len(dsc.ALLOW), ALLOW_COUNT,
                             "ALLOW grew: fix the staleness instead of allowing it")
        self.assertEqual(len(dsc.ALLOW), ALLOW_COUNT,
                         "ALLOW shrank: lower ALLOW_COUNT in this test")

    def test_allow_growth_against_base(self):
        base = "ALLOW = {('a', 1): 'x'}\n"
        self.assertEqual(dsc.allow_growth(base, {("a", 1): "x"}), [])
        self.assertEqual(dsc.allow_growth(base, {}), [])
        self.assertEqual(dsc.allow_growth(base, {("a", 1): "x", ("b", 2): "y"}), [("b", 2)])

    def test_main_base_option_fails_on_growth(self):
        d = tempfile.mkdtemp()
        base = os.path.join(d, "base.py")
        with open(base, "w") as f:
            f.write("ALLOW = {}\n")
        rc = dsc.main(["--allow-base", base])
        self.assertEqual(rc, 1)  # the real ALLOW is non-empty, the base is empty

    def test_real_tree_is_clean(self):
        self.assertEqual(dsc.main([]), 0)


if __name__ == "__main__":
    unittest.main()
