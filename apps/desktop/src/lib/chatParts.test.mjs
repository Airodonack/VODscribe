// A chat message's pieces (chatParts.ts): node --test apps/desktop/src/lib/chatParts.test.mjs
// The specs of `pieces`, each citing where it's said.
import assert from "node:assert/strict";
import { test } from "node:test";
import { pieces } from "./chatParts.ts";

/** @type {Map<string, import("./api").WordEmote>} */
const words = new Map([
  ["KEKW", { name: "KEKW", url: "https://cdn.7tv.app/emote/k/1x.webp", overlay: false }],
  ["PETPET", { name: "PETPET", url: "https://cdn.7tv.app/emote/p/1x.webp", overlay: true }],
  ["HAT", { name: "HAT", url: "https://cdn.frankerfacez.com/emote/h/1", overlay: true }],
]);
/** @param {string} text @returns {import("./api").ChatPart} */
const t = (text) => ({ type: "text", text });
/** @param {string} name @returns {import("./api").ChatPart} */
const own = (name) => /** @type {any} */ ({ type: "emote", name, url: `https://static-cdn.jtvnw.net/${name}` });
/** @param {import("./chatParts").Piece[]} list */
const show = (list) =>
  list
    .map((p) => (p.type === "text" ? p.text : `<${p.name}${p.overlays.map((o) => "+" + o.name).join("")}>`))
    .join("|");

// chatParts.ts `pieces`: "`parts` as pieces to show, with `words` (by exact
// name) made emotes"; word_emotes.rs `WordEmote::name`: "matched exactly
// (emote names are case-sensitive)".
test("words that are emotes become emotes, exactly as written", () => {
  assert.equal(show(pieces([t("lol KEKW that was kekw")], words)), "lol |<KEKW>| that was kekw");
  assert.equal(show(pieces([t("KEKW, KEKWW")], words)), "KEKW, KEKWW", "a word, not part of one");
});

// chatParts.ts's module comment: "its text, with the platform's own emotes … as images".
test("the platform's own emotes are shown as their images, in their place", () => {
  const shown = pieces([t("hi "), own("Kappa"), t(" there")], words);
  assert.equal(show(shown), "hi |<Kappa>| there");
  assert.deepEqual(shown[1], { type: "emote", name: "Kappa", url: "https://static-cdn.jtvnw.net/Kappa", overlays: [] });
});

// chatParts.ts `pieces`: "`parts` as pieces to show".
test("no emotes known: the text as it came", () => {
  assert.equal(show(pieces([t("lol KEKW")], new Map())), "lol KEKW");
});

// chatParts.ts's module comment: "An overlay emote (a hat, a spin) is drawn over the emote
// before it, when there is one, rather than beside it"; 0.2.1: "with
// \"zero-width\" emotes (a hat, a spin) drawn over the emote before them".
test("an overlay goes over the emote before it, spaces and all", () => {
  assert.equal(show(pieces([t("KEKW PETPET wow")], words)), "<KEKW+PETPET>| wow");
  assert.equal(show(pieces([t("KEKW then KEKW PETPET")], words)), "<KEKW>| then |<KEKW+PETPET>", "the one just before");
});

// chatParts.ts's module comment, as above, the emote before being the platform's.
test("an overlay goes over the platform's own emote too", () => {
  assert.equal(show(pieces([own("Kappa"), t(" PETPET")], words)), "<Kappa+PETPET>");
  assert.equal(show(pieces([own("Kappa"), t("PETPET")], words)), "<Kappa+PETPET>", "with no space between");
});

// chatParts.ts's module comment, as above: the emote before a second overlay is the one the
// first went over.
test("overlays one after another all go over the same emote", () => {
  assert.equal(show(pieces([t("KEKW PETPET HAT ok")], words)), "<KEKW+PETPET+HAT>| ok");
});

// chatParts.ts's module comment: "when there is one, rather than beside it".
test("an overlay with nothing to go over stands alone", () => {
  assert.equal(show(pieces([t("hi PETPET")], words)), "hi |<PETPET>");
  assert.equal(show(pieces([t("KEKW and PETPET")], words)), "<KEKW>| and |<PETPET>");
  assert.equal(show(pieces([t("PETPET")], words)), "<PETPET>");
});

// chatParts.ts `pieces`: "`parts` as pieces to show": the text as written.
test("whitespace is kept", () => {
  assert.equal(show(pieces([t("  KEKW\tok ")], words)), "  |<KEKW>|\tok ");
});
