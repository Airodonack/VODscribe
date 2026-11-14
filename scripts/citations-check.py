#!/usr/bin/env python3
"""Refuses comments that cite what a reader of this repository can't see, or
code by its line number.

    python3 -I scripts/citations-check.py            every tracked source file
    python3 -I scripts/citations-check.py --staged   the staged ones, as staged
    python3 -I scripts/citations-check.py FILE...    those files

It reads the comments and doc comments of the source files in crates/, apps/
and tests/ (Rust, TypeScript, JavaScript, Svelte, CSS, Python and shell), and
refuses in them:

- a path to a plan (`plans/<name>.md`), or a plan's section name
  ("sections-074", "walkthrough-0.7.2"): those live outside this repository;
- a document this repository doesn't have (`testing.md`, `TODO.md`, a
  review's or a report's file);
- "the owner", or a dated "owner, 2026-...": a spec says what was decided, in
  plain words, not who said it when;
- a code citation by line number (`clips.rs:744`): lines move, so name the
  file and the symbol instead, `clips.rs` `delete_clip`.

Strings and code are not read, so a data field named `owner` is no problem.
A comment that must say one of these anyway (quoting a tool's output, say)
takes the marker "citations: allow" on the same line. The files listed in
scripts/citations-allow.txt are left out; that list is for files not yet
rewritten, and is to be emptied. A listed file with nothing left to refuse
is reported, so it comes off the list.

Prints one line per problem and exits 1 when there is any.
"""

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ALLOW_FILE = ROOT / "scripts" / "citations-allow.txt"
DIRS = ("crates/", "apps/", "tests/")
SLASH = {".rs", ".ts", ".mjs", ".js", ".cjs", ".svelte", ".css"}
HASH = {".py", ".sh"}
ALLOW_MARK = "citations: allow"

RULES = [
    (
        re.compile(r"\b(plans|reports)/[\w./-]+|\b(sections|walkthrough)-\d[\d.]*"),
        "cites a private plan: state the decision itself in plain words",
    ),
    (
        re.compile(r"\bthe owner\b|\bowner[,:]? \d{4}-\d\d-\d\d|\(owner\)", re.IGNORECASE),
        'names the owner: say what was decided, not who said it ("Decided for 0.7.4: ...")',
    ),
    (
        re.compile(r"\b[\w./-]+\.(rs|ts|tsx|mjs|cjs|js|svelte|py|css|sh):\d+"),
        "cites code by line number: name the file and the symbol (`clips.rs` `delete_clip`)",
    ),
]


# A document named in a comment: its path, or its name.
DOC = re.compile(r"[\w./-]*\b[\w-]+\.md\b")


def unknown_docs(part: str, docs: set[str]) -> list[str]:
    """The documents `part` names that the repository doesn't have, `docs`
    being the names of its own (README.md, CHANGELOG.md). A link's are
    another site's, so they're left alone."""
    names = []
    for m in DOC.finditer(part):
        word = part[: m.start()].rsplit(None, 1)[-1] if part[: m.start()].strip() else ""
        if "://" in word + m[0] or (part[: m.start()].endswith(("/", ":")) and "://" in word):
            continue
        if m[0].rsplit("/", 1)[-1] not in docs:
            names.append(m[0])
    return names


def comments(text: str, suffix: str) -> list[tuple[int, str]]:
    """The comment text of a source file, as (line number, text) for each line
    a comment covers. Strings are skipped, so a `//` in a URL or a `#` in a
    string is no comment."""
    if suffix == ".svelte":
        return svelte_comments(text)
    found: list[tuple[int, str]] = []
    i, line, n = 0, 1, len(text)
    hashes = suffix in HASH
    while i < n:
        c = text[i]
        if c == "\n":
            line += 1
            i += 1
        elif hashes and c == "#" and text[i - 1 : i] in ("", " ", "\t", "\n") or not hashes and text.startswith("//", i):
            end = text.find("\n", i)
            end = n if end < 0 else end
            found.append((line, text[i:end]))
            i = end
        elif not hashes and text.startswith("/*", i):
            end = text.find("*/", i + 2)
            end = n if end < 0 else end + 2
            for k, part in enumerate(text[i:end].split("\n")):
                found.append((line + k, part))
            line += text.count("\n", i, end)
            i = end
        elif suffix == ".py" and (text.startswith('"""', i) or text.startswith("'''", i)):
            # A docstring is a doc, so it is read as one.
            quote = text[i : i + 3]
            end = text.find(quote, i + 3)
            end = n if end < 0 else end + 3
            for k, part in enumerate(text[i:end].split("\n")):
                found.append((line + k, part))
            line += text.count("\n", i, end)
            i = end
        elif suffix == ".rs" and (raw := re.match(r'b?r(#*)"', text[i:])) and not text[i - 1 : i].isalnum():
            close = '"' + raw[1]
            end = text.find(close, i + raw.end())
            end = n if end < 0 else end + len(close)
            line += text.count("\n", i, end)
            i = end
        elif c in "\"'`":
            if suffix == ".rs" and c == "'" and not re.match(r"'(\\.[^']*|[^\\'])'", text[i:]):
                i += 1  # a lifetime, not a character
                continue
            if suffix == ".sh" and c == "`":
                i += 1
                continue
            end = string_end(text, i, c, multiline=c == "`" or suffix == ".sh" or suffix == ".rs" and c == '"')
            line += text.count("\n", i, end)
            i = end
        else:
            i += 1
    return found


