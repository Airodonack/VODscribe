// Sizes and names from platform.ts: node --test apps/desktop/src/lib/platform.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { DEFAULT_HEIGHT, HEIGHTS, hasSitePlayer, keepsVideo, videoSize } from "./platform.ts";

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
  const vod = (platform) => ({ platform });
  for (const p of ["youtube", "twitch", "kick"]) assert.ok(hasSitePlayer(vod(p)) && !keepsVideo(vod(p)));
  assert.ok(keepsVideo(vod("rumble")) && !hasSitePlayer(vod("rumble")));
  assert.ok(!keepsVideo(vod("local")) && !hasSitePlayer(vod("local")));
});
