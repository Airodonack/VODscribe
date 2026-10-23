// Grows a library to the size a search is timed on (scenarios/search-large.mjs):
// copies of a VOD already in it, each with hours of made-up talk, written
// straight into its database while the app is shut. The talk is the same on
// every run (a seeded generator): common words, so a search finds thousands
// of lines, and a few phrases planted where a search should find only them.
//
//   grow(<library.sqlite>, { vods, hours })  → { vods, lines }

import { DatabaseSync } from "node:sqlite";

/** Said in every VOD once, at its middle: a search with one hit a VOD. */
export const RARE = "tranquility base relay";
/** In about one line in eight: a search with many. */
export const COMMON = "chat";

const WORDS = (
  "the a and to of in is that it you this for on with was be are we so but not just like what have can " +
  "there they do all if up out know get go yeah right now one about here think really okay well time " +
  "game play run boss level map team round chat stream clip vod back next win lose fight wait look " +
  "good nice wow let me see oh no yes maybe never always going got make take thing people way much"
).split(" ");

/** A small seeded generator (mulberry32): the same talk on every run. */
function random(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/**
 * Adds `vods` copies of the library's first VOD, each `hours` long with a
 * line of talk every 4 seconds. Returns how many VODs and lines it added.
 */
export function grow(file, { vods = 60, hours = 2 } = {}) {
  const db = new DatabaseSync(file);
  const template = db.prepare("SELECT * FROM vods ORDER BY id LIMIT 1").get();
  if (!template) throw new Error(`no VOD in ${file} to copy`);
  // A VOD is in the library's list by its own clip (clips.whole_of) and that
  // clip's piece (items): copied with it, the piece made as long as the copy.
  const clip = db.prepare("SELECT * FROM clips WHERE whole_of = ?").get(template.id);
  const pieces = db.prepare("SELECT * FROM items WHERE clip_id = ?").all(clip.id);
  if (pieces.some((p) => p.parent !== null)) throw new Error("the VOD's own clip has groups: copy a plainer one");
  const insert = (table, row) => {
    const columns = Object.keys(row).filter((c) => c !== "id");
    return db
      .prepare(
        `INSERT INTO ${table} (${columns.join(", ")}) VALUES (${columns.map(() => "?").join(", ")}) RETURNING id`,
      )
      .get(...columns.map((c) => row[c])).id;
  };
  let position = db.prepare("SELECT coalesce(max(position), 0) AS p FROM clips").get().p;
  const addLine = db.prepare("INSERT INTO segments (vod_id, start, end, text, words) VALUES (?, ?, ?, ?, ?)");
  const next = random(1969);
  const pick = () => WORDS[Math.floor(next() * WORDS.length)];
  const length = hours * 3600;
  let lines = 0;
  db.exec("BEGIN");
  try {
    for (let v = 0; v < vods; v++) {
      const row = {
        ...template,
        platform_id: `${template.platform_id}-large-${v + 1}`,
        url: `${template.url}#large-${v + 1}`,
        title: `Long stream ${v + 1}`,
        duration: length,
      };
      const id = insert("vods", row);
      const own = insert("clips", { ...clip, title: row.title, whole_of: id, position: (position += 1) });
      const scale = length / template.duration;
      for (const piece of pieces)
        insert("items", {
          ...piece,
          clip_id: own,
          vod_id: piece.vod_id === template.id ? id : piece.vod_id,
          start: piece.start === null ? null : Math.round(piece.start * scale),
          end: piece.end === null ? null : Math.round(piece.end * scale),
        });
      for (let start = 0; start < length; start += 4) {
        const said = Array.from({ length: 6 + Math.floor(next() * 8) }, pick);
        if (next() < 0.125) said.splice(Math.floor(next() * said.length), 0, COMMON);
        if (start === Math.floor(length / 2 / 4) * 4) said.push(...RARE.split(" "));
        const step = 3.6 / said.length;
        const words = said.map((text, i) => ({ text, start: start + i * step, end: start + (i + 1) * step }));
        addLine.run(id, start, start + 3.6, said.join(" "), JSON.stringify(words));
        lines += 1;
      }
    }
    db.exec("COMMIT");
  } catch (e) {
    db.exec("ROLLBACK");
    throw e;
  } finally {
    db.close();
  }
  return { vods, lines };
}
