// One selection at a time (selection.svelte.ts), compiled as the app's build
// does (types stripped, then Svelte's runes), with a stretch and a mark
// handed over as cuts.svelte.ts hands its own.
// node --test apps/desktop/src/lib/selection.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

const hooks = `
import { stripTypeScriptTypes } from "node:module";
import { readFile } from "node:fs/promises";
import { compileModule } from ${JSON.stringify(import.meta.resolve("svelte/compiler"))};
export async function resolve(specifier, context, next) {
  if (specifier.startsWith("./") && !/\\.[a-z]+$/.test(specifier) && context.parentURL?.startsWith("file:"))
    return next(specifier + ".ts", context);
  return next(specifier, context);
}
export async function load(url, context, next) {
  if (!url.endsWith(".svelte.ts")) return next(url, context);
  const source = stripTypeScriptTypes(await readFile(new URL(url), "utf8"), { mode: "transform" });
  const { js } = compileModule(source, { filename: url, generate: "client" });
  return { format: "module", source: js.code, shortCircuit: true };
}`;
register("data:text/javascript," + encodeURIComponent(hooks), import.meta.url);

const selection = await import("./selection.svelte.ts");
const { snippets, library } = selection;

// The stretch selected (I and O) and the mark selected, as cuts.svelte.ts keeps them.
/** @type {{ stretch: { start: number, end: number } | null, mark: number | null }} */
const cuts = { stretch: null, mark: null };
selection.clearsStretch(() => {
  cuts.stretch = null;
  cuts.mark = null;
});
selection.clearsMark?.(() => (cuts.mark = null));

test("a plain click on background lets go of what's picked and the mark selected", () => {
  // An Alt-click picked a stretch between cuts, a clip of the bar beside it.
  snippets.set([{ kind: "segment", start: 1, end: 2 }]);
  snippets.click({ kind: "clip", id: 7 }, "toggle", []);
  assert.equal(snippets.size, 2);
  // A plain click on the timeline (or empty space in the clip bar).
  selection.background();
  assert.equal(snippets.size, 0);
  // The library's rows picked go the same way.
  library.set([
    { kind: "vod", id: 1 },
    { kind: "vod", id: 2 },
  ]);
  selection.background();
  assert.equal(library.size, 0);
  // A mark selected goes too.
  cuts.mark = 4;
  selection.background();
  assert.equal(cuts.mark, null);
});

test("a plain click on background keeps the stretch selected (I and O)", () => {
  cuts.stretch = { start: 1, end: 3 };
  selection.background();
  assert.deepEqual(cuts.stretch, { start: 1, end: 3 });
});
