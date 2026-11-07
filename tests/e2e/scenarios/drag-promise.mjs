// A clip dragged out of the window before its file is made: the system's
// drag begins at the edge at once, and the file lands where it's dropped
// once it's made, in the session's file manager (lib/fileManager.mjs), on
// the test's own display and bus: Nautilus waits for the drop's answer;
// Dolphin, in a KDE session, takes KDE's promise and the app copies the
// file in once it's made. The clip's making is held up on purpose (the
// app's ffmpeg is a stand-in that waits before a clip's encode, then runs
// the real one), so each drop comes before its file is there. A minute's
// stretch of a 720p video, made as MOV, carried by Clip to the folder and
// let go: it lands whole, the window answers meanwhile, the app says the
// clip is being made, and the page hears it went; the app's own
// file is kept as another app's (Nautilus took it where it is) or goes
// (Dolphin has its own copy). Carried out, back over the window, out again
// and let go: it lands too. Called off (Escape)
// while the file is waited for: nothing lands, and the app's file goes too.
// One whose encode fails: nothing lands, and the app says it couldn't make
// the clip. Two pills picked and carried together, both being made again
// (in the other format, picked meanwhile): both land, Dolphin's one by one
// in their order. A pill still being made, dragged alone: it lands too.
// Carried in the window, a stretch's clip is made from the press, hidden
// (no pill) and not listed with the jobs; Escape there stops it, its part
// file gone. Let go on another app, it's listed with the jobs until it's
// handed over, and no pill shows meanwhile. Let go on an app that takes
// finished files only (one of the test's: it gives up after 5 s, as Qt's
// do), before the file is made: nothing lands there, the app says that app
// (by its desktop entry's name) takes finished files only (on macOS never
// that it's being made: it waits for nothing), and the clip is kept as a
// pill, which once made drags there and lands with nothing said; on macOS, let go
// on one that refuses the drop (the same app, told to), the same. On macOS the drop
// is on Finder, which takes a file promise: the app writes each file where
// it was let go once it's made (Finder shows it with a progress meanwhile),
// so Finder has its own copies; there also a pill already made lands at
// once, Finder's cancel on the file being written lands nothing and says
// nothing, so does Cancel in the list of jobs, and three clips land one by
// one. Linux (X11), macOS and Windows (on File Explorer: `onWindows`);
// E2E_FILE_MANAGER=dolphin drops on Dolphin outside a KDE session.

import { chmodSync, existsSync, mkdirSync, readdirSync, renameSync, rmSync, statSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, resizeInner, until } from "../lib/app.mjs";
import { clipsDir, dataDirs } from "../lib/data.mjs";
import {
  FINISHED_ONLY_MAC_NAME,
  clearDropped,
  frontmost,
  droppedIn,
  fileManager,
  noFileManager,
  noFinishedOnly,
  openFileManager,
  openFinishedOnly,
  probeDuration,
  setFormat,
  windowsOf,
} from "../lib/fileManager.mjs";
import { longVideo } from "../lib/fixtures.mjs";
import * as os from "../lib/os.mjs";

/** A pointer event's fields at (x, y) with `buttons` down, in the page. */
const AT = `const at = (x, y, buttons) => ({ bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons, isPrimary: true, pointerId: 1 });`;

/** How long the stand-in holds up each clip's encode, in seconds. */
const HELD = 8;
/** How far off its stretch's length a file's duration may be, in seconds. */
const SLACK = 0.1;
/** What the app says while a clip let go on another app is still being made (dragout.ts `making`). */
const MAKING = "Making the clip. It'll be where you dropped it once it's made.";
/** Has the page note each toast shown from now on (`window.__said`), at
 *  each change, so one already gone is still known (`toastsSaid`). */
const HEAR_TOASTS = `window.__said = [];
  window.__saidBy?.disconnect();
  const look = () => {
    const now = document.querySelector(".toast .message")?.textContent.trim();
    if (now && window.__said.at(-1) !== now) window.__said.push(now);
  };
  window.__saidBy = new MutationObserver(look);
  window.__saidBy.observe(document.body, { subtree: true, childList: true, characterData: true });`;

/** The toasts shown since the last drag out began (`HEAR_TOASTS`). */
function toastsSaid(app) {
  return app.js(`return window.__said ?? []`);
}

/** That the app said a clip let go on another app is being made, within
 *  10 s (the receiver may ask for it a moment after the drop). */
function saysMaking(app) {
  return until("the toast saying it's being made", async () => (await toastsSaid(app)).includes(MAKING), 10_000).catch(
    async (e) => {
      throw new Error(`${e.message}: the toasts said ${JSON.stringify(await toastsSaid(app))}`);
    },
  );
}

/** That the app never said a clip was being made since the drag began
 *  (`what` it was: made already, refused). */
async function saidNoMaking(app, what) {
  const all = await toastsSaid(app);
  if (all.includes(MAKING))
    throw new Error(`the app said the clip was being made, let go ${what}: ${JSON.stringify(all)}`);
}

/** What the toast says once `fits` it (an earlier one may show first: the
 *  clip's being made, said as it was let go), within 10 s; else an error
 *  saying what it said. */
async function saysAtLast(app, fits) {
  let last = "";
  return until(
    "the toast",
    async () => {
      last = await app.toastSays();
      return fits(last) && last;
    },
    10_000,
  ).catch((e) => {
    throw new Error(`${e.message}: the toast says "${last}"`);
  });
}

