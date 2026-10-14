"""Exports a VOD's chat from a VODscribe library for the website's demo, with
everyone in it given a made-up name.

    python3 -I scripts/export-demo-chat.py <library.sqlite> <vod id> <out.json>

The chat comes as the app saved it (with its subs, gifts and tips recognized).
Each chatter gets a name like "SleepyOtter", the same one wherever theirs
appears: as the author, in @mentions, and in the site's notices ("… subscribed
at Tier 1"). The streamer's own channel and service bots (StreamElements and
the like) keep theirs: they're who the VOD and its notices are about, not
viewers. The demo says the names are made up (ChatMessage.svelte).

Writes the messages as the app's `chat_windows` returns them (crates/core
`db::ChatLine`), in time order, ids from 1.
"""

import hashlib
import json
import re
import sqlite3
import sys
from pathlib import Path

ADJECTIVES = """Amber Brisk Calm Cosy Dapper Eager Fuzzy Gentle Glossy Jolly Lanky Lucky Mellow
Nimble Plucky Quiet Rapid Rusty Sleepy Snug Sunny Tidy Witty Zesty Breezy Bouncy Chirpy
Clever Cozy Dusky Fluffy Frosty Gusty Hasty Jumpy Leafy Merry Misty Peppy Perky Quirky
Rosy Silky Spry Swift Tawny Wiry Zippy""".split()
ANIMALS = """Otter Heron Badger Lynx Marten Puffin Gecko Ibis Koala Lemur Moose Newt Okapi Panda
Quail Raven Stoat Tapir Vole Walrus Yak Zebra Bison Crane Dingo Egret Ferret Gibbon Hare
Jackal Kiwi Llama Mole Narwhal Ocelot Pika Robin Seal Tern Urchin Viper Wombat Wren Finch
Owl Fox Mink""".split()

# Accounts that aren't viewers: the platforms' and services' own bots.
BOTS = {"streamelements", "nightbot", "moobot", "fossabot", "streamlabs", "kickbot", "botrix", "wizebot", "sery_bot"}


def fake_names(authors: list[str], keep: set[str]) -> dict[str, str]:
    """A made-up name for each author (by lowercase name), fixed by a hash of the real one."""
    taken: set[str] = set()
    out: dict[str, str] = {}
    for author in sorted({a.lower() for a in authors} - keep):
        digest = hashlib.sha256(author.encode()).digest()
        name = ADJECTIVES[digest[0] % len(ADJECTIVES)] + ANIMALS[digest[1] % len(ANIMALS)]
        candidate, n = name, 2
        while candidate in taken:
            candidate, n = f"{name}{n}", n + 1
        taken.add(candidate)
        out[author] = candidate
    return out


def rename_in(text: str, names: dict[str, str], pattern: re.Pattern[str] | None) -> str:
    """`text` with each real name (as a whole word, any case, with or without @) made up."""
    if pattern is None:
        return text
    return pattern.sub(lambda m: m.group(1) + names[m.group(2).lower()], text)


def main(library: Path, vod_id: int, out: Path) -> None:
    db = sqlite3.connect(f"file:{library}?mode=ro", uri=True)
    channel = db.execute("SELECT channel FROM vods WHERE id = ?", (vod_id,)).fetchone()
    if channel is None:
        sys.exit(f"no VOD {vod_id} in {library}")
    rows = db.execute(
        "SELECT time, author, color, parts, kind, amount FROM chat WHERE vod_id = ? ORDER BY time, id", (vod_id,)
    ).fetchall()
    if not rows:
        sys.exit(f"VOD {vod_id} has no chat in {library}")
    keep = BOTS | ({channel[0].lower()} if channel[0] else set())
    names = fake_names([r[1] for r in rows], keep)
    # Longest first, so a name that contains another is matched whole.
    alternatives = "|".join(re.escape(n) for n in sorted(names, key=len, reverse=True))
    pattern = re.compile(rf"(@?)\b({alternatives})\b", re.IGNORECASE) if names else None

    messages = []
    for i, (time, author, color, parts, kind, amount) in enumerate(rows, start=1):
        parts = [
            {**p, "text": rename_in(p["text"], names, pattern)} if p.get("type") == "text" else p
            for p in json.loads(parts)
        ]
        messages.append(
            {
                "id": i,
                "time": time,
                "author": names.get(author.lower(), author),
                "color": color,
                "parts": parts,
                "kind": kind,
                "amount": amount,
            }
        )
    out.write_text(json.dumps(messages, ensure_ascii=False, separators=(",", ":")) + "\n")
    print(f"{len(messages)} messages, {len(names)} chatters renamed, to {out}")


if __name__ == "__main__":
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    main(Path(sys.argv[1]), int(sys.argv[2]), Path(sys.argv[3]))
