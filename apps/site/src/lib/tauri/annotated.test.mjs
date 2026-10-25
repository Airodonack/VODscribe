// A VOD page's sources and the annotations turned into the demo's shapes
// (annotated.ts), against the fixture for the Twitch test VOD.
// node --test apps/site/src/lib/tauri/annotated.test.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import {
  chatBeside,
  chatOf,
  indexEntries,
  pageOf,
  pickDropped,
  pickSource,
  segmentsOf,
  storedId,
  vodOf,
} from "./annotated.ts";
import { parseAnnotations } from "../../../../desktop/src/lib/annotations.ts";

/** @param {string} name */
const fixture = (name) => JSON.parse(readFileSync(new URL(`fixtures/${name}`, import.meta.url), "utf8"));
const annotations = parseAnnotations(fixture("v2901646294.json"));
const PAGE = "https://vodscribe.com/v/twitch/2901646294";

test("the download host's files, by the id the app stores", () => {
  assert.deepEqual(pickSource("twitch", "2901646294", new URLSearchParams(), PAGE), {
    annotations: "https://dl.vodscribe.com/annotations/twitch/v2901646294.json",
    chat: "https://dl.vodscribe.com/annotations/twitch/v2901646294.chat.json",
    custom: false,
  });
  // Twitch's own ids carry no "v"; one typed with it is the same VOD.
  assert.equal(storedId("twitch", "v2901646294"), "v2901646294");
  assert.equal(storedId("youtube", "IaIo3d6cUuo"), "IaIo3d6cUuo");
  assert.equal(
    pickSource("youtube", "IaIo3d6cUuo", new URLSearchParams(), PAGE)?.annotations,
    "https://dl.vodscribe.com/annotations/youtube/IaIo3d6cUuo.json",
  );
  // A platform the page doesn't know has no files to look for.
  assert.equal(pickSource("vimeo", "1", new URLSearchParams(), PAGE), null);
});

test("?src= takes the place of the host, the chat beside it, relative to the page", () => {
  assert.deepEqual(pickSource("twitch", "2901646294", new URLSearchParams("src=/files/run.json"), PAGE), {
    annotations: "https://vodscribe.com/files/run.json",
    chat: "https://vodscribe.com/files/run.chat.json",
    custom: true,
  });
  // Any platform, even one the address doesn't name: the file says which.
  assert.equal(
    pickSource("x", "", new URLSearchParams("src=https://dl.vodscribe.com/a/b.json?v=2"), PAGE)?.chat,
    "https://dl.vodscribe.com/a/b.chat.json?v=2",
  );
  assert.equal(chatBeside("https://example.com/plain"), "https://example.com/plain.chat.json");
});

test("files dropped: the .chat.json is the chat, the other the annotations", () => {
  const a = { name: "v2901646294.json" };
  const c = { name: "v2901646294.chat.json" };
  assert.deepEqual(pickDropped([c, a]), { annotations: a, chat: c });
  assert.deepEqual(pickDropped([a]), { annotations: a, chat: null });
  /** @param {{ name: string }[]} files */
  const refused = (files) => /** @type {{ error: string }} */ (pickDropped(files)).error;
  assert.match(refused([c]), /chat file/);
  assert.match(refused([a, { name: "other.json" }]), /one annotations file/);
  assert.match(refused([]), /No file/);
});

test("the transcript in the demo's shape, keeping the words' starts and the lines' ends", () => {
  const segments = segmentsOf(annotations);
  assert.equal(segments.length, 4);
  const [text, start, starts, end] = segments[1];
  assert.equal(text, "the pelican skip saves us eight seconds");
  assert.equal(start, 7);
  assert.equal(end, 14);
  assert.deepEqual(
    starts,
    annotations.transcript.segments[1].words.map((w) => w.start),
  );
  // Its speaker, where the file names one; none where it doesn't.
  assert.equal(segments[1][4], "airodonack");
  assert.equal(segments[0].length, 4);
});

