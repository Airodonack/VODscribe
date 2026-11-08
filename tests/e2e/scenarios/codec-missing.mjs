// A codec the app's ffmpeg can't make: with an ffmpeg without H.265 in its
// place, Settings › Encoding greys MP4 H.265 out, says why on hover, and
// doesn't take a click on it. With the ffmpeg back, nothing is greyed out:
// the app asks the new one. Picked then, and the ffmpeg without H.265 in
// place again, it stays picked, its hint says clips fail, and a clip in it
// fails saying which codec and where to pick another.
//
// The stand-in is a shell script, so not on Windows.

import { chmodSync, copyFileSync, existsSync, mkdirSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "../lib/app.mjs";
import { dataDirs } from "../lib/data.mjs";
import { setFormat } from "../lib/fileManager.mjs";
import { videos } from "../lib/fixtures.mjs";

const SAYS = "VODscribe's ffmpeg can't make MP4 H.265. Pick another format in Settings › Encoding.";

/** Each codec in Settings › Encoding as shown: picked, greyed out, its hover text; and the hint under them. */
const shown = (app) =>
  app.js(`
    const buttons = [...document.querySelectorAll('[role="radiogroup"][aria-label="Clip format"] button')];
    return {
      codecs: Object.fromEntries(buttons.map((b) => [b.textContent.trim(), {
        on: b.getAttribute("aria-checked") === "true",
        greyed: b.getAttribute("aria-disabled") === "true",
        why: b.getAttribute("title"),
      }])),
      hint: buttons[0]?.closest(".segmented").nextElementSibling?.textContent.trim() ?? null,
    };`);

/** Settings › Encoding opened anew, as shown once the app has said which codecs are missing (`settled`). */
async function openEncoding(app, settled) {
  await app.key("Escape");
  await app.openSettings("Encoding");
  return until(
    "Settings › Encoding as the ffmpeg has it",
    async () => {
      const seen = await shown(app);
      return settled(seen) && seen;
    },
    MINUTE,
  );
}

export default {
  name: "codec-missing",
  async run(app, ctx) {
    if (process.platform === "win32") return ctx.skip("the ffmpeg stand-in is a shell script");
    await app.toLibrary();
    const [vod] = await app.addFiles([videos(path.join(ctx.out, "fixtures")).video]);
    await until("the video read", async () => (await app.invoke("list_vods"))[0]?.duration > 0, MINUTE);

    // The stand-in for the app's ffmpeg (its bin is kept between scenarios:
    // one there is moved aside, and back after; one already aside is the
    // real one, a run that stopped short left so).
    const bin = path.join(dataDirs().at(-1), "bin");
    const ffmpeg = path.join(bin, "ffmpeg");
    const aside = path.join(bin, "ffmpeg.e2e-real");
    mkdirSync(bin, { recursive: true });
    if (existsSync(aside)) rmSync(ffmpeg, { force: true });
    else if (existsSync(ffmpeg)) renameSync(ffmpeg, aside);
    const real = existsSync(aside)
      ? aside
      : execFileSync("sh", ["-c", "command -v ffmpeg"], { encoding: "utf8" }).trim();
    const settings = await app.invoke("get_settings");
    /** An ffmpeg without H.265 in the app's ffmpeg's place. */
    const standIn = () => {
      writeFileSync(
        ffmpeg,
        `#!/bin/sh
# An ffmpeg without H.265 (codec-missing): not in its list, refused if asked.
for a in "$@"; do
  case "$a" in
    -encoders) '${real}' "$@" | grep -v ' libx265 '; exit 0 ;;
    libx265) echo "Unknown encoder 'libx265'" >&2; exit 1 ;;
  esac
done
exec '${real}' "$@"
`,
      );
      chmodSync(ffmpeg, 0o755);
    };
    /** The app's ffmpeg back as it was (none, if it was the one on PATH). */
    const putBack = () => {
      rmSync(ffmpeg, { force: true });
      if (existsSync(aside)) copyFileSync(aside, ffmpeg);
    };
    const everyCodec = (s) => Object.keys(s.codecs).length === 4 && !Object.values(s.codecs).some((c) => c.greyed);
    try {
      // 1. Greyed out, with why on hover; a click on it changes nothing.
      standIn();
      let seen = await openEncoding(app, (s) => s.codecs["MP4 H.265"]?.greyed);
      const others = Object.entries(seen.codecs).filter(([name, c]) => name !== "MP4 H.265" && (c.greyed || c.why));
      if (others.length) throw new Error(`greyed out besides MP4 H.265: ${JSON.stringify(others)}`);
      if (seen.codecs["MP4 H.265"].why !== "VODscribe's ffmpeg can't make MP4 H.265.")
        throw new Error(`MP4 H.265's hover says "${seen.codecs["MP4 H.265"].why}"`);
      await app.js(
        `[...document.querySelectorAll('[role="radiogroup"][aria-label="Clip format"] button')]
           .find((b) => b.textContent.trim() === "MP4 H.265").click();`,
      );
      // Time for a save it might have made (one is quicker than this).
      await sleep(500);
      if ((await app.invoke("get_settings")).clip_codec === "h265") throw new Error("a click picked MP4 H.265");
      ctx.log(`MP4 H.265 greyed out: "${seen.codecs["MP4 H.265"].why}"`);

      // 2. The ffmpeg back: asked again, it has every codec, and H.265 is picked.
      putBack();
      // (Asked outright too: Settings reopened may show nothing greyed before its answer.)
      const { missing } = await app.invoke("missing_clip_codecs");
      if (missing.length) throw new Error(`with the ffmpeg back, missing: ${missing}`);
      await openEncoding(app, everyCodec);
      ctx.log("with the ffmpeg back, nothing greyed out");
      await setFormat(app, "MP4 H.265");

      // 3. Picked before: it stays picked, its hint says so, and a clip in it fails saying why.
      standIn();
      seen = await openEncoding(app, (s) => s.codecs["MP4 H.265"]?.on && s.codecs["MP4 H.265"].greyed);
      const hint = "VODscribe's ffmpeg can't make MP4 H.265. Clips and edits fail until you pick another format.";
      if (seen.hint !== hint) throw new Error(`with MP4 H.265 picked, the hint says "${seen.hint}"`);
      const clip = await app.invoke("add_clip", { vodId: vod.id, start: 1, end: 3 });
      const failed = await until(
        "the clip failed",
        async () => {
          const c = (await app.invoke("list_clips", { vodId: vod.id })).find((c) => c.id === clip.id);
          if (c?.status === "ready") throw new Error(`the clip was made as ${c.file_path}`);
          return c?.status === "error" && c;
        },
        MINUTE,
      );
      if (failed.error !== SAYS) throw new Error(`the clip says "${failed.error}"`);
      if ((await app.invoke("get_settings")).clip_codec !== "h265") throw new Error("the choice was changed");
      ctx.log(`the clip says "${failed.error}"; MP4 H.265 still picked`);
    } finally {
      rmSync(ffmpeg, { force: true });
      if (existsSync(aside)) renameSync(aside, ffmpeg);
      await app.invoke("set_settings", { settings }).catch(() => {});
    }
  },
};
