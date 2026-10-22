// Keys for getting around: Ctrl+F (⌘F on a Mac) goes to the search box from
// anywhere, and back and forward (Alt+← and Alt+→, or ⌘[ and ⌘]) step
// between the VODs opened and a search's results, a removed VOD left out.

import path from "node:path";

import { until } from "../lib/app.mjs";
import { videos } from "../lib/fixtures.mjs";

const mac = process.platform === "darwin";

export default {
  name: "navigation",
  async run(app, ctx) {
    await app.toLibrary();
    const first = await app.addAndTranscribe(ctx.fixtures.landed);
    await app.pickFiles([videos(path.join(ctx.out, "fixtures")).video]);
    await app.clickText("add video files…");
    const second = await until(
      "the second VOD, done",
      async () => (await app.invoke("list_vods")).find((v) => v.id !== first.id && v.status === "done"),
      300_000,
    );
    const press = (key, mods) =>
      app.js(
        `document.body.dispatchEvent(new KeyboardEvent("keydown", Object.assign({ key: arguments[0], bubbles: true }, arguments[1])))`,
        key,
        mods,
      );
    const open = () => app.js(`return Number(document.querySelector(".list .vod.selected")?.dataset.key ?? 0)`);

    await app.openVod(first.id);
    await app.openVod(second.id);
    if ((await open()) !== second.id) throw new Error("opening the second VOD didn't select it");
    await (mac ? press("[", { metaKey: true }) : press("ArrowLeft", { altKey: true }));
    await until("back to the first VOD", async () => (await open()) === first.id, 10_000);
    await (mac ? press("]", { metaKey: true }) : press("ArrowRight", { altKey: true }));
    await until("forward to the second", async () => (await open()) === second.id, 10_000);
    ctx.log("back and forward between two VODs");

    // In Settings, Alt+← on an emote provider moves it, and only that: the
    // view behind doesn't go back too.
    if (!mac) {
      await app.click('button[aria-label="Settings"]');
      const provider = '[aria-label="Chat emote providers, first one wins"] button.provider:last-child';
      await until(
        "the emote providers",
        () => app.js(`return !!document.querySelector(arguments[0])`, provider),
        30_000,
      );
      await app.js(
        `document.querySelector(arguments[0]).dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowLeft", altKey: true, bubbles: true, cancelable: true }))`,
        provider,
      );
      await new Promise((r) => setTimeout(r, 500));
      if ((await open()) !== second.id) throw new Error("Alt+← on an emote provider went back as well");
      await app.key("Escape");
      ctx.log("Alt+← in Settings moves the provider, and the view stays");
    }

    await press("f", mac ? { metaKey: true } : { ctrlKey: true });
    await until(
      "the search box focused",
      () => app.js(`return document.activeElement?.matches('input[aria-label="Search"]') ?? false`),
      10_000,
    );
    ctx.log(`${mac ? "⌘F" : "Ctrl+F"} focuses the search box`);

    // Leaving a search's results (a result double-clicked: the VOD, with the
    // words kept in the box) is a step: back brings them back as they were,
    // the same words and scrolled where they were, and forward the VOD again;
    // the box focused brings the results back too. The results get room below them to
    // scroll in (one match doesn't fill the view).
    await app.js(
      `const room = document.createElement("style");
       room.id = "e2e-room";
       room.textContent = ".results { padding-bottom: 3000px !important }";
       document.head.append(room);`,
    );
    await app.type('input[aria-label="Search"]', "landed");
    await until("a result", () => app.js(`return !!document.querySelector(".results button.row")`), 60_000);
    await app.js(`document.querySelector(".body").scrollTop = 150`);
    // The scroll is noted as its event comes, a frame or so later.
    await new Promise((r) => setTimeout(r, 300));
    await app.doubleClick(".results button.row");
    await until("the transcript", () => app.js(`return !!document.querySelector("#transcript")`), 30_000);
    const search = () =>
      app.js(`return {
        query: document.querySelector('input[aria-label="Search"]').value,
        rows: document.querySelectorAll(".results button.row").length,
        scroll: document.querySelector(".body").scrollTop,
        transcript: !!document.querySelector("#transcript") }`);
    await (mac ? press("[", { metaKey: true }) : press("ArrowLeft", { altKey: true }));
    const results = await until(
      "back to the results, as they were",
      async () => {
        const s = await search();
        return s.query === "landed" && s.rows > 0 && Math.abs(s.scroll - 150) <= 1 && s;
      },
      10_000,
    ).catch(async (e) => {
      throw new Error(`${e.message}: ${JSON.stringify(await search())}`);
    });
    await (mac ? press("]", { metaKey: true }) : press("ArrowRight", { altKey: true }));
    await until(
      "forward to the VOD",
      async () => {
        const s = await search();
        return s.query === "landed" && s.transcript && !s.rows && (await open()) === second.id;
      },
      10_000,
    );
    await app.focus('input[aria-label="Search"]');
    await until("the results, the box focused", async () => (await search()).rows > 0, 10_000);
    await app.js(`document.getElementById("e2e-room").remove()`);
    await app.type('input[aria-label="Search"]', "");
    await until("the results gone", async () => !(await search()).rows, 10_000);
    ctx.log(`back to the results (${results.rows} at ${results.scroll} px), forward to the VOD`);

    // A VOD removed leaves back and forward (nav.svelte.ts forget): back goes
    // to the first, and forward to where it was left (nothing open), never
    // to the VOD removed.
    await app.invoke("delete_vod", { id: second.id });
    await until(
      "the second VOD gone",
      async () => !(await app.invoke("list_vods")).some((v) => v.id === second.id),
      30_000,
    );
    await (mac ? press("[", { metaKey: true }) : press("ArrowLeft", { altKey: true }));
    await until("back to the first VOD", async () => (await open()) === first.id, 10_000);
    await (mac ? press("]", { metaKey: true }) : press("ArrowRight", { altKey: true }));
    await until("forward to nothing open, as it was", async () => (await open()) === 0, 10_000);
    for (let i = 0; i < 3; i++) await (mac ? press("]", { metaKey: true }) : press("ArrowRight", { altKey: true }));
    await new Promise((r) => setTimeout(r, 500));
    if ((await open()) === second.id) throw new Error("forward went to the VOD removed");
    ctx.log("a removed VOD is out of back and forward");
  },
};
