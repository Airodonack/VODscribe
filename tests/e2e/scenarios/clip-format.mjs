// Clips made as chosen in Settings: as MOV DNxHR, a sound-only file's clip
// is a .wav and a video's a .mov, each a file that's there; then the video's
// clip is made again in each codec at a quality step, and its picture read
// back: the codec, its profile, its tag and its pixels.

import { existsSync } from "node:fs";
import path from "node:path";

import { MINUTE, until } from "../lib/app.mjs";
import { probeVideo, setFormat, setQuality } from "../lib/fileManager.mjs";
import { videos } from "../lib/fixtures.mjs";

/** Each choice in turn, and the picture it makes, as ffmpeg names it. */
const STEPS = [
  ["MOV DNxHR", "Standard", { codec: "dnxhd", profile: "DNXHR SQ", tag: "AVdh", pixels: "yuv422p" }],
  ["MOV DNxHR", "High", { codec: "dnxhd", profile: "DNXHR HQ", tag: "AVdh", pixels: "yuv422p" }],
  ["MP4 H.265", "High", { codec: "hevc", profile: "Main", tag: "hvc1", pixels: "yuv420p" }],
  ["MOV ProRes", "Proxy", { codec: "prores", profile: "Proxy", tag: "apco", pixels: "yuv422p10le" }],
  ["MOV ProRes", "High", { codec: "prores", profile: "HQ", tag: "apch", pixels: "yuv422p10le" }],
  ["MP4 H.264", "Proxy", { codec: "h264", profile: "High", tag: "avc1", pixels: "yuv420p" }],
];

export default {
  name: "clip-format",
  async run(app, ctx) {
    await app.toLibrary();
    await setFormat(app, "MOV DNxHR");

    const audio = await app.addAndTranscribe(ctx.fixtures.landed);
    const [added] = await app.addFiles([videos(path.join(ctx.out, "fixtures")).video]);
    const video = await app.transcribe(added.id);
    let clip;
    for (const [vod, ext] of [
      [audio, ".wav"],
      [video, ".mov"],
    ]) {
      await app.openVod(vod.id);
      // The player is made on the first play; paused once it plays.
      await app.click('.bar button[aria-label="Play"]');
      await until(
        "it to play",
        () => app.js(`return !!document.querySelector('.bar button[aria-label="Pause"]')`),
        MINUTE,
      );
      clip = await app.clipOneToThree(vod.id);
      if (path.extname(clip.file_path) !== ext || !existsSync(clip.file_path))
        throw new Error(`${vod.title}'s clip is ${clip.file_path}, not a ${ext} that's there`);
      ctx.log(`${vod.title}: ${path.basename(clip.file_path)}`);
    }

    // The video's two-second clip, made again after each choice but the first.
    for (const [i, [format, quality, want]] of STEPS.entries()) {
      await setFormat(app, format);
      await setQuality(app, quality);
      if (i > 0) {
        await app.invoke("set_clip_range", { id: clip.id, start: clip.start, end: clip.end });
        clip = await until(
          `the clip made as ${format} ${quality}`,
          async () => {
            const c = (await app.invoke("list_clips", { vodId: video.id })).find((c) => c.id === clip.id);
            if (c?.status === "error") throw new Error(`the clip failed: ${c.error}`);
            return c?.status === "ready" && c;
          },
          MINUTE,
        );
      }
      const ext = format.startsWith("MP4") ? ".mp4" : ".mov";
      if (path.extname(clip.file_path) !== ext) throw new Error(`as ${format}, the clip is ${clip.file_path}`);
      const got = probeVideo(clip.file_path);
      if (Object.keys(want).some((k) => got[k] !== want[k]))
        throw new Error(`as ${format} ${quality}, the clip is ${JSON.stringify(got)}, not ${JSON.stringify(want)}`);
      ctx.log(`${format} ${quality}: ${Object.values(got).join(" ")}`);
    }
  },
};
