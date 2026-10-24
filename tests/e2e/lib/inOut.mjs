// I right after a click on the timeline (site-in-out.mjs, in-out-file.mjs):
// a click moves the playhead at once, but the player says it's there only a
// while later; I pressed meanwhile starts the selection at the click.

/** In the page: a click (the page's own pointer events) on the timeline at
 *  `fraction` of its view, then I pressed at every other frame for `ms`;
 *  returns the time clicked and, for each press, how long after the click
 *  (s) and where the selection started (null: none). */
const CLICK_THEN_IN = `
  const [fraction, ms] = arguments;
  const bar = document.querySelector(".scrubber");
  const box = bar.getBoundingClientRect();
  const time = (x) => Number(bar.dataset.viewStart) + ((x - box.left) / box.width) * Number(bar.dataset.span);
  const at = { clientX: box.left + fraction * box.width, clientY: box.top + box.height / 2, button: 0,
               bubbles: true, pointerId: 1, pointerType: "mouse", isPrimary: true };
  const clicked = performance.now();
  document.elementFromPoint(at.clientX, at.clientY).dispatchEvent(new PointerEvent("pointerdown", at));
  window.dispatchEvent(new PointerEvent("pointerup", at));
  const presses = [];
  while (performance.now() - clicked < ms) {
    const pressed = performance.now();
    document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "i", bubbles: true }));
    await new Promise((r) => requestAnimationFrame(r));
    const picked = bar.querySelector(".selection");
    presses.push([(pressed - clicked) / 1000, picked ? time(picked.getBoundingClientRect().left) : null]);
    await new Promise((r) => requestAnimationFrame(r));
  }
  return { clicked: time(at.clientX), presses };`;

/**
 * A click at `fraction` of the timeline, I pressed at every other frame for
 * `ms`: each selection starts within `slack` seconds of the click (or,
 * `playing`, of as far past it as the time since). Escape lets it go after.
 */
export async function clickThenIn(app, ctx, what, { fraction, ms, playing, slack }) {
  const { clicked, presses } = await app.js(CLICK_THEN_IN, fraction, ms);
  const starts = presses.map(([, start]) => start ?? NaN);
  ctx.log(
    `${what}: clicked at ${clicked.toFixed(1)} s, ${presses.length} presses of I over ${ms / 1000} s: ` +
      `the selection started from ${Math.min(...starts).toFixed(1)} s to ${Math.max(...starts).toFixed(1)} s`,
  );
  const off = presses.find(
    ([after, start]) => start === null || start < clicked - slack || start > clicked + (playing ? after : 0) + slack,
  );
  if (off)
    throw new Error(
      `${what}: I ${off[0].toFixed(2)} s after the click at ${clicked.toFixed(1)} s ` +
        `started the selection at ${off[1]?.toFixed(1) ?? "nothing"} s`,
    );
  await app.key("Escape");
}
