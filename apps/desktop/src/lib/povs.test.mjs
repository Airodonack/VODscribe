// Other points of view (povs.ts): node --test apps/desktop/src/lib/povs.test.mjs
// Sources: povs.ts's doc comments; CHANGELOG.md 0.7.0 ("Names link to a stream's same moment.", "A stream in your
// library opens there."). How a file's points of view are read is tested in annotations.test.mjs.
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";
import { watchUrl } from "./platform.ts";

// povs.ts imports "./platform" as the page's bundler finds it; here it is the
// .ts file it is.
register(
  "data:text/javascript," +
    encodeURIComponent(`
export async function resolve(specifier, context, next) {
  if (specifier.startsWith("./") && !/\\.[a-z]+$/.test(specifier) && context.parentURL?.endsWith(".ts"))
    return next(specifier + ".ts", context);
  return next(specifier, context);
}`),
);
const { isVideo, povAt, povLink, povOffset, povTitle, povUrl, sameAccount, siteLabel, unixTime } =
  await import("./povs.ts");

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

/** povs.ts `unixTime` ("`iso` as Unix seconds; null when there's none or it isn't a time"). */
test("a time reads as Unix seconds, or not at all", () => {
  assert.equal(unixTime("2026-10-21T18:00:00Z"), 1792605600);
  assert.equal(unixTime("2026-10-21T20:00:00+02:00"), 1792605600);
  assert.equal(unixTime("2026-10-21T18:00:00.5Z"), 1792605600.5);
  assert.equal(unixTime(null), null);
  assert.equal(unixTime(undefined), null);
  assert.equal(unixTime(""), null);
  assert.equal(unixTime("yesterday"), null);
});

/** povs.ts `sameAccount` ("the same site, the login in any case"). */
test("an account is the same whatever the login's case, on the same site only", () => {
  assert.ok(sameAccount(burn, { platform: "twitch", login: "Burn" }));
  assert.ok(sameAccount(burn, { platform: "twitch", login: " burn " }));
  assert.ok(!sameAccount(burn, { platform: "kick", login: "burn" }));
  assert.ok(!sameAccount(burn, { platform: "twitch", login: "burns" }));
});

const at = /** @type {number} */ (unixTime(started));

/** povs.ts `povAt` ("`account`'s stream running at `at` ...; null when none was"); CHANGELOG 0.7.0 "Names link to a stream's same moment." ("Outside their stream ... the link is their channel as before"). */
test("the stream covering a moment is found, from its first moment to its last, and only that account's", () => {
  assert.equal(povAt([pov], burn, at), pov);
  assert.equal(povAt([pov], burn, start), pov, "its first moment");
  assert.equal(povAt([pov], burn, start + 7200), pov, "its last");
  assert.equal(povAt([pov], burn, start - 1), null, "before it started");
  assert.equal(povAt([pov], burn, start + 7201), null, "after it ended");
  assert.equal(povAt([pov], { platform: "twitch", login: "ada" }, at), null, "someone else's");
  assert.equal(povAt([pov], { platform: "twitch", login: "BURN" }, at), pov, "the login's case aside");
  assert.equal(povAt([], burn, at), null);
});

/** povs.ts `povAt` ("of two (a stream restarted), the later started"). */
test("of two streams covering a moment, the later started is found, whatever their order", () => {
  const restarted = P({ ...pov, video: { platform: "twitch", id: "v2" }, start: "2026-10-21T18:20:00Z" });
  assert.equal(povAt([pov, restarted], burn, at), restarted);
  assert.equal(povAt([restarted, pov], burn, at), restarted);
  assert.equal(povAt([pov, restarted], burn, start + 60), pov, "before the restart: the first");
});

/** povs.ts `povAt` (a point of view whose start isn't a time is never found). */
test("a stream whose start isn't a time is never found", () => {
  assert.equal(povAt([P({ ...pov, start: "yesterday" })], burn, at), null);
});

/** povs.ts `povOffset` ("How far into `pov` the moment `at` ... is, in seconds, kept inside it"). */
test("the offset is how far into the stream the moment is, kept inside it", () => {
  assert.equal(povOffset(pov, at + 83), 1800 + 83);
  assert.equal(povOffset(pov, start - 5), 0);
  assert.equal(povOffset(pov, start + 9000), 7200);
});

