// The library helpers scenarios share (data.mjs): where the library is and
// where clips go, a file's VOD made a site's as if added by its link, and
// the real yt-dlp put back over a stand-in a killed run left.

import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";
import { after, before, test } from "node:test";

import { clipsDir, dataDirs, keptVideo, libraryPath, makeSiteVod, putBackYtDlp, YT_DLP_STAND_IN } from "./data.mjs";

const home = mkdtempSync(path.join(tmpdir(), "vodscribe-data-test-"));
const saved = { ...process.env };

before(() => {
  process.env.XDG_DATA_HOME = home;
  process.env.APPDATA = home;
  process.env.LOCALAPPDATA = home;
  mkdirSync(dataDirs()[0], { recursive: true });
  const db = new DatabaseSync(libraryPath());
  db.exec(`CREATE TABLE vods (id INTEGER PRIMARY KEY, platform TEXT, platform_id TEXT, url TEXT, title TEXT,
    channel TEXT, duration REAL, source_path TEXT, received_ext TEXT, video_ext TEXT, media_path TEXT, chat TEXT, chat_error TEXT)`);
  db.prepare(
    `INSERT INTO vods VALUES (1, 'local', 'a', 'file:///a.mp4', 'a', NULL, 70, '/a.mp4', NULL, NULL, NULL, 'none', NULL)`,
  ).run();
  db.close();
});

after(() => {
  process.env = saved;
  rmSync(home, { recursive: true, force: true });
});

const vod = () => {
  const db = new DatabaseSync(libraryPath(), { readOnly: true });
  try {
    return { ...db.prepare("SELECT * FROM vods WHERE id = 1").get() };
  } finally {
    db.close();
  }
};

test("the library is in the data folder", { skip: process.platform === "darwin" }, () => {
  assert.equal(libraryPath(), path.join(home, "app.vodscribe", "library.sqlite"));
});

test("a file's VOD is made a site's, its file gone and its chat known to have none", () => {
  makeSiteVod(1, "twitch", "2901646294");
  assert.deepEqual(vod(), {
    id: 1,
    platform: "twitch",
    platform_id: "2901646294",
    url: "https://www.twitch.tv/videos/2901646294",
    title: "a",
    channel: null,
    duration: 70,
    source_path: null,
    received_ext: null,
    video_ext: null,
    media_path: null,
    chat: "none",
    chat_error: null,
  });
});

test("the fields given are set, a chat to fetch and a video kept among them", () => {
  const video = path.join(home, "a video.mp4");
  writeFileSync(video, "video");
  makeSiteVod(1, "youtube", "noa29qsTc4g", {
    title: "A stream",
    channel: "Marlowe",
    duration: 14400,
    chat: null,
    video,
  });
  const got = vod();
  assert.equal(got.url, "https://www.youtube.com/watch?v=noa29qsTc4g");
  assert.deepEqual(
    [got.title, got.channel, got.duration, got.chat, got.video_ext, got.media_path],
    ["A stream", "Marlowe", 14400, null, "mp4", keptVideo(1, "mp4")],
  );
  assert.equal(readFileSync(keptVideo(1, "mp4"), "utf8"), "video", "copied in where the app keeps it");
});

test("a field it doesn't know, a site it has no link for or a VOD that isn't there is refused", () => {
  assert.throws(() => makeSiteVod(1, "twitch", "1", /** @type {any} */ ({ thumb: "x" })), /doesn't set thumb/);
  assert.throws(() => makeSiteVod(1, "vimeo", "1"), /no link/);
  assert.throws(() => makeSiteVod(2, "twitch", "1"), /no VOD 2/);
});

test("clips are in the local data folder", () => {
  assert.equal(clipsDir(), path.join(dataDirs().at(-1), "clips"));
});

/** A bin folder of its own holding `files` (name: contents). */
const binWith = (files) => {
  const bin = mkdtempSync(path.join(home, "bin-"));
  for (const [name, text] of Object.entries(files)) writeFileSync(path.join(bin, name), text);
  return bin;
};
const standIn = `#!/bin/sh\n# The e2e tests' ${YT_DLP_STAND_IN} (tests/e2e/lib/siteVod.mjs).\nexit 1\n`;

test("a stand-in left over the real yt-dlp is replaced by the real one", () => {
  const bin = binWith({ "yt-dlp": standIn, "yt-dlp.e2e-real": "the real one" });
  assert.match(putBackYtDlp(bin), /put the real yt-dlp back/);
  assert.equal(readFileSync(path.join(bin, "yt-dlp"), "utf8"), "the real one");
  assert.equal(existsSync(path.join(bin, "yt-dlp.e2e-real")), false);
});

test("a stand-in with no real one aside is removed, for setup to download it", () => {
  const bin = binWith({ "yt-dlp": standIn });
  assert.match(putBackYtDlp(bin), /removed a test's yt-dlp stand-in/);
  assert.equal(existsSync(path.join(bin, "yt-dlp")), false);
});

test("the real one moved aside, with nothing in its place, comes back", () => {
  const bin = binWith({ "yt-dlp.e2e-real": "the real one" });
  assert.match(putBackYtDlp(bin), /put the real yt-dlp back/);
  assert.equal(readFileSync(path.join(bin, "yt-dlp"), "utf8"), "the real one");
});

test("the real yt-dlp, or none, is left as it is", () => {
  const real = binWith({ "yt-dlp": "#!/usr/bin/env python3\n# yt-dlp\n" });
  assert.equal(putBackYtDlp(real), null);
  assert.equal(readFileSync(path.join(real, "yt-dlp"), "utf8"), "#!/usr/bin/env python3\n# yt-dlp\n");
  assert.equal(putBackYtDlp(binWith({})), null);
  assert.equal(putBackYtDlp(path.join(home, "no-bin")), null);
});