export default {
  name: "drag-promise",
  async run(app, ctx) {
    if (process.platform === "win32") return onWindows(app, ctx);
    const why = noFileManager();
    if (why) ctx.skip(why);
    // Dolphin copies the files in (KDE's promise), and Finder has them
    // written in (a file promise); Nautilus takes ours where they are.
    const mac = process.platform === "darwin";
    const started = Date.now();
    const copies = fileManager() === "dolphin" || mac;
    const work = path.join(ctx.out, "drag-promise");
    rmSync(work, { recursive: true, force: true });
    const dest = path.join(work, "drop-here");
    mkdirSync(work, { recursive: true });

    // The stand-in for the app's ffmpeg (its bin is kept between scenarios:
    // one there is moved aside, and back after).
    const bin = path.join(dataDirs().at(-1), "bin");
    mkdirSync(bin, { recursive: true });
    const ffmpeg = path.join(bin, "ffmpeg");
    const aside = path.join(bin, "ffmpeg.e2e-real");
    if (existsSync(ffmpeg)) renameSync(ffmpeg, aside);
    const real = existsSync(aside)
      ? aside
      : execFileSync("sh", ["-c", "command -v ffmpeg"], { encoding: "utf8" }).trim();
    const mode = path.join(work, "ffmpeg-mode");
    /** How the stand-in makes a clip's file: after a wait, or failing after one. */
    const encodes = (how) => writeFileSync(mode, how);
    writeFileSync(
      ffmpeg,
      `#!/bin/sh
# A clip's encode writes a .part file: held up, made to fail, or left part-made
# (saying how far it is, as ffmpeg does, so it can be stopped) (drag-promise).
for a in "$@"; do
  case "$a" in
    *.part)
      case "$(cat '${mode}' 2>/dev/null)" in
        fail) sleep ${HELD / 2}; echo "Made to fail by the test." >&2; exit 1 ;;
        slow) sleep ${HELD} ;;
        partial) head -c 200000 /dev/zero > "$a"
          for i in $(seq 1 120); do echo "out_time_us=$((i * 100000))"; sleep 0.5; done; exit 1 ;;
      esac ;;
  esac
done
exec '${real}' "$@"
`,
    );
    chmodSync(ffmpeg, 0o755);
    encodes("slow");
    let nautilus = null;
    let taker = null;
    // The name the app that takes finished files only goes by: its desktop
    // entry's, where the app looks (the test's own XDG_DATA_HOME, never the
    // user's; without one, its window's class).
    const entry =
      process.env.XDG_DATA_HOME && path.join(process.env.XDG_DATA_HOME, "applications", "clip-taker.desktop");
    const takerName = mac ? FINISHED_ONLY_MAC_NAME : entry ? "Clip Taker" : "Clip-taker";
    try {
      await app.toLibrary();
      const [vod] = await app.addFiles([longVideo(path.join(ctx.out, "fixtures"))]);
      await until("the video read", async () => (await app.invoke("list_vods"))[0]?.duration > 0, 5 * MINUTE);
      await setFormat(app, "MOV");
      await app.openPaused(vod.id);
      // Cuts at one minute and two (B, the playhead there): the first stretch
      // is 0 to 60 s, the next 60 to 120.
      for (const time of [60, 120]) {
        await app.js(
          `${AT}
           const bar = document.querySelector(".scrubber");
           const box = bar.getBoundingClientRect();
           const x = box.left + ((arguments[0] - Number(bar.dataset.viewStart)) / Number(bar.dataset.span)) * box.width;
           const y = box.top + box.height / 2;
           bar.dispatchEvent(new PointerEvent("pointerdown", at(x, y, 1)));
           window.dispatchEvent(new PointerEvent("pointerup", at(x, y, 0)));`,
          time,
        );
        await until(`the playhead at ${time} s`, async () => Math.abs((await app.shownTime()) - time) <= 1, 10_000);
        const had = (await app.invoke("list_marks", { vodId: vod.id })).length;
        await app.key("b");
        await until("the cut", async () => (await app.invoke("list_marks", { vodId: vod.id })).length > had, 10_000);
        await app.key("Escape");
      }
      const cuts = (await app.invoke("list_marks", { vodId: vod.id })).map((m) => m.start);
      /** How long the stretch from `start` is, between the cuts. */
      const stretchFrom = (start) => Math.min(...cuts.filter((c) => c > start + 0.01)) - start;
      // The app's clip files, and the names they're handed out by beside them
      // (all in its data); on macOS only those written since this began.
      const clipDirs = [...new Set([clipsDir(), ...dataDirs()])];
      const appFiles = () =>
        clipDirs.flatMap(filesUnder).filter((f) => /\.(mov|mp4)(\.part)?$/.test(f) && (!mac || mtimeOf(f) >= started));
      /** Whether `file` dropped is whole: Dolphin's copy is renamed into place
       *  once written; Nautilus copies ours in as it goes, so it's whole at our size. */
      const whole = (file) => {
        // (Finder's stands there empty until the app writes it whole, at once.)
        if (mac) return sizeOf(file) > 0;
        if (copies) return existsSync(file);
        const own = appFiles().find((f) => path.basename(f) === path.basename(file));
        return !!own && sizeOf(file) === sizeOf(own);
      };

      // Room for Finder beside the window, on a screen 1280 points wide.
      if (mac) await resizeInner(app, 660, await app.js("return innerHeight"));
      nautilus = await openFileManager(app, ctx, dest, path.join(work, "home"));
      const { drop } = nautilus;
      // What the page hears of the system's drag's end, and when; and the
      // list of jobs it's sent.
      await app.js(
        `window.__ended = [];
         window.__jobs = [];
         const internals = window.__TAURI_INTERNALS__;
         const handler = internals.transformCallback((e) => window.__ended.push({ ...e.payload, at: Date.now() }));
         await internals.invoke("plugin:event|listen", { event: "drag-ended", target: { kind: "Any" }, handler });
         const jobs = internals.transformCallback((e) => window.__jobs.push(e.payload.map((j) => j.id)));
         await internals.invoke("plugin:event|listen", { event: "jobs", target: { kind: "Any" }, handler: jobs });`,
      );
      const centre = async (selector) => {
        const r = await os.rectOf(app, selector);
        return { x: r.x + Math.round(r.w / 2), y: r.y + Math.round(r.h / 2) };
      };
      const toStart = async () => {
        await app.js(
          `${AT}
           const bar = document.querySelector(".scrubber");
           const box = bar.getBoundingClientRect();
           const y = box.top + box.height / 2;
           bar.dispatchEvent(new PointerEvent("pointerdown", at(box.left + 1, y, 1)));
           window.dispatchEvent(new PointerEvent("pointerup", at(box.left + 1, y, 0)));`,
        );
        await until("the playhead at the start", async () => (await app.shownTime()) <= 1, 10_000);
      };
      /** The playhead in the last stretch, past the second cut. */
      const toLast = async () => {
        await app.js(
          `${AT}
           const bar = document.querySelector(".scrubber");
           const box = bar.getBoundingClientRect();
           const x = box.left + ((150 - Number(bar.dataset.viewStart)) / Number(bar.dataset.span)) * box.width;
           const y = box.top + box.height / 2;
           bar.dispatchEvent(new PointerEvent("pointerdown", at(x, y, 1)));
           window.dispatchEvent(new PointerEvent("pointerup", at(x, y, 0)));`,
        );
        await until("the playhead in the last stretch", async () => (await app.shownTime()) > 121, 10_000);
      };
      /** The clips made only for a drag (none in the clip bar). */
      const hidden = async () => (await app.invoke("list_clips", { vodId: vod.id })).filter((c) => !c.shown);
      /** Clip (or `what`) pressed and carried to over the folder (or `to`), the system's drag up; then `then`. */
      const carryOut = async (then = [], what = "button.action.clip", to = drop) => {
        // (A press on a window not in front only brings it there.)
        await frontmost(app);
        await app.js(`performance.clearMarks(); window.__ended = []; ${HEAR_TOASTS}`);
        // (A pill can be scrolled out of sight in a narrow clip bar: brought
        // into it at once, not smoothly, so its place is where it'll stay.)
        await app.js(
          `document.querySelector(arguments[0])?.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "instant" })`,
          what,
        );
        await sleep(300);
        const from = await centre(what);
        const before = new Set(windowsOf(app.driver.proc.pid));
        const steps = [
          ["down", from.x, from.y],
          ["wait", 100],
        ];
        for (let i = 1; i <= 18; i++)
          steps.push(
            ["move", Math.round(from.x + (to.x - from.x) * (i / 18)), Math.round(from.y + (to.y - from.y) * (i / 18))],
            ["wait", 50],
          );
        await os.gesture(steps);
        await until(
          "the system's drag",
          () => app.js(`return performance.getEntriesByName("drag-start").length > 0`),
          10_000,
          { every: 50 },
        );
        // (On macOS the window server draws it.)
        if (!mac)
          await until("its picture", () => windowsOf(app.driver.proc.pid).some((w) => !before.has(w)), 10_000, {
            every: 25,
          });
        await os.gesture([
          ["move", to.x + 8, to.y + 4],
          ["wait", 200],
          ["move", to.x + 16, to.y + 8],
          ["wait", 300],
          ...then,
        ]);
      };
      /** Whether clip `id` shows a pill in the clip bar. */
      const pill = (id) => app.js(`return !!document.querySelector(arguments[0])`, `.chip[data-clip="${id}"]`);
      /** Whether clip `id` is in the list of jobs. */
      const listed = async (id) => (await app.invoke("list_jobs")).some((j) => j.id === id);
      const ended = () =>
        until("the system's drag, over", () => app.js(`return window.__ended[0] ?? false`), 2 * MINUTE);
      const settle = async () => {
        await until("the carry over", () => app.js(`return !document.querySelector(".carry-ghost")`), MINUTE);
        await until("no clip left made for a drag", async () => (await hidden()).length === 0, 30_000);
        await sleep(1000);
        clearDropped(dest);
      };
      /** The file dropped, once whole: as long as its stretch, and when. */
      const landed = async (label) => {
        const [file, at] = await until(
          `${label}: the file whole in the folder`,
          () => {
            // Whole: in place under its own name (Dolphin's copy is written beside, then renamed).
            const there = droppedIn(dest).filter((f) => !f.endsWith(".part"));
            const file = there.length && path.join(dest, there[0]);
            return !!file && whole(file) && [file, Date.now()];
          },
          2 * MINUTE,
          { every: 50 },
        );
        // Whole: as long as the stretch (the app writes it beside, then renames).
        const length = probeDuration(file);
        const want = stretchFrom(0);
        if (Math.abs(length - want) > SLACK)
          throw new Error(`${label}: the file dropped is ${length} s long, not ${want} s`);
        return { file, at, length };
      };

      // 0. Carried in the window from a file here: its clip is made from the
      // press, with no pill and not listed with the jobs; Escape there stops
      // it, and its part file goes.
      encodes("partial");
      await toStart();
      await frontmost(app);
      const clipAt = await centre("button.action.clip");
      const pressed = Date.now();
      await os.gesture([
        ["down", clipAt.x, clipAt.y],
        ["wait", 100],
        // (Inward on macOS, where the window is narrowed to leave room for Finder.)
        ["move", clipAt.x + (mac ? -30 : 30), clipAt.y - 30],
        ["wait", 100],
        ["move", clipAt.x + (mac ? -60 : 60), clipAt.y - 60],
      ]);
      const early = await until("its clip, made from the press", async () => (await hidden())[0] ?? false, 10_000, {
        every: 50,
      });
      const begun = Date.now() - pressed;
      await until("its part file being written", () => appFiles().some((f) => f.endsWith(".part")), 30_000, {
        every: 50,
      });
      if (await pill(early.id)) throw new Error("a pill showed for the clip made for a carry in the window");
      if (await listed(early.id)) throw new Error("the clip made for a carry in the window was listed with the jobs");
      await os.gesture([["key", "Escape"], ["wait", 300], ["up"]]);
      await until("its clip gone", async () => (await hidden()).length === 0, 10_000);
      await until("its part file gone", () => !appFiles().some((f) => f.endsWith(".part")), 10_000);
      ctx.log(
        `carried in the window: its clip made ${begun} ms from the press, no pill, not listed with the jobs; Escape: it and its part file gone`,
      );
      encodes("slow");

      // 1. Let go on Nautilus before the file is made: it lands whole. The
      // window answers while it's waited for (the video plays on), no pill
      // shows, the clip's making is listed with the jobs till it's over, and
      // the app says it's being made.
      await toStart();
      await app.click('.bar button[aria-label="Play"]');
      await until("the video playing", async () => (await app.shownTime()) >= 2, 30_000);
      await carryOut();
      const dropped = Date.now();
      await os.gesture([["letgo", drop.x + 16, drop.y + 8]]);
      const [making] = await hidden();
      if (!making || making.status === "ready")
        throw new Error(`the clip was ${making?.status ?? "gone"} as it was dropped, not still being made`);
      await saysMaking(app);
      await until("its making listed with the jobs, let go elsewhere", () => listed(making.id), 5000, { every: 100 });
      await until(
        "the page sent the list of jobs with it",
        () => app.js(`return window.__jobs.some((ids) => ids.includes(arguments[0]))`, making.id),
        5000,
        { every: 100 },
      );
      const answers = [];
      for (let i = 0; i < 4; i++) {
        const t0 = Date.now();
        const shown = await app.shownTime();
        answers.push({ ms: Date.now() - t0, shown });
        await sleep(1000);
      }
      const slowest = Math.max(...answers.map((a) => a.ms));
      ctx.log(
        `while its file was made: the page answered in ${answers.map((a) => a.ms).join(", ")} ms, the time shown ${answers.map((a) => a.shown).join(", ")} s`,
      );
      if (slowest > 1000) throw new Error(`the window took ${slowest} ms to answer while the drop waited`);
      if (await pill(making.id)) throw new Error("a pill showed for the clip while it was handed over");
      const first = await landed("let go before it was made");
      const end = await ended();
      if (!end.delivered) throw new Error(`the page heard the drag went nowhere: ${JSON.stringify(end)}`);
      await until("the toast gone once it landed", async () => (await app.toastSays()) !== MAKING, 10_000);
      if (await pill(making.id)) throw new Error("a pill showed for the clip once it was handed over");
      // Taken where it is (Nautilus): its file is another app's now, kept once
      // the row made for the drag goes. Copied (Dolphin): the app's goes.
      const size = sizeOf(first.file);
      await settle();
      const own = appFiles().find((f) => path.basename(f) === path.basename(first.file));
      if (copies && own) throw new Error(`the app's file ${own} stayed, though Dolphin has its own copy`);
      if (!copies && (!own || sizeOf(own) !== size))
        throw new Error(`the app's file ${path.basename(first.file)} went, though another app took it`);
      if (await listed(making.id)) throw new Error("the clip handed over was still listed with the jobs");
      ctx.log(
        `let go ${first.at - dropped} ms before the file landed (${first.length.toFixed(2)} s long); the app's file ${copies ? "gone (copied)" : "kept as another app's"}; listed with the jobs meanwhile, no pill, the toast "${MAKING}"`,
      );
      await app.click('.bar button[aria-label="Pause"]').catch(() => {});

      // 2. Out, back over the window, out again, let go: it lands.
      await toStart();
      const page = await os.windowRect(app);
      await carryOut([
        ["move", page.x + Math.round(page.w / 2), drop.y],
        ["wait", 300],
        ["move", page.x + Math.round(page.w / 2) + 20, drop.y],
        ["wait", 1000],
        ["move", drop.x, drop.y],
        ["wait", 300],
        ["move", drop.x + 10, drop.y + 5],
        ["wait", 300],
        ["letgo", drop.x + 10, drop.y + 5],
      ]);
      const again = await landed("out, back in and out again");
      if (!(await ended()).delivered) throw new Error("the page heard the drag out and back in went nowhere");
      ctx.log(`out, back over the window and out again: it landed (${again.length.toFixed(2)} s long)`);
      await settle();

      // 3. Called off (Escape) while its file is waited for: nothing lands,
      // and the app's file goes too. On macOS an Escape posted by a script
      // doesn't reach the system's drag (a real key press does, the same as
      // this): it's let go on the window's title bar, where nothing takes it.
      await toStart();
      const before = new Set(appFiles());
      const title = mac && (await os.windowRect(app));
      await carryOut(
        mac
          ? [
              ["wait", 1000],
              ["move", title.x + 100, title.y - 14],
              ["wait", 300],
              ["move", title.x + 104, title.y - 14],
              ["wait", 300],
              ["letgo", title.x + 104, title.y - 14],
            ]
          : [["wait", 1000], ["key", "Escape"], ["wait", 300], ["up"]],
      );
      const off = await ended();
      if (off.delivered) throw new Error(`the page heard a drag called off went: ${JSON.stringify(off)}`);
      await settle();
      await sleep(HELD * 1000);
      const left = appFiles().filter((f) => !before.has(f));
      if (droppedIn(dest).length || left.length)
        throw new Error(`a drag called off left ${[...droppedIn(dest), ...left].join(", ")}`);
      ctx.log("called off while its file was waited for: nothing landed, nothing left");

      // 4. Its encode fails: nothing lands, and the app says why.
      encodes("fail");
      await toStart();
      await carryOut([["letgo", drop.x + 16, drop.y + 8]]);
      const failed = await ended();
      if (failed.delivered || !failed.failed)
        throw new Error(`the page heard ${JSON.stringify(failed)} of a failed clip`);
      const toast = await saysAtLast(app, (t) => /Couldn't make the clip/.test(t));
      await settle();
      if (droppedIn(dest).length) throw new Error(`a clip that failed left ${droppedIn(dest).join(", ")}`);
      ctx.log(`its encode failed: nothing landed, the toast says "${toast}"`);

      // 5. Two pills picked and carried together (three on macOS), all made
      // again as they go (MP4 picked since they were made): all land;
      // Dolphin's and Finder's one by one, in the order picked.
      encodes("fast");
      const ranges = [[0, 60], [60, 120], ...(mac ? [[120, 170]] : [])];
      const pair = await app.invoke("add_clips", { vodId: vod.id, ranges });
      await until(
        "the two clips made",
        async () => {
          const all = await app.invoke("list_clips", { vodId: vod.id });
          return pair.every((p) => all.find((c) => c.id === p.id)?.status === "ready");
        },
        2 * MINUTE,
      );
      await setFormat(app, "MP4");
      encodes("slow");
      for (const p of pair) {
        await until(
          "its pill",
          () => app.js(`return !!document.querySelector(arguments[0])`, `.chip.ready[data-clip="${p.id}"]`),
          MINUTE,
        );
        await app.js(
          `document.querySelector('.chip[data-clip="' + arguments[0] + '"]')
             .dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, [arguments[1]]: true }));`,
          p.id,
          mac ? "metaKey" : "ctrlKey",
        );
      }
      await until(
        "all picked",
        () => app.js(`return document.querySelectorAll(".chip.chosen").length === arguments[0]`, pair.length),
        10_000,
      );
      await carryOut([["letgo", drop.x + 16, drop.y + 8]], `.chip[data-clip="${pair[0].id}"] .length`);
      const arrivals = [];
      await until(
        "all the files whole in the folder",
        () => {
          for (const f of droppedIn(dest).filter((f) => !f.endsWith(".part"))) {
            if (whole(path.join(dest, f)) && !arrivals.some((a) => a.name === f))
              arrivals.push({ name: f, at: Date.now() });
          }
          return arrivals.length === pair.length;
        },
        2 * MINUTE,
        { every: 50 },
      );
      const lengths = arrivals.map((a) => probeDuration(path.join(dest, a.name)));
      const wanted = ranges.map(([a, b]) => b - a);
      const byLength = (list) => [...list].sort((a, b) => a - b);
      if (byLength(lengths).some((l, i) => Math.abs(l - byLength(wanted)[i]) > SLACK))
        throw new Error(`the files are ${lengths.join(", ")} s long, not ${wanted.join(", ")}`);
      if (!(await ended()).delivered) throw new Error("the page heard the clips went nowhere");
      // Named for their ranges: the first's starts at 0.
      const gaps = arrivals.slice(1).map((a, i) => a.at - arrivals[i].at);
      const inTurn = arrivals.slice(1).every((a, i) => arrivals[i].name < a.name && gaps[i] > 2000);
      if (copies && !inTurn)
        throw new Error(`the copies came ${gaps.join(", ")} ms apart, ${arrivals.map((a) => a.name).join(" then ")}`);
      ctx.log(
        `${pair.length} carried together, all being made: all landed, ${gaps.join(", ")} ms apart (${arrivals.map((a) => a.name).join(", then ")})`,
      );
      await settle();

      // 6. A pill still being made (made again), dragged alone: it lands.
      await app.key("Escape");
      await until("none picked", () => app.js(`return !document.querySelector(".chip.chosen")`), 10_000);
      await app.invoke("set_clip_range", { id: pair[0].id, start: 0, end: 60 });
      await until(
        "its pill being made",
        () => app.js(`return !!document.querySelector(arguments[0])`, `.chip[data-clip="${pair[0].id}"]:not(.ready)`),
        10_000,
        { every: 50 },
      );
      await carryOut([["letgo", drop.x + 16, drop.y + 8]], `.chip[data-clip="${pair[0].id}"] .length`);
      const alone = await landed("a pill being made");
      if (!(await ended()).delivered) throw new Error("the page heard the pill being made went nowhere");
      if (!(await pill(pair[0].id))) throw new Error("the pill dragged out went from the clip bar");
      ctx.log(`a pill still being made, dragged alone: it landed (${alone.length.toFixed(2)} s long)`);
      await settle();

      if (mac) {
        // 6a. A pill already made: it lands at once.
        await until(
          "its pill, made",
          () => app.js(`return !!document.querySelector(arguments[0])`, `.chip.ready[data-clip="${pair[1].id}"]`),
          2 * MINUTE,
        );
        const let1 = Date.now();
        await carryOut([["letgo", drop.x + 16, drop.y + 8]], `.chip[data-clip="${pair[1].id}"] .length`);
        const made = await landed("a pill already made");
        if (!(await ended()).delivered) throw new Error("the page heard the pill already made went nowhere");
        ctx.log(`a pill already made: it landed ${made.at - let1} ms from the press, as the drag ended`);
        await settle();

        /** Let go on Finder before the file's made: the clip, once its empty file stands there.
         *  (The last stretch's: the first two have their pills by now.) */
        const waiting = async () => {
          await toLast();
          await carryOut([["letgo", drop.x + 16, drop.y + 8]]);
          const [clip] = await hidden();
          if (!clip || clip.status === "ready")
            throw new Error(`the clip was ${clip?.status ?? "gone"} as it was let go`);
          await until(
            "its empty file in Finder",
            () => droppedIn(dest).some((f) => sizeOf(path.join(dest, f)) === 0),
            10_000,
            { every: 50 },
          );
          return clip;
        };
        /** Nothing landed and nothing said, the clip made for the drag gone. */
        const nothing = async (label) => {
          const end = await ended();
          if (end.delivered || end.failed || end.finished_only)
            throw new Error(`${label}: the page heard ${JSON.stringify(end)}`);
          await until(`${label}: the empty file gone`, () => droppedIn(dest).length === 0, 10_000);
          await settle();
        };

        // 6b. Finder's cancel on the file being written (the button at its
        // icon's corner, the first place in the folder): nothing lands, and
        // nothing is said.
        encodes("slow");
        await waiting();
        await sleep(1500);
        const icon = nautilus.cancelAt();
        writeFileSync(path.join(work, "finder-cancel.png"), os.screenshot());
        await os.gesture([["move", icon.x, icon.y], ["wait", 300], ["click"]]);
        await nothing("Finder's cancel");
        ctx.log(`Finder's cancel at ${icon.x},${icon.y}: nothing landed, nothing said`);

        // 6c. Cancel in the list of jobs, while Finder waits for the file:
        // nothing lands, and nothing is said.
        const canceled = await waiting();
        await until("its making listed with the jobs", () => listed(canceled.id), 5000, { every: 100 }).catch(
          async (e) => {
            const clip = (await app.invoke("list_clips", { vodId: vod.id })).find((c) => c.id === canceled.id);
            throw new Error(
              `${e.message}; the jobs ${JSON.stringify(await app.invoke("list_jobs"))}, the clip ${clip?.status}`,
            );
          },
        );
        await app.invoke("cancel_job", { kind: "clip", id: canceled.id });
        await nothing("canceled in the list of jobs");
        ctx.log("canceled in the list of jobs while Finder waited: nothing landed, nothing said");
      }

      // 7. Let go on an app that takes finished files only, before the file's
      // made: nothing lands, the app says it takes finished files only, by
      // its name, and keeps the clip as a pill, which once made drags there.
      const noTaker = noFinishedOnly();
      if (noTaker) {
        ctx.log(`no app that takes finished files only to drop on: ${noTaker}`);
        return;
      }
      await nautilus.close();
      nautilus = null;
      if (entry) {
        mkdirSync(path.dirname(entry), { recursive: true });
        writeFileSync(
          entry,
          "[Desktop Entry]\nType=Application\nName=Clip Taker\nExec=true\nStartupWMClass=Clip-taker\n",
        );
      }
      taker = await openFinishedOnly(app, ctx, path.join(work, "taker"));
      // The last stretch, past the second cut: no clip of it yet.
      await toLast();
      await carryOut([["letgo", taker.drop.x + 16, taker.drop.y + 8]], "button.action.clip", taker.drop);
      const [kept] = await hidden();
      if (!kept || kept.status === "ready") throw new Error(`the clip was ${kept?.status ?? "gone"} as it was let go`);
      const refused = await ended();
      if (!refused.finished_only || refused.receiver !== takerName)
        throw new Error(`the page heard ${JSON.stringify(refused)} of a drop on an app that takes finished files only`);
      const want = `${takerName} takes finished files only. The clip is being made; drag it from the clip bar when it's ready.`;
      const said = await saysAtLast(app, (t) => t === want);
      // On Linux it asked and waited, giving up after 5 s: no telling that
      // beforehand, so it's said to be made meanwhile, then this. On macOS
      // it never asks for the promise: nothing said of the making.
      if (mac) await saidNoMaking(app, "on an app that takes finished files only");
      else if ((await toastsSaid(app)).at(-1) !== want)
        throw new Error(`the toasts said ${JSON.stringify(await toastsSaid(app))}, not "${want}" last`);
      // (Qt's give up waiting; on macOS it gets no file at all.)
      if (!taker.said().some((l) => (mac ? / took 0 of /.test(l) : l.endsWith("gave up"))))
        throw new Error(`the app that takes finished files only says ${JSON.stringify(taker.said())}`);
      await until(
        `its pill, made`,
        () => app.js(`return !!document.querySelector(arguments[0])`, `.chip.ready[data-clip="${kept.id}"]`),
        2 * MINUTE,
      );
      await carryOut(
        [["letgo", taker.drop.x + 16, taker.drop.y + 8]],
        `.chip[data-clip="${kept.id}"] .length`,
        taker.drop,
      );
      if (!(await ended()).delivered) throw new Error("the page heard the pill made went nowhere");
      await saidNoMaking(app, "made");
      const took = await until(
        "the app took it",
        () => taker.said().find((l) => / took 1 of 1: /.test(l)) ?? false,
        10_000,
      );
      ctx.log(
        `let go on an app that takes finished files only: nothing taken, the toast says "${said}"; its pill, once made, dragged there: ${took.split(" ").slice(1, 5).join(" ")}`,
      );

      // 8. On macOS, let go on an app that refuses the drop, before the
      // file's made (as VLC does): the same, by that app's name, and the
      // clip kept as a pill. (The system says nothing of the refusal: the
      // app tells it from Escape by the other app's window under the
      // pointer.) A fresh stretch, past a cut at 160 s.
      if (!mac) return;
      taker.close();
      taker = await openFinishedOnly(app, ctx, path.join(work, "refuser"), { refuses: true });
      // (Its window came to the front: the keys go to the app again.)
      await frontmost(app);
      await toTime(app, 160);
      const marks = (await app.invoke("list_marks", { vodId: vod.id })).length;
      await app.key("b");
      await until("the cut", async () => (await app.invoke("list_marks", { vodId: vod.id })).length > marks, 10_000);
      await app.key("Escape");
      await toTime(app, 170);
      await until("the last toast gone", async () => !(await app.toastSays()), 10_000);
      await carryOut([["letgo", taker.drop.x + 16, taker.drop.y + 8]], "button.action.clip", taker.drop);
      const refusedNow = await ended();
      if (!refusedNow.finished_only || refusedNow.receiver !== takerName)
        throw new Error(`the page heard ${JSON.stringify(refusedNow)} of a drop an app refused`);
      const saidNow = await saysAtLast(app, (t) => t === want);
      await saidNoMaking(app, "on an app that refused it");
      if (!taker.said().some((l) => l.endsWith(" refused")))
        throw new Error(`the app says ${JSON.stringify(taker.said())}`);
      const [keptNow] = (await app.invoke("list_clips", { vodId: vod.id })).sort((a, b) => b.id - a.id);
      if (!keptNow || keptNow.id === kept.id) throw new Error("no clip kept of the drop refused");
      await until("its pill", () => pill(keptNow.id), 10_000);
      ctx.log(`let go on an app that refuses the drop: the toast says "${saidNow}", the clip kept as a pill`);
    } finally {
      taker?.close();
      if (entry) rmSync(entry, { force: true });
      // Awaited: on macOS it brings the app back to the front, and a press
      // left in flight after the app is quit ends the whole run. Its own
      // failure is told, not put in place of the step's.
      try {
        await nautilus?.close();
      } catch (e) {
        ctx.log(`the file manager not closed: ${e.message}`);
      }
      rmSync(ffmpeg, { force: true });
      if (existsSync(aside)) renameSync(aside, ffmpeg);
    }
  },
};

/** The files under `dir`, by their paths. */
function filesUnder(dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? filesUnder(path.join(dir, e.name)) : [path.join(dir, e.name)],
  );
}

