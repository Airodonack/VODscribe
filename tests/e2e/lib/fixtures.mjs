// Test files: the recorded ones in fixtures/, and videos made at test time
// with the ffmpeg setup downloaded (or one on PATH), so the repository holds
// no video files.

import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { DatabaseSync } from "node:sqlite";

import { MINUTE, until } from "./app.mjs";
import { dataDirs } from "./data.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
export const LANDED = path.join(here, "..", "fixtures", "eagle-has-landed.mp3");

function ffmpeg() {
  const exe = process.platform === "win32" ? "ffmpeg.exe" : "ffmpeg";
  const downloaded = path.join(dataDirs().at(-1), "bin", exe);
  return existsSync(downloaded) ? downloaded : exe;
}

/** Runs ffmpeg to make `file` (in `dir`) with `args` between the inputs and it. */
function make(dir, file, args) {
  const out = path.join(dir, file);
  if (!existsSync(out)) execFileSync(ffmpeg(), ["-loglevel", "error", "-y", ...args, out], { stdio: "inherit" });
  return out;
}

/** The videos, made in `dir` once; returns their paths by kind. */
export function videos(dir) {
  mkdirSync(dir, { recursive: true });
  const pattern = (size) => ["-f", "lavfi", "-i", `testsrc2=size=${size}:rate=30:duration=5`];
  const h264 = ["-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p"];
  const speech = ["-i", LANDED, "-c:a", "aac", "-shortest"];
  const video = make(dir, "landed.mp4", [...pattern("640x360"), ...speech, ...h264]);
  // A name with letters from other scripts, spaces and a dash, as people's files have.
  const named = path.join(dir, "Ünïcødé name — テスト 1.mp4");
  if (!existsSync(named)) copyFileSync(video, named);
  return {
    video,
    named,
    // Wider than any screen, and than the player's largest frame (MAX_SIDE).
    wide: make(dir, "wide.mp4", [...pattern("5000x400"), ...speech, ...h264]),
    // Sound, but nothing said.
    silent: make(dir, "silent.mp4", [
      ...pattern("640x360"),
      ...["-f", "lavfi", "-i", "anullsrc=r=48000:cl=stereo", "-t", "5", "-c:a", "aac"],
      ...h264,
    ]),
    // No sound at all.
    mute: make(dir, "mute.mp4", [...pattern("640x360"), "-an", ...h264]),
  };
}

/**
 * Ten minutes of speech (the landing, over and over), made in `dir` once: long
 * enough to quit in the middle of, and to scroll its transcript. The landing's
 * own MP3, copied, not encoded again: the format the recorded file has, which
 * plays in each system's webview (an AAC .m4a of it didn't play on macOS).
 */
export function longSpeech(dir) {
  mkdirSync(dir, { recursive: true });
  return make(dir, "landed-10-minutes.mp3", ["-stream_loop", "119", "-i", LANDED, "-c", "copy"]);
}

/**
 * The landing said twice, made in `dir` once: two lines of transcript, one
 * after the other, for a phrase that runs from one into the next. Copied,
 * not encoded again, as longSpeech.
 */
export function landedTwice(dir) {
  mkdirSync(dir, { recursive: true });
  return make(dir, "landed-twice.mp3", ["-stream_loop", "1", "-i", LANDED, "-c", "copy"]);
}

/**
 * The colour of each second of colours.mp4, in order. Twelve seconds, far
 * apart in colour: a slow computer (a real laptop, screenshots taking
 * seconds each) must not reach the end while player-picture is still
 * sampling the playing picture.
 */
export const COLOURS = [
  ["red", [255, 0, 0]],
  ["lime", [0, 255, 0]],
  ["blue", [0, 0, 255]],
  ["yellow", [255, 255, 0]],
  ["magenta", [255, 0, 255]],
  ["cyan", [0, 255, 255]],
  ["white", [255, 255, 255]],
  ["purple", [128, 0, 128]],
  ["orange", [255, 165, 0]],
  ["teal", [0, 128, 128]],
  ["gray", [128, 128, 128]],
  ["brown", [165, 42, 42]],
];

/**
 * A video whose picture says where it is: each second one flat colour
 * (COLOURS), so a screenshot tells which frame shows without reading it.
 * No sound. Made in `dir` once.
 */
export function colours(dir) {
  mkdirSync(dir, { recursive: true });
  const inputs = COLOURS.flatMap(([name]) => ["-f", "lavfi", "-i", `color=c=${name}:s=640x360:r=30:d=1`]);
  const joined = `${COLOURS.map((_, i) => `[${i}:v]`).join("")}concat=n=${COLOURS.length}:v=1:a=0[v]`;
  return make(dir, "colours.mp4", [
    ...inputs,
    "-filter_complex",
    joined,
    "-map",
    "[v]",
    "-c:v",
    "libx264",
    "-preset",
    "ultrafast",
    "-pix_fmt",
    "yuv420p",
    "-g",
    "30",
  ]);
}

/** Seconds between longGop.mp4's keyframes. */
export const GOP = 5;

