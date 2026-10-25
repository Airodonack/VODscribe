// Other points of view (povs.ts, and how a file's are read, annotations.ts): node --test apps/desktop/src/lib/povs.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { parseAnnotations, readPov } from "./annotations.ts";
import {
  clock,
  isVideo,
  povAt,
  povLink,
  povOffset,
  povTitle,
  povUrl,
  sameAccount,
  twitchTime,
  unixTime,
} from "./povs.ts";

/** @typedef {import("./annotations").Pov} Pov */
/** @type {import("./annotations").Account} */
const burn = { platform: "twitch", login: "burn" };
/** A point of view as written here, typed as one. @param {Pov} p */
const P = (p) => p;
// Burn's stream from 18:00 for two hours; the VOD open started at 18:30.
const pov = P({
  player: "Burn",
  account: burn,
  video: { platform: "twitch", id: "v2906773722" },
  start: "2026-10-21T18:00:00Z",
  duration: 7200,
});
const started = "2026-10-21T18:30:00Z";
const start = /** @type {number} */ (unixTime(pov.start));

test("a time reads as Unix seconds, or not at all", () => {
  assert.equal(unixTime("2026-10-21T18:00:00Z"), 1792605600);
  assert.equal(unixTime("2026-10-21T20:00:00+02:00"), 1792605600);
  assert.equal(unixTime(null), null);
  assert.equal(unixTime("yesterday"), null);
});

test("an account is the same whatever the login's case, on the same site only", () => {
  assert.ok(sameAccount(burn, { platform: "twitch", login: "Burn" }));
  assert.ok(!sameAccount(burn, { platform: "kick", login: "burn" }));
});

test("the stream covering a moment is found, the later of two", () => {
  const at = /** @type {number} */ (unixTime(started));
  assert.equal(povAt([pov], burn, at), pov);
  assert.equal(povAt([pov], { platform: "twitch", login: "ada" }, at), null, "someone else's");
  assert.equal(povAt([pov], burn, at + 7200), null, "after it ended");
  assert.equal(povAt([pov], burn, start - 1), null, "before it started");
  assert.equal(povAt([pov], burn, start), pov, "its first moment");
  const restarted = P({ ...pov, video: { platform: "twitch", id: "v2" }, start: "2026-10-21T18:20:00Z" });
  assert.equal(povAt([pov, restarted], burn, at), restarted);
  assert.equal(povAt([restarted, pov], burn, at), restarted);
});

test("the offset is the same moment, kept inside the stream", () => {
  const at = /** @type {number} */ (unixTime(started)) + 83;
  assert.equal(povOffset(pov, at), 1800 + 83);
  assert.equal(povOffset(pov, start - 5), 0);
  assert.equal(povOffset(pov, start + 9000), 7200);
});

test("times as Twitch's links and the app say them", () => {
  assert.equal(twitchTime(3723), "1h2m3s");
  assert.equal(twitchTime(3600), "1h0m0s");
  assert.equal(twitchTime(63.9), "1m3s");
  assert.equal(twitchTime(3), "3s");
  assert.equal(twitchTime(-2), "0s");
  assert.equal(clock(5025), "1:23:45");
  assert.equal(clock(65), "1:05");
});

test("each site's link to a moment of a video", () => {
  assert.equal(povUrl(pov, 3723.6), "https://www.twitch.tv/videos/2906773722?t=1h2m3s", "the id without its v");
  assert.equal(
    povUrl(P({ ...pov, video: { platform: "twitch", id: "123" } }), 5),
    "https://www.twitch.tv/videos/123?t=5s",
  );
  const yt = P({
    ...pov,
    account: { platform: "youtube", login: "@burn" },
    video: { platform: "youtube", id: "abc_-1" },
  });
  assert.equal(povUrl(yt, 90.4), "https://www.youtube.com/watch?v=abc_-1&t=90s");
  const kick = P({ ...pov, account: { platform: "kick", login: "Burn_TV" }, video: { platform: "kick", id: "a1-b2" } });
  assert.equal(povUrl(kick, 90), "https://kick.com/burn_tv/videos/a1-b2", "no time of its own");
});

