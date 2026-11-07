// A link VODscribe can't use: an error under the box, and nothing added.

import { MINUTE, until } from "../lib/app.mjs";

export default {
  name: "add-link-error",
  // What a person should see at each checkpoint (verify.mjs).
  expects: {
    "link-error":
      "The library, still empty: the link box holds https://example.invalid/not-a-vod, and just under it a short message in red says VODscribe can't use that link and asks for a link to a video page; below it No VODs yet, and no new row.",
  },
  async run(app, ctx) {
    await app.toLibrary();
    // Well-formed (the box is type="url"), but no site's VOD, nor anywhere at all.
    await app.type('input[aria-label="VOD link"]', "https://example.invalid/not-a-vod");
    await app.submit('input[aria-label="VOD link"]');
    const error = await until(
      "an error",
      () => app.js(`return document.querySelector(".add-error")?.textContent.trim()`),
      2 * MINUTE,
    );
    ctx.log(`says: "${error.slice(0, 160)}"`);
    await ctx.checkpoint("link-error");
    if ((await app.invoke("list_vods")).length !== 0) throw new Error("a VOD was added from a link that isn't one");
    await until("no stand-in row left", () => app.js(`return !document.querySelector(".vod.pending")`), 10_000);
  },
  // The error is the point: the run's end-of-scenario check would call it a failure.
  allowErrors: true,
};
