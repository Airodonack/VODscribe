#!/usr/bin/env python3
"""Writes how SQLite's `unicode61 remove_diacritics 2` tokenizer, the search
index's, treats each character past ASCII, asked of SQLite itself, as two
tables of the same characters: crates/core/src/transcript/unicode61.rs for
the app's `terms` and apps/site/src/lib/tauri/unicode61.ts for the website
demo's. Run it when SQLite's tokenizer changes; transcript/tests.rs checks
the Rust table against the app's own SQLite, the one rusqlite bundles, whose
version the tables' headers name.

    python3 scripts/unicode61.py           writes both tables
    python3 -I scripts/unicode61.py --check
                                           checks, without SQLite, that both
                                           files are what this script writes
                                           and say the same
"""

import json
import re
import sqlite3
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RUST = ROOT / "crates/core/src/transcript/unicode61.rs"
TS = ROOT / "apps/site/src/lib/tauri/unicode61.ts"


def bundled_sqlite():
    """The version of the SQLite the app bundles: libsqlite3-sys's, as
    Cargo.lock has it, from its sqlite3.h."""
    metadata = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked", "--manifest-path", str(ROOT / "crates/core/Cargo.toml")],
        capture_output=True,
        check=True,
        text=True,
    ).stdout
    package = next(p for p in json.loads(metadata)["packages"] if p["name"] == "libsqlite3-sys")
    header = (Path(package["manifest_path"]).parent / "sqlite3/sqlite3.h").read_text()
    return re.search(r'#define SQLITE_VERSION\s+"([^"]+)"', header).group(1)


def tokenized():
    """Each character past ASCII: None where it parts words, else what it
    becomes in a word (empty where it's dropped)."""
    db = sqlite3.connect(":memory:")
    db.executescript(
        "CREATE VIRTUAL TABLE t USING fts5 (x, tokenize = 'unicode61 remove_diacritics 2');"
        "CREATE VIRTUAL TABLE v USING fts5vocab (t, 'instance');"
    )
    chars = [c for c in range(0x80, 0x110000) if not 0xD800 <= c < 0xE000]
    # Between two z's, a character that parts words leaves two terms.
    db.executemany("INSERT INTO t (rowid, x) VALUES (?, ?)", ((c, f"z{chr(c)}z") for c in chars))
    found = {}
    for doc, term in db.execute("SELECT doc, term FROM v ORDER BY doc, offset"):
        found.setdefault(doc, []).append(term)
    out = {}
    for c in chars:
        terms = found[c]
        if terms == ["z", "z"]:
            out[c] = None
        else:
            assert len(terms) == 1 and len(terms[0]) <= 3, (hex(c), terms)
            out[c] = terms[0][1:-1]
    return out


def tables(chars):
    """The characters that part words, as inclusive ranges in order, and
    what each other character becomes where that's another, in order (0
    where it's dropped)."""
    separators = []
    for c, became in chars.items():
        if became is None:
            if separators and separators[-1][1] == c - 1:
                separators[-1][1] = c
            else:
                separators.append([c, c])
    folds = [(c, ord(b) if b else 0) for c, b in chars.items() if b is not None and b != chr(c)]
    return [tuple(s) for s in separators], folds


def runs(folds):
    """`folds` as runs of the same step: (first, last, step, becomes, by),
    each character `first + k * step` up to `last` becoming `becomes + k *
    by`. A run takes only folds next to each other in order, so no other
    fold falls inside one."""
    out = []
    for c, b in folds:
        if out:
            first, last, step, becomes, by = out[-1]
            if first == last and c - first <= 2:
                out[-1] = [first, c, c - first, becomes, b - becomes]
                continue
            if first != last and c == last + step and b == becomes + (c - first) // step * by:
                out[-1][1] = c
                continue
        out.append([c, c, 1, b, 0])
    return [tuple(r) for r in out]


