// A video with no path of its own dropped on the window, received: it shows
// with the jobs as "Receiving" while it's read, then lands in the library as
// a file of its own, kept in the app's media folder with no part file (nor
// anything else) left, and opens, as a file dropped from a file manager does.
// Then one its app fails to give (it isn't there to give): the app says so,
// with no job or file left. One gesture each, as a person drags: pressed in
// the other app, carried over the window, let go on the library.
//
// - Windows: the video inside a zip folder, open in File Explorer beside the
//   app. Explorer offers it only as a virtual file (no path).
// - macOS: a test app beside the app's window gives it as a file promise
//   only (written a second after it's let go, as an app that makes or copies
//   out its file does).
// - Linux (X11): the video inside a zip, open in an archive manager (File
//   Roller before 43, Engrampa or Ark, lib/archives.mjs), which saves it
//   where the app says once it's let go; where there's none, a stand-in that
//   saves it the same way (XdndDirectSave). Then, where the document portal
//   can run, a file from a stand-in for a sandboxed app, sent through the
//   portal: it's a file on disk, so it's played where it is, as a file
//   dropped from a file manager is, and nothing is received.
//
// The failure is given by the promise giver on macOS and the XDS stand-in on
// Linux; Explorer has no way to fail one, so Windows has no failure step.

import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readdirSync, rmSync } from "node:fs";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, resizeInner, until } from "../lib/app.mjs";
import { dataDirs } from "../lib/data.mjs";
import { videos } from "../lib/fixtures.mjs";
import * as os from "../lib/os.mjs";

/** The video's name inside the zip, or as the promise names it. */
const NAME = process.platform === "darwin" ? "promised landing.mp4" : "zipped landing.mp4";
/** The sandboxed app's video's name. */
const SENT = "portal landing.mp4";
/** The name of the video its app fails to give. */
const MISSING = "missing landing.mp4";

export default {
  name: "drop-virtual",
  async run(app, ctx) {
    if (!["win32", "darwin", "linux"].includes(process.platform))
      ctx.skip("virtual files are received on Windows, macOS and Linux only");
    const why = os.unavailable();
    if (why) ctx.skip(why);
    const work = path.join(ctx.out, "drop-virtual");
    rmSync(work, { recursive: true, force: true });
    mkdirSync(work, { recursive: true });
    const received = path.join(dataDirs().at(-1), "media", "received");
    await app.toLibrary();
    await listenToJobs(app);
    let giver = null;
    try {
      if (process.platform === "win32") await fromZipFolder(app, ctx, work);
      else if (process.platform === "darwin") giver = await fromPromiseGiver(app, ctx, work);
      else await fromArchiveManager(app, ctx, work);
      await receivedIntoLibrary(app, ctx, received);
      if (giver) ctx.log(`the giver said: ${giver.said().join(" | ")}`);
    } finally {
      giver?.close();
    }
    await failedReceive(app, ctx, work, received);
    if (process.platform === "linux") await fromSandbox(app, ctx, work, received);
  },
};

/** The page keeps each job it hears of ("kind:label"), in window.__kinds, and what it hears of each file received, in window.__received. */
function listenToJobs(app) {
  return app.js(
    `window.__kinds = [];
     window.__received = [];
     const internals = window.__TAURI_INTERNALS__;
     const jobs = internals.transformCallback((e) => window.__kinds.push(...e.payload.map((j) => j.kind + ":" + j.label)));
     await internals.invoke("plugin:event|listen", { event: "jobs", target: { kind: "Any" }, handler: jobs });
     const received = internals.transformCallback((e) => window.__received.push(JSON.stringify(e.payload)));
     await internals.invoke("plugin:event|listen", { event: "received", target: { kind: "Any" }, handler: received });`,
  );
}

/** Pressed at `from`, moved past the drag's threshold, carried over the library, let go. */
async function dragOntoLibrary(app, from) {
  const page = await os.windowRect(app);
  const on = { x: page.x + Math.round(page.w * 0.3), y: page.y + Math.round(page.h / 2) };
  await os.gesture([
    ["down", from.x, from.y],
    ["wait", 150],
    ["move", from.x - 10, from.y + 4],
    ["wait", 100],
    ["move", from.x - 40, from.y + 10],
    ["wait", 200],
    ...[0.25, 0.5, 0.75, 1].flatMap((f) => [
      ["move", from.x + (on.x - from.x) * f, from.y + (on.y - from.y) * f],
      ["wait", 120],
    ]),
    ["wait", 400],
    ["letgo", on.x, on.y],
  ]);
}

