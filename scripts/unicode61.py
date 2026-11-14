#!/usr/bin/env python3
"""Writes crates/core/src/transcript/unicode61.rs: how SQLite's
`unicode61 remove_diacritics 2` tokenizer, the search index's, treats each
character past ASCII, asked of SQLite itself. Run it when SQLite's tokenizer
changes; transcript/tests.rs checks the table against the app's own SQLite,
the one rusqlite bundles, whose version the table's header names.
"""

import json
import re
import sqlite3
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "crates/core/src/transcript/unicode61.rs"


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


def rows(items, per_line):
    return "\n".join("    " + " ".join(items[i : i + per_line]) for i in range(0, len(items), per_line))


def main():
    bundled = bundled_sqlite()
    chars = tokenized()
    separators = []
    for c, became in chars.items():
        if became is None:
            if separators and separators[-1][1] == c - 1:
                separators[-1][1] = c
            else:
                separators.append([c, c])
    folds = [(c, ord(b) if b else 0) for c, b in chars.items() if b is not None and b != chr(c)]
    OUT.write_text(
        f"""//! How the search index's tokenizer, SQLite's `unicode61 remove_diacritics
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
    )
    print(f"{OUT}: {len(separators)} separator ranges, {len(folds)} folds", file=sys.stderr)
    print(f"asked of SQLite {sqlite3.sqlite_version}, for the app's {bundled}", file=sys.stderr)


main()
