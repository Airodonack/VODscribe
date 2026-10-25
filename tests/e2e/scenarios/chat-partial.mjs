// A chat fetch that stops partway, as Twitch's does on a long VOD when it
// answers one page with "service error" and keeps doing so: what was fetched
// stays, shown in the chat column and on the minimap, the header says the
// chat stopped and why, with Try again; and Try again carries on from where
// it stopped, the messages shown all along, to the whole chat, each once.
// No transcript: the chat doesn't wait for one. The chat comes from a file
// (e2e-chat.json in the data folder, served by the e2e build in place of
// Twitch: src-tauri/src/e2e.rs), so this needs no network; the VOD is the
// test Twitch VOD's library entry, made from a file added here.

import { writeFileSync } from "node:fs";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

import { MINUTE, until } from "../lib/app.mjs";
import { dataDirs } from "../lib/data.mjs";

const TWITCH_VOD = "2901646294";
const DURATION = 4 * 3600;
// 4,000 messages over four hours, 100 to a page: forty pages, in four slices
// fetched at once. Longer than the transcript view lays out at first (60 rows,
// a minute each with no transcript), as a long stream is.
const MESSAGES = Array.from({ length: 4000 }, (_, i) => ({
  time: i * 3.6,
  author: `viewer${i % 37}`,
  text: `message ${i}`,
  created_at: new Date(Date.parse("2026-10-14T18:00:05Z") + i * 3600).toISOString(),
}));
// Twitch refuses every page from the twelfth on, until told otherwise.
const FAIL_AFTER = 12;

function serveChat(failAfter) {
  writeFileSync(
    path.join(dataDirs()[0], "e2e-chat.json"),
    JSON.stringify({ messages: MESSAGES, fail_after: failAfter }),
  );
}

export default {
  name: "chat-partial",
  expects: {
    "chat-stopped":
      'The transcript header reads "Chat stopped partway" with "service error" after it and a "Try again" link; beside the empty transcript, a chat column shows messages like "viewer22 message 207", and the strip at the right edge draws how busy chat was over the VOD.',
  },
  async run(app, ctx) {
    // The chat fixture is served by the app's e2e build only.
    if (!(await app.hasE2e())) return ctx.skip("this build has no e2e feature, so the chat fixture isn't served");
    await app.toLibrary();
    const { vods } = await app.invoke("add_files", { paths: [ctx.fixtures.landed] });
    const id = vods[0]?.id;
    if (!id) throw new Error("the file wasn't added");

    // The entry made a Twitch VOD's, four hours long, with its chat not fetched
    // yet: the app fetches it as it starts.
    await ctx.quit();
    serveChat(FAIL_AFTER);
    const db = new DatabaseSync(path.join(dataDirs()[0], "library.sqlite"));
    db.prepare(
      `UPDATE vods SET platform = 'twitch', platform_id = ?, url = ?, title = 'Chat that stops partway',
         duration = ?, source_path = NULL, chat = NULL, chat_error = NULL WHERE id = ?`,
    ).run(TWITCH_VOD, `https://www.twitch.tv/videos/${TWITCH_VOD}`, DURATION, id);
    db.close();
    app = await ctx.start();
    await app.toLibrary();
    await app.openVod(id);

    // Each refused page is asked again for about half a minute before the fetch stops.
    const vod = await until(
      "the fetch to stop",
      async () => {
        const v = (await app.invoke("list_vods")).find((v) => v.id === id);
        if (v?.chat === "done") throw new Error("the chat was fetched whole, though Twitch refused pages");
        return v?.chat === "failed" && v;
      },
      2 * MINUTE,
    );
    const overview = () => app.invoke("chat_overview", { vodId: id, duration: DURATION, bins: 100, events: false });
    const saved = (await overview()).count;
    if (!(saved > 0 && saved < MESSAGES.length))
      throw new Error(`the stopped fetch kept ${saved} of ${MESSAGES.length} messages`);
    if (!/service error/.test(vod.chat_error ?? ""))
      throw new Error(`the VOD says its chat failed with ${JSON.stringify(vod.chat_error)}`);
    ctx.log(`stopped with ${saved} messages kept: ${vod.chat_error}`);

    const page = () =>
      app.js(`return {
        head: document.querySelector(".chat-status")?.textContent.replace(/\\s+/g, " ").trim() ?? "",
        title: document.querySelector(".chat-status .error-text")?.title ?? "",
        shown: document.querySelectorAll("#transcript .chat .message").length,
        map: document.querySelector(".minimap")?.title ?? "" }`);
    // Every way the page can let down what was fetched, said together.
    const stopped = await until(
      "the header to say so",
      async () => (await page()).head.includes("Try again") && page(),
      MINUTE,
    );
    const wrong = [];
    if (!/Chat stopped partway: service error/.test(stopped.head))
      wrong.push(`the chat header reads "${stopped.head}"`);
    if (!/service error/.test(stopped.title)) wrong.push(`the header's tooltip reads "${stopped.title}"`);
    if (stopped.shown === 0) wrong.push("the chat column shows no messages");
    if (!/^How busy chat was/.test(stopped.map))
      wrong.push(`the minimap draws "${stopped.map || "nothing"}", not the chat`);
    // Further in (the second of four slices, from 1:00:00), rows show their
    // messages too: scrolled to a screen at a time, as a person would.
    const further = await until(
      "messages from 1:00:00 on, scrolled to",
      () =>
        app.js(`
          const body = document.querySelector(".body");
          const numbers = [...document.querySelectorAll("#transcript .chat .message")]
            .map((m) => Number(/message (\\d+)/.exec(m.textContent)?.[1] ?? -1));
          if (numbers.some((n) => n >= 1000)) return true;
          body.scrollTop += body.clientHeight;
          return false;`),
      30_000,
    ).catch(() => false);
    if (!further) wrong.push("rows scrolled to from 1:00:00 on show none of their messages");
    if (wrong.length) throw new Error(wrong.join("; "));
    ctx.log(`shown: "${stopped.head}", ${stopped.shown} messages in view, more further in, the minimap drawing chat`);
    await ctx.checkpoint("chat-stopped");

    // Twitch answers again: Try again carries on, never taking away what's shown.
    serveChat(null);
    await app.clickText("Try again");
    let fewest = Infinity;
    let least = Infinity;
    await until(
      "the whole chat",
      async () => {
        const [v, p, o] = await Promise.all([
          app.invoke("list_vods").then((vs) => vs.find((v) => v.id === id)),
          page(),
          overview(),
        ]);
        fewest = Math.min(fewest, p.shown);
        least = Math.min(least, o.count);
        if (v?.chat === "failed") throw new Error(`the fetch failed again: ${v.chat_error}`);
        return v?.chat === "done" && o.count === MESSAGES.length && p.shown > 0;
      },
      MINUTE,
      { every: 100 },
    );
    if (fewest === 0) throw new Error("the chat column went empty while the fetch carried on");
    if (least < saved) throw new Error(`the fetch started over: ${least} messages at one point, ${saved} before`);
    const head = (await page()).head;
    if (!head.includes(MESSAGES.length.toLocaleString("en-US")))
      throw new Error(`once fetched, the chat header reads "${head}"`);
    ctx.log(`carried on to all ${MESSAGES.length} messages, each once; the column never emptied`);
  },
};
