#!/usr/bin/env python3
"""Refuses comments and test names that cite what a reader of this repository
can't see, or code by its line number.

    python3 -I scripts/citations-check.py            every tracked source file
    python3 -I scripts/citations-check.py --staged   the staged ones, as staged
    python3 -I scripts/citations-check.py FILE...    those files

It reads the comments and doc comments of the source files in crates/, apps/,
tests/, scripts/, .github/ and .githooks/ (Rust, TypeScript, JavaScript,
Svelte, CSS, Python, shell and YAML), and the names JavaScript tests are
given (the first string of `test(`, `it(` and `describe(`), and refuses in
them:

- a path to a plan (`plans/<name>.md`), a handbook report's (`reports/<name>.md`),
  the handbook, a plan by its version, or a plan's section name
  ("sections-074", a walkthrough with its date or version): those live
  outside this repository;
- a document this repository doesn't have (`testing.md`, `TODO.md`, a
  review's or a report's file);
- the owner as the one who said or decided ("the owner's rule", "as the
  owner said", "Owner's call", an owner with a date): a spec says what was
  decided, in plain words, not who said it when; a file's or a window's
  owner is no problem;
- a code citation by line number, of a file of any kind (`clips.rs:744`,
  `clips.rs line 744`, `clips.rs#L744`, `Cargo.toml:12`): lines move, so
  name the file and the symbol instead, `clips.rs` `delete_clip`.

Other strings and code are not read, so a data field named `owner` is no
problem; but a JavaScript template or a Rust raw string may hold a page's
code, so their // lines are read as comments. A comment that must say one
of these anyway (quoting a tool's output, say) takes the marker
"citations: allow <why>" on the same line; a marker without its reason is
refused, and every run prints how many markers it read. The files listed in scripts/citations-allow.txt are left out; that
list is for files not yet rewritten, and is to be emptied. A listed file with
nothing left to refuse is reported, so it comes off the list. This script and
its tests, which must spell out what they refuse, are not read.

Prints one line per problem and exits 1 when there is any.
"""

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ALLOW_FILE = ROOT / "scripts" / "citations-allow.txt"
DIRS = ("crates/", "apps/", "tests/", "scripts/", ".github/", ".githooks/")
SELF = ("scripts/citations-check.py", "scripts/test_citations_check.py")
JS = {".ts", ".tsx", ".mjs", ".js", ".cjs"}
SLASH = JS | {".rs", ".svelte", ".css"}
YAML = {".yml", ".yaml"}
HASH = {".py", ".sh"} | YAML
ALLOW_MARK = re.compile(r"citations: allow\b[\s:,-]*(\w)?")

DATE = r"\d{4}-\d\d-\d\d"
# What the owner does in a comment that quotes him.
SAYS = (
    r"said|says|asked|asks|decided|decides|wants|wanted|chose|picked|agreed|prefers|preferred"
    r"|likes|liked|called|noted|found|saw|reported|wrote|thinks|thought|approved|rejected"
)

RULES = [
    (
        re.compile(
            rf"\bplans/[\w./-]+|(?<![\w./-])reports/[\w./-]+\.md\b|\bsections-\d[\d.]*"
            rf"|\bwalkthrough\b\W*(?:\w+\W+){{0,3}}?(?:{DATE}|\d+\.\d+)"
            rf"|(?:{DATE}|\d+\.\d+\.\d+)\W+(?:\w+\W+){{0,2}}?walkthrough\b"
            r"|\bhandbook\b|\bplan (?:for )?v?\d+\.\d+",
            re.IGNORECASE,
        ),
        "cites a private plan: state the decision itself in plain words",
    ),
    (
        re.compile(
            rf"\bthe owner(?:'s\b|\s+(?:{SAYS})\b|(?=\s*(?:[^\w\s]|$)))"
            rf"|(?:^|[^\w\s])\s*owner(?:'s\b|\s+(?:{SAYS})\b)"
            rf"|\bowner\b\W+(?:\w+\W+){{0,3}}?{DATE}|\(owner\)"
            rf"|\bAirodonack\s+(?:{SAYS})\b",
            re.IGNORECASE,
        ),
        'names the owner: say what was decided, not who said it ("Decided for 0.7.4: ...")',
    ),
    (
        re.compile(
            r"\b[\w./-]*\w\.(?!(?:com|org|net|io|dev)\b)[A-Za-z]\w{0,9}(?::[ \t]*\d+|#L\d+|,?[ \t]+lines?[ \t]+\d+)"
            r"|\blines?[ \t]+\d+(?:[ \t]*[-–][ \t]*\d+)?[ \t]+(?:of|in)[ \t]+`?[\w./-]+\.[A-Za-z]\w*",
            re.IGNORECASE,
        ),
        "cites code by line number: name the file and the symbol (`clips.rs` `delete_clip`)",
    ),
]


# A document named in a comment: its path, or its name.
DOC = re.compile(r"[\w./-]*\b[\w-]+\.md\b")