/** File `file`'s size, or -1 if it isn't there (gone as it was looked at:
 *  a file manager may write a file under another name and rename it). */
function sizeOf(file) {
  try {
    return statSync(file).size;
  } catch {
    return -1;
  }
}

/** File `file`'s last change (ms), or 0 if it isn't there. */
function mtimeOf(file) {
  try {
    return statSync(file).mtimeMs;
  } catch {
    return 0;
  }
}

/** The ffmpeg stand-in on Windows (C#, built by PowerShell): as the shell one does on Linux, by `mode`. */
const STAND_IN = (
  real,
  mode,
  held,
) => `using System; using System.Diagnostics; using System.IO; using System.Linq; using System.Text; using System.Threading;
static class StandIn {
  static string Quote(string a) {
    if (a.Length > 0 && a.IndexOfAny(new[] { ' ', '\\t', '"' }) < 0) return a;
    var s = new StringBuilder("\\""); int slashes = 0;
    foreach (char c in a) {
      if (c == '\\\\') { slashes++; continue; }
      if (c == '"') { s.Append('\\\\', slashes * 2 + 1); s.Append('"'); slashes = 0; continue; }
      s.Append('\\\\', slashes); slashes = 0; s.Append(c);
    }
    s.Append('\\\\', slashes * 2); s.Append('"'); return s.ToString();
  }
  static int Main(string[] args) {
    string how = ""; try { how = File.ReadAllText(@"${mode}").Trim(); } catch {}
    string part = args.FirstOrDefault(a => a.EndsWith(".part"));
    if (part != null) {
      if (how == "fail") { Thread.Sleep(${held * 500}); Console.Error.WriteLine("Made to fail by the test."); return 1; }
      if (how == "slow") Thread.Sleep(${held * 1000});
    }
    var p = Process.Start(new ProcessStartInfo(@"${real}", string.Join(" ", args.Select(Quote))) { UseShellExecute = false });
    p.WaitForExit(); return p.ExitCode;
  }
}`;

