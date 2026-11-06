// The app's data folders, emptied before each scenario so each starts as a
// new install would, but for the downloaded tools and speech model (bin,
// models, gpu), which setup would only fetch again.
//
// This deletes a library. It refuses unless E2E_RESET_DATA=1, or the folder
// has no library yet: on a computer that's used, point XDG_DATA_HOME (Linux)
// somewhere else, or run on a test machine.

import {
  closeSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  openSync,
  readdirSync,
  readSync,
  renameSync,
  rmSync,
} from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

const ID = "app.vodscribe";
const KEPT = new Set(["bin", "models", "gpu"]);

/** The app's data folders: [roaming, local], the same folder but on Windows. */
export function dataDirs() {
  switch (process.platform) {
    case "win32":
      return [path.join(process.env.APPDATA, ID), path.join(process.env.LOCALAPPDATA, ID)];
    case "darwin":
      return [path.join(homedir(), "Library", "Application Support", ID)];
    default:
      return [path.join(process.env.XDG_DATA_HOME || path.join(homedir(), ".local", "share"), ID)];
  }
}

/** Where the app puts clips' and edits' files (`Paths::clips`): in its local data. */
export const clipsDir = () => path.join(dataDirs().at(-1), "clips");

/** Where VOD `id`'s kept video is, with extension `ext` (`Paths::source_video`). */
export const keptVideo = (id, ext) => path.join(dataDirs().at(-1), "media", `${id}.${ext}`);

/** The app's library: its database in the data folder (`dataDirs`). */
export const libraryPath = () => path.join(dataDirs()[0], "library.sqlite");

/** Each site's link to its video `id`, as a VOD added by its link holds it. */
const SITE_LINKS = {
  twitch: (id) => `https://www.twitch.tv/videos/${id}`,
  youtube: (id) => `https://www.youtube.com/watch?v=${id}`,
  kick: (id) => `https://kick.com/vodscribe/videos/${id}`,
  dailymotion: (id) => `https://www.dailymotion.com/video/${id}`,
};

/** The columns of a VOD `makeSiteVod` sets beside its site's, by the names it takes them by. */
const SITE_VOD_COLUMNS = {
  title: "title",
  channel: "channel",
  duration: "duration",
  chat: "chat",
  chatError: "chat_error",
};

/**
 * Makes VOD `id` (a file added here) `platform`'s video `platformId`, as if
 * added by its link, without the network: its file gone from it (and any
 * video kept for it), its chat known to have none unless `fields` says
 * (`chat: null` has the app fetch it at its next start). `fields` may also
 * set its `title`, `channel`, `duration`, `chatError` and `video` (a file
 * copied in as the video kept for it, as a download leaves it). With the
 * app stopped, as it reads the library at start.
 *
 * @param {number} id
 * @param {string} platform
 * @param {string} platformId
 * @param {{ title?: string, channel?: string | null, duration?: number, chat?: string | null,
 *   chatError?: string | null, video?: string }} [fields]
 */
export function makeSiteVod(id, platform, platformId, fields = {}) {
  const link = SITE_LINKS[platform];
  if (!link) throw new Error(`no link for a ${platform} VOD`);
  const set = {
    platform,
    platform_id: platformId,
    url: link(platformId),
    source_path: null,
    received_ext: null,
    video_ext: null,
    media_path: null,
    chat: "none",
  };
  const { video, ...rest } = fields;
  if (video) {
    const ext = path.extname(video).slice(1);
    mkdirSync(path.dirname(keptVideo(id, ext)), { recursive: true });
    copyFileSync(video, keptVideo(id, ext));
    // Its extension, and its full path beside it as the app writes them
    // (`Db::set_media_path`): without the path, the app takes the video as
    // one a version before 0.7.2 forgot (`library::settle`).
    set.video_ext = ext;
    set.media_path = keptVideo(id, ext);
  }
  for (const [name, value] of Object.entries(rest)) {
    const column = SITE_VOD_COLUMNS[name];
    if (!column) throw new Error(`makeSiteVod doesn't set ${name}`);
    set[column] = value;
  }
  const db = new DatabaseSync(libraryPath());
  try {
    const columns = Object.keys(set);
    const done = db
      .prepare(`UPDATE vods SET ${columns.map((c) => `${c} = ?`).join(", ")} WHERE id = ?`)
      .run(...columns.map((c) => set[c]), id);
    if (done.changes !== 1) throw new Error(`no VOD ${id} in the library to make ${platform}'s`);
  } finally {
    db.close();
  }
}