def string_end(text: str, i: int, quote: str, multiline: bool) -> int:
    """Where the string opening at `i` with `quote` ends: past its close, or
    at the line's end for a one-line string left open."""
    j = i + 1
    while j < len(text):
        if text[j] == "\\":
            j += 2
        elif text[j] == quote:
            return j + 1
        elif text[j] == "\n" and not multiline:
            return j
        else:
            j += 1
    return len(text)


def svelte_comments(text: str) -> list[tuple[int, str]]:
    """A Svelte file's comments: its markup's <!-- --> and those of its
    <script> and <style> blocks."""
    found: list[tuple[int, str]] = []
    for m in re.finditer(r"<!--.*?-->", text, re.DOTALL):
        line = text.count("\n", 0, m.start()) + 1
        found += [(line + k, part) for k, part in enumerate(m[0].split("\n"))]
    for m in re.finditer(r"(<(script|style)\b[^>]*>)(.*?)</\2>", text, re.DOTALL):
        line = text.count("\n", 0, m.start(3))
        found += [(line + n, part) for n, part in comments(m[3], ".ts")]
    return sorted(found)


def problems(path: str, text: str, docs: set[str]) -> list[str]:
    """What's refused in the comments of `path`, whose content is `text`,
    `docs` being the names of the repository's own documents."""
    found = []
    for line, part in comments(text, Path(path).suffix):
        if ALLOW_MARK in part:
            continue
        here = [f"{path}:{line}: {why}: {m[0]!r}" for pattern, why in RULES if (m := pattern.search(part))]
        if not here:
            here = [f"{path}:{line}: names a document this repository doesn't have: {name!r}" for name in unknown_docs(part, docs)]
        found += here
    return found


def in_scope(path: str) -> bool:
    return (
        path.startswith(DIRS)
        and "node_modules/" not in path
        and Path(path).suffix in SLASH | HASH
    )


def allowed() -> set[str]:
    """The files left out, as scripts/citations-allow.txt lists them."""
    if not ALLOW_FILE.exists():
        return set()
    lines = ALLOW_FILE.read_text(encoding="utf-8").split("\n")
    return {line.strip() for line in lines if line.strip() and not line.startswith("#")}


def git(*args: str) -> str:
    return subprocess.run(["git", *args], cwd=ROOT, check=True, capture_output=True, text=True).stdout


def main() -> None:
    args = sys.argv[1:]
    if args[:1] in (["-h"], ["--help"]):
        sys.exit(__doc__)
    staged = args == ["--staged"]
    if staged:
        paths = git("diff", "--cached", "--name-only", "--diff-filter=d").split("\n")
    elif args:
        paths = [str(Path(a).resolve().relative_to(ROOT)) for a in args]
    else:
        paths = git("ls-files").split("\n")
    paths = [p for p in paths if p and in_scope(p)]
    skip = allowed()
    docs = {Path(p).name for p in git("ls-files", "*.md").split("\n") if p}
    found = []
    for path in paths:
        text = git("show", f":{path}") if staged else (ROOT / path).read_text(encoding="utf-8")
        mine = problems(path, text, docs)
        if path in skip:
            if not mine and not staged and not args:
                found.append(f"{path}: nothing left to refuse: take it off scripts/citations-allow.txt")
            continue
        found += mine
    if not staged and not args:
        found += [f"scripts/citations-allow.txt: {p} isn't a source file here" for p in sorted(skip - set(paths))]
    for p in found:
        print(p)
    sys.exit(1 if found else 0)


if __name__ == "__main__":
    main()
