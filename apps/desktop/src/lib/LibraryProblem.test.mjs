// The recovery screen (LibraryProblem.svelte), rendered as text with the app
// faked, on each system: what it says "Start a new library" does with the
// old library. On Windows its file and its videos are in two folders, the
// roaming one and the local one, and the page names both.
// node --test apps/desktop/src/lib/LibraryProblem.test.mjs
import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

// The page's imports, faked; "./platform" says the system named in the
// page's own URL (`?windows`), so each system gets a page of its own.
const fakes = {
  "@tauri-apps/api/window": `export const getCurrentWindow = () => ({ show: async () => {} });`,
  "@tauri-apps/plugin-opener": `export const openUrl = () => {};`,
  "@tauri-apps/plugin-process": `export const relaunch = () => {};`,
  "./api": `export const api = {};`,
  "./Icon.svelte": `export default function Icon() {}`,
  "./Logo.svelte": `export default function Logo() {}`,
  "./links": `export const SUPPORT = "";`,
  "./theme.svelte": `export const theme = { apply() {} };`,
};
const hooks = `
import { readFile } from "node:fs/promises";
import { compile } from ${JSON.stringify(import.meta.resolve("svelte/compiler"))};
const fakes = ${JSON.stringify(fakes)};
const page = (url) => url?.split("?")[0].endsWith("/LibraryProblem.svelte");
const module = (code) => ({ url: "data:text/javascript," + encodeURIComponent(code), shortCircuit: true });
export async function resolve(specifier, context, next) {
  if (page(context.parentURL) && specifier === "./platform")
    return module("export const os = " + JSON.stringify(new URL(context.parentURL).search.slice(1)) + ";");
  if (page(context.parentURL) && fakes[specifier]) return module(fakes[specifier]);
  return next(specifier, context);
}
export async function load(url, context, next) {
  if (!page(url)) return next(url, context);
  const { js } = compile(await readFile(new URL(url), "utf8"), { filename: url, generate: "server" });
  return { format: "module", source: js.code, shortCircuit: true };
}`;
register("data:text/javascript," + encodeURIComponent(hooks), import.meta.url);

const { render } = await import("svelte/server");

/** What the page says on `os`, as text, its spaces made single. @param {string} os */
async function said(os) {
  const { default: page } = await import(`./LibraryProblem.svelte?${os}`);
  const { body } = render(page, { props: { error: "file is not a database" } });
  return body
    .replace(/<[^>]*>/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

test("on Windows the page names both folders the old library moves to", async () => {
  const text = await said("windows");
  assert.match(
    text,
    /The old library moves to folders named library-broken- with today's date, so it can be recovered or sent with a bug report: its file to the one in AppData\\Roaming, its videos and clips to the one in AppData\\Local\./,
  );
  assert.doesNotMatch(text, /with it/);
});

for (const os of ["mac", "linux"]) {
  test(`on ${os} the page says the videos and clips move with the library`, async () => {
    const text = await said(os);
    assert.match(
      text,
      /The old library moves to a folder beside it, named library-broken- with today's date, its videos and clips with it, so it can be recovered or sent with a bug report\./,
    );
    assert.doesNotMatch(text, /AppData/);
  });
}
