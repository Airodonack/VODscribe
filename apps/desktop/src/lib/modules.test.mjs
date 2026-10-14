// Module names that differ only in case: node --test apps/desktop/src/lib/modules.test.mjs
// Windows and macOS don't tell file names apart by case, so there an import of
// "./settings.svelte" found Settings.svelte, not settings.svelte.ts, and the
// 0.2.1 build failed on both while Linux (and CI's checks) built fine.
import assert from "node:assert/strict";
import { readdirSync } from "node:fs";
import { test } from "node:test";

const dirs = ["./", "../../../site/src/lib/", "../../../site/src/lib/tauri/"];

for (const dir of dirs) {
  test(`no two modules in ${dir} have names that differ only in case`, () => {
    const seen = new Map();
    for (const file of readdirSync(new URL(dir, import.meta.url))) {
      // How it's imported: "./x.svelte" names both X.svelte and x.svelte.ts.
      const name = file.replace(/\.(ts|js|mjs)$/, "").toLowerCase();
      assert.ok(!seen.has(name), `${seen.get(name)} and ${file}`);
      seen.set(name, file);
    }
  });
}
