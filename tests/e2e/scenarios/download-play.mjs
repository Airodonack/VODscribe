// A VOD from a site without a player the app can show (Dailymotion) plays
// as its video downloads (videos.rs): Play downloads it all, and the app's
// own player plays as far as it's in. When the download ends, the player
// goes on from the video kept, where it was: the picture stays, it plays
// on, and a seek after works. Once with no seek near the end, once with
// seeks all through it, one in flight as it ends.
//
// Offline: the VODs are files added here, then made Dailymotion's, their
// file gone from them; yt-dlp is a stand-in in the data folder's bin (put
// back after) that answers the site's lookup and "downloads" the file at
// four times its speed, as yt-dlp's ffmpeg writes a section: a growing
// Matroska part file, renamed when whole.

import { chmodSync, copyFileSync, existsSync, renameSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { DatabaseSync } from "node:sqlite";

import { MINUTE, until } from "../lib/app.mjs";
import { dataDirs } from "../lib/data.mjs";
import { minute } from "../lib/fixtures.mjs";

/** How many times its speed the stand-in reads the file: a minute in about 15 s. */
const RATE = 4;

/** A yt-dlp that answers any lookup with `info` and downloads `file`, slowly. */
function standIn(file, info) {
  const quote = (s) => `'${s.replaceAll("'", `'\\''`)}'`;
  return `#!/bin/sh
# The e2e tests' stand-in for yt-dlp (tests/e2e/scenarios/download-play.mjs).
json=0; out=""; ffmpeg=""
while [ $# -gt 0 ]; do
  case "$1" in
    -J) json=1 ;;
    -o) out="$2"; shift ;;
    --ffmpeg-location) ffmpeg="$2"; shift ;;
  esac
  shift
done
if [ "$json" = 1 ]; then
  printf '%s\\n' ${quote(JSON.stringify(info))}
  exit 0
fi
stem="\${out%.%(ext)s}"
echo "[vodscribe-rate]1000"
"$ffmpeg" -loglevel error -y -readrate ${RATE} -i ${quote(file)} -c copy -f matroska "$stem.mkv.part" || exit 1
mv "$stem.mkv.part" "$stem.mkv"
echo "[vodscribe-file]$stem.mkv"
`;
}

/** Makes VOD `id` Dailymotion's video `platformId`, its file gone from it. */
function makeDailymotion(id, platformId) {
  const db = new DatabaseSync(path.join(dataDirs()[0], "library.sqlite"));
  db.prepare(
    `UPDATE vods SET platform = 'dailymotion', platform_id = ?, url = ?, duration = 60,
       source_path = NULL, media_path = NULL, chat = 'none' WHERE id = ?`,
  ).run(platformId, `https://www.dailymotion.com/video/${platformId}`, id);
  db.close();
}

/** What's seen of the player: the stage's height, the app's player, the button, the time. */
const look = (app) =>
  app.js(`
    const state = await window.__TAURI_INTERNALS__.invoke("e2e_player_state");
    const text = document.querySelector("button.time .now > span:not(.widest)")?.textContent ?? "";
    return {
      stage: document.querySelector(".stage")?.getBoundingClientRect().height ?? null,
      open: state.open,
      doc: state.doc,
      button: document.querySelector(".bar button[aria-label='Pause'], .bar button[aria-label='Play']")?.getAttribute("aria-label") ?? null,
      time: text.split(":").reduce((total, part) => total * 60 + Number(part), 0),
    };`);

/** Clicks the timeline at `fraction` of its width, as a click does (pointer down and up). */
const clickTimeline = (app, fraction) =>
  app.js(
    `const box = document.querySelector(".scrubber canvas").getBoundingClientRect();
     const at = { clientX: box.left + box.width * arguments[0], clientY: box.top + box.height / 2, button: 0, bubbles: true, pointerId: 1 };
     const el = document.elementFromPoint(at.clientX, at.clientY);
     el.dispatchEvent(new PointerEvent("pointerdown", at));
     el.dispatchEvent(new PointerEvent("pointerup", at));`,
    fraction,
  );

