// A VOD's own buttons: Transcribe (an added VOD isn't), Pause while it
// transcribes (it stands), Resume (it finishes), then Transcribe again,
// after asking (it's transcribed anew).

import path from "node:path";

import { MINUTE, until } from "../lib/app.mjs";
import { longSpeech } from "../lib/fixtures.mjs";

export default {
  name: "vod-actions",
  async run(app, ctx) {
    await app.toLibrary();
    await app.addFiles([longSpeech(path.join(ctx.out, "fixtures"))]);
    const status = async () => (await app.invoke("list_vods"))[0]?.status;
    // The row's buttons, not the player bar's Play/Pause.
    const row = (label) => `.list .vod .actions button[aria-label="${label}"]`;
    const shown = () => app.js(`return document.querySelector(".list .vod .status-area .status")?.textContent.trim()`);
    await until("the row to say Not transcribed", async () => (await shown()) === "Not transcribed", 10_000);
    if ((await status()) !== "none") throw new Error(`added, it's ${await status()}, not untranscribed`);
    await app.click(row("Transcribe"));
    await until("it to transcribe", async () => (await status()) === "transcribing", 5 * MINUTE);

    await app.click(row("Pause"));
    await until("it paused", async () => (await status()) === "paused", MINUTE);
    await until("the row to say Paused", async () => (await shown()) === "Paused", 10_000);
    ctx.log("paused");
    await app.click(row("Resume"));
    await until("it to finish", async () => (await status()) === "done", 30 * MINUTE);
    ctx.log("resumed, and done");

    await app.answer("Transcribe again");
    await app.click(row("Transcribe again"));
    await until("it to start again", async () => (await status()) !== "done", MINUTE);
    await until("it done again", async () => (await status()) === "done", 30 * MINUTE);
    if ((await app.asked()) !== 1) throw new Error("Transcribe again didn't ask first");
    const [vod] = await app.invoke("list_vods");
    const text = (await app.invoke("transcript", { vodId: vod.id })).map((l) => l.text).join(" ");
    if (!/landed/i.test(text)) throw new Error(`the new transcript doesn't have "landed": ${text.slice(0, 200)}`);
    ctx.log("transcribed again");
  },
};
