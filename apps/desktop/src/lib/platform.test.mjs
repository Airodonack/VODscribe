// Sizes and names from platform.ts: node --test apps/desktop/src/lib/platform.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  DEFAULT_HEIGHT,
  chaptersText,
  HEIGHTS,
  hasSitePlayer,
  keepsVideo,
  videoSize,
  formatFraction,
} from "./platform.ts";

test("a video's size is a range, in MB under a GB and GB above", () => {
  assert.equal(videoSize(1080), "3 to 5 GB");
  assert.equal(videoSize(720, 2.5), "3.8 to 6.3 GB");
  assert.equal(videoSize(1080, 4), "12 to 20 GB");
  assert.equal(videoSize(480, 0.5), "350 to 600 MB");
  assert.equal(videoSize(480, 0.001), "1 to 1 MB");
});

test("an unknown height is sized as the default one", () => {
  assert.ok(HEIGHTS.includes(DEFAULT_HEIGHT));
  assert.equal(videoSize(360), videoSize(DEFAULT_HEIGHT));
});

test("sites with their own player don't keep their video; other sites do; files are files", () => {
  /** @param {string} platform */
  const vod = (platform) => /** @type {import("./api").Vod} */ (/** @type {unknown} */ ({ platform }));
  for (const p of ["youtube", "twitch", "kick"]) assert.ok(hasSitePlayer(vod(p)) && !keepsVideo(vod(p)));
  assert.ok(keepsVideo(vod("rumble")) && !hasSitePlayer(vod("rumble")));
  assert.ok(!keepsVideo(vod("local")) && !hasSitePlayer(vod("local")));
});

test("chapters copy as YouTube reads them", () => {
  const text = chaptersText([
    { start: 0, text: "Intro" },
    { start: 75.9, text: null },
    { start: 3725, text: "The race" },
  ]);
  assert.equal(text, "0:00 Intro\n1:15 Untitled chapter\n1:02:05 The race");
});

test("a time to a tenth or hundredth shows a frame's own start, not just under it", () => {
  // Frame starts (k/fps) land a hair under the hundredth in floating point.
  assert.equal(formatFraction(36 / 30, 2), "0:01.20");
  assert.equal(formatFraction(1.2, 2), "0:01.20");
  assert.equal(formatFraction(3723.25, 2), "1:02:03.25");
  assert.equal(formatFraction(59.99, 1), "0:59.9");
  // Just under a whole second: that second, not .100 of the one before.
  assert.equal(formatFraction(2 - 1e-12, 2), "0:02.00");
  for (const fps of [24, 25, 30, 50, 60])
    for (let k = 0; k < 600; k++) {
      const exact = Math.round((k / fps) * 100);
      const shown = formatFraction(k / fps, 2)
        .split(/[:.]/)
        .map(Number);
      const hundredths = (shown[0] * 60 + shown[1]) * 100 + shown[2];
      assert.ok(
        Math.abs(hundredths - Math.floor((k * 100) / fps + 1e-9)) === 0,
        `${k}/${fps}: ${formatFraction(k / fps, 2)} (${exact})`,
      );
    }
});
