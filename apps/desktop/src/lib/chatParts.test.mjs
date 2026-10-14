// A chat message's pieces (chatParts.ts): node --test apps/desktop/src/lib/chatParts.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { pieces } from "./chatParts.ts";

/** @type {Map<string, import("./api").WordEmote>} */
const words = new Map([
  ["KEKW", { name: "KEKW", url: "https://cdn.7tv.app/emote/k/1x.webp", overlay: false }],
  ["PETPET", { name: "PETPET", url: "https://cdn.7tv.app/emote/p/1x.webp", overlay: true }],
]);
/** @param {string} text @returns {import("./api").ChatPart} */
const t = (text) => ({ type: "text", text });
/** @param {import("./chatParts").Piece[]} list */
const show = (list) =>
  list
    .map((p) => (p.type === "text" ? p.text : `<${p.name}${p.overlays.map((o) => "+" + o.name).join("")}>`))
    .join("|");

test("words that are emotes become emotes, exactly as written", () => {
  assert.equal(show(pieces([t("lol KEKW that was kekw")], words)), "lol |<KEKW>| that was kekw");
});

test("no emotes known: the text as it came", () => {
  assert.equal(show(pieces([t("lol KEKW")], new Map())), "lol KEKW");
});

test("an overlay goes over the emote before it, spaces and all", () => {
  assert.equal(show(pieces([t("KEKW PETPET wow")], words)), "<KEKW+PETPET>| wow");
});

test("an overlay goes over the platform's own emote too", () => {
  const parts = [{ type: "emote", name: "Kappa", url: "https://static-cdn.jtvnw.net/x" }, t(" PETPET")];
  assert.equal(show(pieces(/** @type {any} */ (parts), words)), "<Kappa+PETPET>");
});

test("an overlay with nothing to go over stands alone", () => {
  assert.equal(show(pieces([t("hi PETPET")], words)), "hi |<PETPET>");
  assert.equal(show(pieces([t("KEKW and PETPET")], words)), "<KEKW>| and |<PETPET>");
});

test("whitespace is kept", () => {
  assert.equal(show(pieces([t("  KEKW\tok ")], words)), "  |<KEKW>|\tok ");
});
