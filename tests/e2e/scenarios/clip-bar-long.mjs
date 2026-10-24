// A clip bar with more pills than fit, so it scrolls sideways: every pill
// in view is still what's under the pointer at its middle (the page's own
// hit test, elementFromPoint), and (Linux) the system's mouse reaches them:
// a click on a pill's name and F2 renames it, a click on its × deletes it.
// The same once the bar is scrolled to its end. The wheel turned over the
// bar scrolls it sideways; a drag with the middle button pressed on a pill
// scrolls it with the pointer, the pill picked staying picked and nothing
// carried off. Last, the system's double-click on a pill not picked opens
// it in the editor (the click that picks it moves nothing under the pointer).

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

    // The wheel over the bar, and a drag with the middle button: the system's
    // where it's driven (X11), else the same events sent to the page.
    const why =
      os.unavailable() ?? (process.platform !== "linux" ? "the system's keys are driven on Linux only" : null);
    const scrolled = () => app.js(`return document.querySelector(".clip-bar").scrollLeft`);
    const toStart = () => app.js(`document.querySelector(".clip-bar").scrollLeft = 0`);
    const pillInView = (n) =>
      app.js(
        `const strip = document.querySelector(".clip-bar");
         const view = strip.getBoundingClientRect();
         return [...strip.querySelectorAll(".chip")].filter((p) => {
           const r = p.getBoundingClientRect();
           return r.left >= view.left && r.right <= view.left + strip.clientWidth;
         })[arguments[0]]?.dataset.clip;`,
        n,
      );
    await toStart();
    const under = `.chip[data-clip="${await pillInView(1)}"]`;
    if (why)
      await app.js(
        `const el = document.querySelector(arguments[0]);
         const r = el.getBoundingClientRect();
         for (let i = 0; i < 3; i++)
           el.dispatchEvent(new WheelEvent("wheel", { bubbles: true, cancelable: true, clientX: r.left + r.width / 2, clientY: r.top + r.height / 2, deltaY: 100 }));`,
        under,
      );
    else {
      const r = await os.rectOf(app, under);
      await os.gesture([
        ["move", Math.round(r.x + r.w / 2), Math.round(r.y + r.h / 2)],
        ["wait", 100],
        ["wheel", "down", 3],
      ]);
    }
    const wheeled = await until(
      "the wheel scrolled the bar sideways",
      async () => (await scrolled()) > 0 && (await scrolled()),
      5_000,
    );
    ctx.log(`the wheel turned down over the bar scrolled it ${wheeled} px sideways`);

    // A pill picked (Ctrl-click), then a middle-button drag 200 px to the
    // left from the pill after it: the bar follows the pointer, the pick stays.
    await toStart();
    const keep = await pillInView(0);
    await app.js(
      `const mac = /Mac/.test(navigator.platform);
       document.querySelector('.chip[data-clip="' + arguments[0] + '"]')
         .dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, detail: 1, ctrlKey: !mac, metaKey: mac }));`,
      keep,
    );
    const chosen = () =>
      app.js(`return [...document.querySelectorAll(".chip.chosen")].map((c) => c.dataset.clip).join()`);
    await until("a pill picked", async () => (await chosen()) === keep, 5_000);
    if (why)
      await app.js(
        `const el = document.querySelector(arguments[0]);
         const r = el.getBoundingClientRect();
         const at = (x, buttons) => ({ bubbles: true, cancelable: true, clientX: x, clientY: r.top + r.height / 2, button: 1, buttons, isPrimary: true, pointerId: 1 });
         const x = r.left + r.width / 2;
         el.dispatchEvent(new PointerEvent("pointerdown", at(x, 4)));
         for (let dx = 20; dx <= 200; dx += 20) window.dispatchEvent(new PointerEvent("pointermove", { ...at(x - dx, 4), button: -1 }));
         window.dispatchEvent(new PointerEvent("pointerup", at(x - 200, 0)));`,
        under,
      );
    else {
      const r = await os.rectOf(app, under);
      const [x, y] = [Math.round(r.x + r.w / 2), Math.round(r.y + r.h / 2)];
      const steps = [["down", x, y, 2]];
      for (let dx = 20; dx <= 200; dx += 20) steps.push(["move", x - Math.round(dx * r.scale), y], ["wait", 20]);
      await os.gesture([...steps, ["up", 2], ["wait", 100]]);
    }
    const panned = await until(
      "the middle drag scrolled the bar",
      async () => (await scrolled()) > 150 && (await scrolled()),
      5_000,
    );
    const after = await app.js(
      `return { chosen: [...document.querySelectorAll(".chip.chosen")].map((c) => c.dataset.clip).join(), box: !!document.querySelector(".select-box"), carried: !!document.querySelector(".clip-bar .drop") }`,
    );
    if (after.chosen !== keep) throw new Error(`the middle drag changed what's picked: ${keep} → "${after.chosen}"`);
    if (after.box || after.carried)
      throw new Error(`the middle drag drew a box (${after.box}) or carried a pill (${after.carried})`);
    ctx.log(`a middle drag of 200 px to the left scrolled the bar ${panned} px; pill ${keep} still picked alone`);
    await app.key("Escape");
    await until("Escape let it go", async () => !(await chosen()), 5_000);
    await toStart();

    // The system's mouse: on the first pill in view, a click on its name and
    // F2 opens its name to edit (Escape leaves it); a click on its × deletes it.
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

    if (!why) {
      await app.key("Escape");
      await until("nothing picked", async () => !(await chosen()), 5_000);
      const id = await pillInView(0);
      const r = await os.rectOf(app, `.chip[data-clip="${id}"] .time`);
      await os.gesture([
        ["move", Math.round(r.x + r.w / 2), Math.round(r.y + r.h / 2)],
        ["wait", 100],
        ["click", 2],
      ]);
      await until(
        `pill ${id} open in the editor`,
        () => app.js(`return !!document.querySelector('section.edit-view input.title[aria-label="Clip name"]')`),
        10_000,
      );
      ctx.log(`the system's double-click on pill ${id}, not picked, opened it in the editor`);
    }
  },
};
