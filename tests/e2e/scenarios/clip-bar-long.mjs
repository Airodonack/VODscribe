// A clip bar with more pills than fit, so it scrolls sideways: every pill
// in view is still what's under the pointer at its middle (the page's own
// hit test, elementFromPoint), and (Linux) the system's mouse reaches them:
// a click on a pill's name and F2 renames it, a click on its × deletes it.
// The same once the bar is scrolled to its end.

import { until } from "../lib/app.mjs";
import * as os from "../lib/os.mjs";

/** Seven stretches of the landing, each a clip with words in its name, so the pills are wide. */
const RANGES = [
  [0, 1],
  [0, 2],
  [0, 3],
  [1, 2],
  [1, 3],
  [2, 3],
  [2, 4],
];

export default {
  name: "clip-bar-long",
  async run(app, ctx) {
    await app.toLibrary();
    const vod = await app.addAndTranscribe(ctx.fixtures.landed);
    await app.openPaused(vod.id);
    await app.pause();
    await app.invoke("add_clips", { vodId: vod.id, ranges: RANGES });
    await until(
      "seven pills, made",
      () => app.js(`return document.querySelectorAll(".clip-bar .chip.ready").length === 7`),
      60_000,
    );
    const overflow = await app.js(`
      const strip = document.querySelector(".clip-bar");
      return [strip.scrollWidth, strip.clientWidth];`);
    if (overflow[0] <= overflow[1]) throw new Error(`the clip bar doesn't scroll: ${overflow.join(" ≤ ")} px`);
    ctx.log(`the clip bar is ${overflow[0]} px wide in ${overflow[1]} px: it scrolls`);

    // What's under each pill's middle, of those in the bar's view: the pill, or a part of it.
    const missed = () =>
      app.js(`
        const strip = document.querySelector(".clip-bar");
        const view = strip.getBoundingClientRect();
        const missed = [];
        let seen = 0;
        for (const pill of strip.querySelectorAll(".chip")) {
          const r = pill.getBoundingClientRect();
          const [x, y] = [r.left + r.width / 2, r.top + r.height / 2];
          if (x < view.left || x > view.left + strip.clientWidth) continue;
          seen++;
          const hit = document.elementFromPoint(x, y);
          if (!pill.contains(hit)) missed.push(pill.dataset.clip + ": " + (hit ? hit.tagName + "." + [...hit.classList].join(".") : "nothing"));
        }
        return { seen, missed };`);
    const allHit = async (where) => {
      const { seen, missed: off } = await missed();
      if (!seen) throw new Error(`no pill in the clip bar's view ${where}`);
      if (off.length)
        throw new Error(`${where}, the pointer at a pill's middle finds something else: ${off.join(", ")}`);
      ctx.log(`${where}: each of the ${seen} pills in view is what's under its middle`);
    };
    await allHit("at its start");

    // The system's mouse: on the first pill in view, a click on its name and
    // F2 opens its name to edit (Escape leaves it); a click on its × deletes it.
    const why =
      os.unavailable() ?? (process.platform !== "linux" ? "the system's keys are driven on Linux only" : null);
    const clipsLeft = async () => (await app.invoke("list_clips", { vodId: vod.id })).filter((c) => c.shown).length;
    const pointAt = async (which) => {
      const id = await app.js(
        `const strip = document.querySelector(".clip-bar");
         const view = strip.getBoundingClientRect();
         const pills = [...strip.querySelectorAll(".chip")].filter((p) => {
           const r = p.getBoundingClientRect();
           return r.left >= view.left && r.right <= view.left + strip.clientWidth;
         });
         return (arguments[0] === "last" ? pills.at(-1) : pills[0])?.dataset.clip;`,
        which,
      );
      if (!id) throw new Error(`no pill wholly in view for the ${which}`);
      const pill = `.chip[data-clip="${id}"]`;
      const name = await os.rectOf(app, `${pill} span.name`);
      os.click(name.x + name.w / 2, name.y + name.h / 2, name.scale);
      await os.gesture([["key", "F2"]]);
      await until(
        `pill ${id}'s name open to edit`,
        () => app.js(`return !!document.querySelector(arguments[0])`, `${pill} input.name`),
        5_000,
      );
      await os.gesture([["key", "Escape"]]);
      await until(
        `pill ${id}'s name closed`,
        () => app.js(`return !document.querySelector(arguments[0])`, `${pill} input.name`),
        5_000,
      );
      const before = await clipsLeft();
      const x = await os.rectOf(app, `${pill} button[aria-label="Delete clip"]`);
      os.click(x.x + x.w / 2, x.y + x.h / 2, x.scale);
      await until(`pill ${id} deleted`, async () => (await clipsLeft()) === before - 1, 10_000);
      ctx.log(`the system's mouse on pill ${id}: its name opened with F2, its × deleted it`);
    };
    if (why) ctx.log(`the system's mouse isn't driven here: ${why}`);
    else await pointAt("first");

    // Scrolled to its end: the same.
    await app.js(`const strip = document.querySelector(".clip-bar"); strip.scrollLeft = strip.scrollWidth;`);
    await allHit("scrolled to its end");
    if (!why) await pointAt("last");
  },
};