# A Rust raw string's opening, `r#"`, and a character, `'a'` or `'\''`.
RAW = re.compile(r'b?r(#*)"')
CHAR = re.compile(r"'(\\.[^']*|[^\\'])'")

# The start of a shell heredoc, `<<EOF`, `<<-'EOF'`: its body is no comment.
HEREDOC = re.compile(r"<<(-?)[ \t]*(['\"]?)([A-Za-z_]\w*)\2")

# What a JavaScript test is called with, just before the string that names it.
TEST_CALL = re.compile(r"(?<![\w.$])(?:test|it|describe)(?:\.\w+)*\(\s*$")

# The keywords after which a `/` opens a JavaScript regex, not a division.
BEFORE_REGEX = re.compile(r"(?:^|[^\w$])(?:return|typeof|case|do|else|in|of|void|yield|await|delete|throw|new)\s*$")


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


def kind(path: str) -> str:
    """The language of `path`, by its suffix; a git hook has none, and is
    shell."""
    suffix = Path(path).suffix
    return ".sh" if not suffix and path.startswith(".githooks/") else suffix


def comments(text: str, suffix: str) -> list[tuple[int, str]]:
    """The comment text of a source file, as (line number, text) for each line
    a comment covers, and the names of its JavaScript tests. Other strings
    are skipped, so a `//` in a URL or a `#` in a string is no comment."""
    if suffix == ".svelte":
        return svelte_comments(text)
    if suffix in YAML:
        return yaml_comments(text)
    found: list[tuple[int, str]] = []
    i, line, n = 0, 1, len(text)
    hashes = suffix in HASH
    quotes = "\"'`" if suffix in JS else "\"'"
    heredocs: list[tuple[bool, str]] = []
    while i < n:
        c = text[i]
        if c == "\n":
            line += 1
            i += 1
            for tabs, word in heredocs:
                while i < n:
                    end = text.find("\n", i)
                    end = n if end < 0 else end
                    body = text[i:end]
                    i = min(end + 1, n)
                    line += 1
                    if (body.lstrip("\t") if tabs else body) == word:
                        break
            heredocs = []
        elif hashes and c == "#" and (suffix == ".py" or text[i - 1 : i] in ("", " ", "\t", "\n", ";", "|", "&", "(", ")")) or not hashes and text.startswith("//", i):
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
        elif suffix == ".sh" and text[i - 1 : i] != "<" and (doc := HEREDOC.match(text, i)):
            heredocs.append((doc[1] == "-", doc[3]))
            i = doc.end()
        elif suffix == ".rs" and (raw := RAW.match(text, i)) and not text[i - 1 : i].isalnum():
            close = '"' + raw[1]
            end = text.find(close, raw.end())
            end = n if end < 0 else end + len(close)
            found += code_comments(text[i:end], line)
            line += text.count("\n", i, end)
            i = end
        elif suffix in JS and c == "/" and opens_regex(text, i):
            i = regex_end(text, i)
        elif c in quotes:
            if suffix == ".rs" and c == "'" and not CHAR.match(text, i):
                i += 1  # a lifetime, not a character
                continue
            # A shell's single quotes take no escapes, but its $'...' does.
            escapes = not (suffix == ".sh" and c == "'" and text[i - 1 : i] != "$")
            if c == "`":
                end = template_end(text, i)
                found += code_comments(text[i:end], line)
            else:
                end = string_end(text, i, c, multiline=suffix == ".sh" or suffix == ".rs" and c == '"', escapes=escapes)
            if suffix in JS and TEST_CALL.search(text[max(0, i - 80) : i]):
                found += [(line + k, part) for k, part in enumerate(text[i:end].split("\n"))]
            line += text.count("\n", i, end)
            i = end
        else:
            i += 1
    return found


def code_comments(string: str, line: int) -> list[tuple[int, str]]:
    """The // lines of a string, starting on `line`, that holds code for a
    page: a JavaScript template or a Rust raw string may, and its comments
    are read."""
    return [(line + k, part) for k, part in enumerate(string.split("\n")) if part.lstrip().startswith("//")]


def string_end(text: str, i: int, quote: str, multiline: bool, escapes: bool = True) -> int:
    """Where the string opening at `i` with `quote` ends: past its close, or
    at the line's end for a one-line string left open."""
    j = i + 1
    while j < len(text):
        if text[j] == "\\" and escapes:
            j += 2
        elif text[j] == quote:
            return j + 1
        elif text[j] == "\n" and not multiline:
            return j
        else:
            j += 1
    return len(text)


