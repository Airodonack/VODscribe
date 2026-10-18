// Keys for getting around: Ctrl+F (⌘F on a Mac) goes to the search box from
// anywhere, and back and forward (Alt+← and Alt+→, or ⌘[ and ⌘]) step
// between the VODs opened.

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

    await press("f", mac ? { metaKey: true } : { ctrlKey: true });
    await until(
      "the search box focused",
      () => app.js(`return document.activeElement?.matches('input[aria-label="Search"]') ?? false`),
      10_000,
    );
    ctx.log(`${mac ? "⌘F" : "Ctrl+F"} focuses the search box`);
  },
};