/**
 * Twenty seconds as colours.mp4 (each second one flat colour, COLOURS over
 * again from the thirteenth), at 60 fps with a keyframe only every `GOP`
 * seconds, as a screen recording has: its storyboard has a frame only every
 * five seconds, though its slots are two apart. No sound. Made in `dir` once.
 */
export function longGop(dir) {
  mkdirSync(dir, { recursive: true });
  const seconds = 20;
  const names = Array.from({ length: seconds }, (_, i) => COLOURS[i % COLOURS.length][0]);
  const inputs = names.flatMap((name) => ["-f", "lavfi", "-i", `color=c=${name}:s=640x360:r=60:d=1`]);
  const joined = `${names.map((_, i) => `[${i}:v]`).join("")}concat=n=${seconds}:v=1:a=0[v]`;
  const keyint = GOP * 60;
  return make(dir, "long-gop.mp4", [
    ...inputs,
    "-filter_complex",
    joined,
    "-map",
    "[v]",
    ...["-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p"],
    // No keyframe where the colour changes, either.
    ...["-x264-params", `keyint=${keyint}:min-keyint=${keyint}:scenecut=0`],
  ]);
}

/** A minute of picture and sound (a tone), made in `dir` once: long enough to seek far in. */
export function minute(dir) {
  mkdirSync(dir, { recursive: true });
  return make(dir, "minute.mp4", [
    ...["-f", "lavfi", "-i", "testsrc2=size=640x360:rate=30:duration=60"],
    ...["-f", "lavfi", "-i", "sine=frequency=440:duration=60", "-c:a", "aac", "-shortest"],
    ...["-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p"],
  ]);
}

/**
 * Three minutes of 720p picture and a tone, made in `dir` once: a clip of
 * all of it takes the app seconds to make, long enough to quit in the middle of.
 */
export function longVideo(dir) {
  mkdirSync(dir, { recursive: true });
  return make(dir, "three-minutes.mp4", [
    ...["-f", "lavfi", "-i", "testsrc2=size=1280x720:rate=30:duration=180"],
    ...["-f", "lavfi", "-i", "sine=frequency=440:duration=180", "-c:a", "aac", "-shortest"],
    ...["-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p"],
  ]);
}

/** The test Twitch VOD's id: a VOD made from a file is given it, and chat() as its chat. */
export const TWITCH_VOD = "2901646294";

