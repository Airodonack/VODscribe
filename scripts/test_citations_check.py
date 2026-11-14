"""Tests for citations-check.py, the check of what comments cite:
python3 -I scripts/test_citations_check.py"""

import importlib.util
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("citations_check", HERE / "citations-check.py")
assert spec is not None and spec.loader is not None
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)

DOCS = {"README.md", "CHANGELOG.md"}


def found(path: str, text: str) -> list[str]:
    return check.problems(path, text, DOCS)


class Refused(unittest.TestCase):
    def test_a_plan_path(self):
        self.assertEqual(len(found("a.rs", '/// Spec (plans/sections-074.md, 2C).\nfn a() {}\n')), 1)

    def test_a_section_name(self):
        self.assertEqual(len(found("a.mjs", "// as sections-074 2A1 says\n")), 1)
        self.assertEqual(len(found("a.ts", "/* walkthrough-0.7.2 step 20 */\n")), 1)

    def test_the_owner(self):
        self.assertEqual(len(found("a.rs", "// The owner's rule.\n")), 1)
        self.assertEqual(len(found("a.rs", '//! "Yes" (owner, 2026-11-11).\n')), 1)

    def test_a_line_citation(self):
        problems = found("a.rs", '/// app clips.rs:744, `delete_clip`: "One only".\n')
        self.assertEqual(len(problems), 1)
        self.assertIn("a.rs:1:", problems[0])
        self.assertIn("clips.rs:744", problems[0])

    def test_a_document_the_repository_lacks(self):
        self.assertEqual(len(found("a.rs", "// a known stall (TODO.md)\n")), 1)

    def test_in_a_block_comment_on_its_own_line(self):
        problems = found("a.ts", 'const a = 1;\n/**\n * Fine.\n * dragout.ts:308 `madeAsWanted`.\n */\n')
        self.assertEqual(len(problems), 1)
        self.assertIn("a.ts:4:", problems[0])

    def test_in_a_svelte_files_markup_and_script(self):
        text = '<script lang="ts">\n  // see vods.rs:12\n</script>\n<p>it\'s</p>\n<!-- the owner -->\n'
        lines = sorted(p.split(":")[1] for p in found("A.svelte", text))
        self.assertEqual(lines, ["2", "5"])

    def test_in_a_python_docstring_and_comment(self):
        self.assertEqual(len(found("a.py", '"""See plans/x.md."""\nx = 1  # main.rs:4\n')), 2)


class Allowed(unittest.TestCase):
    def test_a_field_named_owner(self):
        self.assertEqual(found("a.rs", "struct Rule { owner: usize }\nlet a = Rule { owner: 1 };\n"), [])
        self.assertEqual(found("a.mjs", 'const w = { owner: "the owner", t: "x" };\n'), [])

    def test_strings_are_not_comments(self):
        text = 'let url = "https://example.com//x";\nlet s = "plans/a.md and clips.rs:4";\n'
        self.assertEqual(found("a.rs", text), [])
        self.assertEqual(found("a.ts", "const s = 'a // b clips.rs:4';\nconst t = `x // plans/a.md`;\n"), [])

    def test_a_lifetime_is_no_string(self):
        text = "fn a<'a>(s: &'a str) -> &'a str {\n    // see clips.rs:4\n    s\n}\n"
        self.assertEqual(len(found("a.rs", text)), 1)

    def test_a_raw_string(self):
        self.assertEqual(found("a.rs", 'let q = r#"// clips.rs:4 "the owner""#;\n'), [])

    def test_names_and_symbols(self):
        text = '/// app `clips.rs` `delete_clip`: "One only"; 0.7.1 Fixes: "Undo works."\n'
        self.assertEqual(found("a.rs", text + "// README.md and CHANGELOG.md, at 1:02:03\n"), [])

    def test_a_link_to_another_sites_document(self):
        self.assertEqual(found("a.rs", "// https://github.com/x/y/blob/main/docs/guide.md\n"), [])

    def test_the_marker(self):
        self.assertEqual(found("a.rs", "// ffmpeg says \"in.rs:3\" (citations: allow)\n"), [])

    def test_shell_variables_are_no_comment(self):
        self.assertEqual(found("a.sh", 'n=${#list[@]}\necho "$# # main.rs:4"\n'), [])
        self.assertEqual(len(found("a.sh", "x=1 # main.rs:4\n")), 1)


class Scope(unittest.TestCase):
    def test_source_files_in_the_checked_folders(self):
        self.assertTrue(check.in_scope("crates/core/src/db.rs"))
        self.assertTrue(check.in_scope("apps/desktop/src/lib/App.svelte"))
        self.assertTrue(check.in_scope("tests/e2e/run.mjs"))
        self.assertFalse(check.in_scope("scripts/release.py"))
        self.assertFalse(check.in_scope("apps/desktop/node_modules/x/index.js"))
        self.assertFalse(check.in_scope("crates/core/tests/data/vectors.json"))


if __name__ == "__main__":
    unittest.main()
