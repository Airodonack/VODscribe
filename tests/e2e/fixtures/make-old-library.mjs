// Makes fixtures/library-<version>.sqlite: a library as an older release left
// it, for the upgrade scenario (scenarios/upgrade.mjs). It drives that
// release (any build, through WebDriver: see run.mjs) to add and transcribe
// eagle-has-landed.mp3 and choose the dark theme; from 0.5.0, also to add
// ten minutes of it and pause that part-way through transcribing. It keeps
// the library with the fixtures folder made a placeholder ({FIXTURES}, and
// {SPEECH} for the ten minutes' folder), and refuses if anything else of
// this computer's (the home folder, the user) is left in it. Later
// versions' features (clips came in 0.4.0) belong in a newer one.
//
//   node tests/e2e/fixtures/make-old-library.mjs <the old app> <its version>
//
// Run it with the app's data in a folder of its own (XDG_DATA_HOME on Linux).

import { copyFileSync, existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";
import { fileURLToPath } from "node:url";

import { App, MINUTE, until } from "../lib/app.mjs";
import { dataDirs, resetData } from "../lib/data.mjs";
import { webdriver } from "../lib/drivers.mjs";
import { LANDED, longSpeech } from "../lib/fixtures.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const [app, version] = process.argv.slice(2);
if (!app || !/^\d+\.\d+\.\d+$/.test(version ?? "")) {
  console.error("usage: node tests/e2e/fixtures/make-old-library.mjs <the old app> <its version>");
  process.exit(2);
}
const log = (what) => console.log(what);
// Before 0.5.0 (transcription in stretches came after it), only the landing.
const [major, minor] = version.split(".").map(Number);
const paused = major > 0 || minor >= 5;
const speechDir = path.join(here, "..", "out", "fixtures");

resetData();
const driver = await webdriver(app);
const page = new App(driver, log);
try {
  await page.ready();
  await page.toLibrary();
  // Releases before 0.5.2 transcribe what's added; later ones on Transcribe.
  const status = async (id) => (await page.invoke("list_vods")).find((v) => v.id === id)?.status;
  const [vod] = await page.addFiles([LANDED]);
  if ((await status(vod.id)) === "none")
    await page.click(`.vod[data-key="${vod.id}"] .actions button[aria-label="Transcribe"]`);
  await until("the landing transcribed", async () => (await status(vod.id)) === "done", 15 * MINUTE);
  log(`transcribed VOD ${vod.id}`);
  if (paused) {
    const [long] = await page.addFiles([longSpeech(speechDir)]);
    if ((await status(long.id)) === "none")
      await page.click(`.vod[data-key="${long.id}"] .actions button[aria-label="Transcribe"]`);
    // Paused as soon as it has lines: a few, to keep the file small.
    await until(
      "its first lines",
      async () => (await page.invoke("transcript", { vodId: long.id })).length > 0,
      15 * MINUTE,
      { every: 100 },
    );
    await page.invoke("pause_vod", { id: long.id });
    await until("it paused", async () => (await status(long.id)) === "paused", MINUTE);
    log(`paused VOD ${long.id} with ${(await page.invoke("transcript", { vodId: long.id })).length} lines`);
  }
  // A setting, to see it kept.
  await page.click('button[aria-label="Settings"]');
  await until("Settings", () => page.js(`return !!document.querySelector('[aria-label="Theme"]')`), MINUTE);
  await page.js(
    `[...document.querySelectorAll('[role="radiogroup"][aria-label="Theme"] button')].find((b) => b.textContent.trim() === "Dark").click()`,
  );
  await until("the setting saved", async () => (await page.invoke("get_settings")).theme === "dark", 10_000);
} finally {
  await driver.close();
}

// Its library, with its write-ahead log folded in, and the folders made placeholders.
const dir = dataDirs()[0];
const out = path.join(here, `library-${version}.sqlite`);
const db = new DatabaseSync(path.join(dir, "library.sqlite"));
db.exec("PRAGMA wal_checkpoint(TRUNCATE)");
db.close();
copyFileSync(path.join(dir, "library.sqlite"), out);
const copy = new DatabaseSync(out);
const places = [
  [path.dirname(LANDED), "{FIXTURES}"],
  [speechDir, "{SPEECH}"],
];
for (const { name } of copy.prepare("SELECT name FROM sqlite_schema WHERE type = 'table'").all()) {
  const columns = copy.prepare(`SELECT name, type FROM pragma_table_info('${name}')`).all();
  for (const { name: column } of columns.filter((c) => /TEXT/i.test(c.type))) {
    for (const [from, to] of places)
      copy
        .prepare(`UPDATE "${name}" SET "${column}" = replace("${column}", ?, ?) WHERE "${column}" LIKE ?`)
        .run(from, to, `%${from}%`);
  }
}
copy.exec("VACUUM");
copy.close();
// Nothing of this computer's may be left in it.
const text = readFileSync(out).toString("latin1");
for (const home of [process.env.HOME, process.env.USERPROFILE, process.env.USER].filter(Boolean))
  if (text.includes(home)) throw new Error(`${out} still has "${home}" in it`);
writeFileSync(path.join(here, `settings-${version}.json`), readFileSync(path.join(dir, "settings.json")));
log(`made ${out}, from ${readdirSync(dir).join(", ")}`);
process.exit(existsSync(out) ? 0 : 1);
