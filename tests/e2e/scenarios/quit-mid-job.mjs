// Quitting while a file transcribes: nothing the app started goes on after
// it, and at the next start the transcription carries on to the end.

import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "../lib/app.mjs";
import { longSpeech } from "../lib/fixtures.mjs";
import { leftovers } from "../lib/procs.mjs";

export default {
  name: "quit-mid-job",
  async run(app, ctx) {
    const file = longSpeech(path.join(ctx.out, "fixtures"));
    await app.toLibrary();
    const [added] = await app.addFiles([file]);
    await app.click(`.vod[data-key="${added.id}"] .actions button[aria-label="Transcribe"]`);
    let seen = "";
    const vod = await until(
      "the transcription to be under way",
      async () => {
        const [v] = await app.invoke("list_vods");
        const now = v ? v.status : "not in the library yet";
        if (now !== seen) ctx.log((seen = now));
        // Finished or failed: no use waiting on.
        if (v?.status === "done" || v?.status === "error") return v;
        return v?.status === "transcribing" && v;
      },
      5 * MINUTE,
    );
    if (vod.status === "error") throw new Error(`it failed: ${vod.error}`);
    if (vod.status === "done") throw new Error("it finished before the test could quit: make the file longer");
    // A few seconds in, so the worker is at work (its progress comes as events, not in list_vods).
    await sleep(3000);
    const [still] = await app.invoke("list_vods");
    if (still.status !== "transcribing") throw new Error(`3 s in, it's ${still.status}: make the file longer`);
    ctx.log("quitting while it transcribes");
    if (!leftovers().length)
      throw new Error("no process of the app's found while it transcribes: the check can't work");

    await ctx.quit();
    await until("what the app started to end", () => leftovers().length === 0 || null, 10_000).catch(() => {
      throw new Error(`still running after quitting:\n  ${leftovers().join("\n  ")}`);
    });
    app = await ctx.start();
    await app.toLibrary();
    const done = await until(
      "the transcription to finish",
      async () => {
        const [v] = await app.invoke("list_vods");
        if (v?.status === "error") throw new Error(`after the restart it failed: ${v.error}`);
        return v?.status === "done" && v;
      },
      30 * MINUTE,
    );
    const text = (await app.invoke("transcript", { vodId: done.id })).map((l) => l.text).join(" ");
    if (!/landed/i.test(text)) throw new Error(`the transcript doesn't have "landed": ${text.slice(0, 200)}`);
  },
};
