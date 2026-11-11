#!/usr/bin/env python3
"""Checks the shape of CHANGELOG.md's "(unreleased)" entries.

    python3 -I scripts/changelog-check.py [CHANGELOG.md]
    git show :CHANGELOG.md | python3 -I scripts/changelog-check.py -

The rules, as CHANGELOG.md's intro states them (keep the two in step):
"Each change opens with a headline of six words or fewer (eight when a
feature's name takes two), a plain statement ending in a full stop with no
colon, dash or parenthesis, then one sentence of detail", and the fixes go
"under the closing "Fixes:"".

What this checks, in every entry whose heading says "(unreleased)":

- each bullet's headline, its first sentence, has eight words or fewer and
  no colon or parenthesis;
- the fixes come last, as sub-bullets ("  - ") under one "- Fixes:" bullet,
  with nothing else under it (no heading, no other kind of bullet, no
  prose), and no sub-bullet sits above it;

and, anywhere in the file, no merge conflict's marker is left in.

The releases page shows a headline plain and the rest muted, and reads the
fixes from the "- Fixes:" bullet, so an entry that breaks these shows wrong
there. Released entries are left as they were published.

With no argument it reads the repository's CHANGELOG.md; "-" reads standard
input. Prints one line per problem and exits 1 when there is any.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
HEADLINE_WORDS = 8
MARKER = re.compile(r"\s*(<{7}|\|{7}|={7}|>{7})( |$)")
SUB_BULLET = re.compile(r"  +- ")
# A wrapped line of a sub-bullet: indented past the "  - ", and not a list
# item or a heading of its own.
CONTINUATION = re.compile(r"   +[^\s*+#-]")
MARKER_PROBLEM = "it has a merge conflict's marker left in"


def headline(text: str) -> str:
    """The bullet's first sentence, split as the releases page splits it
    (apps/site/src/lib/changelog.ts `sentences`): at the first ". " followed
    by a capital letter outside a `code` run, or at the end."""
    for m in re.finditer(r"\. (?=[A-Z])", text):
        before = text[: m.start() + 1]
        if before.count("`") % 2 == 0:
            return before
    return text


def headline_problem(text: str) -> str:
    """What's wrong with a bullet's headline, `text` being the bullet without
    its "- ", or "" when nothing is."""
    first = headline(text)
    words = len(first.split())
    if words > HEADLINE_WORDS:
        return f"opens with {words} words before its first full stop (at most {HEADLINE_WORDS})"
    if ":" in first or "(" in first:
        return "has a colon or a parenthesis in its headline"
    return ""


def fixes_block(body: list[str]) -> tuple[set[int], int | None]:
    """The lines of an entry's "- Fixes:" bullet, `body` being the entry's
    lines below its heading: the bullet, its sub-bullets, their continuation
    lines and the blank lines between them. Second, the index of the first
    other line under the bullet, which ends it (None when only blank lines
    follow). No "- Fixes:" line: an empty set and None."""
    if "- Fixes:" not in body:
        return set(), None
    start = body.index("- Fixes:")
    block, blanks = {start}, []
    for k in range(start + 1, len(body)):
        line = body[k]
        if not line.strip():
            blanks.append(k)
        elif SUB_BULLET.match(line) or (CONTINUATION.match(line) and len(block) > 1):
            block.update(blanks, {k})
            blanks = []
        else:
            return block, k
    return block, None


def entry_problems(body: str) -> list[str]:
    """What's out of place in a changelog entry, `body` being its text below
    the heading: the fixes go last, as sub-bullets under one "- Fixes:"
    bullet, after the new behaviour, and every bullet opens with a short
    headline."""
    lines = body.split("\n")
    top = [line for line in lines if line.startswith("- ")]
    problems = []
    if top.count("- Fixes:") > 1:
        problems.append('it has more than one "- Fixes:" bullet')
    elif "- Fixes:" in top and top[-1] != "- Fixes:":
        problems.append('"- Fixes:" is not its last bullet: new behaviour goes above it')
    elif (stray := fixes_block(lines)[1]) is not None:
        problems.append(f"\"- Fixes:\" holds a line that isn't a fix's sub-bullet: \"{lines[stray].strip()[:40]}\"")
    fixes = lines.index("- Fixes:") if "- Fixes:" in lines else len(lines)
    if any(line.startswith("  - ") for line in lines[:fixes]):
        problems.append('it has sub-bullets above "- Fixes:": a fix goes under it')
    if any(MARKER.match(line) for line in lines):
        problems.append(MARKER_PROBLEM)
    for line in lines:
        bullet = re.match(r"\s*- (.*)", line)
        if bullet and bullet[1] != "Fixes:" and (why := headline_problem(bullet[1])):
            problems.append(f'the bullet "{" ".join(bullet[1].split()[:5])}..." {why}')
    return problems


def problems(changelog: str) -> list[str]:
    """Every problem in the changelog: its "(unreleased)" entries' shape, and
    a conflict's marker on any line (named by line, so the entries' own
    report of it is left out)."""
    found = [
        f"CHANGELOG.md:{n}: a merge conflict's marker is left in"
        for n, line in enumerate(changelog.split("\n"), 1)
        if MARKER.match(line)
    ]
    for section in ("\n" + changelog).split("\n## ")[1:]:
        heading, _, body = section.partition("\n")
        if "(unreleased)" in heading:
            found += [
                f"CHANGELOG.md's entry for {heading.split()[0]}: {p}"
                for p in entry_problems(body.strip())
                if p != MARKER_PROBLEM
            ]
    return found


def main() -> None:
    args = sys.argv[1:]
    if len(args) > 1:
        sys.exit(__doc__)
    if args == ["-"]:
        text = sys.stdin.read()
    else:
        text = Path(args[0] if args else ROOT / "CHANGELOG.md").read_text(encoding="utf-8")
    found = problems(text)
    for p in found:
        print(p)
    sys.exit(1 if found else 0)


if __name__ == "__main__":
    main()
