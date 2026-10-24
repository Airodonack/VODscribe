// Makes fixtures/library-<version>.sqlite and settings-<version>.json for one
// of the upgrade matrix's versions (scenarios/upgrade-<version>.mjs): a library
// as that release left it, with every kind of row the app keeps: a VOD of each
// site and a file, transcripts with words, chat with authors, clips, marks,
// edits with pieces (and, from 0.5, groups) and, as of 0.5.2's development, the
// stretches table. Unlike make-old-library.mjs it doesn't drive the old app
// (chat and the sites' VODs can't be fetched offline): it builds the schema
// the release's own code builds, with the SQL migrations read from that
// release's crates/core/src/db.rs in git, and fills it with rows in the
// shape that release wrote them. A library one release upgraded is the next
// one's: the rows are put in at format 21 (0.4.0's), and the migrations after
// it (the clip model's, in 0.5.0) turn them into the later shapes, as a
// user's own library was turned; what a later release makes (groups, the
// stretches) is added after.
//
//   node tests/e2e/fixtures/make-matrix-library.mjs <version>
//
// The files' folder is the placeholder {FIXTURES}, as in make-old-library.mjs.

import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));

// The git revision whose db.rs is the release's, and the settings it wrote.
// 0.5.2-dev is main as of "Version 0.5.2" (4bdb439), before key bindings.
const SETTINGS = {
  device: "cpu",
  theme: "light",
  emote_providers: [
    { id: "ffz", on: false },
    { id: "7tv", on: false },
    { id: "bttv", on: false },
  ],
  video_height: 720,
  clip_format: "mov",
};
const RELEASES = {
  "0.4.0": { rev: "v0.4.0", settings: SETTINGS },
  "0.5.1": { rev: "v0.5.1", settings: SETTINGS },
  "0.5.2-dev": { rev: "4bdb439", settings: SETTINGS },
};
const FLICKS = 705600000;
const flicks = (seconds) => Math.round(seconds * FLICKS);
const SEED_FORMAT = 21;

const version = process.argv[2];
const release = RELEASES[version];
if (!release) {
  console.error(`usage: node tests/e2e/fixtures/make-matrix-library.mjs <${Object.keys(RELEASES).join("|")}>`);
  process.exit(2);
}