/** Windows: the video dragged out of a zip folder in Explorer. */
async function fromZipFolder(app, ctx, work) {
  const explorer = await import("../lib/explorer.mjs");
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
  let source = null;
  try {
    // The app on the left of the screen, Explorer on the right.
    const screen = explorer.screenSize();
    const width = Math.round(screen.w * 0.62);
    explorer.placeWindowOf(app.driver.proc.pid, { x: 0, y: 0, w: width, h: screen.h - 48 });
    const beside = { x: width + 8, y: 40, w: screen.w - width - 16, h: Math.round(screen.h * 0.7) };
    source = await explorer.openExplorer(zip, beside);
    const item = await until("the video in Explorer's list", () => explorer.itemIn(zip, NAME), 20_000);
    await dragOntoLibrary(app, item);
    await waitForVideo(app, NAME);
    await sleep(500);
    const dialogs = explorer.explorerDialogs();
    if (dialogs.length) ctx.log(`Explorer's dialogs after: ${JSON.stringify(dialogs)}`);
  } finally {
    source?.close();
  }
}

/** macOS: the video given as a file promise by a test app; that app, to close once it's received. */
async function fromPromiseGiver(app, ctx, work) {
  const { openPromiseGiver } = await import("../lib/fileManager.mjs");
  // Narrowed, for room beside it (the Mac's screen may be 1280 wide).
  await resizeInner(app, 660, await app.js("return innerHeight"));
  const giver = await openPromiseGiver(app, ctx, work, videos(path.join(ctx.out, "fixtures")).video, NAME);
  try {
    await dragOntoLibrary(app, giver.from);
    return giver;
  } catch (e) {
    giver.close();
    throw e;
  }
}

/** Linux: the video dragged out of a zip in an archive manager, or out of the XDS stand-in. */
async function fromArchiveManager(app, ctx, work) {
  const archives = await import("../lib/archives.mjs");
  const program = archives.archiveManager();
  const noStandIn = program ? null : archives.noStandIn();
  if (noStandIn) ctx.skip(`no archive manager to drag out of, and ${noStandIn} for its stand-in`);
  const inside = path.join(work, "inside");
  mkdirSync(inside);
  const video = path.join(inside, NAME);
  copyFileSync(videos(path.join(ctx.out, "fixtures")).video, video);
  let source = null;
  try {
    if (program) {
      const zip = path.join(work, "videos.zip");
      archives.zipStored(zip, [{ name: NAME, file: video }]);
      source = await archives.openArchive(app, ctx, program, zip, path.join(work, "home"));
      ctx.log(`dragged out of ${program}'s list`);
    } else {
      source = await archives.openStandIn(app, ctx, "xds", video, work);
      ctx.log("no archive manager here: dragged out of the stand-in that saves as one (XdndDirectSave)");
    }
    await dragOntoLibrary(app, source.from);
    await waitForVideo(app, NAME).catch((e) => {
      if (source.said) ctx.log(`the stand-in said: ${source.said().join(" | ")}`);
      throw e;
    });
    offered(app, ctx);
  } finally {
    source?.close();
  }
}

/** What the app said each drag offered, where it says (its builds for the tests, through the bridge). */
function offered(app, ctx) {
  const said = (app.driver.proc?.stderr ?? "").split("\n").filter((l) => /^drop: (a drag|let go|nothing)/.test(l));
  for (const line of said) ctx.log(`the app: ${line.slice("drop: ".length)}`);
}

/** The library's video named as `name` once it's there. */
function waitForVideo(app, name) {
  return until(
    "the video received into the library",
    async () => (await app.invoke("list_vods")).find((v) => v.title === path.parse(name).name) ?? false,
    2 * MINUTE,
  );
}

/** The video received kept in `received`, whole, with no job, part file or saved copy left. */
async function receivedIntoLibrary(app, ctx, received) {
  const vod = await waitForVideo(app, NAME);
  ctx.log(`received as ${vod.source_path}`);
  await until(
    "the video received, open",
    () =>
      app.js(
        `return !!document.querySelector('.vod.selected[data-key="${vod.id}"]') && !!document.querySelector("button.time")`,
      ),
    30_000,
  ).catch(async (e) => {
    ctx.log(`the page heard: ${(await app.js(`return window.__received`)).join(" | ") || "(nothing)"}`);
    throw e;
  });
  ctx.log("it opened once received");
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
  // Part files, and (Linux) the hidden folder an archive manager saves into first.
  const leftovers = () => readdirSync(received).filter((f) => f.endsWith(".part") || f.startsWith(".dragndrop-"));
  await until("nothing left beside the video", () => leftovers().length === 0, 10_000).catch(() => {
    throw new Error(`left in ${received}: ${leftovers().join(", ")}`);
  });
}

