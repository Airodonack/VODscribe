// Upgrading from 0.5.0, the last release to transcribe a VOD whole: its
// library (fixtures/, made by make-old-library.mjs) has the landing
// transcribed and ten minutes of it paused part-way. In this version the
// first is one done stretch, all of it, and the second one paused request
// for all of it that has reached its last line's end, its lines kept; a
// copy of the old library is kept beside it.

import { existsSync } from "node:fs";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

import { checkFormat } from "../fixtures/upgrade-matrix.mjs";
import { dataDirs, oldLibrary } from "../lib/data.mjs";
import { LANDED, longSpeech } from "../lib/fixtures.mjs";

const VERSION = "0.5.0";
const fixtures = path.dirname(LANDED);

export default {
  name: "upgrade-0.5.0",
  before(ctx) {
    // The paused one's file, made where it's made for the other scenarios.
    const speech = path.dirname(longSpeech(path.join(ctx.out, "fixtures")));
    oldLibrary(fixtures, VERSION, { "{FIXTURES}": fixtures, "{SPEECH}": speech });
  },
  async run(app, ctx) {
    await app.toLibrary();
    const vods = await app.invoke("list_vods");
    const by = (title) => vods.find((v) => v.title.startsWith(title));
    const done = by("eagle-has-landed");
    const paused = by("landed-10-minutes");
    if (vods.length !== 2 || !done || !paused)
      throw new Error(`after the upgrade the library has ${JSON.stringify(vods.map((v) => v.title))}`);
    const stretches = async (vod) =>
      (await app.invoke("transcript_stretches", { vodId: vod.id })).map((s) => [s.start, s.end, s.status, s.reached]);

    const whole = await stretches(done);
    if (JSON.stringify(whole) !== JSON.stringify([[0, null, "done", null]]) || done.status !== "done")
      throw new Error(`the transcribed one is ${done.status} (${done.transcribed}), in ${JSON.stringify(whole)}`);

    const lines = await app.invoke("transcript", { vodId: paused.id });
    if (!lines.length) throw new Error("the paused one lost its lines");
    const last = Math.max(...lines.map((l) => l.end));
    const asked = await stretches(paused);
    if (JSON.stringify(asked) !== JSON.stringify([[0, null, "paused", last]]))
      throw new Error(`the paused one is in ${JSON.stringify(asked)}, not paused at its last line's end (${last})`);
    if (paused.status !== "paused" || paused.transcribed !== "some")
      throw new Error(`the paused one is ${paused.status}, transcribed ${paused.transcribed}`);
    await ctx.checkpoint("upgraded-library");
    checkFormat(VERSION, 26);
    const settings = await app.invoke("get_settings");
    if (settings.theme !== "dark" || settings.clip_format !== "mp4" || Object.keys(settings.key_bindings).length)
      throw new Error(
        `the settings are ${JSON.stringify(settings)}, not 0.5.0's dark theme and MP4 clips, with no key bindings`,
      );

    const copy = path.join(dataDirs()[0], "library-v26.sqlite");
    if (!existsSync(copy)) throw new Error(`no copy of the ${VERSION} library at ${copy}`);
    const kept = new DatabaseSync(copy, { readOnly: true });
    const old = kept.prepare("SELECT count(*) AS n FROM vods").get().n;
    kept.close();
    if (old !== 2) throw new Error(`the kept copy has ${old} VODs`);
    app.log(`kept: one done stretch, and one paused at ${last} s with ${lines.length} lines`);
  },
};
