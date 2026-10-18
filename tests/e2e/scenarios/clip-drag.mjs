// The Clip button dragged rather than clicked: the lit-up stretch carried,
// with no key held, here onto the clip bar, where it becomes a clip. (Past the
// window's edge the same carry hands it to another app: drag-out.) A click,
// pressed and let go in place, still clips as before (clip, export-whole).

import { until } from "../lib/app.mjs";

export default {
  name: "clip-drag",
  async run(app, ctx) {
    await app.toLibrary();
    const vod = await app.addAndTranscribe(ctx.fixtures.landed);
    await app.openPaused(vod.id);
    // Cuts at 1 and 3 s, a clip of 1 to 3; then the playhead into 3 s to the end.
    await app.clipOneToThree(vod.id);
    await app.key("ArrowRight", { shift: true });
    await app.key("ArrowRight", { shift: true });
    await until("the player at 4 s", async () => (await app.shownTime()) === 4, 10_000);
    // Pressed on Clip, moved, and let go over the clip bar (as the pointer
    // would, in the page's events).
    await app.js(
      `const button = document.querySelector("button.action.clip");
       const bar = document.querySelector(".clip-bar").getBoundingClientRect();
       const from = button.getBoundingClientRect();
       const at = (x, y) => ({ clientX: x, clientY: y, button: 0, buttons: 1, bubbles: true, pointerId: 1, isPrimary: true });
       const [x0, y0] = [from.left + from.width / 2, from.top + from.height / 2];
       const [x1, y1] = [bar.left + bar.width / 2, bar.top + bar.height / 2];
       button.dispatchEvent(new PointerEvent("pointerdown", at(x0, y0)));
       window.dispatchEvent(new PointerEvent("pointermove", at(x0 + 10, y0 + 10)));
       for (let i = 1; i <= 5; i++) window.dispatchEvent(new PointerEvent("pointermove", at(x0 + (x1 - x0) * i / 5, y0 + (y1 - y0) * i / 5)));
       window.dispatchEvent(new PointerEvent("pointerup", { ...at(x1, y1), buttons: 0 }));`,
    );
    const made = await until(
      "the stretch from 3 s a clip",
      async () => {
        const clips = await app.invoke("list_clips", { vodId: vod.id });
        const c = clips.find((c) => Math.abs(c.start - 3) < 0.05);
        if (c) return c;
        throw new Error(`clips: ${JSON.stringify(clips.map((c) => [c.start, c.end]))}`);
      },
      30_000,
    );
    if (Math.abs(made.end - vod.duration) > 0.05 && made.end < 5) throw new Error(`the clip ends at ${made.end}`);
    const all = await app.invoke("list_clips", { vodId: vod.id });
    if (all.length !== 2) throw new Error(`${all.length} clips, not 2`);
    ctx.log(`dragged Clip onto the clip bar: a clip of ${made.start} to ${made.end}`);
  },
};
