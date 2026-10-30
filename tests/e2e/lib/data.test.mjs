// The library helpers scenarios share (data.mjs): where the library is, and
// a file's VOD made a site's as if added by its link.

import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";
import { after, before, test } from "node:test";

import { dataDirs, libraryPath, makeSiteVod } from "./data.mjs";

const home = mkdtempSync(path.join(tmpdir(), "vodscribe-data-test-"));
const saved = { ...process.env };

before(() => {
  process.env.XDG_DATA_HOME = home;
  process.env.APPDATA = home;
  process.env.LOCALAPPDATA = home;
  mkdirSync(dataDirs()[0], { recursive: true });
  const db = new DatabaseSync(libraryPath());
  db.exec(`CREATE TABLE vods (id INTEGER PRIMARY KEY, platform TEXT, platform_id TEXT, url TEXT, title TEXT,
    channel TEXT, duration REAL, source_path TEXT, media_path TEXT, chat TEXT, chat_error TEXT)`);
  db.prepare(
    `INSERT INTO vods VALUES (1, 'local', 'a', 'file:///a.mp4', 'a', NULL, 70, '/a.mp4', NULL, 'none', NULL)`,
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
    media_path: null,
    chat: "none",
    chat_error: null,
  });
});

test("the fields given are set, a chat to fetch and a video kept among them", () => {
  makeSiteVod(1, "youtube", "noa29qsTc4g", {
    title: "A stream",
    channel: "Marlowe",
    duration: 14400,
    chat: null,
    mediaPath: "/kept.mp4",
  });
  const got = vod();
  assert.equal(got.url, "https://www.youtube.com/watch?v=noa29qsTc4g");
  assert.deepEqual(
    [got.title, got.channel, got.duration, got.chat, got.media_path],
    ["A stream", "Marlowe", 14400, null, "/kept.mp4"],
  );
});

test("a field it doesn't know, a site it has no link for or a VOD that isn't there is refused", () => {
  assert.throws(() => makeSiteVod(1, "twitch", "1", /** @type {any} */ ({ thumb: "x" })), /doesn't set thumb/);
  assert.throws(() => makeSiteVod(1, "vimeo", "1"), /no link/);
  assert.throws(() => makeSiteVod(2, "twitch", "1"), /no VOD 2/);
});