/** povs.ts `povUrl` ("Where `pov` plays from `offset` seconds in, on its site, as platform.ts `watchUrl` links a VOD"). */
test("a Twitch stream's link names its video, without a v, and the offset in hours, minutes and seconds", () => {
  assert.equal(povUrl(pov, 3723.6), "https://www.twitch.tv/videos/2906773722?t=1h2m3s", "the id without its v");
  assert.equal(
    povUrl(P({ ...pov, video: { platform: "twitch", id: "123" } }), 5),
    "https://www.twitch.tv/videos/123?t=0h0m5s",
  );
});

/** povs.ts `povUrl` (YouTube: the video and a time in seconds); a negative offset is the start. */
test("a YouTube stream's link names its video and the second", () => {
  const yt = P({
    ...pov,
    account: { platform: "youtube", login: "@burn" },
    video: { platform: "youtube", id: "abc_-1" },
  });
  assert.equal(povUrl(yt, 90.4), "https://www.youtube.com/watch?v=abc_-1&t=90s");
  assert.equal(povUrl(yt, -5), "https://www.youtube.com/watch?v=abc_-1&t=0s");
});

/** povs.ts `povUrl` ("A Kick video's address has no time of its own (`linksHaveTime`): it opens at the start"); CHANGELOG 0.7.0 "A stream in your library opens there." ("a Kick video at its start, as Kick's links can't name a time"). */
test("a Kick stream's link has no time: its channel's video, from its start", () => {
  const kick = P({ ...pov, account: { platform: "kick", login: "Burn_TV" }, video: { platform: "kick", id: "a1-b2" } });
  assert.equal(povUrl(kick, 90), "https://kick.com/burn_tv/videos/a1-b2");
});

/** povs.ts `siteLabel` ("The site a video is on, as it's read out"). */
test("a site is read out as its address", () => {
  assert.equal(siteLabel("twitch"), "twitch.tv");
  assert.equal(siteLabel("kick"), "kick.com");
  assert.equal(siteLabel("youtube"), "youtube.com");
});

/** povs.ts `isVideo` ("Whether the library's `vod` is `video`: the same site and id, a Twitch id with or without its \"v\""). */
test("the library's copy of a Twitch video is found by its id, with or without its v", () => {
  assert.ok(isVideo({ platform: "twitch", platform_id: "v2906773722" }, pov.video));
  assert.ok(isVideo({ platform: "twitch", platform_id: "2906773722" }, pov.video));
  assert.ok(isVideo({ platform: "twitch", platform_id: "v2906773722" }, { platform: "twitch", id: "2906773722" }));
  assert.ok(!isVideo({ platform: "twitch", platform_id: "v2906773723" }, pov.video));
});

/** povs.ts `isVideo` (the same site and id: another site's, or another's id, isn't it). */
test("another site's video, or a YouTube id with a v, isn't the library's copy", () => {
  assert.ok(!isVideo({ platform: "youtube", platform_id: "v2906773722" }, pov.video));
  assert.ok(isVideo({ platform: "youtube", platform_id: "abc" }, { platform: "youtube", id: "abc" }));
  assert.ok(!isVideo({ platform: "youtube", platform_id: "vabc" }, { platform: "youtube", id: "abc" }));
  assert.ok(!isVideo({ platform: "kick", platform_id: "k1" }, { platform: "youtube", id: "k1" }));
});

/** povs.ts `povLink` ("their stream running then, at the same moment"); CHANGELOG 0.7.0 "Names link to a stream's same moment." (a transcript heading at when its lines start). */
test("a link goes to their stream at the same moment", () => {
  assert.deepEqual(povLink([pov], burn, started, 3723), {
    pov,
    offset: 5523,
    url: "https://www.twitch.tv/videos/2906773722?t=1h32m3s",
    title: "Burn's stream at 1:32:03 · twitch.tv",
  });
});