/**
 * Windows: the drag is OLE's, each clip offered as a virtual file (Explorer
 * takes that, copying it in once it's made, its own copy dialog up
 * meanwhile) and as itself (an app that takes real files only gets it only
 * if it's made by the drop). On a File Explorer window beside the app's:
 * let go before the file is made, it lands once it is, the window answering
 * meanwhile, the copy dialog up and the app saying it's being made; made
 * before the drop, it lands at once, nothing said;
 * called off (Escape), nothing lands; its encode failing, nothing lands,
 * Explorer says nothing and the app says why; canceled from the list of jobs
 * while Explorer waits, nothing lands and nobody says anything; three
 * pills picked and carried together, being made again, all land, one by one
 * in their order. On an app that takes real files only (Windows PowerShell's
 * form), before the file's made: nothing lands, the app says Windows
 * PowerShell takes finished files only (never that it's being made) and
 * keeps the clip as a pill, which once made drags there and is taken; let go
 * on one that refuses the drop (the same form, told to), the same. Each step logs the app's own lines
 * about the drag (an e2e build's).
 */
async function onWindows(app, ctx) {
  const why = os.unavailable();
  if (why) ctx.skip(why);
  const explorer = await import("../lib/explorer.mjs");
  const work = path.join(ctx.out, "drag-promise");
  rmSync(work, { recursive: true, force: true });
  const dest = path.join(work, "drop-here");
  mkdirSync(dest, { recursive: true });

  // The stand-in for the app's ffmpeg (its bin is kept between scenarios:
  // one there is moved aside, and back after).
  const bin = path.join(dataDirs().at(-1), "bin");
  mkdirSync(bin, { recursive: true });
  const ffmpeg = path.join(bin, "ffmpeg.exe");
  const aside = path.join(bin, "ffmpeg.e2e-real.exe");
  // (One already aside is the real one: what's in its place is a stand-in a
  // run that stopped short left there. Stand-ins put out of the way while
  // still running go now.)
  for (const f of readdirSync(bin).filter((f) => f.startsWith("ffmpeg.e2e-old-")))
    rmSync(path.join(bin, f), { force: true });
  if (existsSync(aside)) rmSync(ffmpeg, { force: true });
  else if (existsSync(ffmpeg)) renameSync(ffmpeg, aside);
  const real = existsSync(aside)
    ? aside
    : execFileSync("where.exe", ["ffmpeg"], { encoding: "utf8" }).split(/\r?\n/)[0].trim();
  const mode = path.join(work, "ffmpeg-mode");
  const encodes = (how) => writeFileSync(mode, how);
  const source = path.join(work, "stand-in.cs");
  writeFileSync(source, STAND_IN(real, mode, HELD));
  execFileSync("powershell.exe", [
    ...["-NoProfile", "-NonInteractive", "-Command"],
    `Add-Type -TypeDefinition (Get-Content -Raw '${source}') -OutputAssembly '${ffmpeg}' -OutputType ConsoleApplication`,
  ]);
  encodes("slow");
  // The clips folder (in the local data), as it was: what the test adds there goes after.
  const clips = clipsDir();
  const appFiles = () =>
    [...filesUnder(clips), ...filesUnder(dataDirs()[0])].filter((f) => /\.(mov|mp4)(\.part)?$/.test(f));
  const had = new Set(appFiles());
  let target = null;
  let taker = null;
  try {
    await app.toLibrary();
    const [vod] = await app.addFiles([longVideo(path.join(ctx.out, "fixtures"))]);
    await until("the video read", async () => (await app.invoke("list_vods"))[0]?.duration > 0, 5 * MINUTE);
    await setFormat(app, "MOV");
    // The app on the left of the screen, Explorer on the right.
    const screen = explorer.screenSize();
    const width = Math.round(screen.w * 0.62);
    explorer.placeWindowOf(app.driver.proc.pid, { x: 0, y: 0, w: width, h: screen.h - 48 });
    const beside = { x: width + 8, y: 40, w: screen.w - width - 16, h: Math.round(screen.h * 0.7) };
    await app.openPaused(vod.id);
    // Cuts at 10, 20 and 30 s: stretches of ten seconds.
    for (const time of [10, 20, 30]) {
      await toTime(app, time);
      const marks = (await app.invoke("list_marks", { vodId: vod.id })).length;
      await app.key("b");
      await until("the cut", async () => (await app.invoke("list_marks", { vodId: vod.id })).length > marks, 10_000);
      await app.key("Escape");
    }
    // The stretches between the cuts, as made (the playhead lands within a second of each).
    const cuts = [0, ...(await app.invoke("list_marks", { vodId: vod.id })).map((m) => m.start).sort((a, b) => a - b)];
    const stretchFrom = (start) => cuts[cuts.findIndex((c) => c > start + 0.01)] - start;
    const stretchAt = (time) => {
      const start = Math.max(...cuts.filter((c) => c <= time));
      return stretchFrom(start);
    };
    target = await explorer.openExplorer(dest, beside);
    await app.js(
      `window.__ended = [];
       window.__jobs = [];
       const internals = window.__TAURI_INTERNALS__;
       const handler = internals.transformCallback((e) => window.__ended.push({ ...e.payload, at: Date.now() }));
       await internals.invoke("plugin:event|listen", { event: "drag-ended", target: { kind: "Any" }, handler });
       const jobs = internals.transformCallback((e) => window.__jobs.push(e.payload.map((j) => j.id)));
       await internals.invoke("plugin:event|listen", { event: "jobs", target: { kind: "Any" }, handler: jobs });`,
    );
    const centre = async (selector) => {
      const r = await os.rectOf(app, selector);
      return { x: r.x + Math.round(r.w / 2), y: r.y + Math.round(r.h / 2) };
    };
    const hidden = async () => (await app.invoke("list_clips", { vodId: vod.id })).filter((c) => !c.shown);
    const pill = (id) => app.js(`return !!document.querySelector(arguments[0])`, `.chip[data-clip="${id}"]`);
    const listed = async (id) => (await app.invoke("list_jobs")).some((j) => j.id === id);
    const ended = () => until("the system's drag, over", () => app.js(`return window.__ended[0] ?? false`), 2 * MINUTE);
    // The app's lines about each drag (an e2e build's), from where the last step's ended.
    let read = 0;
    const lines = () => {
      const all = (app.driver.proc.stderr ?? "").split(/\r?\n/).filter((l) => l.startsWith("drag: "));
      const fresh = all.slice(read);
      read = all.length;
      return fresh;
    };
    const say = (label) => ctx.log(`${label}, the app's lines:\n    ${lines().join("\n    ") || "(none)"}`);
    const droppedHere = () => readdirSync(dest);
    const settle = async () => {
      await until("the carry over", () => app.js(`return !document.querySelector(".carry-ghost")`), MINUTE);
      await until("no clip left made for a drag", async () => (await hidden()).length === 0, 30_000);
      await sleep(1000);
      explorer.closeExplorerDialogs();
      for (const f of droppedHere()) rmSync(path.join(dest, f), { force: true });
    };
    let shots = 0;
    /**
     * Clip (or `what`) pressed and carried over the window's edge to `to`, in
     * one gesture (the page can't be asked anything while the system's drag
     * is up); there, `then` (letting go, say). A screenshot of each as it's
     * over the drop (drag-promise-<n>.png): the drag's picture, drawn by the
     * shell, isn't a window of the app's to look for. The app's window in
     * front first: the app a drop went to comes to the front, with the keys.
     */
    const carryOut = async (then, what = "button.action.clip", to = target.drop) => {
      await os.toFront(app);
      await app.js(`performance.clearMarks(); window.__ended = []; ${HEAR_TOASTS}`);
      const from = await centre(what);
      const steps = [
        ["down", from.x, from.y],
        ["wait", 100],
      ];
      for (let i = 1; i <= 18; i++)
        steps.push(
          ["move", Math.round(from.x + (to.x - from.x) * (i / 18)), Math.round(from.y + (to.y - from.y) * (i / 18))],
          ["wait", 50],
        );
      steps.push(
        ["move", to.x + 8, to.y + 4],
        ["wait", 300],
        ["shot", path.join(ctx.out, `drag-promise-${++shots}.png`)],
      );
      steps.push(["move", to.x + 16, to.y + 8], ["wait", 300], ...then);
      await os.gesture(steps);
    };
    /** The files dropped, once Explorer has written them. It makes each at
     *  its full length at once and fills it in from there, the movie's index
     *  (written last) still missing, so a file there isn't whole until its
     *  copies are over: they end the drag (EndOperation). */
    const landed = async (label, count = 1) => {
      const got = await until(
        `${label}: ${count} file${count > 1 ? "s" : ""} in the folder`,
        () => droppedHere().length >= count && droppedHere(),
        2 * MINUTE,
        { every: 100 },
      );
      await ended();
      return got.map((f) => ({ name: f, length: probeDuration(path.join(dest, f)) }));
    };

    // 1. Let go on Explorer before the file is made: Explorer's copy dialog
    // is up while it's made, the window answers meanwhile, the app says it's
    // being made, the clip's making is listed with the jobs, and the file
    // lands whole once made. The app's own file goes (Explorer has its copy).
    await toTime(app, 0);
    await carryOut([["letgo", target.drop.x + 16, target.drop.y + 8]]);
    const dropped = Date.now();
    const [making] = await hidden();
    if (!making || making.status === "ready")
      throw new Error(`the clip was ${making?.status ?? "gone"} as it was dropped, not still being made`);
    await saysMaking(app);
    await until("its making listed with the jobs, let go elsewhere", () => listed(making.id), 5000, { every: 100 });
    const copying = await until(
      "Explorer's copy dialog, while the file is made",
      () => explorer.explorerDialogs().find((d) => /cop/i.test(`${d.title} ${d.texts.join(" ")}`)) ?? false,
      HELD * 1000,
      { every: 200 },
    );
    const answers = [];
    for (let i = 0; i < 3; i++) {
      const t0 = Date.now();
      await app.shownTime();
      answers.push(Date.now() - t0);
      await sleep(500);
    }
    if (Math.max(...answers) > 1000) throw new Error(`the window took ${Math.max(...answers)} ms to answer`);
    const end = await ended();
    if (!end.delivered) throw new Error(`the page heard the drag went nowhere: ${JSON.stringify(end)}`);
    const [first] = await landed("let go before it was made");
    await until("the toast gone once it landed", async () => (await app.toastSays()) !== MAKING, 10_000);
    if (Math.abs(first.length - stretchAt(0)) > SLACK)
      throw new Error(`the file dropped is ${first.length} s long, not ${stretchAt(0)} s`);
    await settle();
    if (appFiles().some((f) => path.basename(f) === first.name))
      throw new Error(`the app's file ${first.name} stayed, though Explorer has its own copy`);
    ctx.log(
      `let go before it was made: Explorer's "${copying.title}" up meanwhile, the page answering in ${answers.join(", ")} ms; landed ${end.at - dropped} ms after the drop (${first.name}, ${first.length.toFixed(2)} s)`,
    );
    say("1");

    // 2. Made before the drop (held over Explorer while it's made): it lands at once.
    encodes("fast");
    await toTime(app, 12);
    await carryOut([
      ["wait", 6000],
      ["letgo", target.drop.x + 16, target.drop.y + 8],
    ]);
    const quick = Date.now();
    await landed("made before the drop");
    const atOnce = Date.now() - quick;
    if (!(await ended()).delivered) throw new Error("the page heard the made clip went nowhere");
    await saidNoMaking(app, "made");
    await settle();
    ctx.log(`made before the drop: landed ${atOnce} ms after it, nothing said`);
    say("2");

    // 3. Called off (Escape) while its file is made: nothing lands, nothing left.
    encodes("slow");
    await toTime(app, 0);
    const before = new Set(appFiles());
    await carryOut([["wait", 1000], ["key", "Escape"], ["wait", 300], ["up"]]);
    const off = await ended();
    if (off.delivered) throw new Error(`the page heard a drag called off went: ${JSON.stringify(off)}`);
    await settle();
    await sleep(HELD * 1000);
    const left = appFiles().filter((f) => !before.has(f));
    if (droppedHere().length || left.length) throw new Error(`a drag called off left ${[...droppedHere(), ...left]}`);
    ctx.log("called off while its file was made: nothing landed, nothing left");
    say("3");

    // 4. Its encode fails: nothing lands, Explorer says nothing, the app says why.
    encodes("fail");
    await toTime(app, 0);
    await carryOut([["letgo", target.drop.x + 16, target.drop.y + 8]]);
    const failed = await ended();
    if (failed.delivered || !failed.failed)
      throw new Error(`the page heard ${JSON.stringify(failed)} of a failed clip`);
    const toast = await saysAtLast(app, (t) => /Couldn't make the clip/.test(t));
    await sleep(1500);
    const errors = explorer.explorerDialogs();
    if (errors.length) throw new Error(`Explorer put up ${JSON.stringify(errors)} for a clip that failed`);
    await settle();
    if (droppedHere().length) throw new Error(`a clip that failed left ${droppedHere()}`);
    ctx.log(`its encode failed: nothing landed, no dialog of Explorer's, the toast says "${toast}"`);
    say("4");

    // 5. Canceled from the list of jobs while Explorer waits for it: its
    // dialog goes, nothing lands, and nothing's said.
    encodes("slow");
    await toTime(app, 0);
    await carryOut([["letgo", target.drop.x + 16, target.drop.y + 8]]);
    const [waited] = await hidden();
    await until("its making listed with the jobs", () => listed(waited.id), 5000, { every: 100 });
    const job = (await app.invoke("list_jobs")).find((j) => j.id === waited.id);
    // (What the list's Cancel does: Jobs.svelte.)
    await app.invoke("cancel_job", { kind: job.kind, id: job.id });
    const canceled = await ended();
    if (canceled.delivered || canceled.failed || canceled.finished_only)
      throw new Error(`the page heard ${JSON.stringify(canceled)} of a clip canceled`);
    await sleep(1500);
    const still = explorer.explorerDialogs();
    if (still.length) throw new Error(`Explorer still shows ${JSON.stringify(still)} after the cancel`);
    if (await app.toastSays()) throw new Error(`the toast says "${await app.toastSays()}" of a cancel`);
    await settle();
    if (droppedHere().length) throw new Error(`a clip canceled left ${droppedHere()}`);
    ctx.log("canceled from the list of jobs while Explorer waited: its dialog gone, nothing landed, nothing said");
    say("5");

    // 6. Three pills picked and carried together, all being made again (MP4
    // picked since they were made): all land, one by one, in their order.
    encodes("fast");
    const three = await app.invoke("add_clips", {
      vodId: vod.id,
      ranges: [
        [0, 10],
        [10, 20],
        [20, 30],
      ],
    });
    await until(
      "the three made",
      async () => {
        const all = await app.invoke("list_clips", { vodId: vod.id });
        return three.every((p) => all.find((c) => c.id === p.id)?.status === "ready");
      },
      2 * MINUTE,
    );
    await os.toFront(app);
    await setFormat(app, "MP4");
    encodes("slow");
    for (const p of three) {
      await until(
        "its pill",
        () => app.js(`return !!document.querySelector(arguments[0])`, `.chip[data-clip="${p.id}"]`),
        MINUTE,
      );
      await app.js(
        `document.querySelector('.chip[data-clip="' + arguments[0] + '"]')
           .dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, ctrlKey: true }));`,
        p.id,
      );
    }
    await until(
      "all three picked",
      () => app.js(`return document.querySelectorAll(".chip.chosen").length === 3`),
      10_000,
    );
    await carryOut([["letgo", target.drop.x + 16, target.drop.y + 8]], `.chip[data-clip="${three[0].id}"] .length`);
    const arrivals = [];
    await until(
      "the three files in the folder",
      () => {
        for (const f of droppedHere())
          if (!arrivals.some((a) => a.name === f)) arrivals.push({ name: f, at: Date.now() });
        return arrivals.length === 3;
      },
      3 * MINUTE,
      { every: 100 },
    );
    if (!(await ended()).delivered) throw new Error("the page heard the three went nowhere");
    const lengths = arrivals.map((a) => probeDuration(path.join(dest, a.name)));
    if (lengths.some((l) => Math.abs(l - 10) > SLACK)) throw new Error(`the three are ${lengths} s long, not 10`);
    const names = arrivals.map((a) => a.name);
    if (names.join() !== [...names].sort().join()) throw new Error(`the three came in the order ${names.join(", ")}`);
    ctx.log(
      `three carried together, all being made: landed ${arrivals.map((a) => a.at - arrivals[0].at).join(", ")} ms apart, in order (${names.join(", ")})`,
    );
    await settle();
    say("6");
    await os.toFront(app);
    await app.key("Escape");
    // (Off the clip bar again, so the next pill shows in its width.)
    for (const p of three) await app.invoke("delete_clip", { id: p.id });

    // 7. Let go on an app that takes real files only, before the file's made:
    // nothing lands, the app says that app takes finished files only, by the
    // name its program gives itself, and keeps the clip as a pill, which once
    // made drags there and is taken.
    target.close();
    target = null;
    taker = await explorer.openFilesOnly(work, beside);
    await toTime(app, 40);
    await carryOut([["letgo", taker.drop.x + 16, taker.drop.y + 8]], "button.action.clip", taker.drop);
    // (The newest clip: kept as a pill as soon as the drag's over, which is at once here.)
    const [kept] = (await app.invoke("list_clips", { vodId: vod.id })).sort((a, b) => b.id - a.id);
    if (!kept || kept.status === "ready") throw new Error(`the clip was ${kept?.status ?? "gone"} as it was let go`);
    const refused = await ended();
    const takerName = "Windows PowerShell";
    if (!refused.finished_only || refused.receiver !== takerName)
      throw new Error(`the page heard ${JSON.stringify(refused)} of a drop on an app that takes finished files only`);
    const want = `${takerName} takes finished files only. The clip is being made; drag it from the clip bar when it's ready.`;
    const said = await saysAtLast(app, (t) => t === want);
    await saidNoMaking(app, "on an app that takes finished files only");
    if (!taker.said().includes("gave nothing")) throw new Error(`the app says ${JSON.stringify(taker.said())}`);
    say("7, before it was made");
    await until(
      "its pill, made",
      () => app.js(`return !!document.querySelector(arguments[0])`, `.chip.ready[data-clip="${kept.id}"]`),
      2 * MINUTE,
    );
    await carryOut(
      [["letgo", taker.drop.x + 16, taker.drop.y + 8]],
      `.chip[data-clip="${kept.id}"] .length`,
      taker.drop,
    );
    if (!(await ended()).delivered) throw new Error("the page heard the pill made went nowhere");
    await saidNoMaking(app, "made");
    const took = await until("the app took it", () => taker.said().find((l) => l.startsWith("took ")) ?? false, 10_000);
    ctx.log(
      `let go on an app that takes real files only: nothing taken, the toast says "${said}"; its pill, once made, dragged there: ${took}`,
    );
    say("7, once made");

    // 8. Let go on an app that refuses the drop, before the file's made (as
    // CapCut's timeline does): the same, by that app's name, and the clip
    // kept as a pill. A fresh stretch, past a cut at 50 s.
    taker.close();
    const refuserWork = path.join(work, "refuser");
    mkdirSync(refuserWork, { recursive: true });
    taker = await explorer.openFilesOnly(refuserWork, beside, { refuses: true });
    // (The form came to the front: the keys go to the app again.)
    await os.toFront(app);
    await toTime(app, 50);
    const marks = (await app.invoke("list_marks", { vodId: vod.id })).length;
    await app.key("b");
    await until("the cut", async () => (await app.invoke("list_marks", { vodId: vod.id })).length > marks, 10_000);
    await app.key("Escape");
    await toTime(app, 55);
    await until("the last toast gone", async () => !(await app.toastSays()), 10_000);
    await carryOut([["letgo", taker.drop.x + 16, taker.drop.y + 8]], "button.action.clip", taker.drop);
    const refusedNow = await ended();
    if (!refusedNow.finished_only || refusedNow.receiver !== takerName)
      throw new Error(`the page heard ${JSON.stringify(refusedNow)} of a drop an app refused`);
    const saidNow = await saysAtLast(app, (t) => t === want);
    await saidNoMaking(app, "on an app that refused it");
    if (!taker.said().includes("refused")) throw new Error(`the app says ${JSON.stringify(taker.said())}`);
    const [keptNow] = (await app.invoke("list_clips", { vodId: vod.id })).sort((a, b) => b.id - a.id);
    if (!keptNow || keptNow.id === kept.id) throw new Error("no clip kept of the drop refused");
    await until("its pill", () => pill(keptNow.id), 10_000);
    ctx.log(`let go on an app that refuses the drop: the toast says "${saidNow}", the clip kept as a pill`);
    say("8");
  } finally {
    // Cleaned up without hiding why the steps failed, if they did.
    try {
      // A clip Explorer still waits for, canceled: it closes its dialog with no
      // word (with the app gone instead, it would say the call failed).
      for (const job of await app.invoke("list_jobs").catch(() => []))
        await app.invoke("cancel_job", { kind: job.kind, id: job.id }).catch(() => {});
      await sleep(1500);
      taker?.close();
      target?.close();
      explorer.closeExplorerDialogs();
      // The stand-in passes everything on from now. Still running (the app's
      // own ffmpeg may be busy a while), it can't be deleted, only moved out
      // of the way: the next run deletes it.
      encodes("fast");
      if (existsSync(ffmpeg)) renameSync(ffmpeg, path.join(bin, `ffmpeg.e2e-old-${Date.now()}.exe`));
      if (existsSync(aside)) renameSync(aside, ffmpeg);
      for (const f of appFiles()) if (!had.has(f)) rmSync(f, { force: true });
    } catch (e) {
      ctx.log(`not all cleaned up: ${e.message}`);
    }
  }
}

/** Puts the playhead at `time` s, as a press on the timeline there does. The
 *  press lands on a whole pixel, which on a narrow timeline (a small screen's,
 *  Finder beside it) is more than a second of a three-minute video: the
 *  playhead is looked for at the time of that pixel. */
async function toTime(app, time) {
  const at = await app.js(
    `${AT}
     const bar = document.querySelector(".scrubber");
     const box = bar.getBoundingClientRect();
     const [start, span] = [Number(bar.dataset.viewStart), Number(bar.dataset.span)];
     const x = Math.round(box.left + 1 + ((arguments[0] - start) / span) * (box.width - 2));
     const y = box.top + box.height / 2;
     bar.dispatchEvent(new PointerEvent("pointerdown", at(x, y, 1)));
     window.dispatchEvent(new PointerEvent("pointerup", at(x, y, 0)));
     return start + ((x - box.left) / box.width) * span;`,
    time,
  );
  await until(`the playhead at ${time} s`, async () => Math.abs((await app.shownTime()) - at) <= 1, 10_000);
}
