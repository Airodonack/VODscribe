// The OS-level layer (lib/os.mjs): what the player shows, seen on the
// screen, and played with the OS's own mouse. The app's own player draws
// outside the page, so only a screenshot shows it. With a video whose every
// second is one flat colour (fixtures colours.mp4), the screen says which
// frame is up: the frame a seek lands on, a picture that changes while it
// plays, and one that stands while paused. A panel opened over it shows.
// Timed, in the app's own player: from opening the VOD (and Play) to its
// first frame, and from a seek to the frame there.

import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { MINUTE, until } from "../lib/app.mjs";
import { COLOURS, colours } from "../lib/fixtures.mjs";
import * as os from "../lib/os.mjs";

/** How far a colour may be from the one meant: the video's own YUV rounding, scaling, the screen's colour depth. */
const NEAR = 90;

export default {
  name: "player-picture",
  async run(app, ctx) {
    const why = os.unavailable();
    if (why) ctx.skip(why);
    try {
      os.screenshot();
    } catch (e) {
      ctx.skip(`no screenshot here: ${e.message.split("\n")[0]}`);
    }
    const file = colours(path.join(ctx.out, "fixtures"));
    await app.toLibrary();
    await app.pickFiles([file]);
    await app.clickText("add video files…");
    const [vod] = await until(
      "the video, done",
      async () => {
        const vods = await app.invoke("list_vods");
        if (vods[0]?.status === "error") throw new Error(vods[0].error);
        return vods[0]?.status === "done" && vods;
      },
      5 * MINUTE,
    );
    // Either player is made on the first play; paused once it plays (a pause
    // before then finds nothing to pause, and it plays on).
    const open = async () => {
      await app.openVod(vod.id);
      await app.click('.bar button[aria-label="Play"]');
    };
    // The app's own player, where there's a screen for it, says which frame it shows.
    const own = await app.invoke("player_available");
    if (own) await app.timeToFrame("open to first frame", open);
    else await open();
    await until(
      "it to play",
      () => app.js(`return !!document.querySelector('.bar button[aria-label="Pause"]')`),
      MINUTE,
    );
    // The app's own player's box, or the web view's <video> where it plays there.
    const native = await until(
      "a player",
      () =>
        app.js(
          `return document.querySelector(".player .picture") ? "native" : document.querySelector(".player video") ? "web" : null`,
        ),
      MINUTE,
    ).then((kind) => kind === "native");
    const picture = native ? ".player .picture" : ".player video";
    ctx.log(`playing in ${native ? "the app's own player" : "the web view"}`);
    const shown = async () => os.colourIn(os.screenshot(), os.middle(await os.rectOf(app, picture)));
    const named = (rgb) => {
      const [name, , d] = COLOURS.map(([n, c]) => [n, c, os.distance(rgb, c)]).sort((a, b) => a[2] - b[2])[0];
      return d < NEAR ? name : `none (${rgb.join(", ")})`;
    };
    const expect = async (second) => {
      const [name] = COLOURS[second];
      const seen = await until(
        `${name} (second ${second}) on the screen`,
        async () => {
          const now = named(await shown());
          if (now === name) return now;
          throw new Error(`the picture is ${now}`);
        },
        10_000,
      );
      ctx.log(`at ${second} s the screen shows ${seen}`);
    };
    const to = (seconds) =>
      until(`the player at ${seconds} s`, async () => (await app.shownTime()) === seconds, 10_000);

    // Seeks, each landing on its frame: the first of each second.
    await app.pause();
    const seek = (act, at) => (native ? app.timeToFrame("seek to frame shown", act, at) : act());
    await seek(() => app.toStart(), 0);
    await to(0);
    await expect(0);
    await seek(() => app.key("ArrowRight", { shift: true }), 1);
    await to(1);
    await expect(1);
    await app.key("ArrowRight", { shift: true });
    await app.key("ArrowRight", { shift: true });
    await to(3);
    await expect(3);
    // The time by the button is the frame on screen (the app's own player says which).
    if (native) ctx.log(await app.timeIsPicture("after the seeks, paused"));

    // Played with a real click on Play (trusted, hit-tested): it moves on.
    await app.toStart();
    await to(0);
    os.activate();
    const play = await os.rectOf(app, '.bar button[aria-label="Play"]');
    os.click(play.x + play.w / 2, play.y + play.h / 2, play.scale);
    await until(
      "Pause to show (it plays)",
      () => app.js(`return !!document.querySelector('.bar button[aria-label="Pause"]')`),
      10_000,
    );
    const first = named(await shown());
    await sleep(1500);
    const later = named(await shown());
    ctx.log(`playing: ${first}, then ${later}`);
    if (first === later) throw new Error(`playing, the picture stayed ${first} for 1.5 s`);
    if (native) ctx.log(await app.timeIsPicture("playing", { playing: true }));

    // Paused with a real click: it stands.
    const pause = await os.rectOf(app, '.bar button[aria-label="Pause"]');
    os.click(pause.x + pause.w / 2, pause.y + pause.h / 2, pause.scale);
    await until(
      "Play to show (it's paused)",
      () => app.js(`return !!document.querySelector('.bar button[aria-label="Play"]')`),
      10_000,
    );
    await sleep(300);
    const still = await shown();
    await sleep(1200);
    const stillLater = await shown();
    if (os.distance(still, stillLater) > 30)
      throw new Error(`paused, the picture changed from ${still} to ${stillLater}`);
    ctx.log(`paused: stays ${named(still)}`);
    if (native) ctx.log(await app.timeIsPicture("paused"));

    // A panel opened over the picture (Settings) shows there: the picture
    // draws on a surface over the page, which mustn't cover the panel.
    await app.click('button[aria-label="Settings"]');
    const panel = '[role=dialog][aria-label="Settings"]';
    await until("Settings open", () => app.js(`return !!document.querySelector(arguments[0])`, panel), 10_000);
    await sleep(500);
    const [p, v] = [await os.rectOf(app, panel), await os.rectOf(app, picture)];
    const x = Math.max(p.x, v.x);
    const y = Math.max(p.y, v.y);
    const over = { x, y, w: Math.min(p.x + p.w, v.x + v.w) - x, h: Math.min(p.y + p.h, v.y + v.h) - y };
    if (over.w < 20 || over.h < 20) {
      ctx.log("Settings doesn't overlap the picture in this window");
    } else {
      const background = await app.js(
        `return getComputedStyle(document.querySelector(arguments[0])).backgroundColor.match(/[0-9.]+/g).slice(0, 3).map(Number)`,
        panel,
      );
      // Drawn without a GPU (as in containers), the page can take a moment to show there.
      const waited = Date.now();
      await until(
        "the panel on screen where it overlaps the picture",
        () => {
          const there = os.colourIn(os.screenshot(), os.middle(over));
          if (os.distance(there, background) <= 40) return true;
          throw new Error(`the screen shows ${named(there)} there, not the panel`);
        },
        5_000,
      );
      ctx.log(`Settings shows over the picture (after ${Date.now() - waited} ms)`);
      // The rest of the picture keeps its frame (the page draws it while the
      // surface is hidden), not black.
      const free = { x: v.x, y: v.y, w: p.x - v.x, h: v.h };
      if (free.w >= 40) {
        await until(
          "the frame kept beside the panel",
          () => {
            const there = os.colourIn(os.screenshot(), os.middle(free));
            if (os.distance(there, still) <= NEAR) return true;
            throw new Error(`beside the panel the picture shows ${named(there)}, not ${named(still)}`);
          },
          5_000,
        );
        ctx.log(`beside the panel, the picture keeps its frame (${named(still)})`);
      }
    }
    await app.key("Escape");

    // The timeline's tip, the pointer over it, stays out of the picture's box
    // (moving the OS's pointer is Linux-only, so far).
    if (process.platform === "linux") {
      const timeline = await os.rectOf(app, ".timeline");
      os.activate();
      await os.gesture([["move", timeline.x + Math.round(timeline.w / 2), timeline.y + Math.round(timeline.h / 2)]]);
      await until("the timeline's tip", () => app.js(`return !!document.querySelector(".timeline .tip")`), 10_000);
      const [tip, box] = [await os.rectOf(app, ".timeline .tip"), await os.rectOf(app, picture)];
      if (tip.y < box.y + box.h && tip.y + tip.h > box.y)
        throw new Error(`the timeline's tip (${JSON.stringify(tip)}) is over the picture (${JSON.stringify(box)})`);
      ctx.log("the timeline's tip is clear of the picture");
    }
  },
};
