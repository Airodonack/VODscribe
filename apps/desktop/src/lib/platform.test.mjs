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
  isTwitchClip,
  labelOf,
  listed,
  siteName,
  twitchId,
  twitchTime,
  watchUrl,
} from "./platform.ts";

test("a list is written with commas and a last and", () => {
  assert.equal(listed([]), "");
  assert.equal(listed(["Undo"]), "Undo");
  assert.equal(listed(["Undo", "Redo"]), "Undo and Redo");
  assert.equal(listed(["Undo", "Redo", "Search"]), "Undo, Redo and Search");
});

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

test("sites are named, by their host where they aren't known", () => {
  const vod = (/** @type {string} */ platform, /** @type {string} */ url) => /** @type {any} */ ({ platform, url });
  assert.equal(siteName(vod("twitch", "https://www.twitch.tv/videos/1")), "Twitch");
  assert.equal(siteName(vod("rumble", "https://www.rumble.com/v1-x.html")), "rumble.com");
  assert.equal(siteName(vod("odd", "not a url")), "odd");
});

test("Twitch clips are told from VODs by their id", () => {
  const twitch = (/** @type {string} */ platform_id) => /** @type {any} */ ({ platform: "twitch", platform_id });
  assert.ok(!isTwitchClip(twitch("v2277656159")) && !isTwitchClip(twitch("2277656159")));
  assert.ok(isTwitchClip(twitch("FunnyClipName-AbCd123")));
  assert.ok(!isTwitchClip(/** @type {any} */ ({ platform: "kick", platform_id: "abc" })));
});

test("things being added are called by their link or file name", () => {
  assert.equal(labelOf(" https://www.twitch.tv/videos/1 "), "twitch.tv/videos/1");
  assert.equal(labelOf("HTTP://kick.com/x"), "kick.com/x");
  assert.equal(labelOf("/home/me/Videos/talk.mp4"), "talk.mp4");
  assert.equal(labelOf("C:\\Users\\me\\talk.mkv"), "talk.mkv");
  assert.equal(labelOf("talk.mp4"), "talk.mp4");
});

test("a link to a moment on a VOD's site: Twitch's time always with its hours, none on Kick", () => {
  const vod = (/** @type {string} */ platform, /** @type {string} */ platform_id, /** @type {string} */ url) =>
    /** @type {any} */ ({ platform, platform_id, url });
  assert.equal(twitchTime(3723), "1h2m3s");
  assert.equal(twitchTime(63.9), "0h1m3s");
  assert.equal(twitchTime(3), "0h0m3s");
  assert.equal(twitchTime(-2), "0h0m0s");
  const twitch = vod("twitch", "v2901646294", "https://www.twitch.tv/videos/2901646294");
  assert.equal(watchUrl(twitch, 65.7), "https://www.twitch.tv/videos/2901646294?t=0h1m5s");
  const youtube = vod("youtube", "IaIo3d6cUuo", "https://www.youtube.com/watch?v=IaIo3d6cUuo");
  assert.equal(watchUrl(youtube, 75.2), "https://www.youtube.com/watch?v=IaIo3d6cUuo&t=75s");
  // Kick's links name no time (none is documented): the VOD's own link.
  const kick = vod("kick", "01a169a1", "https://kick.com/airodonack/videos/01a169a1");
  assert.equal(watchUrl(kick, 75), "https://kick.com/airodonack/videos/01a169a1");
  const other = vod("rumble", "x", "https://rumble.com/v1-x.html");
  assert.equal(watchUrl(other, 75), "https://rumble.com/v1-x.html?t=75");
});

test("a Twitch id loses its v only before a number, never a clip's slug's", () => {
  const twitch = (/** @type {string} */ platform_id) => /** @type {any} */ ({ platform: "twitch", platform_id });
  assert.equal(twitchId(twitch("v2901646294")), "2901646294");
  assert.equal(twitchId(twitch("2901646294")), "2901646294");
  assert.equal(twitchId(twitch("vanityClip-AbCd")), "vanityClip-AbCd");
  // crates/core annotations.rs `a_twitch_id_loses_its_v_only_before_a_number` has the same cases.
});
