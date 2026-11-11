"""Tests for changelog-check.py, the shape check of CHANGELOG.md's unreleased
entries:
python3 -I scripts/test_changelog_check.py"""

import importlib.util
import subprocess
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("changelog_check", HERE / "changelog-check.py")
assert spec is not None and spec.loader is not None
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)

HEAD = "# Changelog\n\nIntro.\n\n"
OLD = "\n## 0.7.3 — 2026-11-08\n\n- Old thing here. Detail.\n- Fixes:\n  - Old fix here. Detail.\n"


def log(entry_body: str) -> str:
    return HEAD + "## 0.7.4 (unreleased)\n\n" + entry_body + OLD


class WholeFile(unittest.TestCase):
    def test_a_good_entry_passes(self):
        self.assertEqual(check.problems(log("- New thing here. Detail.\n- Fixes:\n  - A fix here. Detail.\n")), [])

    def test_only_fixes_passes(self):
        self.assertEqual(check.problems(log("- Fixes:\n  - A fix here. Detail.\n  - Another fix here. Detail.\n")), [])

    def test_new_behaviour_below_fixes_is_refused(self):
        found = check.problems(log("- Fixes:\n  - A fix here. Detail.\n- New thing here. Detail.\n"))
        self.assertTrue(any("not its last bullet" in p for p in found), found)

    def test_a_fix_above_fixes_is_refused(self):
        found = check.problems(log("- New thing here. Detail.\n  - A fix here. Detail.\n- Fixes:\n  - Fix. Detail.\n"))
        self.assertTrue(any("sub-bullets above" in p for p in found), found)

    def test_a_long_headline_is_refused(self):
        found = check.problems(log("- This headline has far more than eight words in it. Detail.\n"))
        self.assertTrue(any("bullet" in p for p in found), found)

    def test_a_conflict_marker_is_refused_wherever_it_is(self):
        text = log("- Fixes:\n<<<<<<< HEAD\n  - A fix here. Detail.\n=======\n  - Other fix. Detail.\n>>>>>>> abc (x)\n")
        found = check.problems(text)
        self.assertEqual(sum("marker is left in" in p for p in found), 3, found)
        # In a released entry too, where nothing else is checked.
        self.assertEqual(len(check.problems(log("- New thing here. Detail.\n") + "=======\n")), 1)

    def test_released_entries_are_not_held_to_it(self):
        old_bad = "\n## 0.1.0 — 2026-01-01\n\n- Fixes:\n  - Fix. Detail.\n- Feature after fixes. Detail.\n"
        self.assertEqual(check.problems(log("- New thing here. Detail.\n") + old_bad), [])

    def test_every_unreleased_entry(self):
        text = HEAD + "## 0.8.0 (unreleased)\n\n- Fixes:\n  - A fix.\n- New.\n\n" + log("- New.\n")
        found = check.problems(text)
        self.assertEqual(len(found), 1, found)
        self.assertIn("entry for 0.8.0", found[0])