// A tiny seeded generator, so a chat is the same on every run and machine.
function random(seed) {
  let a = seed;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** The chat's streamer: the VOD's channel, whose chat messages `from:` finds with their speech. */
export const CHANNEL = "Marlowe";
/** Chatters by name, most messages first; "Some One" has a space, as `from:"Some One"` is for. */
export const CHATTERS = ["Ada", "Brigid", "Cormac", "Some One", "Dara"];
/** When the chat's stream started, as the app keeps it (RFC 3339, UTC, to the second). */
export const STARTED = "2026-10-14T18:00:05Z";

/**
 * A synthetic Twitch chat for a VOD of `duration` seconds (two hours), the
 * same on every run: about 3,000 messages, in the shape src-tauri/src/e2e.rs
 * serves (time, author, text, and when it was sent; nothing more, so no emote
 * images: Twitch's emote fragments aren't part of it, and 7TV and the others
 * need the network).
 *
 * - Named chatters (CHATTERS) and the streamer (CHANNEL) among thirty
 *   "viewerN"s, each named one under 500 messages so `from:` finds all of
 *   theirs (a search shows 500 at most).
 * - Bursts at known times (BURSTS): a crowd all writing one emote's name
 *   within seconds, which the minimap's histogram shows as spikes. KEKW in
 *   the first, PogChamp in the second, both in the third.
 * - Events as Twitch's chat posts them: subs, gifts and tips (bits, as
 *   "Cheer500"), which the app reads from the author's name and the text.
 * - Mentions ("@Ada ..."), the streamer saying "landed" (as the transcribed
 *   fixture does), and "gg" in some messages for a `-gg`.
 * - Each message sent (`created_at`) its time after STARTED, as the VOD's
 *   start the app works out from them should be.
 *
 * Returns the messages, sorted by time, with the facts the tests check.
 */
export function chat(duration = 2 * 3600) {
  const rand = random(20261023);
  const pick = (list) => list[Math.floor(rand() * list.length)];
  const viewers = Array.from({ length: 30 }, (_, i) => `viewer${i + 1}`);
  // Who writes a background message: weights, then the viewers share the rest.
  const weights = [
    [CHANNEL, 0.04],
    ["Ada", 0.12],
    ["Brigid", 0.09],
    ["Cormac", 0.07],
    ["Some One", 0.03],
    ["Dara", 0.02],
  ];
  const author = () => {
    let r = rand();
    for (const [name, w] of weights) if ((r -= w) < 0) return name;
    return pick(viewers);
  };
  const lines = [
    "that was close",
    "nice one",
    "no way",
    "let's go",
    "clean",
    "oof",
    "chat is moving fast",
    "what game is this",
    "how long have you been streaming",
    "same here",
    "I missed the start",
    "play the other map",
    "this part is hard",
    "good luck",
    "wait what happened",
  ];
  const messages = [];
  const background = 2600;
  for (let i = 0; i < background; i++) {
    const time = Math.round(((i + rand()) * duration * 10) / background) / 10;
    const who = author();
    let text = pick(lines);
    const r = rand();
    if (r < 0.08) text = `gg ${text}`;
    else if (r < 0.2) text = `@${pick(CHATTERS.filter((c) => !c.includes(" ")))} ${text}`;
    if (who === CHANNEL && rand() < 0.1) text = "we landed it";
    messages.push({ time, author: who, text });
  }
  // The streamer says it three times at fixed moments, whatever the dice gave.
  for (const time of [600, 2400, 4800]) messages.push({ time, author: CHANNEL, text: "the eagle has landed" });

  const bursts = [
    { at: 1200, seconds: 30, count: 120, words: ["KEKW"] },
    { at: 3600, seconds: 20, count: 150, words: ["PogChamp"] },
    { at: 5400, seconds: 40, count: 200, words: ["KEKW", "PogChamp"] },
  ];
  for (const burst of bursts) {
    for (let i = 0; i < burst.count; i++) {
      // At most a few to a second, so Twitch's pages (which end on a second) lose none.
      const time = Math.round((burst.at + (i / burst.count) * burst.seconds) * 10) / 10;
      messages.push({ time, author: pick(viewers), text: `${pick(burst.words)} ${pick(burst.words)}` });
    }
  }

  const events = [];
  for (const time of [300, 1500, 2700, 4100, 5000, 6900]) {
    const who = pick(viewers);
    events.push({
      time,
      author: who,
      text: `${who} subscribed at Tier 1. They've subscribed for ${1 + Math.floor(rand() * 20)} months!`,
    });
  }
  for (const time of [900, 3300, 6000]) {
    const who = pick(viewers);
    events.push({ time, author: who, text: `${who} is gifting 5 Tier 1 Subs to ${CHANNEL}'s community!` });
  }
  for (const time of [1800, 3900, 5700, 7000]) {
    const who = pick(viewers);
    events.push({ time, author: who, text: `${who} Cheer500 keep it up` });
  }
  messages.push(...events);

  messages.sort((a, b) => a.time - b.time);
  for (const m of messages) m.created_at = new Date(Date.parse(STARTED) + m.time * 1000).toISOString();
  const count = (name) => messages.filter((m) => m.author.toLowerCase() === name.toLowerCase()).length;
  for (const name of [...CHATTERS, CHANNEL]) {
    if (count(name) > 450) throw new Error(`${name} has ${count(name)} messages: a search would cut them off`);
  }
  const hasWord = (m, word) => new RegExp(`(^|\\s)${word}(\\s|$)`, "i").test(m.text);
  return {
    duration,
    channel: CHANNEL,
    started: STARTED,
    messages,
    bursts,
    // Subs, gifts and tips, as the app counts them: 6, 3 and 4.
    events: { sub: 6, gift: 3, tip: 4 },
    /** Messages by `author` (any case). */
    by: (name) => messages.filter((m) => m.author.toLowerCase() === name.toLowerCase()),
    /** Messages with `word` as a whole word (any case). */
    withWord: (word) => messages.filter((m) => hasWord(m, word)),
    /** Messages from `from` seconds to before `to`. */
    between: (from, to) => messages.filter((m) => m.time >= from && m.time < to),
  };
}

/** Leaves `fixture` (chat()) in the data folder for the e2e build to serve as Twitch's chat; `failAfter` refuses Twitch's pages from that one on. */
export function serveChat(fixture, failAfter = null) {
  writeFileSync(
    path.join(dataDirs()[0], "e2e-chat.json"),
    JSON.stringify({ messages: fixture.messages, fail_after: failAfter }),
  );
}

/**
 * Makes VOD `id` (added from a file, transcribed or not) the test Twitch VOD
 * of `fixture`'s channel and duration, with its chat not fetched yet, serves
 * `fixture` as Twitch's chat and starts the app again, which fetches it:
 * returns the new app and the VOD once its chat is whole. Its speech stays.
 */
export async function fetchChat(app, ctx, id, fixture) {
  await ctx.quit();
  serveChat(fixture);
  const db = new DatabaseSync(path.join(dataDirs()[0], "library.sqlite"));
  db.prepare(
    `UPDATE vods SET platform = 'twitch', platform_id = ?, url = ?, title = 'A stream with chat', channel = ?,
       duration = ?, source_path = NULL, chat = NULL, chat_error = NULL WHERE id = ?`,
  ).run(TWITCH_VOD, `https://www.twitch.tv/videos/${TWITCH_VOD}`, fixture.channel, fixture.duration, id);
  db.close();
  app = await ctx.start();
  await app.toLibrary();
  const vod = await until(
    "the chat to be fetched",
    async () => {
      const v = (await app.invoke("list_vods")).find((v) => v.id === id);
      if (v?.chat === "failed") throw new Error(`the chat fetch failed: ${v.chat_error}`);
      return v?.chat === "done" && v;
    },
    2 * MINUTE,
  );
  return { app, vod };
}
