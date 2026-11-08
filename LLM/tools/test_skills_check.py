"""Unit tests for skills_check.py: a clean skill passes, and each rule fires."""

import os
import tempfile
import unittest

import skills_check as sc

GOOD = "---\nname: demo\ndescription: Use when testing.\n---\n\n# Demo\n"


class SkillsCheck(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = self.tmp.name
        self.skills = os.path.join(self.root, "LLM", "skills")
        self.claude = os.path.join(self.root, ".claude", "skills")
        os.makedirs(os.path.join(self.skills, "demo"))
        os.makedirs(self.claude)
        os.makedirs(os.path.join(self.root, "LLM", "real"))
        open(os.path.join(self.root, "LLM", "real", "x.md"), "w").close()
        self.write(GOOD)
        os.symlink("../../LLM/skills/demo", os.path.join(self.claude, "demo"))

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, text):
        with open(os.path.join(self.skills, "demo", "SKILL.md"), "w") as f:
            f.write(text)

    def manifest(self, *paths):
        with open(os.path.join(self.root, "LLM", "ARCHIVE.manifest.tsv"), "w") as f:
            f.write("path\tarchive\n" + "".join(f"{p}\ta\n" for p in paths))

    def run_check(self):
        return sc.check(self.skills, self.claude, self.root)

    def test_clean_skill_passes(self):
        self.write(GOOD + "See `LLM/real/x.md:3,5` and `LLM/real/*.md`.\n")
        self.assertEqual(self.run_check(), [])

    def test_missing_path_is_reported(self):
        self.write(GOOD + "See `LLM/nope.md`.\n")
        self.assertIn("cites missing LLM/nope.md", self.run_check()[0])

    def test_archived_path_is_reported_as_archived(self):
        self.manifest("LLM/old.md")
        self.write(GOOD + "See `LLM/old.md`.\n")
        self.assertIn("cites archived LLM/old.md", self.run_check()[0])

    def test_existing_path_that_is_also_in_the_manifest_is_fine(self):
        self.manifest("LLM/real/x.md")
        self.write(GOOD + "See `LLM/real/x.md`.\n")
        self.assertEqual(self.run_check(), [])

    def test_name_description_size_and_symlink(self):
        self.write("---\nname: other\ndescription: Does things.\n---\n" + "x" * sc.MAX_BYTES)
        os.remove(os.path.join(self.claude, "demo"))
        text = "\n".join(self.run_check())
        for needle in ("name 'other'", "description must start", "bytes, over", "not a symlink"):
            self.assertIn(needle, text)


if __name__ == "__main__":
    unittest.main()
