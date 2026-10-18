// Removing a VOD asks first: "Keep" keeps it, "Remove" removes it.

import { MINUTE, until } from "../lib/app.mjs";
import { setTimeout as sleep } from "node:timers/promises";

export default {
  name: "remove",
  async run(app, { fixtures }) {
    await app.toLibrary();
    await app.addAndTranscribe(fixtures.landed);
    await app.answer("Keep");
    await app.click('button[aria-label="Remove"]');
    await sleep(2000);
    const asked = await app.asked();
    if (asked !== 1) throw new Error(`Remove asked ${asked} times, not once`);
    if ((await app.invoke("list_vods")).length !== 1) throw new Error(`answering "Keep" removed the VOD`);
    await app.answer("Remove");
    await app.click('button[aria-label="Remove"]');
    await until("the VOD to be removed", async () => (await app.invoke("list_vods")).length === 0, MINUTE);
  },
};