def template_end(text: str, i: int) -> int:
    """Where the JavaScript template opening at `i` ends, past its close;
    the code of each `${...}` in it may hold strings, regexes and templates
    of its own."""
    j = i + 1
    while j < len(text):
        if text[j] == "\\":
            j += 2
        elif text[j] == "`":
            return j + 1
        elif text.startswith("${", j):
            j, depth = j + 2, 0
            while j < len(text) and not (text[j] == "}" and depth == 0):
                if text[j] == "`":
                    j = template_end(text, j)
                    continue
                if text[j] in "\"'":
                    j = string_end(text, j, text[j], multiline=False)
                    continue
                if text[j] == "/" and opens_regex(text, j):
                    j = regex_end(text, j)
                    continue
                depth += {"{": 1, "}": -1}.get(text[j], 0)
                j += 1
            j += 1
        else:
            j += 1
    return len(text)


def opens_regex(text: str, i: int) -> bool:
    """Whether the `/` at `i` opens a regex literal: it does where a value
    is expected, after an operator, a bracket opened or a keyword, and not
    after a value, where it divides."""
    j = i - 1
    while j >= 0 and text[j] in " \t\n":
        j -= 1
    return j < 0 or text[j] in "(,=:[!&|?{};+-*%<>~^" or bool(BEFORE_REGEX.search(text[max(0, j - 11) : j + 1]))


def regex_end(text: str, i: int) -> int:
    """Where the regex literal opening at `i` ends: past its closing `/`,
    a `/` in a [class] or escaped not closing it. One that doesn't close on
    its line was no regex, so only its `/` is passed."""
    j, in_class = i + 1, False
    while j < len(text) and text[j] != "\n":
        if text[j] == "\\":
            j += 2
            continue
        if text[j] == "[":
            in_class = True
        elif text[j] == "]":
            in_class = False
        elif text[j] == "/" and not in_class:
            return j + 1
        j += 1
    return i + 1


def yaml_comments(text: str) -> list[tuple[int, str]]:
    """A YAML file's comments: from a `#` that starts a line or follows a
    space, to the line's end. Quotes aren't followed, since a run step's
    shell has its own."""
    found = []
    for k, part in enumerate(text.split("\n")):
        if m := re.search(r"(?:^|(?<=\s))#", part):
            found.append((k + 1, part[m.start() :]))
    return found


def svelte_comments(text: str) -> list[tuple[int, str]]:
    """A Svelte file's comments: its markup's <!-- --> and // lines, and
    those of its <script> and <style> blocks."""
    found: list[tuple[int, str]] = []
    for m in re.finditer(r"<!--.*?-->", text, re.DOTALL):
        line = text.count("\n", 0, m.start()) + 1
        found += [(line + k, part) for k, part in enumerate(m[0].split("\n"))]
    blocks = list(re.finditer(r"(<(script|style)\b[^>]*>)(.*?)</\2>", text, re.DOTALL))
    for m in blocks:
        line = text.count("\n", 0, m.start(3))
        found += [(line + n, part) for n, part in comments(m[3], ".ts")]
    # The markup's code, in its { }, has // comments of its own lines.
    starts = [m.start() for m in blocks]
    ends = [m.end() for m in blocks]
    at = 0
    for k, part in enumerate(text.split("\n")):
        if part.lstrip().startswith("//") and not any(a <= at < b for a, b in zip(starts, ends)):
            found.append((k + 1, part))
        at += len(part) + 1
    return sorted(set(found))


def problems(path: str, text: str, docs: set[str], markers: list[str] | None = None) -> list[str]:
    """What's refused in the comments of `path`, whose content is `text`,
    `docs` being the names of the repository's own documents. Each allow
    marker read is added to `markers`, when given."""
    found = []
    for line, part in comments(text, kind(path)):
        if mark := ALLOW_MARK.search(part):
            if markers is not None:
                markers.append(f"{path}:{line}")
            if not mark[1]:
                found.append(f"{path}:{line}: an allow marker without its reason: write \"citations: allow <why>\"")
            continue
        here = [f"{path}:{line}: {why}: {m[0]!r}" for pattern, why in RULES if (m := pattern.search(part))]
        if not here:
            here = [f"{path}:{line}: names a document this repository doesn't have: {name!r}" for name in unknown_docs(part, docs)]
        found += here
    return found


def in_scope(path: str) -> bool:
    return (
        path.startswith(DIRS)
        and path not in SELF
        and "node_modules/" not in path
        and kind(path) in SLASH | HASH
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
    found: list[str] = []
    markers: list[str] = []
    for path in paths:
        text = git("show", f":{path}") if staged else (ROOT / path).read_text(encoding="utf-8")
        mine = problems(path, text, docs, markers)
        if path in skip:
            if not mine and not staged and not args:
                found.append(f"{path}: nothing left to refuse: take it off scripts/citations-allow.txt")
            continue
        found += mine
    if not staged and not args:
        found += [f"scripts/citations-allow.txt: {p} isn't a source file here" for p in sorted(skip - set(paths))]
    for p in found:
        print(p)
    print(f"{len(markers)} allow marker{'' if len(markers) == 1 else 's'}")
    sys.exit(1 if found else 0)


if __name__ == "__main__":
    main()