/** The migrations of db.rs at `rev`, in the order `Db::open` runs them. */
function migrationsAt(rev) {
  const source = execFileSync("git", ["show", `${rev}:crates/core/src/db.rs`], { cwd: here, encoding: "utf8" });
  const constants = new Map();
  for (const m of source.matchAll(/^const (SCHEMA|MIGRATE_V\d+): &str =\s*(?:r#"([\s\S]*?)"#|"([\s\S]*?)");$/gm))
    constants.set(m[1], m[2] ?? m[3]);
  const list = source.match(/let migrations = \[([\s\S]*?)\];/)[1];
  return [...list.matchAll(/(SCHEMA|MIGRATE_V\d+),/g)].map((m) => {
    if (!constants.has(m[1])) throw new Error(`${m[1]} isn't a string constant of ${rev}'s db.rs`);
    return constants.get(m[1]).replace(/FLICKS/g, String(FLICKS));
  });
}

const BASE = Date.UTC(2026, 9, 18) / 1000;

/** A line of `text` from `start`, its words 0.4 s apart. */
function line(start, text) {
  const words = text.split(" ").map((w, i) => ({
    text: w,
    start: +(start + i * 0.4).toFixed(2),
    end: +(start + i * 0.4 + 0.36).toFixed(2),
  }));
  return { start: words[0].start, end: words.at(-1).end, text, words: JSON.stringify(words) };
}

const VODS = [
  {
    id: 1,
    platform: "local",
    platform_id: "{FIXTURES}/eagle-has-landed.mp3",
    url: "{FIXTURES}/eagle-has-landed.mp3",
    title: "eagle-has-landed",
    duration: 5.36,
    status: "done",
    progress: 1,
    source_path: "{FIXTURES}/eagle-has-landed.mp3",
    chat: "none",
    lines: [line(0.88, "Houston, Tranquility Base here. The Eagle has landed.")],
    chatLines: [],
  },
  {
    id: 2,
    platform: "youtube",
    platform_id: "IaIo3d6cUuo",
    url: "https://www.youtube.com/watch?v=IaIo3d6cUuo",
    title: "Building a lighthouse on the cliff",
    channel: "Airodonack",
    duration: 90,
    published: "2026-10-12",
    status: "done",
    progress: 1,
    chat: "done",
    lines: [
      line(0, "welcome back everybody today we are building a lighthouse"),
      line(8, "first we need to find the right cliff above the harbour"),
      line(16, "this one looks perfect and the wind is calm tonight"),
      line(24, "now the foundation needs a lot of grey stone"),
      line(32, "the harbour master wants a lamp that never goes out"),
      line(40, "almost done with the tower so check the view"),
    ],
    chatLines: [
      [2, "NightOwl42", "#1e90ff", "hello from the night shift", "message", null],
      [9, "harbor_master", "#ff7f50", "that cliff is too steep", "message", null],
      [17, "NightOwl42", "#1e90ff", "calm wind for once", "message", null],
      [25, "LoudLarry", null, "gift subs for the stone crew", "gift", 5],
      [33, "NightOwl42", "#1e90ff", "build the lamp bigger", "message", null],
      [41, "harbor_master", "#ff7f50", "lighthouse looks great", "message", null],
    ],
  },
  {
    id: 3,
    platform: "twitch",
    platform_id: "2901646294",
    url: "https://www.twitch.tv/videos/2901646294",
    title: "Speedrunning the old tutorial",
    channel: "airodonack",
    duration: 75,
    published: "2026-10-14",
    status: "done",
    progress: 1,
    chat: "done",
    lines: [
      line(0, "chat asked for a speedrun of the old tutorial"),
      line(7, "the pelican skip saves us eight seconds"),
      line(15, "do not blink here because the door closes fast"),
      line(23, "and that is the whole tutorial in one minute"),
    ],
    chatLines: [
      [3, "NightOwl42", "#1e90ff", "pelican skip when", "message", null],
      [8, "pelicanfan", "#9acd32", "the pelican skip works", "message", null],
      [16, "Subbed_Sam", "#da70d6", "resubscribed", "sub", 1],
      [24, "BigTipper", null, "great run take this", "tip", 5],
    ],
  },
  {
    id: 4,
    platform: "kick",
    platform_id: "01a169a1-9aa8-7653-8a9e-390e694cc11a",
    url: "https://kick.com/airodonack/videos/01a169a1-9aa8-7653-8a9e-390e694cc11a",
    title: "Walking the wetland trail",
    channel: "airodonack",
    duration: 120,
    status: "paused",
    progress: 0.75,
    chat: "none",
    // Transcribed to 90 s of its 120, paused there.
    lines: [
      line(0, "we start at the edge of the wetland"),
      line(20, "the boardwalk goes over the reeds"),
      line(45, "a heron is standing in the shallows"),
      line(70, "the trail turns back along the river"),
      line(85, "that is the last stretch before the car park"),
    ],
    chatLines: [],
  },
];

// Clips on the VODs' bars (format 21): [vod, start, end, title].
const CLIPS = [
  [1, 1, 3, "Houston"],
  [2, 0, 5, "Welcome back"],
  [2, 32, 38, "The lamp"],
  [3, 7, 12, "Pelican skip"],
];
// Marks on VODs (format 21): [vod, kind, start, text].
const MARKS = [
  [2, "chapter", 0, "Intro"],
  [2, "cut", 24, null],
  [2, "highlight", 40, "Tower done"],
  [3, "chapter", 7, "The skip"],
];
// Edits (format 21): their pieces [vod, start, end, title, struck seconds].
const EDITS = [
  [
    "Landing highlights",
    [
      [1, 0.9, 2, "Houston", []],
      [1, 2.2, 4, "Base", [[2.6, 3]]],
      [1, 4.2, 5.2, "Landed", []],
    ],
  ],
  [
    "Stream highlights",
    [
      [2, 16, 24, "Calm wind", []],
      [3, 7, 15, "Pelican", [[10, 11.5]]],
      [4, 20, 45, "Boardwalk", []],
    ],
  ],
];

/** The rows of format 21 (0.4.x): the clip model came after. */
function seed(db) {
  const vod = db.prepare(
    `INSERT INTO vods (id, platform, platform_id, url, title, channel, duration, thumbnail, published, status,
       progress, added_at, model, position, thumb, source_path, chat)
     VALUES (?, ?, ?, ?, ?, ?, ?, NULL, ?, ?, ?, ?, 'parakeet-tdt-0.6b-v3-int8wo', ?, '', ?, ?)`,
  );
  const segment = db.prepare("INSERT INTO segments (vod_id, start, end, text, words) VALUES (?, ?, ?, ?, ?)");
  const chat = db.prepare(
    "INSERT INTO chat (vod_id, time, author, color, text, parts, kind, amount) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
  );
  for (const v of VODS) {
    // Newest first: the last added is at the top.
    vod.run(
      v.id,
      v.platform,
      v.platform_id,
      v.url,
      v.title,
      v.channel ?? null,
      v.duration,
      v.published ?? null,
      v.status,
      v.progress,
      BASE + v.id * 3600,
      VODS.length - v.id,
      v.source_path ?? null,
      v.chat,
    );
    for (const l of v.lines) segment.run(v.id, l.start, l.end, l.text, l.words);
    for (const [time, author, color, text, kind, amount] of v.chatLines)
      chat.run(v.id, time, author, color, text, JSON.stringify([{ type: "text", text }]), kind, amount);
  }
  const clip = db.prepare(
    `INSERT INTO clips (vod_id, start, end, title, position, created_at, status, file_path, shown)
     VALUES (?, ?, ?, ?, ?, ?, 'ready', '{FIXTURES}/eagle-has-landed.mp3', 1)`,
  );
  CLIPS.forEach(([v, a, b, title], i) => clip.run(v, a, b, title, i + 1, BASE + 86400 + i));
  const mark = db.prepare(
    "INSERT INTO marks (vod_id, kind, start, text, source, created_at) VALUES (?, ?, ?, ?, 'user', ?)",
  );
  MARKS.forEach(([v, kind, start, text], i) => mark.run(v, kind, start, text, BASE + 90000 + i));
  const edit = db.prepare(
    "INSERT INTO edits (title, position, created_at, version, status) VALUES (?, ?, ?, 3, 'waiting')",
  );
  const piece = db.prepare(
    "INSERT INTO edit_pieces (edit_id, vod_id, position, start, end, title, struck) VALUES (?, ?, ?, ?, ?, ?, ?)",
  );
  EDITS.forEach(([title, pieces], i) => {
    const id = Number(edit.run(title, i + 1, BASE + 100000 + i).lastInsertRowid);
    pieces.forEach(([v, a, b, name, struck], j) => piece.run(id, v, j, a, b, name, JSON.stringify(struck)));
  });
}

/** What the clip model (format 22 on) lets an edit hold: a group of pieces, a mark of its own. */
function groups(db) {
  const { id: edit } = db.prepare("SELECT id FROM clips WHERE title = 'Landing highlights' AND whole_of IS NULL").get();
  const item = db.prepare(
    "INSERT INTO items (clip_id, parent, position, kind, vod_id, start, end, title) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
  );
  const group = Number(item.run(edit, null, 3, "group", null, null, null, "Houston, again").lastInsertRowid);
  item.run(edit, group, 0, "piece", 1, flicks(0.9), flicks(1.6), "");
  item.run(edit, group, 1, "piece", 1, flicks(1.8), flicks(2.4), "");
  db.prepare(
    "INSERT INTO marks (clip_id, vod_id, kind, at, text, source, created_at) VALUES (?, 1, 'chapter', ?, 'Arrival', 'user', ?)",
  ).run(edit, flicks(1), BASE + 200000);
}

/**
 * 0.5.2 in development: the Kick VOD's transcription as a first minute done
 * and the rest asked for and paused at 90 s, as stretches (the format-27
 * migration made one request of all of it).
 */
function stretches(db) {
  db.exec("DELETE FROM stretches WHERE vod_id = 4");
  db.exec(`INSERT INTO stretches (vod_id, start, end, status, progress, reached, model)
    VALUES (4, 0, 60, 'done', 1, 60, 'parakeet-tdt-0.6b-v3-int8wo'),
           (4, 60, 120, 'paused', 0.5, 90, 'parakeet-tdt-0.6b-v3-int8wo')`);
}

const migrations = migrationsAt(release.rev);
const tmp = mkdtempSync(path.join(tmpdir(), "matrix-library-"));
const file = path.join(tmp, "library.sqlite");
try {
  const db = new DatabaseSync(file);
  db.exec("PRAGMA foreign_keys = ON");
  db.exec(`BEGIN; ${migrations.slice(0, SEED_FORMAT).join("\n")}; PRAGMA user_version = ${SEED_FORMAT}; COMMIT;`);
  seed(db);
  if (migrations.length > SEED_FORMAT) {
    // Db::open's upgrade: the struck stretches, seconds till format 22, are put in flicks.
    db.exec(`BEGIN; ${migrations.slice(SEED_FORMAT).join("\n")}`);
    const struck = db.prepare("SELECT id, struck FROM items WHERE struck != '[]'").all();
    for (const { id, struck: seconds } of struck) {
      const flicked = JSON.parse(seconds).map(([a, b]) => [flicks(a), flicks(b)]);
      db.prepare("UPDATE items SET struck = ? WHERE id = ?").run(JSON.stringify(flicked), id);
    }
    db.exec(`PRAGMA user_version = ${migrations.length}; COMMIT;`);
    groups(db);
  }
  if (version === "0.5.2-dev") stretches(db);
  db.exec("PRAGMA wal_checkpoint(TRUNCATE)");
  db.exec("VACUUM");
  db.close();
  const out = path.join(here, `library-${version}.sqlite`);
  writeFileSync(out, readFileSync(file));
  const text = readFileSync(out).toString("latin1");
  for (const home of [process.env.HOME, process.env.USERPROFILE, process.env.USER].filter(Boolean))
    if (text.includes(home)) throw new Error(`${out} still has "${home}" in it`);
  writeFileSync(path.join(here, `settings-${version}.json`), JSON.stringify(release.settings, null, 2) + "\n");
  console.log(`made ${out} (format ${migrations.length}) from ${release.rev}'s migrations`);
} finally {
  rmSync(tmp, { recursive: true, force: true });
}
