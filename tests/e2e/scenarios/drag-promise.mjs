// A clip dragged out of the window before its file is made: the system's
// drag begins at the edge at once, and the file lands where it's dropped
// once it's made, in the session's file manager (lib/fileManager.mjs), on
// the test's own display and bus: Nautilus waits for the drop's answer;
// Dolphin, in a KDE session, takes KDE's promise and the app copies the
// file in once it's made. The clip's making is held up on purpose (the
// app's ffmpeg is a stand-in that waits before a clip's encode, then runs
// the real one), so each drop comes before its file is there. A minute's
// stretch of a 720p video, made as MOV, carried by Clip to the folder and
// let go: it lands whole, the window answers meanwhile, and the page hears
// it went; the app's own file is kept as another app's (Nautilus took it
// where it is) or goes (Dolphin has its own copy). Carried out, back over
// the window, out again and let go: it lands too. Called off (Escape)
// while the file is waited for: nothing lands, and the app's file goes too.
// One whose encode fails: nothing lands, and the app says it couldn't make
// the clip. Two pills picked and carried together, both being made again
// (in the other format, picked meanwhile): both land, Dolphin's one by one
// in their order. Linux (X11) only, so far.

import { chmodSync, existsSync, mkdirSync, readdirSync, renameSync, rmSync, statSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "../lib/app.mjs";
import { dataDirs } from "../lib/data.mjs";
import {
  fileManager,
  noFileManager,
  openFileManager,
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

export default {
  name: "drag-promise",
  async run(app, ctx) {
    const why = noFileManager();
    if (why) ctx.skip(why);
    // Dolphin copies the files in (KDE's promise); Nautilus takes ours where they are.
    const copies = fileManager() === "dolphin";
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
# A clip's encode writes a .part file: held up, or made to fail (drag-promise).
for a in "$@"; do
  case "$a" in
    *.part)
      case "$(cat '${mode}' 2>/dev/null)" in
        fail) sleep ${HELD / 2}; echo "Made to fail by the test." >&2; exit 1 ;;
        slow) sleep ${HELD} ;;
      esac ;;
  esac
done
exec '${real}' "$@"
`,
    );
    chmodSync(ffmpeg, 0o755);
    encodes("slow");
    let nautilus = null;
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
      // The app's clip files (where they are: the Videos folder, or its data's).
      const appFiles = () => filesUnder(dataDirs()[0]).filter((f) => /\.(mov|mp4)(\.part)?$/.test(f));
      /** Whether `file` dropped is whole: Dolphin's copy is renamed into place
       *  once written; Nautilus copies ours in as it goes, so it's whole at our size. */
      const whole = (file) => {
        if (copies) return existsSync(file);
        const own = appFiles().find((f) => path.basename(f) === path.basename(file));
        return !!own && sizeOf(file) === sizeOf(own);
      };

      nautilus = await openFileManager(app, ctx, dest, path.join(work, "home"));
      const { drop } = nautilus;
      // What the page hears of the system's drag's end, and when.
      await app.js(
        `window.__ended = [];
         const internals = window.__TAURI_INTERNALS__;
         const handler = internals.transformCallback((e) => window.__ended.push({ ...e.payload, at: Date.now() }));
         await internals.invoke("plugin:event|listen", { event: "drag-ended", target: { kind: "Any" }, handler });`,
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
      /** The clips made only for a drag (none in the clip bar). */
      const hidden = async () => (await app.invoke("list_clips", { vodId: vod.id })).filter((c) => !c.shown);
      /** Clip (or `what`) pressed and carried to over the folder, the system's drag up; then `then`. */
      const carryOut = async (then = [], what = "button.action.clip") => {
        await app.js(`performance.clearMarks(); window.__ended = [];`);
        const from = await centre(what);
        const before = new Set(windowsOf(app.driver.proc.pid));
        const steps = [
          ["down", from.x, from.y],
          ["wait", 100],
        ];
        for (let i = 1; i <= 18; i++)
          steps.push(
            [
              "move",
              Math.round(from.x + (drop.x - from.x) * (i / 18)),
              Math.round(from.y + (drop.y - from.y) * (i / 18)),
            ],
            ["wait", 50],
          );
        await os.gesture(steps);
        await until(
          "the system's drag",
          () => app.js(`return performance.getEntriesByName("drag-start").length > 0`),
          10_000,
          { every: 50 },
        );
        await until("its picture", () => windowsOf(app.driver.proc.pid).some((w) => !before.has(w)), 10_000, {
          every: 25,
        });
        await os.gesture([
          ["move", drop.x + 8, drop.y + 4],
          ["wait", 200],
          ["move", drop.x + 16, drop.y + 8],
          ["wait", 300],
          ...then,
        ]);
      };
      const ended = () =>
        until("the system's drag, over", () => app.js(`return window.__ended[0] ?? false`), 2 * MINUTE);
      const settle = async () => {
        await until("the carry over", () => app.js(`return !document.querySelector(".carry-ghost")`), MINUTE);
        await until("no clip left made for a drag", async () => (await hidden()).length === 0, 30_000);
        await sleep(1000);
        for (const f of readdirSync(dest)) rmSync(path.join(dest, f), { force: true });
      };
      /** The file dropped, once whole: as long as its stretch, and when. */
      const landed = async (label) => {
        const [file, at] = await until(
          `${label}: the file whole in the folder`,
          () => {
            // Whole: in place under its own name (Dolphin's copy is written beside, then renamed).
            const there = readdirSync(dest).filter((f) => !f.startsWith(".") && !f.endsWith(".part"));
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

      // 1. Let go on Nautilus before the file is made: it lands whole. The
      // window answers while it's waited for (the video plays on).
      await toStart();
      await app.click('.bar button[aria-label="Play"]');
      await until("the video playing", async () => (await app.shownTime()) >= 2, 30_000);
      await carryOut();
      const dropped = Date.now();
      await os.gesture([["letgo", drop.x + 16, drop.y + 8]]);
      const [making] = await hidden();
      if (!making || making.status === "ready")
        throw new Error(`the clip was ${making?.status ?? "gone"} as it was dropped, not still being made`);
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
      const first = await landed("let go before it was made");
      const end = await ended();
      if (!end.delivered) throw new Error(`the page heard the drag went nowhere: ${JSON.stringify(end)}`);
      // Taken where it is (Nautilus): its file is another app's now, kept once
      // the row made for the drag goes. Copied (Dolphin): the app's goes.
      const size = sizeOf(first.file);
      await settle();
      const own = appFiles().find((f) => path.basename(f) === path.basename(first.file));
      if (copies && own) throw new Error(`the app's file ${own} stayed, though Dolphin has its own copy`);
      if (!copies && (!own || sizeOf(own) !== size))
        throw new Error(`the app's file ${path.basename(first.file)} went, though another app took it`);
      ctx.log(
        `let go ${first.at - dropped} ms before the file landed (${first.length.toFixed(2)} s long); the app's file ${copies ? "gone (copied)" : "kept as another app's"}`,
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
      // and the app's file goes too.
      await toStart();
      const before = new Set(appFiles());
      await carryOut([["wait", 1000], ["key", "Escape"], ["wait", 300], ["up"]]);
      const off = await ended();
      if (off.delivered) throw new Error(`the page heard a drag called off went: ${JSON.stringify(off)}`);
      await settle();
      await sleep(HELD * 1000);
      const left = appFiles().filter((f) => !before.has(f));
      if (readdirSync(dest).length || left.length)
        throw new Error(`a drag called off left ${[...readdirSync(dest), ...left].join(", ")}`);
      ctx.log("called off while its file was waited for: nothing landed, nothing left");

      // 4. Its encode fails: nothing lands, and the app says why.
      encodes("fail");
      await toStart();
      await carryOut([["letgo", drop.x + 16, drop.y + 8]]);
      const failed = await ended();
      if (failed.delivered || !failed.failed)
        throw new Error(`the page heard ${JSON.stringify(failed)} of a failed clip`);
      const toast = await until("the toast", async () => (await app.toastSays()) || false, 10_000);
      if (!/Couldn't make the clip/.test(toast)) throw new Error(`the toast says "${toast}"`);
      await settle();
      if (readdirSync(dest).length) throw new Error(`a clip that failed left ${readdirSync(dest).join(", ")}`);
      ctx.log(`its encode failed: nothing landed, the toast says "${toast}"`);

      // 5. Two pills picked and carried together, both made again as they go
      // (MP4 picked since they were made): both land; Dolphin's one by one,
      // in the order picked.
      encodes("fast");
      const pair = await app.invoke("add_clips", {
        vodId: vod.id,
        ranges: [
          [0, 60],
          [60, 120],
        ],
      });
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
             .dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, ctrlKey: true }));`,
          p.id,
        );
      }
      await until("both picked", () => app.js(`return document.querySelectorAll(".chip.chosen").length === 2`), 10_000);
      await carryOut([["letgo", drop.x + 16, drop.y + 8]], `.chip[data-clip="${pair[0].id}"] .length`);
      const arrivals = [];
      await until(
        "both files whole in the folder",
        () => {
          for (const f of readdirSync(dest).filter((f) => !f.startsWith(".") && !f.endsWith(".part"))) {
            if (whole(path.join(dest, f)) && !arrivals.some((a) => a.name === f))
              arrivals.push({ name: f, at: Date.now() });
          }
          return arrivals.length === 2;
        },
        2 * MINUTE,
        { every: 50 },
      );
      const lengths = arrivals.map((a) => probeDuration(path.join(dest, a.name)));
      if (lengths.some((l) => Math.abs(l - 60) > SLACK))
        throw new Error(`the two files are ${lengths.join(", ")} s long, not 60`);
      if (!(await ended()).delivered) throw new Error("the page heard the two went nowhere");
      // Named for their ranges: the first's starts at 0.
      const apart = arrivals[1].at - arrivals[0].at;
      if (copies && !(arrivals[0].name < arrivals[1].name && apart > 2000))
        throw new Error(`Dolphin's two came ${apart} ms apart, ${arrivals.map((a) => a.name).join(" then ")}`);
      ctx.log(
        `two carried together, both being made: both landed, ${apart} ms apart (${arrivals.map((a) => a.name).join(", then ")})`,
      );
      await settle();
    } finally {
      nautilus?.close();
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