test("a line whose words don't match its text gets the nearest word's start for each", () => {
  /** @param {string} text @param {import("desktop/lib/annotations.ts").AnnotatedWord[]} words */
  const one = (text, words) =>
    segmentsOf({
      ...annotations,
      transcript: { corrected: 1, segments: [{ id: 0, start: 10, end: 13, text, words }] },
    })[0];
  const words = [
    { text: "a", start: 10, end: 11 },
    { text: "b", start: 11, end: 12 },
    { text: "c", start: 12, end: 13 },
  ];
  assert.deepEqual(one("x y", words)[2], [10, 12]);
  assert.deepEqual(one("v w x y z", words)[2], [10, 11, 11, 12, 12]);
  // Doubled spaces don't make empty words; no timed words, no starts (each the line's).
  assert.deepEqual(one("x  y", [])[0], "x y");
  assert.deepEqual(one("x  y", [])[2], []);
  // Lines come in time order.
  const out = segmentsOf({
    ...annotations,
    transcript: {
      corrected: 0,
      segments: [
        { id: 0, start: 5, end: 6, text: "later", words: [] },
        { id: 1, start: 1, end: 2, text: "first", words: [] },
      ],
    },
  });
  assert.deepEqual(
    out.map((s) => s[0]),
    ["first", "later"],
  );
});

test("the chat file as the chat commands give it", () => {
  const lines = chatOf(fixture("v2901646294.chat.json"));
  assert.equal(lines.length, 4);
  assert.deepEqual(lines[1], {
    id: 2,
    time: 8,
    author: "pelicanfan",
    color: "#9acd32",
    parts: [{ type: "text", text: "the pelican skip works" }],
    kind: "message",
    amount: null,
  });
  assert.equal(lines[2].kind, "sub");
  assert.throws(() => chatOf({}), /isn't a VODscribe chat file/);
});

test("the library's one VOD", () => {
  const vod = vodOf(annotations, 1001, "pending");
  assert.equal(vod.platform, "twitch");
  assert.equal(vod.platform_id, "v2901646294");
  assert.equal(vod.url, "https://www.twitch.tv/videos/2901646294");
  assert.equal(vod.duration, 75);
  assert.equal(vod.published, "2026-10-14");
  assert.equal(vod.transcribed, "all");
  assert.equal(vod.chat, "pending");
});

test("the host's index: VODs with a platform and id, newest first, each to its page", () => {
  const entry = (/** @type {string} */ id, /** @type {string} */ at) => ({
    platform: "twitch",
    id,
    channel: "airodonack",
    title: id,
    duration: 75,
    produced: { at },
  });
  const listed = indexEntries([
    entry("v1", "2026-10-01T00:00:00Z"),
    { title: "no id" },
    entry("v2", "2026-10-20T00:00:00Z"),
  ]);
  assert.deepEqual(
    listed.map((e) => e.id),
    ["v2", "v1"],
  );
  assert.throws(() => indexEntries({}), /couldn't be read/);
  // Twitch's page drops the "v" (pickSource puts it back); a clip's slug and other sites' ids stay.
  assert.equal(pageOf("twitch", "v2901646294"), "/v/twitch/2901646294");
  assert.equal(pageOf("twitch", "vSlugName"), "/v/twitch/vSlugName");
  assert.equal(pageOf("youtube", "IaIo3d6cUuo"), "/v/youtube/IaIo3d6cUuo");
  assert.equal(
    pickSource("twitch", "2901646294", new URLSearchParams(), PAGE)?.annotations.endsWith("/v2901646294.json"),
    true,
  );
});

test("a highlight's cast is read when it's there, left out when it isn't, refused when it isn't names", () => {
  assert.deepEqual(annotations.highlights[0].cast, ["airodonack"]);
  const without = structuredClone(fixture("v2901646294.json"));
  delete without.highlights[0].cast;
  assert.equal(parseAnnotations(without).highlights[0].cast, undefined);
  without.highlights[0].cast = "airodonack";
  assert.throws(() => parseAnnotations(without), /a highlight's cast isn't a list of names/);
});

test("a chapter's icon is read as the file has it; one that isn't one emoji is left out", () => {
  assert.deepEqual(
    annotations.chapters.map((c) => c.icon),
    [undefined, "🚪"],
  );
  const odd = structuredClone(fixture("v2901646294.json"));
  odd.chapters[1].icon = "door";
  assert.equal(parseAnnotations(odd).chapters[1].icon, undefined);
  assert.equal(parseAnnotations(odd).chapters[1].title, "The door");
});

test("the streamer's account and name are read from the file", () => {
  assert.deepEqual(annotations.cast[0].account, { platform: "twitch", login: "airodonack" });
  assert.equal(annotations.cast[0].player, "Airodonack");
});
