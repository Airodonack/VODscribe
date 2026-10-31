// How long a clip dragged out of the window takes to be a whole file where
// it's dropped: from the press to the file at its full size in a file
// manager's folder (Nautilus, on the test's own display and bus). Each drag
// travels the same path at the same speed (about a second from the press to
// over the folder), waits there for the system's drag if it hasn't begun
// (today the drag begins once the files are made), and lets go. Logged per
// case, and timed (perf.json, with --repeat: medians): press to the edge
// ("carry-out"), the edge to the system's drag up (its picture's window
// showing), the drop to the file whole, and the press to the file whole. Cases: a minute of 360p
// and three minutes of 720p, each in MP4 and in MOV (Settings), the first
// half carried from the timeline (Clip pressed and dragged), the second half
// as its pill, and both picked and carried together by the pill (its file
// made, the stretch's not); last, a stretch carried while two long clips
// wait to be made ahead of it. Linux (X11) with Nautilus only, so far.

import { readdirSync, rmSync, statSync } from "node:fs";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "../lib/app.mjs";
import { dataDirs } from "../lib/data.mjs";
import { findIn, noFileManager, openFileManager, probeDuration, setFormat, windowsOf } from "../lib/fileManager.mjs";
import { longVideo, minute } from "../lib/fixtures.mjs";
import * as os from "../lib/os.mjs";

/** A click's modifier that picks one (Ctrl; Linux only here). */
const TOGGLE = `const toggle = { ctrlKey: true, metaKey: false };`;
/** A pointer event's fields at (x, y) with `buttons` down, in the page. */
const AT = `const at = (x, y, buttons) => ({ bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons, isPrimary: true, pointerId: 1 });`;

/** How far off its stretch's length a file's duration may be, in seconds. */
const SLACK = 0.1;