/** Whether VOD `id` has its video kept now. */
const kept = async (app, id) => (await app.invoke("list_vods")).find((v) => v.id === id)?.media;

/** The picture is there and plays: the stage has room, the app's player is open on VOD `id`, the time runs. */
async function stillPlays(app, label, id) {
  const seen = await look(app);
  const there = (s) => s.stage > 50 && s.open === true && s.doc?.kind === "vod" && s.doc.id === id;
  if (!there(seen)) throw new Error(`${label}: no picture: ${JSON.stringify(seen)}`);
  const from = seen.time;
  await until(
    `${label}: the time to run on from ${from}`,
    async () => {
      const now = await look(app);
      if (!there(now)) throw new Error(`${label}: the picture went: ${JSON.stringify(now)}`);
      return now.time >= from + 2 && now.button === "Pause";
    },
    20_000,
  );
}

export default {
  name: "download-play",
  async run(app, ctx) {
    if (process.platform === "win32") ctx.skip("the stand-in yt-dlp is a shell script");
    const dir = path.join(ctx.out, "fixtures");
    const file = minute(dir);
    const second = path.join(dir, "minute again.mp4");
    if (!existsSync(second)) copyFileSync(file, second);
    await app.toLibrary();
    const added = await app.addFiles([file, second]);
    const ids = added.map((v) => v.id).sort((a, b) => a - b);
    await ctx.quit();
    ids.forEach((id, i) => makeDailymotion(id, `xe2e${i}`));

    const bin = path.join(dataDirs().at(-1), "bin");
    const ytDlp = path.join(bin, "yt-dlp");
    const real = path.join(bin, "yt-dlp.e2e-real");
    // Moved aside, not written over: the tools in bin may be links to a seed's.
    if (existsSync(ytDlp)) renameSync(ytDlp, real);
    // In place before the app starts: without one, setup would download yt-dlp.
    // The lookup's answer is only kept beside the download, so one does for both.
    const info = {
      id: "xe2e0",
      extractor_key: "Dailymotion",
      webpage_url: "https://www.dailymotion.com/video/xe2e0",
      duration: 60,
    };
    writeFileSync(ytDlp, standIn(file, info));
    chmodSync(ytDlp, 0o755);
    try {
      app = await ctx.start();
      await app.toLibrary();
      // The rights note asks before the first download; not here.
      await app.js(`localStorage.setItem("vodscribe.clipNoteHidden", "true")`);

      for (const [i, id] of ids.entries()) {
        const seeking = i === 1;
        const label = seeking ? "seeking all through" : "no seek";
        // No player till Play (app.openVod waits for one): its poster.
        await app.click(`.vod[data-key="${id}"]`);
        await until("its poster", () => app.js(`return !!document.querySelector(".stage button.poster")`), MINUTE);
        await app.click(".stage button.poster");
        await until(
          `${label}: the app's player open and playing`,
          async () => {
            const s = await look(app);
            return s.open === true && s.time >= 2 && s.button === "Pause" && s;
          },
          MINUTE,
        );
        if (await kept(app, id)) throw new Error(`${label}: the download ended before it was watched`);
        // While it comes in: now and then, back near the start (which is in) and on.
        let seeks = 0;
        await until(
          `${label}: the download to end`,
          async () => {
            if (seeking) {
              await clickTimeline(app, 0.02 + 0.05 * (seeks % 3));
              seeks++;
            }
            return kept(app, id);
          },
          2 * MINUTE,
          { every: seeking ? 150 : 500 },
        );
        ctx.log(`${label}: the video is kept${seeking ? ` (${seeks} seeks)` : ""}`);
        // What the page made of it settles.
        await sleep(1500);
        await stillPlays(app, `${label}, after the download`, id);
        // A seek after works, and it plays on from there.
        await clickTimeline(app, 0.75);
        await until(`${label}: the seek to 45 s`, async () => (await look(app)).time >= 44, 10_000);
        await stillPlays(app, `${label}, after a seek`, id);
        await app.pause();
      }
    } finally {
      rmSync(ytDlp, { force: true });
      if (existsSync(real)) renameSync(real, ytDlp);
    }
  },
};