class EntryShape(unittest.TestCase):
    def test_fixes_last_as_sub_bullets(self):
        self.assertEqual(check.entry_problems("- New.\n- Also new.\n- Fixes:\n  - A fix.\n  - Another."), [])
        self.assertEqual(check.entry_problems("- New only."), [])

    def test_new_behaviour_under_fixes(self):
        self.assertEqual(len(check.entry_problems("- Fixes:\n  - A fix.\n- New, below the fixes.")), 1)
        self.assertEqual(len(check.entry_problems("- Fixes:\n  - A fix.\n- Fixes:\n  - Another.")), 1)

    def test_a_fix_above_fixes(self):
        self.assertEqual(len(check.entry_problems("- New.\n  - A fix out of place.\n- Fixes:\n  - A fix.")), 1)

    def test_only_fixes_under_fixes(self):
        # A heading or another kind of bullet under "- Fixes:" would show on
        # the releases page as a fix; the entry is refused instead.
        stray = check.entry_problems("- New.\n- Fixes:\n  - A fix.\n### New\n* A whole new editor.")
        self.assertEqual(len(stray), 1)
        self.assertIn('"### New"', stray[0])
        self.assertEqual(len(check.entry_problems("- New.\n- Fixes:\n  - A fix.\n* A whole new editor.")), 1)
        self.assertEqual(len(check.entry_problems("- New.\n- Fixes:\n  - A fix.\nA line of prose.")), 1)
        # A wrapped line of a fix, and a blank line between two, belong to it.
        self.assertEqual(check.entry_problems("- New.\n- Fixes:\n  - A fix.\n    Wrapped.\n\n  - Another."), [])

    def test_the_fixes_block(self):
        body = ["Prose.", "", "- New.", "- Fixes:", "  - A fix.", "    Wrapped.", "", "  - Two.", "", "### Stray"]
        self.assertEqual(check.fixes_block(body), ({3, 4, 5, 6, 7}, 9))
        self.assertEqual(check.fixes_block(body[:-1]), ({3, 4, 5, 6, 7}, None))
        self.assertEqual(check.fixes_block(["- New."]), (set(), None))
        # A continuation needs a sub-bullet above it.
        self.assertEqual(check.fixes_block(["- Fixes:", "    Wrapped."]), ({0}, 1))

    def test_a_stray_line_is_refused(self):
        # Whatever line ends the "- Fixes:" bullet early, the entry is refused.
        for body in [
            ["- New.", "- Fixes:", "  - A fix.", "### New", "* A whole new editor."],
            ["- New.", "- Fixes:", "  - A fix.", "* A whole new editor."],
            ["- New.", "- Fixes:", "  - A fix.", "A line of prose."],
            ["- New.", "- Fixes:", "  - A fix.", "- New, below the fixes."],
            ["- Fixes:", "    Wrapped, with no sub-bullet above."],
        ]:
            with self.subTest(body=body):
                self.assertIsNotNone(check.fixes_block(body)[1])
                self.assertNotEqual(check.entry_problems("\n".join(body)), [])

    def test_a_merge_conflicts_marker(self):
        body = "- New.\n- Fixes:\n  - A fix.\n    ||||||| parent of 0123abc (A fix)\n  - Another."
        self.assertEqual(len(check.entry_problems(body)), 1)


class Headline(unittest.TestCase):
    def test_a_long_headline(self):
        self.assertEqual(check.entry_problems("- Clips copy and paste. Ctrl+C copies, Ctrl+V pastes."), [])
        long = check.entry_problems("- Ctrl+C copies the selection or the clips picked, and Ctrl+V pastes them. More.")
        self.assertEqual(len(long), 1)
        self.assertIn('"Ctrl+C copies the selection or', long[0])
        # A fix is held to it too; the full stop ends the headline only before a capital.
        self.assertEqual(len(check.entry_problems("- New.\n- Fixes:\n  - One, two, three, four, five, six, seven, eight, nine.")), 1)
        self.assertEqual(len(check.entry_problems("- Pasting works in v. two of edits, and more things too.")), 1)

    def test_a_headline_with_a_colon_or_parenthesis(self):
        self.assertEqual(len(check.entry_problems("- Search learns from: names. More.")), 1)
        self.assertEqual(len(check.entry_problems("- Ctrl+A (⌘A) selects all. More.")), 1)
        # In the detail they are fine, and so is a full stop inside `code`.
        self.assertEqual(check.entry_problems("- Search learns more words. Such as `from:a. B` (or `in:chat`)."), [])

    def test_headline_problem(self):
        self.assertEqual(check.headline_problem("Eight words is the most it takes. Detail."), "")
        self.assertIn("9 words", check.headline_problem("Nine words is one more than it can take. Detail."))
        self.assertIn("colon", check.headline_problem("Search learns from: names. Detail."))
        self.assertEqual(check.headline_problem("Removing a VOD works without H.264. The stretch is kept."), "")


class CommandLine(unittest.TestCase):
    def run_check(self, *args, text=None):
        return subprocess.run(
            (sys.executable, "-I", str(HERE / "changelog-check.py"), *args), input=text, capture_output=True, text=True
        )

    def test_a_bad_entry_from_standard_input(self):
        done = self.run_check("-", text=log("- Fixes:\n  - A fix here. Detail.\n- New thing here. Detail.\n"))
        self.assertEqual(done.returncode, 1)
        self.assertEqual(done.stdout.splitlines(), [
            "CHANGELOG.md's entry for 0.7.4: \"- Fixes:\" is not its last bullet: new behaviour goes above it"
        ])

    def test_the_real_changelog_passes(self):
        done = self.run_check()
        self.assertEqual((done.returncode, done.stdout), (0, ""))
        self.assertEqual(check.problems((HERE.parent / "CHANGELOG.md").read_text(encoding="utf-8")), [])


if __name__ == "__main__":
    unittest.main()