/** povs.ts `povLink` ("null when none was, or when this VOD's start isn't known (their channel, then)"); CHANGELOG 0.7.0 ("Outside their stream, or when VODscribe doesn't know when this VOD started, the link is their channel as before"). */
test("with no stream running then, no start known or no moment, there is no link (their channel's is used)", () => {
  assert.equal(povLink([pov], burn, started, 7200), null, "their stream had ended by then");
  assert.equal(povLink([pov], burn, null, 60), null, "this VOD's start isn't known");
  assert.equal(povLink([pov], burn, undefined, 60), null);
  assert.equal(povLink([pov], burn, "yesterday", 60), null);
  assert.equal(povLink([pov], burn, started, null), null, "no moment");
  assert.equal(povLink([pov], burn, started, NaN), null);
  assert.equal(povLink([pov], burn, started, Infinity), null);
  assert.equal(povLink([], burn, started, 60), null);
});

/** povs.ts `povTitle` (""Burn's stream at 1:23:45 · twitch.tv"; `here` when the library has it, which opens there"). */
test("a link's tooltip says whose stream, at what time, and where it opens", () => {
  assert.equal(povTitle(pov, 5523), "Burn's stream at 1:32:03 · twitch.tv");
  assert.equal(povTitle(pov, 5523, true), "Burn's stream at 1:32:03 · in your library");
});

/** povs.ts `povTitle` (a Kick video opens at its start: its tooltip says so); CHANGELOG 0.7.0 "A stream in your library opens there.". */
test("a Kick stream's tooltip says it opens from its start", () => {
  const kick = P({ ...pov, account: { platform: "kick", login: "burn" }, video: { platform: "kick", id: "k1" } });
  assert.equal(povTitle(kick, 60), "Burn's stream at 1:00 · kick.com, from its start");
  assert.equal(povTitle(kick, 60, true), "Burn's stream at 1:00 · in your library");
});

/** povs.ts `povUrl` ("as platform.ts `watchUrl` links a VOD in the library"). */
test("a link to another point of view is the link the library's copy of it would give", () => {
  const vod = (/** @type {string} */ platform, /** @type {string} */ platform_id, /** @type {string} */ url) =>
    /** @type {any} */ ({ platform, platform_id, url });
  const twitch = vod("twitch", "v2906773722", "https://www.twitch.tv/videos/2906773722");
  for (const t of [0, 5, 63.9, 3723.6]) assert.equal(povUrl(pov, t), watchUrl(twitch, t));
  const kick = P({ ...pov, account: { platform: "kick", login: "Burn_TV" }, video: { platform: "kick", id: "a1-b2" } });
  const kickVod = vod("kick", "a1-b2", "https://kick.com/burn_tv/videos/a1-b2");
  assert.equal(povUrl(kick, 90), watchUrl(kickVod, 90));
  const youtube = P({ ...pov, video: { platform: "youtube", id: "abc_-1" } });
  const ytVod = vod("youtube", "abc_-1", "https://www.youtube.com/watch?v=abc_-1");
  assert.equal(povUrl(youtube, 90), watchUrl(ytVod, 90));
});

/** povs.ts `sameAccount` ("the login in any case"): spaces around either login don't matter. */
test("an account is the same whatever the spaces around either login", () => {
  assert.ok(sameAccount({ platform: "twitch", login: " BURN " }, burn));
  assert.ok(sameAccount(burn, { platform: "twitch", login: " Burn " }));
});

/** povs.ts `isVideo` ("a Twitch id with or without its \"v\""): only a Twitch id's v is one. */
test("a v at the start of a YouTube id is part of it", () => {
  assert.ok(!isVideo({ platform: "youtube", platform_id: "v123" }, { platform: "youtube", id: "123" }));
});

/** povs.ts `povAt` (a stream whose start isn't a time is never found), however early the moment. */
test("a stream whose start isn't a time is not found even at a moment near the epoch", () => {
  assert.equal(povAt([P({ ...pov, start: "yesterday", duration: 7200 })], burn, 100), null);
});

/** povs.ts `povLink` ("null ... when this VOD's start isn't known (their channel, then)"), whatever stream there is. */
test("a link is none when this VOD's start isn't known, though a stream would cover the moment from the epoch", () => {
  const early = P({ ...pov, start: "1970-01-01T00:00:00Z", duration: 7200 });
  assert.equal(povLink([early], burn, "yesterday", 60), null);
  assert.equal(povLink([early], burn, null, 60), null);
  assert.equal(povLink([early], burn, "1970-01-01T00:00:00Z", 60)?.offset, 60, "known: it's found");
});
