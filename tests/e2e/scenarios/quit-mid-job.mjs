// Quitting while a file transcribes: nothing the app started goes on after
// it, nor the app itself, and at the next start the transcription carries
// on to the end.
//
// Under WebDriver (Linux) the driver ends the app's process from outside
// when the session closes, so its own quit never runs: there, this checks
// that the worker stops with the app however it ends (it watches its stdin,
// a pipe from the app). Through the test hook (Windows, macOS) it's the
// app's quit.

import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "../lib/app.mjs";
import { appLine } from "../lib/dump.mjs";
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
    // The app's own process too (its binary): a quit that hangs is told
    // apart from a worker left behind, and when it ended is in the log.
    const proc = app.driver.proc;
    if (!leftovers(proc.exe).length)
      throw new Error("no process of the app's found while it transcribes: the check can't work");

    await ctx.quit();
    await until("what the app started to end", () => leftovers(proc.exe).length === 0 || null, 10_000).catch(() => {
      throw new Error(`still running after quitting (${appLine(proc)}):\n  ${leftovers(proc.exe).join("\n  ")}`);
    });
    // Its end is noted a tenth of a second at most after it (lib/procs.mjs watchExit).
    await until("the app's end noted", () => proc.exitedAt || !proc.pid, 1000).catch(() => {});
    ctx.log(appLine(proc));
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
