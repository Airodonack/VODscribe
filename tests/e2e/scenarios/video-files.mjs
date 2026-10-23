// Video files of several kinds, added together: each is transcribed (or,
// with nothing said or no sound, done with an empty transcript) and plays.

import path from "node:path";

import { MINUTE, until } from "../lib/app.mjs";
import { videos } from "../lib/fixtures.mjs";

export default {
  name: "video-files",
  async run(app, { out, log, checkpoint }) {
    const files = videos(path.join(out, "fixtures"));
    await app.toLibrary();
    for (const vod of await app.addFiles(Object.values(files)))
      await app.invoke("transcribe_stretch", { vodId: vod.id, start: 0, end: null });
    const byFile = await until(
      "every file in the library, finished",
      async () => {
        const vods = await app.invoke("list_vods");
        if (vods.length < Object.keys(files).length || vods.some((v) => !["done", "error"].includes(v.status)))
          throw new Error(vods.map((v) => `${v.title}: ${v.status}`).join(", ") || "no VODs");
        return Object.fromEntries(
          Object.entries(files).map(([kind, file]) => [
            kind,
            vods.find((v) => v.title === path.basename(file, path.extname(file))),
          ]),
        );
      },
      15 * MINUTE,
    );
    for (const [kind, vod] of Object.entries(byFile)) {
      if (!vod) throw new Error(`no VOD titled after ${path.basename(files[kind])}`);
      log(`${kind}: ${vod.status}${vod.error ? ` (${vod.error})` : ""}`);
    }
    for (const [kind, vod] of Object.entries(byFile)) {
      if (vod.status !== "done") throw new Error(`${kind}: ${vod.status}: ${vod.error}`);
      const text = (await app.invoke("transcript", { vodId: vod.id })).map((l) => l.text).join(" ");
      const speaks = !["silent", "mute"].includes(kind);
      if (speaks !== /landed/i.test(text)) throw new Error(`${kind}: the transcript is "${text}"`);
      await app.openVod(vod.id);
      const at = await app.plays();
      log(`${kind}: transcribed${speaks ? "" : " (nothing said)"}, played to ${at} s`);
      if (kind === "video" || kind === "wide") await checkpoint(`playing-${kind}`);
    }
  },
};
