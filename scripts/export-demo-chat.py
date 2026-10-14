"""Exports a VOD's chat from a VODscribe library for the website's demo, with
everyone in it given a made-up name.

    python3 -I scripts/export-demo-chat.py <library.sqlite> <vod id> <out.json> [--also WORD[=HANDLE] ...]

The chat comes as the app saved it (with its subs, gifts and tips recognized).
Each chatter, and anyone @mentioned, gets a name like "SleepyOtter", the same
one wherever theirs appears: as the author, in @mentions, written plainly, and
in the site's notices ("… subscribed at Tier 1"). The streamer's own channel
and service bots (StreamElements and the like) keep theirs: they're who the
VOD and its notices are about, not viewers.

Names a reader would still recognize take a look by hand: `--also Barry=BarryCarlyon`
gives a first name its account's made-up name, `--also somebody` a name of its
own. The demo says the names are made up (ChatMessage.svelte).

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
# @words that aren't anyone: Twitch's ping for the whole chat.
NOT_PEOPLE = {"channel", "everyone", "here"}
# A name to rename: letters, digits and underscores (so not the app's "?" for no author).
NAME = re.compile(r"\w+")


def fake_names(people: set[str], keep: set[str]) -> dict[str, str]:
    """A made-up name for each person (by lowercase name), fixed by a hash of the real one."""
    taken: set[str] = set()
    out: dict[str, str] = {}
    for author in sorted({p for p in people if NAME.fullmatch(p)} - keep):
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


def text_of(parts: str) -> str:
    return "".join(p.get("text", "") for p in json.loads(parts))


def main(library: Path, vod_id: int, out: Path, also: list[str]) -> None:
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
    people = {r[1].lower() for r in rows}
    people |= {m.lower() for r in rows for m in re.findall(r"@(\w+)", text_of(r[3]))} - NOT_PEOPLE
    words = {a.split("=")[0].lower(): a.partition("=")[2].lower() for a in also}
    names = fake_names(people | {w for w, handle in words.items() if not handle}, keep)
    for word, handle in words.items():
        if handle:
            if handle not in names:
                sys.exit(f"--also {word}={handle}: no one called {handle} in this chat")
            names[word] = names[handle]
    # Longest first, so a name that contains another is matched whole; never
    # inside a longer word (a URL's "?", say).
    alternatives = "|".join(re.escape(n) for n in sorted(names, key=len, reverse=True))
    pattern = re.compile(rf"(@?)(?<!\w)({alternatives})(?!\w)", re.IGNORECASE) if names else None

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
    args = sys.argv[1:]
    also = [args[i + 1] for i, a in enumerate(args) if a == "--also" and i + 1 < len(args)]
    rest = [a for i, a in enumerate(args) if a != "--also" and (i == 0 or args[i - 1] != "--also")]
    if len(rest) != 3:
        sys.exit(__doc__)
    main(Path(rest[0]), int(rest[1]), Path(rest[2]), also)
