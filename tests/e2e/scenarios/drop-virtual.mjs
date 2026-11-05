// A video with no path of its own dropped on the window: on Windows, one
// inside a zip folder, open in File Explorer beside the app, is pressed in
// Explorer's list and let go on the library (one gesture, as a person
// drags). Explorer offers it only as a virtual file (no path), so the app
// receives it: it shows with the jobs as "Receiving" while it's read, then
// lands in the library as a file of its own, kept in the app's media folder
// with no part file left. Windows only (Explorer's zip folders).

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, rmSync } from "node:fs";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "../lib/app.mjs";
import { dataDirs } from "../lib/data.mjs";
import { videos } from "../lib/fixtures.mjs";
import * as os from "../lib/os.mjs";

/** The video's name inside the zip. */
const NAME = "zipped landing.mp4";

export default {
  name: "drop-virtual",
  async run(app, ctx) {
    if (process.platform !== "win32") ctx.skip("virtual files are received on Windows only, so far");
    const why = os.unavailable();
    if (why) ctx.skip(why);
    const explorer = await import("../lib/explorer.mjs");
    const work = path.join(ctx.out, "drop-virtual");
    rmSync(work, { recursive: true, force: true });
    mkdirSync(work, { recursive: true });
    // The landing video, zipped under another name.
    const inside = path.join(work, "inside");
    mkdirSync(inside);
    const video = path.join(inside, NAME);
    execFileSync("powershell.exe", [
      ...["-NoProfile", "-NonInteractive", "-Command"],
      `Copy-Item -LiteralPath '${videos(path.join(ctx.out, "fixtures")).video}' -Destination '${video}';
       Compress-Archive -LiteralPath '${video}' -DestinationPath '${path.join(work, "videos.zip")}'`,
    ]);
    const zip = path.join(work, "videos.zip");
    const received = path.join(dataDirs().at(-1), "media", "received");
    let source = null;
    try {
      await app.toLibrary();
      // The app on the left of the screen, Explorer on the right.
      const screen = explorer.screenSize();
      const width = Math.round(screen.w * 0.62);
      explorer.placeWindowOf(app.driver.proc.pid, { x: 0, y: 0, w: width, h: screen.h - 48 });
      const beside = { x: width + 8, y: 40, w: screen.w - width - 16, h: Math.round(screen.h * 0.7) };
      source = await explorer.openExplorer(zip, beside);
      const item = await until("the video in Explorer's list", () => explorer.itemIn(zip, NAME), 20_000);
      await app.js(
        `window.__kinds = [];
         const internals = window.__TAURI_INTERNALS__;
         const jobs = internals.transformCallback((e) => window.__kinds.push(...e.payload.map((j) => j.kind + ":" + j.label)));
         await internals.invoke("plugin:event|listen", { event: "jobs", target: { kind: "Any" }, handler: jobs });`,
      );
      const page = await os.windowRect(app);
      const on = { x: page.x + Math.round(page.w * 0.3), y: page.y + Math.round(page.h / 2) };
      // Pressed, moved past the drag's threshold, carried over the window, let go.
      await os.gesture([
        ["down", item.x, item.y],
        ["wait", 150],
        ["move", item.x - 10, item.y + 4],
        ["wait", 100],
        ["move", item.x - 40, item.y + 10],
        ["wait", 200],
        ...[0.25, 0.5, 0.75, 1].flatMap((f) => [
          ["move", item.x + (on.x - item.x) * f, item.y + (on.y - item.y) * f],
          ["wait", 120],
        ]),
        ["wait", 400],
        ["letgo", on.x, on.y],
      ]);
      const vod = await until(
        "the video received into the library",
        async () => (await app.invoke("list_vods")).find((v) => v.title === path.parse(NAME).name) ?? false,
        2 * MINUTE,
      );
      ctx.log(`received as ${vod.source_path}`);
      if (!vod.source_path || path.dirname(vod.source_path).toLowerCase() !== received.toLowerCase())
        throw new Error(`it isn't kept in ${received}: ${vod.source_path}`);
      if (!existsSync(vod.source_path)) throw new Error(`${vod.source_path} isn't there`);
      await until(
        "its duration read",
        async () => {
          const v = (await app.invoke("list_vods")).find((v) => v.id === vod.id);
          return v?.duration > 4 && v.duration < 6;
        },
        MINUTE,
      );
      const kinds = await app.js(`return window.__kinds`);
      ctx.log(`jobs heard meanwhile: ${[...new Set(kinds)].join(", ") || "(none)"}`);
      if (!kinds.includes(`receive:${NAME}`)) throw new Error(`no "Receiving" job was listed: ${kinds.join(", ")}`);
      await until("the job's gone", async () => (await app.invoke("list_jobs")).length === 0, 30_000);
      const parts = readdirSync(received).filter((f) => f.endsWith(".part"));
      if (parts.length) throw new Error(`part files left: ${parts.join(", ")}`);
      await sleep(500);
      const dialogs = explorer.explorerDialogs();
      if (dialogs.length) ctx.log(`Explorer's dialogs after: ${JSON.stringify(dialogs)}`);
    } finally {
      source?.close();
    }
  },
};
