// Settings › Encoding's formats when the ffmpeg in use can't make some (codecs.ts):
// node --test apps/desktop/src/lib/codecs.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";

const { codecChoices } = await import("./codecs.ts");

const CODECS = [
  { id: "h264", label: "MP4 H.264", hint: "Small files." },
  { id: "h265", label: "MP4 H.265", hint: "Smaller files." },
  { id: "dnxhr", label: "MOV DNxHR", hint: "Large files." },
];

test("a format the ffmpeg can't make is greyed out, says why and can't be picked; the others are as they were", () => {
  const { shown, hint } = codecChoices(CODECS, "h264", ["h265"]);
  assert.deepEqual(
    shown.map((c) => [c.id, c.on, c.greyed, c.why, c.pickable]),
    [
      ["h264", true, false, undefined, true],
      ["h265", false, true, "VODscribe's ffmpeg can't make MP4 H.265.", false],
      ["dnxhr", false, false, undefined, true],
    ],
  );
  assert.equal(hint, "Small files.");
});

test("one already picked stays picked, its hint says clips fail, and it doesn't say so again on hover", () => {
  const { shown, hint } = codecChoices(CODECS, "h265", ["h265"]);
  assert.deepEqual(
    shown.filter((c) => c.on).map((c) => [c.id, c.greyed, c.why]),
    [["h265", true, undefined]],
  );
  assert.equal(hint, "VODscribe's ffmpeg can't make MP4 H.265. Clips and edits fail until you pick another format.");
});

test("with every format there, or the ffmpeg unable to say, nothing is greyed out", () => {
  const { shown, hint } = codecChoices(CODECS, "dnxhr", []);
  assert.ok(shown.every((c) => !c.greyed && c.why === undefined && c.pickable));
  assert.equal(hint, "Large files.");
});

test("until the app has answered, nothing is greyed out and nothing can be picked", () => {
  const { shown, hint } = codecChoices(CODECS, "h264", null);
  assert.ok(shown.every((c) => !c.greyed && !c.pickable));
  assert.equal(hint, "Small files.");
});
