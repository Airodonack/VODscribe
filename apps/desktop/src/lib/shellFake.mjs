// The page's stores (`.svelte.ts`) loaded under node, with the shell behind
// them faked: Tauri's `invoke` and `listen` are `globalThis.shell`'s, so the
// real api.ts, platform.ts and the rest run as they do in the window. Compiled
// as the app's build does (types stripped, then Svelte's runes).
import { register } from "node:module";

let registered = false;

/** Call once, before importing a store. Then set `globalThis.shell = { invoke, listen }`. */
export function loadStoresWithShell() {
  if (registered) return;
  registered = true;
  const fakes = {
    "@tauri-apps/api/core": `export const invoke = (...a) => globalThis.shell.invoke(...a);`,
    "@tauri-apps/api/event": `export const listen = (...a) => globalThis.shell.listen(...a);`,
  };
  const hooks = `
import { stripTypeScriptTypes } from "node:module";
import { readFile } from "node:fs/promises";
import { compileModule } from ${JSON.stringify(import.meta.resolve("svelte/compiler"))};
const fakes = ${JSON.stringify(fakes)};
export async function resolve(specifier, context, next) {
  if (fakes[specifier])
    return { url: "data:text/javascript," + encodeURIComponent(fakes[specifier]), shortCircuit: true };
  if (specifier.startsWith("./") && !/\\.[a-z]+$/.test(specifier) && context.parentURL?.startsWith("file:"))
    return next(specifier + ".ts", context);
  return next(specifier, context);
}
export async function load(url, context, next) {
  if (!/\\.svelte\\.ts(\\?|$)/.test(url)) return next(url, context);
  const source = stripTypeScriptTypes(await readFile(new URL(url.split("?")[0]), "utf8"), { mode: "transform" });
  const { js } = compileModule(source, { filename: url, generate: "client" });
  return { format: "module", source: js.code, shortCircuit: true };
}`;
  register("data:text/javascript," + encodeURIComponent(hooks), import.meta.url);
}