test("the library's copy of a video is found by site and id, a Twitch id with or without its v", () => {
  assert.ok(isVideo({ platform: "twitch", platform_id: "v2906773722" }, pov.video));
  assert.ok(isVideo({ platform: "twitch", platform_id: "2906773722" }, pov.video));
  assert.ok(!isVideo({ platform: "twitch", platform_id: "v2906773723" }, pov.video));
  assert.ok(!isVideo({ platform: "youtube", platform_id: "v2906773722" }, pov.video));
  assert.ok(isVideo({ platform: "youtube", platform_id: "abc" }, { platform: "youtube", id: "abc" }));
  assert.ok(!isVideo({ platform: "youtube", platform_id: "vabc" }, { platform: "youtube", id: "abc" }));
});

test("a link goes to their stream at the same moment, or to the channel (null)", () => {
  const link = povLink([pov], burn, started, 3723);
  assert.deepEqual(link, {
    pov,
    offset: 5523,
    url: "https://www.twitch.tv/videos/2906773722?t=1h32m3s",
    title: "Burn's stream at 1:32:03 · twitch.tv",
  });
  assert.equal(povLink([pov], burn, started, 7200), null, "their stream had ended by then");
  assert.equal(povLink([pov], burn, null, 60), null, "this VOD's start isn't known");
  assert.equal(povLink([pov], burn, started, null), null, "no moment");
  assert.equal(povLink([], burn, started, 60), null);
  assert.equal(povTitle(pov, 5523, true), "Burn's stream at 1:32:03 · in your library");
  const kick = P({ ...pov, account: { platform: "kick", login: "burn" }, video: { platform: "kick", id: "k1" } });
  assert.equal(povTitle(kick, 60), "Burn's stream at 1:00 · kick.com, from its start");
});

test("a file's points of view are read, and one that can't be is left out", () => {
  const ok = {
    player: " Burn ",
    account: { platform: "Twitch", login: "burn" },
    video: { platform: "TWITCH", id: " v1 " },
    start: "2026-10-21T18:00:00Z",
    duration: 60,
  };
  assert.deepEqual(readPov(ok), {
    player: "Burn",
    account: burn,
    video: { platform: "twitch", id: "v1" },
    start: "2026-10-21T18:00:00Z",
    duration: 60,
  });
  assert.ok(readPov({ ...ok, start: "2026-10-21T18:00:00.250+02:00" }), "a fraction and an offset");
  for (const bad of [
    { ...ok, player: "" },
    { ...ok, account: { platform: "vimeo", login: "burn" } },
    { ...ok, video: { platform: "vimeo", id: "1" } },
    { ...ok, video: { platform: "twitch", id: "../1" } },
    { ...ok, start: "yesterday" },
    { ...ok, start: "2026-10-21" },
    { ...ok, duration: 0 },
    { ...ok, duration: "60" },
    "Burn",
    null,
  ])
    assert.equal(readPov(bad), null, JSON.stringify(bad));

  const file = {
    format: "vodscribe-annotations",
    version: 1,
    source: { platform: "twitch", id: "v9", channel: "c", title: "t", duration: 60, recorded_at: started },
    produced: { at: started, build: "b" },
    transcript: { segments: [], corrected: 0 },
    chapters: [],
    highlights: [],
    cast: [],
    activity: [],
  };
  assert.deepEqual(parseAnnotations(file).povs, [], "a file may leave them out");
  assert.deepEqual(parseAnnotations({ ...file, povs: [ok, { ...ok, duration: -1 }] }).povs, [readPov(ok)]);
  assert.deepEqual(parseAnnotations({ ...file, povs: "none" }).povs, []);
});
