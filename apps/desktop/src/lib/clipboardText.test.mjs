// What Ctrl+C writes and what a copy and a paste say (clipboardText.ts):
// node --test apps/desktop/src/lib/clipboardText.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

// clipboardText.ts imports "./platform" as the page's bundler finds it.
register(
  "data:text/javascript," +
    encodeURIComponent(`
export async function resolve(specifier, context, next) {
  if (specifier.startsWith("./") && !/\\.[a-z]+$/.test(specifier) && context.parentURL?.endsWith(".ts"))
    return next(specifier + ".ts", context);
  return next(specifier, context);
}`),
);
const { copiedLine, copiedSaid, copiedText, pastedSaid } = await import("./clipboardText.ts");

/** @type {any} A site's VOD, as much of one as the text needs. */
const youtube = {
  id: 1,
  title: "Stream",
  platform: "youtube",
  platform_id: "noa29qsTc4g",
  url: "https://www.youtube.com/watch?v=noa29qsTc4g",
  source_path: null,
};
/** @type {any} */
const file = { id: 2, title: "My file", platform: "local", platform_id: "x", url: "", source_path: "/home/me/x.mp4" };
/** @param {number} vodId @param {number} start @param {number} end @returns {any} */
const stretch = (vodId, start, end, more = {}) => ({
  kind: "stretch",
  title: "Hello there",
  first: { vodId, vodTitle: vodId === 1 ? "Stream" : "My file", start, end },
  pieces: 1,
  length: end - start,
  ...more,
});

test("a site VOD's copy is its title, its stretch and a link to where it starts", () => {
  // clipboardText.ts:24; plans/copy-paste.md "What goes on the system clipboard"; 0.7.1: "Copied clips go on the clipboard too."
  assert.equal(
    copiedLine(stretch(1, 3723, 3765), youtube),
    "Hello there (1:02:03–1:02:45) https://www.youtube.com/watch?v=noa29qsTc4g&t=3723s",
  );
});

test("a file here's copy is named, never its path", () => {
  // clipboardText.ts:5: "a file here is named, never its path (pasted into a chat, a path tells of the user's folders)".
  const line = copiedLine(stretch(2, 1, 2), file);
  assert.equal(line, "Hello there (0:01–0:02) — “My file”");
  assert.ok(!line.includes("/home"));
  // Gone from the library: the name it had.
  assert.equal(copiedLine(stretch(2, 1, 2)), "Hello there (0:01–0:02) — “My file”");
});

test("several pieces copy as the first one's line and how many more", () => {
  // clipboardText.ts:25: " + 2 more pieces" if it has more.
  assert.equal(copiedLine(stretch(2, 1, 2, { pieces: 2 }), file), "Hello there (0:01–0:02) — “My file” + 1 more piece");
  assert.equal(
    copiedLine(stretch(2, 1, 2, { pieces: 4 }), file),
    "Hello there (0:01–0:02) — “My file” + 3 more pieces",
  );
});

test("each thing copied is a line, in order", () => {
  // clipboardText.ts:36: "The text for all of `copied`, a line each."
  /** @type {Map<number, any>} */
  const vods = new Map([
    [1, youtube],
    [2, file],
  ]);
  const text = copiedText(
    [stretch(2, 1, 2), { ...stretch(1, 5, 9), kind: "clip", title: "Pill" }],
    (/** @type {number} */ id) => vods.get(id),
  );
  assert.deepEqual(text.split("\n"), [
    "Hello there (0:01–0:02) — “My file”",
    "Pill (0:05–0:09) https://www.youtube.com/watch?v=noa29qsTc4g&t=5s",
  ]);
});

test("what a copy and a paste say", () => {
  // clipboardText.ts:51 (copiedSaid) and :59 (pastedSaid): what the toasts say.
  assert.equal(copiedSaid([stretch(2, 1, 13)]), "Copied 0:12 of “My file”");
  assert.equal(copiedSaid([{ ...stretch(2, 1, 2), kind: "clip" }]), "Copied “Hello there”");
  const clip = { ...stretch(2, 1, 2), kind: "clip" };
  assert.equal(copiedSaid([clip, clip]), "Copied 2 clips");
  assert.equal(copiedSaid([clip, stretch(2, 1, 2), stretch(2, 3, 4)]), "Copied 1 clip and 2 stretches");
  assert.equal(copiedSaid([clip, stretch(2, 1, 2)]), "Copied 1 clip and 1 stretch");
  assert.equal(copiedSaid([stretch(2, 1, 13, { first: null })]), "Copied 0:12");
  assert.equal(pastedSaid({ clips: 2, span: null, dropped: 0, refused: 0 }), "Pasted 2 clips");
  assert.equal(
    pastedSaid({ clips: 1, span: null, dropped: 1, refused: 0 }),
    "Pasted 1 clip, less 1 piece whose VOD is gone",
  );
  assert.equal(pastedSaid({ clips: 0, span: [1, 13], dropped: 0, refused: 0 }), "Pasted 0:12");
  assert.equal(
    pastedSaid({ clips: 1, span: null, dropped: 2, refused: 1 }),
    "Pasted 1 clip, less 2 pieces whose VOD is gone; 1 of another VOD left out (those go in an edit)",
  );
});