export default {
  name: "drag-file-time",
  async run(app, ctx) {
    const why = noFileManager();
    if (why) ctx.skip(why);
    const work = path.join(ctx.out, "drag-file-time");
    rmSync(work, { recursive: true, force: true });
    const dest = path.join(work, "drop-here");
    const fixtures = path.join(ctx.out, "fixtures");
    const files = [minute(fixtures), longVideo(fixtures)];

    await app.toLibrary();
    const byFile = [];
    for (const file of files) byFile.push((await app.addFiles([file]))[0]);
    const durations = await until(
      "the videos read",
      async () => {
        const all = await app.invoke("list_vods");
        const found = byFile.map((v) => all.find((a) => a.id === v.id)?.duration);
        return found.every((d) => d > 0) && found;
      },
      5 * MINUTE,
    );

    // Nautilus on the folder, its own home (never the user's), beside the app's window.
    const nautilus = await openFileManager(app, ctx, dest, path.join(work, "home"));
    try {
      const { drop } = nautilus;

      // The marks the page sets, in wall-clock time, and the press's.
      await app.js(
        `window.__pressed = [];
         window.addEventListener("pointerdown", () => window.__pressed.push(performance.timeOrigin + performance.now()), true);`,
      );

      /**
       * One drag from screen point `from` to the folder; `count` files of
       * `lengths` seconds expected there. Logged and timed as `label`.
       */
      const measure = async (label, from, lengths) => {
        await app.js(`performance.clearMarks(); window.__pressed = [];`);
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
        const marks = await until(
          "the system's drag",
          () =>
            app.js(
              `const at = (n) => { const [m] = performance.getEntriesByName(n); return m ? performance.timeOrigin + m.startTime : null; };
               const drag = at("drag-start");
               return drag ? { pressed: window.__pressed[0], out: at("carry-out"), drag } : false;`,
            ),
          10 * MINUTE,
          { every: 50 },
        );
        // Asked for: up once its picture's window shows (a drop before then
        // isn't one: the drag is refused with no button held, as it should be).
        const up = await until(
          "the system's drag up",
          () => windowsOf(app.driver.proc.pid).some((w) => !before.has(w)) && Date.now(),
          30_000,
          { every: 25 },
        ).catch(async (e) => {
          throw new Error(`${e.message}; the page's errors: ${JSON.stringify(await app.errors().catch(() => null))}`);
        });
        await os.gesture([
          ["move", drop.x + 8, drop.y + 4],
          ["wait", 200],
          ["move", drop.x + 16, drop.y + 8],
          ["wait", 300],
        ]);
        const dropped = Date.now();
        await os.gesture([["letgo", drop.x + 16, drop.y + 8]]);
        // Whole: as many files as were dragged, each the size of the app's.
        const sources = new Map();
        const whole = await until(
          "the files whole in the folder",
          () => {
            const there = readdirSync(dest).filter((f) => !f.startsWith("."));
            if (there.length < lengths.length) return false;
            for (const f of there) {
              const source = sources.get(f) ?? findIn(dataDirs()[0], f);
              if (!source) throw new Error(`${f} in the folder isn't one of the app's files`);
              sources.set(f, source);
              if (statSync(path.join(dest, f)).size !== statSync(source).size) return false;
            }
            return { at: Date.now(), there };
          },
          10 * MINUTE,
          { every: 50 },
        ).catch(async (e) => {
          throw new Error(`${e.message}; the page's errors: ${JSON.stringify(await app.errors().catch(() => null))}`);
        });
        const got = whole.there.map((f) => probeDuration(path.join(dest, f))).sort((a, b) => a - b);
        const want = [...lengths].sort((a, b) => a - b);
        if (got.length !== want.length || got.some((d, i) => Math.abs(d - want[i]) > SLACK))
          throw new Error(`${label}: the files dropped last ${got.join(", ")} s, not ${want.join(", ")} s`);
        const parts = {
          "to the edge": Math.round(marks.out - marks.pressed),
          "edge to drag": Math.round(up - marks.out),
          "drop to whole": whole.at - dropped,
          "press to whole": Math.round(whole.at - marks.pressed),
        };
        for (const [part, ms] of Object.entries(parts)) ctx.timing(`${label}: ${part}`, ms, ctx.clock() - ms / 1000);
        ctx.log(
          `${label}: ${Object.entries(parts)
            .map(([p, ms]) => `${p} ${ms} ms`)
            .join(
              ", ",
            )} (${whole.there.length} file${whole.there.length > 1 ? "s" : ""}, ${got.map((d) => d.toFixed(2)).join(", ")} s)`,
        );
        await until("the system's drag, over", () => app.js(`return !document.querySelector(".carry-ghost")`), MINUTE);
        await sleep(1500);
        for (const f of readdirSync(dest)) rmSync(path.join(dest, f), { force: true });
        await sleep(500);
      };

      const centre = async (selector) => {
        const r = await os.rectOf(app, selector);
        return { x: r.x + Math.round(r.w / 2), y: r.y + Math.round(r.h / 2) };
      };
      const scrubTo = (time, picking) =>
        app.js(
          `${AT} ${TOGGLE}
           const bar = document.querySelector(".scrubber");
           const box = bar.getBoundingClientRect();
           const x = box.left + ((arguments[0] - Number(bar.dataset.viewStart)) / Number(bar.dataset.span)) * box.width;
           const y = box.top + box.height / 2;
           const mods = arguments[1] ? toggle : {};
           bar.dispatchEvent(new PointerEvent("pointerdown", { ...at(x, y, 1), ...mods }));
           window.dispatchEvent(new PointerEvent("pointerup", { ...at(x, y, 0), ...mods }));`,
          time,
          picking,
        );

      for (const format of ["MP4", "MOV"]) {
        await setFormat(app, format);
        for (const [i, vod] of byFile.entries()) {
          const name = `${i === 0 ? "minute 360p" : "3 minutes 720p"} ${format}`;
          await app.openPaused(vod.id);
          // A cut halfway along (B, the playhead there): its first half and its second, two stretches.
          if (!(await app.invoke("list_marks", { vodId: vod.id })).length) {
            await scrubTo(durations[i] / 2, false);
            await until(
              "the playhead halfway",
              async () => (await app.shownTime()) >= Math.floor(durations[i] / 2) - 1,
              10_000,
            );
            await app.key("b");
            await until("the cut, selected", () => app.js(`return !!document.querySelector(".cut.selected")`), 10_000);
            await app.key("Escape");
          }
          const [cut] = await until("the cut", () => app.invoke("list_marks", { vodId: vod.id }), 10_000);
          const half = cut.start;
          // The first half, carried from the timeline: Clip pressed and dragged.
          await scrubTo(0, false);
          await until("the playhead at the start", async () => (await app.shownTime()) === 0, 10_000);
          await measure(`${name}, timeline`, await centre("button.action.clip"), [half]);
          // The second half made a clip (C, the playhead in it), then its pill carried.
          await scrubTo(half + 1, false);
          await until("the playhead in the second half", async () => (await app.shownTime()) > half, 10_000);
          await app.key("c");
          const pill = await until(
            "the second half's clip made",
            async () =>
              (await app.invoke("list_clips", { vodId: vod.id })).find(
                (c) => c.shown && c.status === "ready" && Math.abs(c.start - half) < 0.1,
              ),
            10 * MINUTE,
          );
          const chip = `.chip.ready[data-clip="${pill.id}"] .length`;
          await until("its pill", () => app.js(`return !!document.querySelector(arguments[0])`, chip), MINUTE);
          await measure(`${name}, pill`, await centre(chip), [pill.end - pill.start]);
          // Both halves picked (the first a stretch, its file not made), carried by the pill.
          await sleep(600);
          await scrubTo(half / 2, true);
          await app.js(
            `${TOGGLE}
             document.querySelector('.chip[data-clip="' + arguments[0] + '"]')
               .dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, ...toggle }));`,
            pill.id,
          );
          await until(
            "both picked",
            () => app.js(`return document.querySelectorAll(".scrubber .chosen, .chip.chosen").length === 2`),
            10_000,
          );
          await measure(`${name}, both picked`, await centre(chip), [half, pill.end - pill.start]);
          await app.key("Escape");
          await app.invoke("delete_clip", { id: pill.id });
        }
      }

      // A stretch carried while two long clips wait to be made ahead of it
      // (MOV, the three minutes': each takes a while).
      {
        const [vod, length] = [byFile[1], durations[1]];
        await app.openPaused(vod.id);
        await scrubTo(0, false);
        await until("the playhead at the start", async () => (await app.shownTime()) === 0, 10_000);
        const [cut] = await app.invoke("list_marks", { vodId: vod.id });
        await app.invoke("add_clips", {
          vodId: vod.id,
          ranges: [
            [cut.start, length],
            [cut.start + 10, length],
          ],
        });
        // Their pills shown first: the clip bar coming in moves the buttons.
        await until("their pills", () => app.js(`return document.querySelectorAll(".chip").length === 2`), 10_000);
        await sleep(300);
        await measure("3 minutes 720p MOV, timeline, behind two clips", await centre("button.action.clip"), [
          cut.start,
        ]);
      }
    } finally {
      nautilus.close();
    }
  },
};
