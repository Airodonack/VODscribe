// Upgrading: a library and settings as 0.3.0 left them (fixtures/, made by
// make-old-library.mjs) open in this version with the VOD, its transcript
// (one stretch, all of it) and the setting kept, a copy of the old library
// kept beside it, and this version's features working on the old VOD (a
// clip: 0.3.0 had none).

import { existsSync } from "node:fs";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

import { MINUTE, until } from "../lib/app.mjs";
import { dataDirs, oldLibrary } from "../lib/data.mjs";
import { LANDED } from "../lib/fixtures.mjs";

const VERSION = "0.3.0";
const fixtures = path.dirname(LANDED);

export default {
  name: "upgrade",
  before() {
    // The fixture names its file by a placeholder for the fixtures folder.
    oldLibrary(fixtures, VERSION, { "{FIXTURES}": fixtures });
  },
  async run(app, ctx) {
    await app.toLibrary();
    const vods = await app.invoke("list_vods");
    if (vods.length !== 1 || vods[0].status !== "done")
      throw new Error(`after the upgrade the library has ${JSON.stringify(vods.map((v) => [v.title, v.status]))}`);
    const [vod] = vods;
    // Transcribed whole, before stretches: one done from its start to its end.
    const stretches = await app.invoke("transcript_stretches", { vodId: vod.id });
    const whole = stretches.map((s) => [s.start, s.end, s.status]);
    if (JSON.stringify(whole) !== JSON.stringify([[0, null, "done"]]) || vod.transcribed !== "all")
      throw new Error(`after the upgrade it's transcribed ${vod.transcribed}, in ${JSON.stringify(stretches)}`);
    await ctx.checkpoint("upgraded-library");
    const text = (await app.invoke("transcript", { vodId: vod.id })).map((l) => l.text).join(" ");
    if (!/landed/i.test(text)) throw new Error(`the transcript doesn't have "landed": ${text}`);
    const theme = (await app.invoke("get_settings")).theme;
    if (theme !== "dark") throw new Error(`the theme is ${theme}, not the dark one chosen in ${VERSION}`);

    const copy = path.join(dataDirs()[0], "library-v16.sqlite");
    if (!existsSync(copy)) throw new Error(`no copy of the ${VERSION} library at ${copy}`);
    const kept = new DatabaseSync(copy, { readOnly: true });
    const old = kept.prepare("SELECT count(*) AS n FROM vods").get().n;
    kept.close();
    if (old !== 1) throw new Error(`the kept copy has ${old} VODs`);

    await app.openPaused(vod.id);
    const clip = await app.clipOneToThree(vod.id);
    app.log(`kept: the VOD, its transcript, the theme and a copy; clipped "${clip.title}"`);
    await until(
      "the clip in the list",
      async () => (await app.invoke("list_clips", { vodId: vod.id })).length === 1,
      MINUTE,
    );
  },
};