/** A video its app fails to give (macOS, Linux): said in a toast, with no job, VOD or file left. */
async function failedReceive(app, ctx, work, received) {
  if (process.platform === "win32") return ctx.log("no failed receive: Explorer has no way to fail one");
  const failing = path.join(work, "failing");
  mkdirSync(failing, { recursive: true });
  // Never written: the app asked for it can't give it.
  const missing = path.join(failing, MISSING);
  let source;
  if (process.platform === "darwin") {
    const { openPromiseGiver } = await import("../lib/fileManager.mjs");
    source = await openPromiseGiver(app, ctx, failing, missing, MISSING, { delay: 0 });
  } else {
    const archives = await import("../lib/archives.mjs");
    const no = archives.noStandIn();
    if (no) return ctx.log(`no failed receive: ${no} for the stand-in`);
    source = await archives.openStandIn(app, ctx, "xds", missing, failing);
  }
  try {
    await dragOntoLibrary(app, source.from);
    const said = await until(
      "the toast saying it wasn't received",
      async () => {
        const said = await app.toastSays();
        return said.startsWith("Couldn't receive") && said;
      },
      MINUTE,
    ).catch((e) => {
      ctx.log(`its app said: ${source.said().join(" | ")}`);
      offered(app, ctx);
      throw e;
    });
    ctx.log(`the toast: ${said}`);
    // Named where its app named it (a promise); an archive manager that saves nothing gives no name.
    const what = process.platform === "darwin" ? `“${MISSING}”` : "the dropped file";
    const expected = `Couldn't receive ${what}. The app it came from didn't hand it over.`;
    if (said !== expected) throw new Error(`the toast says “${said}”, not “${expected}”`);
    // Dismissed, as the harness takes an error left showing for a failure.
    await app.js(`document.querySelector(".toast .close").click()`);
    await until("the toast dismissed", async () => !(await app.toastSays()), 10_000);
    await until("no job left", async () => (await app.invoke("list_jobs")).length === 0, 30_000);
    if ((await app.invoke("list_vods")).some((v) => v.title === path.parse(MISSING).name))
      throw new Error("a VOD was added for the file not received");
    const left = () =>
      readdirSync(received).filter((f) => f.startsWith(".dragndrop-") || f.startsWith(path.parse(MISSING).name));
    await until("nothing of it left", () => left().length === 0, 10_000).catch(() => {
      throw new Error(`left in ${received}: ${left().join(", ")}`);
    });
  } finally {
    source.close();
  }
}

/** Linux: a file from a sandboxed app, through the document portal, played where it is. */
async function fromSandbox(app, ctx, work, received) {
  const archives = await import("../lib/archives.mjs");
  const no = archives.noPortal();
  if (no) return ctx.log(`no file sent through the portal: ${no}`);
  const file = path.join(work, SENT);
  copyFileSync(videos(path.join(ctx.out, "fixtures")).video, file);
  await app.js(`window.__kinds = []`);
  const source = await archives.openStandIn(app, ctx, "portal", file, work);
  try {
    await dragOntoLibrary(app, source.from);
    const vod = await waitForVideo(app, SENT).catch((e) => {
      ctx.log(`the stand-in said: ${source.said().join(" | ")}`);
      throw e;
    });
    offered(app, ctx);
    ctx.log(`the stand-in: ${source.said().find((l) => l.includes("sent as transfer")) ?? "(it didn't say)"}`);
    if (vod.source_path !== file) throw new Error(`it isn't played where it is (${file}): ${vod.source_path}`);
    const kinds = await app.js(`return window.__kinds`);
    if (kinds.some((k) => k.startsWith("receive:"))) throw new Error(`it was received, not played in place: ${kinds}`);
    if (readdirSync(received).some((f) => f.startsWith(path.parse(SENT).name)))
      throw new Error(`a copy of it is in ${received}`);
    ctx.log("a file sent through the portal: played where it is");
  } finally {
    source.close();
  }
}
