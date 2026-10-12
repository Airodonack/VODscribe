"""Exports the demo's VODs from a VODscribe library into static/demo/.

Transcribe the VODs with the dev CLI into a fresh library first:

    VODSCRIBE_DATA=/tmp/demo vodscribe-cli add <url>      (once per VOD)
    python3 apps/site/scripts/export-demo.py /tmp/demo/library.sqlite

Writes vods.json (the library, in library order), <id>.json for each
transcript and thumbs/<id>.jpg. A transcript is a list of segments, each
[text, start, [start of each word]], where the words are the text split on spaces.
"""

import json
import sqlite3
import subprocess
import sys
import urllib.request
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "static" / "demo"


def main(db_path: str) -> None:
    db = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
    db.row_factory = sqlite3.Row
    (OUT / "thumbs").mkdir(parents=True, exist_ok=True)
    vods = []
    for v in db.execute("SELECT * FROM vods WHERE status = 'done' ORDER BY position, id DESC"):
        thumb = f"{v['id']}.jpg"
        if v["thumbnail"]:
            req = urllib.request.Request(v["thumbnail"], headers={"User-Agent": "Mozilla/5.0"})
            raw = OUT / "thumbs" / f"{v['id']}.orig"
            raw.write_bytes(urllib.request.urlopen(req, timeout=30).read())
            # 960 px wide is plenty for the player's poster, and drops the metadata.
            subprocess.run(
                ["ffmpeg", "-v", "error", "-y", "-i", raw, "-vf", "scale=960:-2", "-q:v", "4", OUT / "thumbs" / thumb],
                check=True,
            )
            raw.unlink()
        vods.append(
            {
                "id": v["id"],
                "platform": v["platform"],
                "platform_id": v["platform_id"],
                "url": v["url"],
                "title": v["title"],
                "channel": v["channel"],
                "duration": v["duration"],
                "thumbnail": None,
                "thumb": thumb if v["thumbnail"] else None,
                "source_path": None,
                "published": v["published"],
                "status": "done",
                "progress": 1,
                "error": None,
                "added_at": v["added_at"],
                "note": None,
                "outdated": False,
            }
        )
        segments = []
        for text, words in db.execute(
            "SELECT text, words FROM segments WHERE vod_id = ? ORDER BY start", (v["id"],)
        ):
            words = json.loads(words)
            assert " ".join(w["text"] for w in words) == text, text
            starts = [round(w["start"], 2) for w in words]
            segments.append([text, starts[0] if starts else 0, starts])
        (OUT / f"{v['id']}.json").write_text(json.dumps(segments, separators=(",", ":"), ensure_ascii=False))
        print(f"#{v['id']} {v['title']}: {len(segments)} segments")
    (OUT / "vods.json").write_text(json.dumps(vods, indent=2, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    main(sys.argv[1])