def rows(items, per_line):
    return "\n".join("    " + " ".join(items[i : i + per_line]) for i in range(0, len(items), per_line))


def rust(bundled, separators, folds):
    return f"""//! How the search index's tokenizer, SQLite's `unicode61 remove_diacritics
//! 2`, treats each character past ASCII, as the app's SQLite {bundled} does:
//! written by scripts/unicode61.py, checked against it by transcript/tests.rs.

/// The characters that part words, as inclusive ranges in order.
#[rustfmt::skip]
pub(super) const SEPARATORS: &[(u32, u32)] = &[
{rows([f"(0x{a:X}, 0x{b:X})," for a, b in separators], 5)}
];

/// What a character becomes in a word, where that's another (lowercase, a
/// Latin letter's accent taken off), in order; 0 where it's dropped.
#[rustfmt::skip]
pub(super) const FOLDS: &[(u32, u32)] = &[
{rows([f"(0x{a:X}, 0x{b:X})," for a, b in folds], 5)}
];
"""


def typescript(bundled, separators, folds):
    def numbers(groups, per_line):
        return "\n".join(
            "  " + " ".join(",".join(str(n) for n in g) + "," for g in groups[i : i + per_line])
            for i in range(0, len(groups), per_line)
        )

    return f"""// How the search index's tokenizer, SQLite's `unicode61 remove_diacritics
// 2`, treats each character past ASCII, as the app's SQLite {bundled} does:
// written by scripts/unicode61.py from the same characters as the app's
// table (crates/core transcript/unicode61.rs), for query.ts `terms`.

/** The characters that part words: inclusive ranges in order, two numbers each. */
// prettier-ignore
export const SEPARATORS: readonly number[] = [
{numbers(separators, 8)}
];

/** What a character becomes in a word, where that's another (lowercase, a
 *  Latin letter's accent taken off), in order: five numbers a run, `first,
 *  last, step, becomes, by`, so `first + k * step` up to `last` becomes
 *  `becomes + k * by`, and one that becomes 0 is dropped. */
// prettier-ignore
export const FOLDS: readonly number[] = [
{numbers(runs(folds), 4)}
];
"""


def read_rust():
    """The SQLite version, separators and folds unicode61.rs holds."""
    text = RUST.read_text()
    bundled = re.search(r"as the app's SQLite (\S+) does", text).group(1)
    pairs = lambda part: [(int(a, 16), int(b, 16)) for a, b in re.findall(r"\(0x([0-9A-F]+), 0x([0-9A-F]+)\)", part)]
    separators, folds = text.split("pub(super) const FOLDS")
    return bundled, pairs(separators), pairs(folds)


def check():
    bundled, separators, folds = read_rust()
    ok = True
    for path, want in ((RUST, rust(bundled, separators, folds)), (TS, typescript(bundled, separators, folds))):
        if not path.exists() or path.read_text() != want:
            name = path.relative_to(ROOT)
            print(f"{name} isn't what scripts/unicode61.py writes from unicode61.rs's table", file=sys.stderr)
            ok = False
    if not ok:
        print("run python3 scripts/unicode61.py, then cargo test -p vodscribe-core transcript", file=sys.stderr)
        return 1
    print(f"unicode61 tables current and alike: {len(separators)} separator ranges, {len(folds)} folds")
    return 0


def main():
    if sys.argv[1:] == ["--check"]:
        return check()
    if sys.argv[1:]:
        print(__doc__, file=sys.stderr)
        return 2
    bundled = bundled_sqlite()
    separators, folds = tables(tokenized())
    RUST.write_text(rust(bundled, separators, folds))
    TS.write_text(typescript(bundled, separators, folds))
    print(f"{RUST}, {TS}: {len(separators)} separator ranges, {len(folds)} folds", file=sys.stderr)
    print(f"asked of SQLite {sqlite3.sqlite_version}, for the app's {bundled}", file=sys.stderr)
    return 0


sys.exit(main())