/** The line every yt-dlp stand-in of the tests carries (siteVod.mjs, scenarios/download-play.mjs). */
export const YT_DLP_STAND_IN = "stand-in for yt-dlp";

/**
 * Puts the real yt-dlp back in `bin` (the data folder's) where a run that
 * died left a stand-in there: the stand-ins are put back in a `finally`
 * that a killed run never reaches, and one left in place answers every
 * site's lookup in every run after (one sat on a runner for days). The real
 * one moved aside (yt-dlp.e2e-real) comes back; with none, the stand-in is
 * removed, and setup downloads yt-dlp again. Says what it did, or null.
 */
export function putBackYtDlp(bin = path.join(dataDirs().at(-1), "bin")) {
  const ytDlp = path.join(bin, "yt-dlp");
  const real = path.join(bin, "yt-dlp.e2e-real");
  if (existsSync(ytDlp) && !isStandIn(ytDlp)) return null;
  if (existsSync(real)) {
    renameSync(real, ytDlp);
    return `put the real yt-dlp back over a test's stand-in in ${bin}`;
  }
  if (!existsSync(ytDlp)) return null;
  rmSync(ytDlp, { force: true });
  return `removed a test's yt-dlp stand-in from ${bin}: setup downloads yt-dlp again`;
}

/** Whether `file` is one of the tests' stand-ins: its first lines, not the whole (the real one is tens of MB). */
function isStandIn(file) {
  const fd = openSync(file, "r");
  try {
    const head = Buffer.alloc(512);
    return head
      .subarray(0, readSync(fd, head, 0, head.length, 0))
      .toString("latin1")
      .includes(YT_DLP_STAND_IN);
  } finally {
    closeSync(fd);
  }
}

export function resetData() {
  const dirs = [...new Set(dataDirs())];
  const library = dirs.map((d) => path.join(d, "library.sqlite")).find(existsSync);
  if (library && process.env.E2E_RESET_DATA !== "1") {
    throw new Error(`${library} exists: set E2E_RESET_DATA=1 to let the test delete it (and the rest of the library)`);
  }
  for (const dir of dirs) {
    if (!existsSync(dir)) continue;
    for (const name of readdirSync(dir)) {
      if (!KEPT.has(name)) removeForSure(path.join(dir, name));
    }
  }
}

/**
 * Deletes `target` (a file or a folder), trying for up to 20 s: on Windows
 * the last app's WebView2 processes can hold its folder for some seconds
 * after it's gone ("EPERM, Permission denied: ...\EBWebView"), and node's
 * own retries don't always cover that (eleven scenarios failed in a second
 * once, 2026-10-23).
 */
function removeForSure(target) {
  const until = Date.now() + 20_000;
  for (;;) {
    try {
      rmSync(target, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
      return;
    } catch (e) {
      if (Date.now() >= until) throw e;
      Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 250);
    }
  }
}

/**
 * Puts the library and settings release `version` left (`fixtures`'
 * library-<version>.sqlite and settings-<version>.json, made by
 * make-old-library.mjs) in the data folder, each of `places`' placeholders
 * in its files' paths made a folder here ({ "{FIXTURES}": fixtures }).
 */
export function oldLibrary(fixtures, version, places) {
  const dir = dataDirs()[0];
  mkdirSync(dir, { recursive: true });
  const library = path.join(dir, "library.sqlite");
  copyFileSync(path.join(fixtures, `library-${version}.sqlite`), library);
  copyFileSync(path.join(fixtures, `settings-${version}.json`), path.join(dir, "settings.json"));
  const db = new DatabaseSync(library);
  // The paths a library holds: a file's, and (in the matrix's richer ones)
  // the files of clips and rendered edits, in whichever table a version keeps them.
  const paths = [
    ["vods", "source_path"],
    ["clips", "file_path"],
    ["edits", "file_path"],
    ["renders", "file_path"],
  ].filter(([table, column]) => db.prepare(`SELECT 1 FROM pragma_table_info('${table}') WHERE name = ?`).get(column));
  for (const [from, to] of Object.entries(places))
    for (const [table, column] of paths)
      db.prepare(`UPDATE ${table} SET ${column} = replace(${column}, ?, ?)`).run(from, to);
  db.close();
}
